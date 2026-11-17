// og-app-login: the one sign-in for every OG front end (task #684). Spec: docs/APP-LOGIN.md in
// og-vault. The app shows its icon, name, a vault address field and Continue; the vault's own
// sign-in page takes the password (the app never sees it) and sends the browser back with a
// one-time code (OAuth 2.0 authorization code + PKCE S256). Plain ES module, no dependencies,
// browsers and Node >= 18. Kotlin apps follow APP-LOGIN.md section 5.

import { normalizeAddress, probe } from "./og-profiles.js";

export const PENDING_KEY = "og:login";
/** A pending sign-in older than this is dropped (the vault's own page expires after 15 min). */
export const PENDING_MAX_MS = 20 * 60 * 1000;

function b64url(bytes) {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** `{verifier, challenge, state}`: 64-char verifier, its S256 challenge, a random state. */
export async function createPkce({ crypto: c = globalThis.crypto } = {}) {
  const verifier = b64url(c.getRandomValues(new Uint8Array(48)));
  const challenge = b64url(new Uint8Array(await c.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
  const state = b64url(c.getRandomValues(new Uint8Array(24)));
  return { verifier, challenge, state };
}

/** The app's own return address: this page without query or hash (what web apps register). */
export function defaultRedirect(loc = globalThis.location) {
  return loc.origin + loc.pathname;
}

/** `<vault>/auth/vault/start?…` for the vault's sign-in page. */
export function startUrl({ address, clientId, redirectUri, challenge, state, device, stepUp = false }) {
  const q = new URLSearchParams({ client_id: clientId, redirect_uri: redirectUri, code_challenge: challenge, code_challenge_method: "S256", state });
  if (device) q.set("device", device);
  if (stepUp) q.set("step_up", "1");
  return normalizeAddress(address) + "/auth/vault/start?" + q;
}

/**
 * Continue: probe the address (it must be an OG Vault), keep the pending sign-in in `storage`
 * (sessionStorage), then go to the vault's sign-in page. Returns the probe on failure
 * (`{ok:false, error}`) and never navigates then.
 */
export async function startLogin({
  address, clientId, redirectUri, device, stepUp = false,
  storage = globalThis.sessionStorage, fetch: f = globalThis.fetch, location: loc = globalThis.location,
  crypto: c = globalThis.crypto, navigate = (url) => loc.assign(url),
}) {
  if (!clientId) throw new Error("clientId is required");
  const p = await probe(address, { fetch: f });
  if (!p.ok) return p;
  const redirect = redirectUri || defaultRedirect(loc);
  const { verifier, challenge, state } = await createPkce({ crypto: c });
  storage.setItem(PENDING_KEY, JSON.stringify({
    verifier, state, address: p.address, vault_id: p.vault_id, name: p.name, url: p.url, redirect_uri: redirect,
    hash: loc.hash || "", at: Date.now(),
  }));
  const url = startUrl({ address: p.address, clientId, redirectUri: redirect, challenge, state, device, stepUp });
  navigate(url);
  return { ok: true, url, vault_id: p.vault_id };
}

/**
 * On page load: are we back from the vault? `null` if not. Otherwise checks `state`, swaps the
 * code at the same vault, checks the vault is still the one probed, saves the profile (when a
 * `store` from og-profiles is given: profile + tokens) and removes `code`/`state` from the URL
 * (putting the saved hash back). → `{profile, tokens, user, vault_id, address}` or `{error}`.
 */
export async function finishLogin({
  store, storage = globalThis.sessionStorage, fetch: f = globalThis.fetch, location: loc = globalThis.location,
  history: hist = globalThis.history, now = Date.now(),
} = {}) {
  const url = new URL(loc.href);
  const q = url.searchParams;
  if (!q.has("code") && !q.has("login_error") && !q.has("error")) return null;
  const raw = storage.getItem(PENDING_KEY);
  storage.removeItem(PENDING_KEY);
  let pend = null;
  try {
    pend = raw ? JSON.parse(raw) : null;
  } catch { /* none */ }
  const code = q.get("code");
  const err = q.get("login_error") || q.get("error");
  const state = q.get("state");
  for (const k of ["code", "state", "login_error", "error"]) q.delete(k);
  if (hist && hist.replaceState) hist.replaceState(null, "", url.pathname + (q.toString() ? "?" + q : "") + ((pend && pend.hash) || ""));
  if (!pend || now - (pend.at || 0) > PENDING_MAX_MS) return { error: "This sign-in expired; try again." };
  if (!state || state !== pend.state) return { error: "This sign-in didn't match; try again." };
  if (!code) return { error: err || "Sign-in was cancelled." };
  let tokens;
  try {
    const r = await f(pend.address + "/auth/token", {
      method: "POST", credentials: "omit", headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, verifier: pend.verifier, redirect_uri: pend.redirect_uri }),
    });
    tokens = await r.json().catch(() => ({}));
    if (!r.ok || !tokens.access_token) return { error: tokens.message || "The vault refused the sign-in; try again." };
  } catch {
    return { error: "The vault didn't answer; try again." };
  }
  const p = await probe(pend.address, { fetch: f });
  if (!p.ok || p.vault_id !== pend.vault_id) return { error: "That address now answers as another vault; try again." };
  const user = tokens.user ? { id: tokens.user.id, name: tokens.user.name, email: tokens.user.email } : null;
  const secret = { access_token: tokens.access_token, access_expires: tokens.access_expires, refresh_token: tokens.refresh_token };
  let profile = null;
  if (store) {
    profile = await store.save({ vault_id: p.vault_id, name: p.name, addresses: [p.address], user });
    if (p.url) profile = await store.addAddress(p.vault_id, p.address, { name: p.name, url: p.url });
    await store.setTokens(p.vault_id, secret);
    profile = await store.touch(p.vault_id, now);
  }
  return { profile, tokens: secret, user, vault_id: p.vault_id, address: p.address };
}

/**
 * A vault connect link (docs/APP-LOGIN.md "Connect link") or a plain web address → `{address,
 * vault_id, name}`, or null. Accepts `ogvault://connect?u=&id=&n=`, the same query on an
 * https URL whose path ends in /connect, and a bare address. Never contains secrets.
 */
export function parseConnectLink(text) {
  const t = String(text ?? "").trim();
  if (!t || /\s/.test(t)) return null;
  let u = null;
  try { u = new URL(t); } catch { /* bare address */ }
  const web = u && (u.protocol === "https:" || u.protocol === "http:");
  if (u && (u.protocol === "ogvault:" || (web && u.searchParams.has("u") && /\/connect\/?$/.test(u.pathname)))) {
    if (u.protocol === "ogvault:" && u.hostname !== "connect") return null;
    const a = u.searchParams.get("u");
    if (!a) return null;
    try { return { address: normalizeAddress(a), vault_id: u.searchParams.get("id") || null, name: u.searchParams.get("n") || "" }; } catch { return null; }
  }
  // "host:8080" parses as a URL with a scheme "host:"; anything else with a scheme is not an address
  if (u && !web && !/^[^/:]+:\d+(\/|$)/.test(t)) return null;
  try { return { address: normalizeAddress(t), vault_id: null, name: "" }; } catch { return null; }
}

/** Can this browser scan QR codes with the camera? */
export function canScan(g = globalThis) {
  return !!(g.BarcodeDetector && g.navigator && g.navigator.mediaDevices && g.navigator.mediaDevices.getUserMedia);
}

/**
 * Open the camera and resolve with the first QR code's text, or `{error}`:
 * `denied` (permission refused), `no_camera`, `unsupported`, `cancelled` (via `signal`).
 * `video` is a <video> element the caller shows; the stream is stopped before returning.
 */
export async function scanQr({ video, signal, g = globalThis, intervalMs = 250 } = {}) {
  if (!canScan(g)) return { error: "unsupported" };
  let stream;
  try {
    stream = await g.navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: "environment" } }, audio: false });
  } catch (e) {
    const n = e && e.name;
    return { error: n === "NotAllowedError" || n === "SecurityError" ? "denied" : "no_camera" };
  }
  try {
    const det = new g.BarcodeDetector({ formats: ["qr_code"] });
    video.srcObject = stream;
    video.setAttribute("playsinline", "");
    await video.play();
    while (!(signal && signal.aborted)) {
      const hits = await det.detect(video).catch(() => []);
      if (hits.length && hits[0].rawValue) return { text: hits[0].rawValue };
      await new Promise((r) => setTimeout(r, intervalMs));
    }
    return { error: "cancelled" };
  } finally {
    for (const t of stream.getTracks()) t.stop();
    video.srcObject = null;
  }
}

/**
 * The ready-made "Connect to your vault" screen: the app's icon and name, a vault address field,
 * Continue. Nothing else (OG UI copy rule). `onContinue(address)` runs on submit; whatever it
 * returns with `{ok:false}` (or throws) is shown under the field. `error` shows a message at once
 * (a `login_error` from the vault). When the browser can scan (`canScan`), a QR icon button sits
 * beside the address field: it opens the camera, reads a connect link, fills the address and
 * continues; a pasted connect link in the field works the same. Returns the root element.
 */
export function renderConnectScreen(root, { appName, icon, onContinue, address = "", error = "", document: d = globalThis.document, scan = canScan() } = {}) {
  const el = (tag, attrs = {}, ...kids) => {
    const e = d.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (k === "class") e.className = v;
      else e.setAttribute(k, v);
    }
    for (const k of kids) if (k != null) e.append(k);
    return e;
  };
  const input = el("input", { id: "og-vault-address", name: "vault", type: "url", inputmode: "url", autocomplete: "url", autocapitalize: "none", spellcheck: "false", required: "" });
  input.value = address;
  const out = el("p", { class: "og-connect-error", role: "alert" });
  out.textContent = error;
  const btn = el("button", { type: "submit", class: "primary" });
  btn.textContent = "Continue";
  const label = el("label", { for: "og-vault-address" });
  label.textContent = "Vault address";
  const title = el("h1", {});
  title.textContent = appName;
  let scanBtn = null;
  let video = null;
  let ctl = null;
  if (scan) {
    scanBtn = el("button", { type: "button", class: "og-connect-scan", title: "Scan QR code", "aria-label": "Scan QR code" });
    scanBtn.innerHTML = '<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 18h2v2h-2zM14 18h2M18 14h2"/></svg>';
    video = el("video", { class: "og-connect-video", muted: "", playsinline: "" });
    video.hidden = true;
  }
  const row = el("div", { class: "og-connect-row" }, input, scanBtn);
  const form = el("form", { class: "og-connect" },
    icon ? el("img", { class: "og-connect-icon", src: icon, alt: "", width: "72", height: "72" }) : null,
    title, label, row, video, out, btn);
  const go = async () => {
    btn.disabled = true;
    out.textContent = "";
    try {
      const link = parseConnectLink(input.value);
      if (link && /connect/i.test(input.value)) input.value = link.address;
      const r = await onContinue(input.value.trim());
      if (r && r.ok === false) out.textContent = PROBE_ERRORS[r.error] || "That address isn't an OG Vault.";
    } catch (e) {
      out.textContent = e && e.message ? e.message : String(e);
    } finally {
      btn.disabled = false;
    }
  };
  form.addEventListener("submit", (ev) => { ev.preventDefault(); return go(); });
  if (scanBtn) {
    scanBtn.addEventListener("click", async () => {
      if (ctl) { ctl.abort(); return; }
      ctl = new AbortController();
      out.textContent = "";
      video.hidden = false;
      let r;
      try { r = await scanQr({ video, signal: ctl.signal }); } finally { ctl = null; video.hidden = true; }
      if (r.text !== undefined) {
        const link = parseConnectLink(r.text);
        if (!link) { out.textContent = "That isn't a vault QR code."; return; }
        input.value = link.address;
        await go();
      } else if (r.error && r.error !== "cancelled") {
        out.textContent = SCAN_ERRORS[r.error] || SCAN_ERRORS.no_camera;
      }
    });
  }
  root.replaceChildren(form);
  return form;
}

const PROBE_ERRORS = {
  bad_address: "That isn't a web address.",
  timeout: "No vault answered at that address.",
  network: "No vault answered at that address.",
  not_a_vault: "That address isn't an OG Vault.",
};

const SCAN_ERRORS = {
  denied: "Camera access is blocked. Allow it for this site, or type the address.",
  no_camera: "No camera available. Type the address instead.",
  unsupported: "This browser can't scan. Type the address instead.",
};
