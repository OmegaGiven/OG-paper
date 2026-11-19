// OG Paper web shell: pages kept in an OG Vault (vault mode). The other
// account kind next to OG Studio (og-hosted.js): the person enters a vault
// address (or scans its QR code), signs in on the vault's own page (PKCE, the
// app never sees the password) and comes back here. The vault speaks the
// hosted API and live protocol (og-vault docs/PAPER.md), so everything in
// og-hosted.js works on it through VaultProvider; only the base address,
// bearer tokens and one-use socket tickets differ.
//
// Profiles follow og-vault docs/PROFILES.md: one per vault, one active at a
// time, switchable; tokens encrypted with a non-extractable key; local data
// keyed by vault and person. The shared modules in ./vault/ are copies of
// og-vault's clients/og-profiles (keep them in step).

import { createProfileStore, createWebSecretStore, localStorageAdapter, normalizeAddress, probe, probeProfiles, dataKey } from './vault/og-profiles.js';
import { startLogin, finishLogin, parseConnectLink, canScan, scanQr } from './vault/og-app-login.js';
import { HostedProvider } from './og-hosted.js';

export const CLIENT_ID = 'og-paper';
/** Which account this browser uses: 'studio' or 'vault:<vault_id>'. App-wide. */
const BACKEND = 'og-paper-backend';
const EARLY_MS = 60_000;

// Keys are prefixed so OG Paper's profiles and tokens never mix with another
// OG app served from the same origin (the vault serves several).
function prefixed(ls, pre) {
  const base = localStorageAdapter(ls);
  return {
    get: k => base.get(pre + k), set: (k, v) => base.set(pre + k, v), remove: k => base.remove(pre + k),
    async keys() { return (await base.keys()).filter(k => k?.startsWith(pre)).map(k => k.slice(pre.length)); },
  };
}
let store = null;
export function profiles() {
  if (!store) {
    const s = prefixed(localStorage, 'og-paper:');
    store = createProfileStore(s, createWebSecretStore({ storage: s }));
  }
  return store;
}

export const backendChoice = () => { try { return localStorage.getItem(BACKEND) || ''; } catch (e) { return ''; } };
export function setBackend(v) { try { if (v) localStorage.setItem(BACKEND, v); else localStorage.removeItem(BACKEND); } catch (e) {} }

/**
 * A signed-in vault profile: where it is, who, and fresh access tokens.
 * Tokens go only to an address that answered with this vault's id.
 */
export class VaultSession {
  constructor(profile, tokens) {
    this.profile = profile;
    this.tokens = tokens;
    this.address = profile.addresses[0];
    this.refreshing = null;
  }
  get id() { return this.profile.vault_id; }
  get label() { return `${this.profile.user?.name || 'me'} @ ${this.profile.name || 'vault'}`; }
  /** Probe the saved addresses; the first that answers as this vault is used. */
  async reach() {
    const r = (await probeProfiles([this.profile])).get(this.id);
    if (r?.reachable) {
      this.address = r.address;
      this.profile = await profiles().addAddress(this.id, r.address, { name: r.name, url: r.url });
    }
    return !!r?.reachable;
  }
  async token(force = false) {
    // access_expires: unix seconds (vaultd), or ms / a date string.
    const x = this.tokens.access_expires;
    const exp = typeof x === 'number' ? (x < 1e12 ? x * 1000 : x) : x ? Date.parse(x) : NaN;
    if (!force && (!(exp > 0) || exp - Date.now() > EARLY_MS)) return this.tokens.access_token;
    if (!this.refreshing) {
      this.refreshing = (async () => {
        const r = await fetch(this.address + '/auth/refresh', {
          method: 'POST', credentials: 'omit', headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ refresh_token: this.tokens.refresh_token }),
        });
        const t = await r.json().catch(() => ({}));
        if (!r.ok || !t.access_token) {
          const e = new Error('Your vault session ended: sign in again');
          e.status = r.status >= 500 ? r.status : 401;
          throw e;
        }
        this.tokens = { access_token: t.access_token, access_expires: t.access_expires, refresh_token: t.refresh_token || this.tokens.refresh_token };
        await profiles().setTokens(this.id, this.tokens);
        return this.tokens.access_token;
      })().finally(() => { this.refreshing = null; });
    }
    return this.refreshing;
  }
  /** fetch() with the bearer token; one refresh and retry on 401. */
  async fetch(path, init = {}) {
    for (const retry of [false, true]) {
      const tok = await this.token(retry);
      const r = await fetch(this.address + path, { ...init, credentials: 'omit', headers: { ...(init.headers || {}), authorization: 'Bearer ' + tok } });
      if (r.status !== 401 || retry) return r;
    }
  }
  /** Data keys for this vault and person (PROFILES.md section 3). */
  key(name) { return dataKey(this.id, this.profile.user?.id || 'me', name); }
}

/** The active vault session, or null (no vault chosen, or signed out). */
export async function activeSession() {
  const b = backendChoice();
  if (!b.startsWith('vault:')) return null;
  const id = b.slice(6);
  const p = await profiles().get(id);
  const t = p && await profiles().getTokens(id);
  return p && t ? new VaultSession(p, t) : null;
}

/** Loopback, private, link-local, tailnet (100.64/10) or a bare / .local-style name. */
export function privateHost(h) {
  h = String(h).toLowerCase().replace(/^\[|\]$/g, '');
  const m = h.match(/^(\d+)\.(\d+)\.(\d+)\.(\d+)$/);
  if (m) { const [a, b] = [+m[1], +m[2]]; return a === 10 || a === 127 || (a === 172 && b >= 16 && b < 32) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b < 128); }
  if (h.includes(':')) return h === '::1' || /^f[cd]/.test(h) || /^fe[89ab]/.test(h);
  return !h.includes('.') || /\.(local|lan|home|internal|localdomain)$/.test(h);
}

/** Sign-in at a vault: probe it, then off to its sign-in page (this page comes back). */
export async function signInAt(text) {
  const c = parseConnectLink(text);
  if (!c) return { ok: false, error: 'Enter a vault address' };
  // Tokens go to this address: plain http only on a private network.
  if (/^http:/i.test(c.address) && !privateHost(new URL(c.address).hostname)) return { ok: false, error: 'Use https for a vault on the internet' };
  const r = await startLogin({
    address: c.address, clientId: CLIENT_ID, redirectUri: location.origin + location.pathname,
    device: `OG Paper · ${deviceName()}`,
  });
  if (!r.ok) return { ok: false, error: r.error === 'not_a_vault' ? 'That address is not an OG Vault' : 'That vault did not answer' };
  if (c.vault_id && r.vault_id !== c.vault_id) return { ok: false, error: 'That address answers as another vault' };
  return r;
}

/** Back from a vault's sign-in page? Saves the profile and makes it active. */
export async function finishVaultSignIn() {
  const r = await finishLogin({ store: profiles() });
  if (!r) return null;
  if (r.error) return r;
  setBackend('vault:' + r.vault_id);
  return r;
}

/** Sign out of one vault profile here (its tokens and keyed data go). */
export async function signOutVault(session) {
  try { await session.fetch('/auth/logout', { method: 'POST' }); } catch (e) { /* offline: tokens still go */ }
  await profiles().signOut(session.id);
  if (backendChoice() === 'vault:' + session.id) setBackend('');
}

/** Every saved profile with whether it answers now: [{profile, reachable}]. */
export async function listProfiles(probeNow = true) {
  const list = await profiles().list();
  if (!probeNow) return list.map(p => ({ profile: p, reachable: null }));
  const reach = await probeProfiles(list);
  return list.map(p => ({ profile: p, reachable: !!reach.get(p.vault_id)?.reachable }));
}

function deviceName() {
  const ua = navigator.userAgent;
  return /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iPhone or iPad' : /Mac/.test(ua) ? 'Mac' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : 'Browser';
}

export { canScan, scanQr, normalizeAddress, probe };

/**
 * The vault's paper API (PAPER.md) behind HostedProvider's calls. Sharing
 * is with vault people (not usernames or links); links, offers, chat,
 * history and reports don't exist there.
 */
export class VaultProvider extends HostedProvider {
  constructor(session, app, api = '/api/v1/paper') {
    super(session.address + api, app);
    this.kind = 'vault';
    this.session = session;
    this.path = api;
    this.features = { links: false, offers: false, chat: false, history: false, report: false, transfer: false, coOwners: false };
  }
  async call(method, path, body) {
    const r = await this.session.fetch(this.path + path, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let data = {};
    try { data = await r.json(); } catch (e) { /* empty */ }
    if (!r.ok) { const e = new Error(data.error || data.message || `Request failed (${r.status})`); e.status = r.status; e.data = data; throw e; }
    for (const o of [data, data.open, data.from]) if (o && typeof o.link === 'string') o.link = this.here(o.link);
    return data;
  }
  /**
   * A relay link at the address this vault answered on: the vault names its
   * own public address, which another network (LAN, tailnet) may not reach.
   */
  here(link) {
    const ws = this.session.address.replace(/^http/, 'ws');
    const i = link.indexOf(this.path + '/pages/');
    return i > 0 && /^wss?:\/\//.test(link) ? ws + link.slice(i) : i === 0 ? ws + link : link;
  }
  async me() {
    const m = await this.call('GET', '/me');
    const u = this.session.profile.user || {};
    const p = m.person && typeof m.person === 'object' ? m.person : { id: m.person };
    return { ...m, user: { id: uid(p.id || u.id || ''), name: m.name || p.name || u.name || '' }, linkSharing: false };
  }
  // Person ids come with or without dashes: one form, so owners are recognised.
  async list() { const l = await this.call('GET', '/pages'); for (const k of ['mine', 'shared', 'trash']) for (const p of l[k] || []) if (p.owner) p.owner.id = uid(p.owner.id); return l; }
  async detail(id) { const d = await this.call('GET', `/pages/${id}`); if (d.owner) d.owner.id = uid(d.owner.id); return d; }
  /** People of this vault to share with: [{id, name}] (not me). */
  async people() {
    const r = await this.session.fetch('/api/v1/people');
    const j = await r.json().catch(() => ({}));
    if (!r.ok) return [];
    return (j.people || []).filter(p => p.account_id && p.id !== j.me).map(p => ({ id: p.id, name: p.name || p.username }));
  }
  share(page, who, role) { return this.call('POST', `/pages/${page.id}/members`, { person: who, role }); }
  setRole(page, uid, role) { return this.call('PATCH', `/pages/${page.id}/members/${memberPath(uid)}`, { role }); }
  unshare(page, uid) { return this.call('DELETE', `/pages/${page.id}/members/${memberPath(uid)}`); }
  async leave(page, me) {
    const r = await this.call('DELETE', `/pages/${encodeURIComponent(page.id)}/members/${memberPath(me)}`);
    await this.drop(page.id);
    return r;
  }
  /** The live socket's address for a page link: a fresh one-use ticket each dial. */
  async dialUrl(url) {
    const m = url.match(/\/pages\/([0-9a-f]{32})\/relay\b/);
    if (!m || !this.isRoom(url)) return url;
    const r = await this.call('POST', `/pages/${m[1]}/open`);
    // The app dials the room without the link's key (rooms authorize by role);
    // the vault's own link has it, which it also accepts.
    const at = r.link || this.here(url);
    return r.ticket ? `${at}${at.includes('?') ? '&' : '?'}ticket=${encodeURIComponent(r.ticket)}` : at;
  }
  /** A room of this vault (at any of its addresses: links are kept from earlier). */
  isRoom(url) {
    if (!/^wss?:\/\//.test(url) || !url.includes(this.path + '/pages/')) return false;
    const origin = url.replace(/^ws/, 'http').split('/').slice(0, 3).join('/');
    return [this.session.address, ...this.session.profile.addresses].includes(origin) || url.startsWith(this.session.address.replace(/^http/, 'ws'));
  }
}
const uid = x => String(x || '').replace(/-/g, '').toLowerCase();
// Members: 'person:<uuid>' / 'team:<uuid>' (or a bare person uuid).
const memberPath = uid => { const [k, id] = String(uid).includes(':') ? String(uid).split(':') : ['person', uid]; return `${k}/${encodeURIComponent(id)}`; };
