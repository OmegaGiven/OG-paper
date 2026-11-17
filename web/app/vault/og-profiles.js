// og-profiles: one OG app, many vaults (task #660). The spec is docs/PROFILES.md in og-vault.
// Plain ES module, no dependencies, works in browsers and Node >= 18. Kotlin apps mirror it
// (PROFILES.md section 8).

export const VERSION = 1;
export const PROBE_TIMEOUT_MS = 4000;
const NS = "og:v" + VERSION;
const LIST_KEY = NS + ":profiles";

// ------------------------------------------------------------------ addresses

/** `"vault.ts.net/x"` → `"https://vault.ts.net"`. Origins only; http only when written out. */
export function normalizeAddress(s) {
  let t = String(s ?? "").trim();
  if (!t) throw new Error("empty address");
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(t)) t = "https://" + t;
  const u = new URL(t);
  if (u.protocol !== "https:" && u.protocol !== "http:") throw new Error("not a web address: " + s);
  return u.origin;
}

// ------------------------------------------------------------------ probing

/** One address → `{ok:true, address, vault_id, name, url, ms}` or `{ok:false, address, error}`. */
export async function probe(address, { fetch: f = globalThis.fetch, timeoutMs = PROBE_TIMEOUT_MS } = {}) {
  let addr;
  try {
    addr = normalizeAddress(address);
  } catch (e) {
    return { ok: false, address: String(address), error: "bad_address" };
  }
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), timeoutMs);
  const t0 = Date.now();
  try {
    const r = await f(addr + "/.well-known/og-vault", {
      method: "GET", credentials: "omit", cache: "no-store", redirect: "error", signal: ctl.signal,
      headers: { accept: "application/json" },
    });
    if (!r.ok) return { ok: false, address: addr, error: "http_" + r.status };
    let j;
    try {
      j = await r.json();
    } catch {
      return { ok: false, address: addr, error: "not_json" };
    }
    if (!j || j.server !== "og-vault" || typeof j.vault_id !== "string" || !j.vault_id) {
      return { ok: false, address: addr, error: "not_a_vault" };
    }
    return { ok: true, address: addr, vault_id: j.vault_id, name: String(j.name ?? ""), url: j.url ? safeOrigin(j.url) : null, ms: Date.now() - t0 };
  } catch (e) {
    return { ok: false, address: addr, error: ctl.signal.aborted ? "timeout" : "network" };
  } finally {
    clearTimeout(timer);
  }
}

function safeOrigin(u) {
  try {
    return normalizeAddress(u);
  } catch {
    return null;
  }
}

/**
 * Probe every address of every profile in parallel. → `Map<vault_id, {reachable, address, name, url}>`
 * with an entry per profile; `address` = the first of the profile's addresses (in its order) that
 * answered with that profile's vault_id. Answers with another vault_id never count for it.
 */
export async function probeProfiles(profiles, opts = {}) {
  const addrs = [...new Set(profiles.flatMap((p) => p.addresses || []))];
  const results = new Map();
  await Promise.all(addrs.map(async (a) => results.set(a, await probe(a, opts))));
  const out = new Map();
  for (const p of profiles) {
    let hit = null;
    for (const a of p.addresses || []) {
      const r = results.get(a);
      if (r && r.ok && r.vault_id === p.vault_id) {
        hit = r;
        break;
      }
    }
    out.set(p.vault_id, hit ? { reachable: true, address: hit.address, name: hit.name, url: hit.url } : { reachable: false });
  }
  return out;
}

/** PROFILES.md section 4: current if it answers, else the most recently used that answers, else null. */
export function choose(profiles, reach, currentId) {
  if (currentId && reach.get(currentId)?.reachable) return currentId;
  const up = profiles.filter((p) => reach.get(p.vault_id)?.reachable);
  up.sort((a, b) => (b.last_used_at || 0) - (a.last_used_at || 0));
  return up.length ? up[0].vault_id : null;
}

// ------------------------------------------------------------------ keys

function part(s, what) {
  const v = String(s ?? "");
  if (!v || /[:\s/]/.test(v)) throw new Error("bad " + what + ": " + JSON.stringify(s));
  return v;
}

/** Prefix of every local key of one vault + user: `og:v1:<vault_id>:<user_id>:`. */
export function profilePrefix(vaultId, userId) {
  return `${NS}:${part(vaultId, "vault_id")}:${part(userId, "user id")}:`;
}

/** `og:v1:<vault_id>:<user_id>:<name>` for localStorage, DataStore and the like. */
export function dataKey(vaultId, userId, name) {
  if (!name) throw new Error("bad name");
  return profilePrefix(vaultId, userId) + name;
}

/** `og-<vault_id>-<user_id>-<name>` for IndexedDB databases, Room files, directories. */
export function dbName(vaultId, userId, name) {
  return `og-${part(vaultId, "vault_id")}-${part(userId, "user id")}-${part(name, "name")}`;
}

/** Throw before flushing a queue or cache of `vaultId` while `profile` is current. */
export function assertSameVault(profile, vaultId) {
  if (!profile || profile.vault_id !== vaultId) {
    throw new Error(`refusing to send data of vault ${vaultId} to vault ${profile ? profile.vault_id : "(none)"}`);
  }
}

// ------------------------------------------------------------------ storage

/** In-memory storage (tests, or apps that persist elsewhere). */
export function memoryStorage(init) {
  const m = new Map(init ? Object.entries(init) : []);
  return {
    async get(k) { return m.has(k) ? m.get(k) : null; },
    async set(k, v) { m.set(k, String(v)); },
    async remove(k) { m.delete(k); },
    async keys() { return [...m.keys()]; },
    _map: m,
  };
}

/** Async adapter over a Web Storage object (localStorage). */
export function localStorageAdapter(ls) {
  return {
    async get(k) { return ls.getItem(k); },
    async set(k, v) { ls.setItem(k, String(v)); },
    async remove(k) { ls.removeItem(k); },
    async keys() {
      const out = [];
      for (let i = 0; i < ls.length; i++) out.push(ls.key(i));
      return out;
    },
  };
}

const b64 = {
  enc(u8) {
    let s = "";
    for (const b of u8) s += String.fromCharCode(b);
    return btoa(s);
  },
  dec(s) {
    return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
  },
};

/** A non-extractable AES-GCM key kept in IndexedDB (`og-profiles-key`). Browsers only. */
export function indexedDbKey({ indexedDB: idb = globalThis.indexedDB, crypto: c = globalThis.crypto } = {}) {
  let cached = null;
  const open = () => new Promise((res, rej) => {
    const r = idb.open("og-profiles-key", 1);
    r.onupgradeneeded = () => r.result.createObjectStore("k");
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
  const tx = (db, mode, fn) => new Promise((res, rej) => {
    const t = db.transaction("k", mode);
    const q = fn(t.objectStore("k"));
    t.oncomplete = () => res(q.result);
    t.onerror = () => rej(t.error);
  });
  return async () => {
    if (cached) return cached;
    const db = await open();
    let key = await tx(db, "readonly", (s) => s.get("aes"));
    if (!key) {
      key = await c.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
      await tx(db, "readwrite", (s) => s.put(key, "aes"));
    }
    cached = key;
    return key;
  };
}

/**
 * Secret store for tokens: AES-GCM with a key from `getKey()` (default: non-extractable, in
 * IndexedDB); ciphertext in `storage`. The key name is bound as additional data, so a value
 * copied to another key does not decrypt.
 */
export function createWebSecretStore({ storage, getKey = indexedDbKey(), crypto: c = globalThis.crypto } = {}) {
  const enc = new TextEncoder();
  return {
    async get(k) {
      const v = await storage.get(k);
      if (!v) return null;
      const [iv, ct] = v.split(".");
      try {
        const pt = await c.subtle.decrypt({ name: "AES-GCM", iv: b64.dec(iv), additionalData: enc.encode(k) }, await getKey(), b64.dec(ct));
        return new TextDecoder().decode(pt);
      } catch {
        return null; // another key (storage wiped, other browser profile): sign in again
      }
    },
    async set(k, v) {
      const iv = c.getRandomValues(new Uint8Array(12));
      const ct = await c.subtle.encrypt({ name: "AES-GCM", iv, additionalData: enc.encode(k) }, await getKey(), enc.encode(String(v)));
      await storage.set(k, b64.enc(iv) + "." + b64.enc(new Uint8Array(ct)));
    },
    async remove(k) { await storage.remove(k); },
    async keys() { return storage.keys(); },
  };
}

// ------------------------------------------------------------------ profiles

const tokenKey = (vaultId) => `${NS}:${part(vaultId, "vault_id")}:tokens`;

/**
 * Saved profiles. Metadata (vault_id, name, addresses, user, last_used_at) in `storage`;
 * tokens only in `secrets`.
 */
export function createProfileStore(storage, secrets) {
  if (!secrets) throw new Error("tokens need a secret store");
  async function list() {
    const v = await storage.get(LIST_KEY);
    if (!v) return [];
    try {
      const a = JSON.parse(v);
      return Array.isArray(a) ? a : [];
    } catch {
      return [];
    }
  }
  async function write(a) {
    await storage.set(LIST_KEY, JSON.stringify(a));
  }
  async function get(vaultId) {
    return (await list()).find((p) => p.vault_id === vaultId) || null;
  }
  /** Insert or merge by vault_id (addresses are merged, new ones first). */
  async function save(p) {
    part(p.vault_id, "vault_id");
    const a = await list();
    const i = a.findIndex((x) => x.vault_id === p.vault_id);
    const old = i >= 0 ? a[i] : {};
    const addresses = [...new Set([...(p.addresses || []).map(normalizeAddress), ...(old.addresses || [])])];
    const next = { vault_id: p.vault_id, name: p.name ?? old.name ?? "", addresses, user: p.user !== undefined ? p.user : old.user ?? null,
                   last_used_at: p.last_used_at ?? old.last_used_at ?? 0 };
    if (i >= 0) a[i] = next;
    else a.push(next);
    await write(a);
    return next;
  }
  /**
   * `address` answered for this vault: move it to the front of the list; refresh the name; add
   * the vault's own `url` (from the probe) at the end if new.
   */
  async function addAddress(vaultId, address, { name, url } = {}) {
    const a = await list();
    const p = a.find((x) => x.vault_id === vaultId);
    if (!p) throw new Error("no profile " + vaultId);
    const addr = normalizeAddress(address);
    p.addresses = [addr, ...p.addresses.filter((x) => x !== addr)];
    if (url && !p.addresses.includes(normalizeAddress(url))) p.addresses.push(normalizeAddress(url));
    if (name) p.name = name;
    await write(a);
    return p;
  }
  async function touch(vaultId, now = Date.now()) {
    const a = await list();
    const p = a.find((x) => x.vault_id === vaultId);
    if (!p) throw new Error("no profile " + vaultId);
    p.last_used_at = now;
    await write(a);
    return p;
  }
  async function getTokens(vaultId) {
    const v = await secrets.get(tokenKey(vaultId));
    return v ? JSON.parse(v) : null;
  }
  async function setTokens(vaultId, t) {
    await secrets.set(tokenKey(vaultId), JSON.stringify(t));
  }
  /**
   * Forget a profile on this device: tokens, every local key of vault_id + user, the profile.
   * `clearExtra(prefix, dbPrefix)` lets the app drop IndexedDB databases/files too. The server
   * logout (`POST /auth/logout`) is the caller's, before this.
   */
  async function signOut(vaultId, { clearExtra } = {}) {
    const p = await get(vaultId);
    await secrets.remove(tokenKey(vaultId));
    if (p && p.user && p.user.id) {
      const prefix = profilePrefix(vaultId, p.user.id);
      for (const k of await storage.keys()) if (k.startsWith(prefix)) await storage.remove(k);
      if (clearExtra) await clearExtra(prefix, `og-${vaultId}-${p.user.id}-`);
    }
    await write((await list()).filter((x) => x.vault_id !== vaultId));
  }
  return { list, get, save, addAddress, touch, getTokens, setTokens, signOut };
}

// ------------------------------------------------------------------ switching

/**
 * The auto-switch loop (PROFILES.md section 4).
 *  - `isBusy()` → true while unsaved edits, uploads, recordings or a queue flush run.
 *  - `confirmSwitch(from, to)` → true to switch anyway while busy (ask the user); default: no.
 *  - `onSwitch(profile, {address, from})` swaps the app over (API base, tokens, stores).
 *  - `onReach(map)` gets every probe result (for the dots in the account menu).
 */
export function createSwitcher({ store, fetch: f = globalThis.fetch, timeoutMs, isBusy = () => false, confirmSwitch = async () => false,
                                 onSwitch = () => {}, onReach = () => {}, now = () => Date.now() } = {}) {
  let current = null; // vault_id
  let address = null;
  let running = null;
  let lastReach = new Map();

  async function check(reason = "manual") {
    if (running) return running;
    running = (async () => {
      const profiles = await store.list();
      if (!profiles.length) return { switched: false, current, reason, reach: new Map() };
      if (!current) {
        // start: the last used one is current until a probe says otherwise
        current = [...profiles].sort((a, b) => (b.last_used_at || 0) - (a.last_used_at || 0))[0].vault_id;
      }
      const reach = await probeProfiles(profiles, { fetch: f, timeoutMs });
      lastReach = reach;
      onReach(reach);
      for (const p of profiles) {
        const r = reach.get(p.vault_id);
        if (r.reachable) await store.addAddress(p.vault_id, r.address, { name: r.name, url: r.url });
      }
      const pick = choose(await store.list(), reach, current);
      if (!pick) return { switched: false, offline: true, current, reason, reach };
      const to = await store.get(pick);
      if (pick === current) {
        const first = !address;
        address = reach.get(pick).address;
        if (first) {
          await store.touch(pick, now());
          await onSwitch(to, { address, from: null, reason });
        }
        return { switched: first, current, address, reason, reach };
      }
      if (await isBusy()) {
        const from = await store.get(current);
        if (!(await confirmSwitch(from, to))) return { switched: false, deferred: true, current, candidate: pick, reason, reach };
      }
      const from = current;
      current = pick;
      address = reach.get(pick).address;
      await store.touch(pick, now());
      await onSwitch(await store.get(pick), { address, from, reason });
      return { switched: true, current, address, from, reason, reach };
    })();
    try {
      return await running;
    } finally {
      running = null;
    }
  }

  /** Probe an address and save it: a new profile, or another address of a saved one. */
  async function addVault(addr) {
    const r = await probe(addr, { fetch: f, timeoutMs });
    if (!r.ok) return r;
    const known = await store.get(r.vault_id);
    if (!known) await store.save({ vault_id: r.vault_id, name: r.name, addresses: [r.address] });
    const profile = await store.addAddress(r.vault_id, r.address, { name: r.name, url: r.url });
    return { ...r, profile, existing: !!known };
  }

  /** Switch by hand (account menu); same busy rule. */
  async function switchTo(vaultId) {
    const to = await store.get(vaultId);
    if (!to) throw new Error("no profile " + vaultId);
    if (vaultId === current) return { switched: false, current };
    const reach = await probeProfiles([to], { fetch: f, timeoutMs });
    const r = reach.get(vaultId);
    if (!r.reachable) return { switched: false, unreachable: true, current };
    if (await isBusy()) {
      if (!(await confirmSwitch(current ? await store.get(current) : null, to))) return { switched: false, deferred: true, current };
    }
    const from = current;
    current = vaultId;
    address = r.address;
    await store.addAddress(vaultId, r.address, { name: r.name, url: r.url });
    await store.touch(vaultId, now());
    await onSwitch(await store.get(vaultId), { address, from, reason: "manual" });
    return { switched: true, current, address, from };
  }

  /** A real request failed at the network level (not an HTTP error): probe again. */
  function reportFailure() {
    return check("failure");
  }

  /** Web triggers: online, connection change, visible again after > 30 s, 60 s while offline. */
  function watch(win = globalThis.window) {
    if (!win) return () => {};
    let hiddenAt = 0;
    let offlineTimer = null;
    const run = (why) => check(why).then((r) => {
      clearInterval(offlineTimer);
      offlineTimer = r.offline ? setInterval(() => { if (!win.document || win.document.visibilityState !== "hidden") check("retry"); }, 60000) : null;
      return r;
    });
    const online = () => run("network");
    const vis = () => {
      if (win.document.visibilityState === "hidden") hiddenAt = Date.now();
      else if (hiddenAt && Date.now() - hiddenAt > 30000) run("visible");
    };
    const conn = win.navigator && win.navigator.connection;
    win.addEventListener("online", online);
    win.document && win.document.addEventListener("visibilitychange", vis);
    conn && conn.addEventListener && conn.addEventListener("change", online);
    run("start");
    return () => {
      clearInterval(offlineTimer);
      win.removeEventListener("online", online);
      win.document && win.document.removeEventListener("visibilitychange", vis);
      conn && conn.removeEventListener && conn.removeEventListener("change", online);
    };
  }

  return {
    check, addVault, switchTo, reportFailure, watch,
    get current() { return current; },
    get address() { return address; },
    get reach() { return lastReach; },
  };
}
