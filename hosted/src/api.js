// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// The hosted OG Paper API: page lists, sharing, quotas, reports, and the
// door to each page's room. Mounted by the host site at a prefix
// (default /paper/api). Who is signed in comes from the host site through
// an auth adapter (see index.js); there is no user system here.

import { configFrom } from './config.js';
import { account } from './quota.js';
import { cleanName, cleanText } from './chat.js';
import { ensureSchema } from './schema.js';
import { fileReport, setReportHook } from './report.js';

const ROLES = ['viewer', 'editor', 'owner'];
const rank = r => ROLES.indexOf(r);
const HEX32 = /^[0-9a-f]{32}$/;
const HEX64 = /^[0-9a-f]{64}$/;
const DAY = 86_400_000;
const EDIT_KEY = /^e[A-Za-z0-9_-]{40,50}$/;
const VIEW_KEY = /^v[A-Za-z0-9_-]{80,90}$/;

const enc = new TextEncoder();
const b64url = buf => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const randomHex = n => [...crypto.getRandomValues(new Uint8Array(n))].map(b => b.toString(16).padStart(2, '0')).join('');
const sha256 = async s => b64url(await crypto.subtle.digest('SHA-256', enc.encode(s)));

class HttpError extends Error {
  constructor(status, msg, extra = {}) { super(msg); this.status = status; this.extra = extra; }
}
const fail = (status, msg, extra) => { throw new HttpError(status, msg, extra); };
const json = (data, status = 200) => new Response(JSON.stringify(data), { status, headers: { 'content-type': 'application/json', 'cache-control': 'no-store' } });

/** A function that sends `op` to page `id`'s room. */
function roomCaller(rooms) {
  return (id, op, body, headers = {}) => rooms.get(rooms.idFromName(id))
    .fetch(new Request('https://room/' + op, { method: 'POST', body: JSON.stringify(body || {}), headers: { 'x-paper-page': id, ...headers } }));
}

/** Delete a page for good: its room (content, chat, R2) and its directory rows. */
async function deletePageRows(db, roomCall, p) {
  const r = await (await roomCall(p.id, 'destroy', {}, { 'x-paper-owner': p.owner_id })).json().catch(() => ({}));
  await db.batch([
    db.prepare('DELETE FROM paper_members WHERE page_id = ?1').bind(p.id),
    db.prepare('DELETE FROM paper_links WHERE page_id = ?1').bind(p.id),
    db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(p.id),
    db.prepare('DELETE FROM paper_chatters WHERE page_id = ?1').bind(p.id),
    db.prepare('DELETE FROM paper_pages WHERE id = ?1').bind(p.id),
  ]);
  return { ok: true, freed: r.bytes || 0 };
}

/** Turn links off: out go the people they brought in, and anyone on them without an account. */
async function dropLinkRows(db, roomCall, p, ids, why) {
  for (const id of ids) {
    const via = (await db.prepare('SELECT user_id FROM paper_members WHERE page_id = ?1 AND via_link = ?2').bind(p.id, id).all()).results;
    await db.batch([
      db.prepare('DELETE FROM paper_links WHERE page_id = ?1 AND id = ?2').bind(p.id, id),
      db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND via_link = ?2').bind(p.id, id),
    ]);
    const out = what => roomCall(p.id, 'kick', what).catch(() => {});
    for (const v of via) await out({ user: v.user_id, why });
    await out({ via: id, why });
  }
}

/**
 * Delete everything OG Paper keeps about an account (the site calls this
 * from its own "Delete account"; see HOSTED.md): every page it owns, the
 * trash too (rooms, chat, R2); its places on other pages, the links it made
 * there and the people who came in by them; offers from or to it; its chat
 * messages and reactions on other pages; its blocks; its quota row. Reports
 * it made or that are about it stay for the admin, without its id.
 */
export async function deleteAccount(env, uid, { roomBinding = 'PAPER_ROOMS' } = {}) {
  uid = String(uid);
  const db = env.DB;
  await ensureSchema(db);
  const roomCall = roomCaller(env[roomBinding]);
  const out = { pages: 0, left: 0, links: 0, chats: 0 };
  for (const p of (await db.prepare('SELECT * FROM paper_pages WHERE owner_id = ?1').bind(uid).all()).results) {
    await deletePageRows(db, roomCall, p);
    out.pages++;
  }
  for (const m of (await db.prepare('SELECT page_id FROM paper_members WHERE user_id = ?1').bind(uid).all()).results) {
    const p = await db.prepare('SELECT * FROM paper_pages WHERE id = ?1').bind(m.page_id).first();
    if (!p) continue;
    const links = (await db.prepare('SELECT id FROM paper_links WHERE page_id = ?1 AND created_by = ?2').bind(p.id, uid).all()).results.map(l => l.id);
    await dropLinkRows(db, roomCall, p, links, 'The person who made that link deleted their account');
    out.links += links.length;
    await db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).run();
    await roomCall(p.id, 'kick', { user: uid, why: 'This account was deleted' }).catch(() => {});
    out.left++;
  }
  for (const c of (await db.prepare('SELECT page_id FROM paper_chatters WHERE user_id = ?1').bind(uid).all()).results) {
    const p = await db.prepare('SELECT owner_id FROM paper_pages WHERE id = ?1').bind(c.page_id).first();
    if (!p) continue;
    await roomCall(c.page_id, 'forget-user', { user: uid }, { 'x-paper-owner': p.owner_id }).catch(e => console.error('og-paper delete account', e));
    out.chats++;
  }
  await db.batch([
    db.prepare('DELETE FROM paper_links WHERE created_by = ?1').bind(uid),
    db.prepare('DELETE FROM paper_transfers WHERE from_id = ?1 OR to_id = ?1').bind(uid),
    db.prepare('DELETE FROM paper_chatters WHERE user_id = ?1').bind(uid),
    db.prepare('DELETE FROM paper_blocks WHERE user_id = ?1 OR blocked_id = ?1').bind(uid),
    db.prepare('DELETE FROM paper_members WHERE user_id = ?1').bind(uid),
    db.prepare('UPDATE paper_reports SET reporter_id = NULL WHERE reporter_id = ?1').bind(uid),
    db.prepare('UPDATE paper_reports SET subject_id = NULL WHERE subject_id = ?1').bind(uid),
    db.prepare('DELETE FROM paper_accounts WHERE user_id = ?1').bind(uid),
  ]);
  return out;
}

export function createApi({ auth, config = {}, roomBinding = 'PAPER_ROOMS', onReport } = {}) {
  if (!auth || typeof auth.user !== 'function') throw new Error('og-paper hosted: an auth adapter with user(req, env) is needed');
  // Rooms file reports too (the chat's word filter): they use the same hook.
  setReportHook(onReport);

  return async function handle(req, env, ctx) {
    const cfg = configFrom(env, config);
    const url = new URL(req.url);
    if (!url.pathname.startsWith(cfg.prefix + '/') && url.pathname !== cfg.prefix) return null;
    const path = url.pathname.slice(cfg.prefix.length) || '/';
    const db = env.DB;
    const rooms = env[roomBinding];
    const room = id => rooms.get(rooms.idFromName(id));
    const roomCall = roomCaller(rooms);
    const deletePage = p => deletePageRows(db, roomCall, p);
    const dropLinks = (p, ids, why) => dropLinkRows(db, roomCall, p, ids, why);
    const waitUntil = p => ctx?.waitUntil?.(p);
    const limited = async (kind, key) => typeof auth.limit === 'function' && await auth.limit(env, kind, key);
    const wsOrigin = (cfg.origin || url.origin).replace(/^http/, 'ws');

    try {
      // Same-site only for anything that changes things or opens a room (the
      // session is a cookie: no other site may use it).
      const origin = req.headers.get('Origin');
      const allowed = [url.origin, ...(cfg.origins || [])];
      if ((req.method !== 'GET' || req.headers.get('Upgrade') === 'websocket') && origin && !allowed.includes(origin)) fail(403, 'Bad origin');
      if (req.method !== 'GET' && req.headers.get('Sec-Fetch-Site') && !['same-origin', 'none'].includes(req.headers.get('Sec-Fetch-Site'))) fail(403, 'Bad origin');
      // Every browser sends one of the two on a write; a write with neither isn't from this site's page.
      if (req.method !== 'GET' && !origin && !req.headers.get('Sec-Fetch-Site')) fail(403, 'Bad origin');

      await ensureSchema(db);
      const user = await auth.user(req, env);
      // Names come from the host site: one line, no markup, whatever it allows.
      if (user) { user.id = String(user.id); user.name = cleanName(user.name, 40) || null; }
      const ip = req.headers.get('CF-Connecting-IP') || 'local';
      if (await limited(req.method === 'GET' ? 'api' : 'write', user ? 'u' + user.id : 'ip' + ip)) fail(429, 'Too many requests. Please slow down and try again in a minute.');
      const isAdmin = !!(user && typeof auth.isAdmin === 'function' && await auth.isAdmin(user, env));
      // An account the admin blocked can still see and download its pages,
      // but not change anything, open a live page or chat.
      if (user && !isAdmin && (req.method !== 'GET' || req.headers.get('Upgrade') === 'websocket')) {
        const a = await db.prepare('SELECT blocked FROM paper_accounts WHERE user_id = ?1').bind(user.id).first();
        if (a?.blocked) fail(403, 'This account is blocked. You can still see and download your pages, but not change them or chat.');
      }
      const body = async () => {
        if (req.method === 'GET') return {};
        const t = await req.text();
        if (t.length > 16 * 1024) fail(413, 'Too big');
        try { return t ? JSON.parse(t) : {}; } catch (e) { fail(400, 'Bad request'); }
      };
      const need = () => user || fail(401, 'Sign in to use your pages');
      const names = async ids => {
        ids = [...new Set(ids.filter(Boolean))];
        if (!ids.length || typeof auth.names !== 'function') return {};
        const got = (await auth.names(env, ids)) || {};
        return Object.fromEntries(Object.entries(got).map(([k, v]) => [k, v == null ? null : cleanName(v, 40) || null]));
      };

      // A page in the trash is gone for everyone (only its owner's Trash and
      // the admin reach it, with trashed: true).
      const page = async (id, { trashed = false } = {}) => {
        if (!HEX32.test(id)) fail(404, 'No such page');
        const p = await db.prepare('SELECT * FROM paper_pages WHERE id = ?1').bind(id).first();
        if (!p || (p.deleted_at != null && !trashed)) fail(404, 'No such page');
        return p;
      };
      const roleOf = async (p, u) => {
        if (!u) return null;
        const m = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, u.id).first();
        return m?.role || null;
      };
      // A page and the caller's role on it, if at least `min`. No access looks
      // the same as no page (nobody learns which page ids exist).
      const access = async (id, min = 'viewer') => {
        const p = await page(id);
        let role = await roleOf(p, user);
        if (p.blocked && !isAdmin) fail(451, 'This page was taken down');
        // The admin may look at any page (view only) to judge a report.
        if (!role && isAdmin && min === 'viewer') role = 'viewer';
        if (!role || rank(role) < rank(min)) {
          if (role) fail(403, min === 'owner' ? 'Only the page\'s owners can do that' : 'You can only view this page');
          fail(404, 'No such page');
        }
        return { p, role };
      };
      const card = (p, role, owners = {}) => ({
        id: p.id, name: p.name, canvas: p.canvas, role, bytes: p.bytes,
        owner: { id: p.owner_id, name: owners[p.owner_id] || null },
        created: p.created_at, updated: p.updated_at,
      });
      const openInfo = (p, role, relayPath) => {
        const key = rank(role) >= rank('editor') ? p.edit_key : p.view_key;
        return {
          page: p.id, name: p.name, canvas: p.canvas, role,
          link: `${wsOrigin}${cfg.prefix}${relayPath || `/pages/${p.id}/relay`}?k=${key}&c=${p.canvas}`,
        };
      };
      const usage = async () => {
        const a = await account(db, user.id, cfg.freeQuota);
        const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_pages WHERE owner_id = ?1').bind(user.id).first();
        return { quota: a.quota, used: a.used, pages: n.n, maxPages: cfg.maxPages, blocked: !!a.blocked };
      };
      const newPage = async (b, name) => {
        const canvas = String(b.canvas || '').toLowerCase();
        if (!HEX32.test(canvas) || !EDIT_KEY.test(b.edit || '') || !VIEW_KEY.test(b.view || '')) fail(400, 'Bad page keys');
        if (await limited('create', 'u' + user.id)) fail(429, 'Too many new pages at once. Try again in a minute.');
        const u = await usage();
        if (u.blocked) fail(403, 'This account cannot keep pages');
        if (u.pages >= cfg.maxPages) fail(409, `You have ${u.pages} pages, the most an account can have. Delete some to make new ones.`, { full: 'pages' });
        if (u.used >= u.quota) fail(409, 'Your storage is full. Export or delete pages to free space.', { full: 'bytes', ...u });
        const id = randomHex(16), now = Date.now();
        name = cleanName(name, cfg.maxName) || 'Untitled';
        await db.batch([
          db.prepare('INSERT INTO paper_pages (id, owner_id, name, canvas, edit_key, view_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)')
            .bind(id, user.id, name, canvas, b.edit, b.view, now),
          db.prepare('INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, \'owner\', ?2, ?3)').bind(id, user.id, now),
        ]);
        // The browser made the keys, so it can name the room now: pinned at
        // creation, nobody joining first can pin another.
        if (HEX64.test(String(b.room || ''))) await roomCall(id, 'pin', { room: String(b.room) }, { 'x-paper-owner': user.id });
        return page(id);
      };
      const kick = (id, what) => roomCall(id, 'kick', what).catch(() => {});

      let m;
      // ---- the signed-in account ------------------------------------------------
      if (path === '/me' && req.method === 'GET') {
        if (!user) return json({ user: null, linkSharing: cfg.linkSharing });
        return json({ user: { id: user.id, name: user.name || null }, admin: isAdmin, ...(await usage()), maxOpBytes: cfg.maxOpBytes, linkSharing: cfg.linkSharing, chat: !!cfg.chat, upgrade: cfg.upgradeUrl || null });
      }

      // ---- page lists -------------------------------------------------------------
      if (path === '/pages' && req.method === 'GET') {
        need();
        // Pages past their days in the trash go now (their room's alarm does
        // it too; this catches a room that never woke).
        const now = Date.now();
        // (Not one the admin took down: that waits for the admin.)
        for (const old of (await db.prepare('SELECT * FROM paper_pages WHERE owner_id = ?1 AND deleted_at < ?2 AND blocked = 0').bind(user.id, now - cfg.trashDays * DAY).all()).results) {
          try { await deletePage(old); } catch (e) { console.error('og-paper purge', old.id, e); }
        }
        await db.prepare('DELETE FROM paper_transfers WHERE created_at < ?1').bind(now - cfg.offerDays * DAY).run();
        const all = (await db.prepare('SELECT p.* FROM paper_pages p WHERE p.owner_id = ?1 ORDER BY p.updated_at DESC').bind(user.id).all()).results;
        const mine = all.filter(p => p.deleted_at == null);
        const trash = all.filter(p => p.deleted_at != null).sort((a, b) => b.deleted_at - a.deleted_at);
        const shared = (await db.prepare(`SELECT p.*, m.role AS my_role FROM paper_members m JOIN paper_pages p ON p.id = m.page_id
          WHERE m.user_id = ?1 AND p.owner_id != ?1 AND p.blocked = 0 AND p.deleted_at IS NULL ORDER BY p.updated_at DESC`).bind(user.id).all()).results;
        // Give-away offers: to me (accept or decline) and from me (waiting).
        const toMe = (await db.prepare(`SELECT t.*, p.name, p.bytes FROM paper_transfers t JOIN paper_pages p ON p.id = t.page_id
          WHERE t.to_id = ?1 AND p.owner_id = t.from_id AND p.deleted_at IS NULL ORDER BY t.created_at DESC`).bind(user.id).all()).results;
        const fromMe = (await db.prepare('SELECT page_id, to_id, created_at FROM paper_transfers WHERE from_id = ?1').bind(user.id).all()).results;
        const who = await names([...shared.map(p => p.owner_id), ...toMe.map(t => t.from_id), ...fromMe.map(t => t.to_id)]);
        const waiting = Object.fromEntries(fromMe.map(t => [t.page_id, { to: { id: t.to_id, name: who[t.to_id] || null }, created: t.created_at }]));
        return json({
          mine: mine.map(p => ({ ...card(p, 'owner'), blocked: !!p.blocked, offer: waiting[p.id] || null })),
          shared: shared.map(p => card(p, p.my_role, who)),
          trash: trash.map(p => ({ ...card(p, 'owner'), deleted: p.deleted_at, purge: p.deleted_at + cfg.trashDays * DAY })),
          offers: toMe.map(t => ({ page: { id: t.page_id, name: t.name, bytes: t.bytes }, from: { id: t.from_id, name: who[t.from_id] || null }, leave: !!t.leave, created: t.created_at })),
          usage: await usage(),
        });
      }
      if (path === '/pages' && req.method === 'POST') {
        need();
        const b = await body();
        const p = await newPage(b, b.name);
        return json({ page: card(p, 'owner'), open: openInfo(p, 'owner') }, 201);
      }

      // ---- one page -----------------------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})$/))) {
        need();
        if (req.method === 'GET') {
          const { p, role } = await access(m[1]);
          const out = card(p, role, await names([p.owner_id]));
          if (rank(role) >= rank('owner')) {
            const members = (await db.prepare('SELECT user_id, role, via_link, added_at FROM paper_members WHERE page_id = ?1 ORDER BY added_at').bind(p.id).all()).results;
            const who = await names(members.map(x => x.user_id));
            out.members = members.map(x => ({ id: x.user_id, name: who[x.user_id] || null, role: x.role, viaLink: x.via_link || null, billing: x.user_id === p.owner_id }));
            out.links = (await db.prepare('SELECT id, role, created_by, created_at FROM paper_links WHERE page_id = ?1 ORDER BY created_at').bind(p.id).all()).results
              .map(l => ({ id: l.id, role: l.role, created: l.created_at, by: { id: l.created_by, name: who[l.created_by] || null } }));
            const t = await db.prepare('SELECT to_id, created_at FROM paper_transfers WHERE page_id = ?1').bind(p.id).first();
            out.offer = t ? { to: { id: t.to_id, name: (await names([t.to_id]))[t.to_id] || null }, created: t.created_at } : null;
            // The page's chat settings (owners change them: PATCH /pages/:id/chat).
            if (cfg.chat) out.chat = await (await roomCall(p.id, 'chat-get', {}, { 'x-paper-owner': p.owner_id })).json();
          }
          return json(out);
        }
        if (req.method === 'PATCH') {
          const { p } = await access(m[1], 'editor');
          const b = await body();
          const name = cleanName(b.name, cfg.maxName);
          if (!name) fail(400, 'A page needs a name');
          await db.prepare('UPDATE paper_pages SET name = ?1, updated_at = ?2 WHERE id = ?3').bind(name, Date.now(), p.id).run();
          return json({ ok: true, name });
        }
        if (req.method === 'DELETE') {
          // Deleting moves the page to its owner's trash for trashDays;
          // ?forever=1 on a page already in the trash deletes it for good.
          const p = await page(m[1], { trashed: true });
          if (p.owner_id !== user.id && !isAdmin) {
            if (p.deleted_at != null) fail(404, 'No such page');
            await access(m[1]);
            fail(403, 'Only the page\'s owner can delete it (you can leave it instead)');
          }
          if (p.deleted_at != null) {
            if (url.searchParams.get('forever') !== '1') fail(409, 'It is already in the trash');
            if (p.blocked && !isAdmin) fail(403, 'This page was taken down and is being looked at: it can\'t be deleted for good yet');
            return json(await deletePage(p));
          }
          const now = Date.now(), until = now + cfg.trashDays * DAY;
          await db.batch([
            db.prepare('UPDATE paper_pages SET deleted_at = ?1 WHERE id = ?2').bind(now, p.id),
            db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(p.id),
          ]);
          await roomCall(p.id, 'trash', { at: now, until }, { 'x-paper-owner': p.owner_id }).catch(() => {});
          return json({ ok: true, trashed: now, purge: until });
        }
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/untrash$/)) && req.method === 'POST') {
        need();
        const p = await page(m[1], { trashed: true });
        if (p.owner_id !== user.id && !isAdmin) fail(404, 'No such page');
        if (p.deleted_at == null) return json({ ok: true, page: card(p, 'owner') });
        await db.prepare('UPDATE paper_pages SET deleted_at = NULL WHERE id = ?1').bind(p.id).run();
        await roomCall(p.id, 'untrash', {}, { 'x-paper-owner': p.owner_id }).catch(() => {});
        return json({ ok: true, page: card({ ...p, deleted_at: null }, 'owner') });
      }

      // ---- what full copies replaced (kept keepReplacedDays) -------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/history$/)) && req.method === 'GET') {
        need();
        const { p } = await access(m[1], 'owner');
        return json(await (await roomCall(p.id, 'history', {}, { 'x-paper-owner': p.owner_id })).json());
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/restore$/)) && req.method === 'POST') {
        // Bring back what full copies at seq >= since replaced (all of it by
        // default). Nothing is taken away: the page gains it back.
        need();
        const { p } = await access(m[1], 'owner');
        const b = await body();
        // Charged to the owner's storage: refused when it has no room.
        const r = await roomCall(p.id, 'restore', { since: Math.max(0, Number(b.since) || 0), only: b.only === true || b.only === 1 }, { 'x-paper-owner': p.owner_id });
        const out = await r.json();
        if (r.status !== 200) { const { error, ...extra } = out; fail(r.status, error || 'Could not bring it back', extra); }
        return json(out);
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/open$/)) && req.method === 'POST') {
        need();
        const { p, role } = await access(m[1]);
        return json(openInfo(p, role));
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/copy$/)) && req.method === 'POST') {
        need();
        const { p } = await access(m[1]);
        const b = await body();
        const c = await newPage(b, b.name || `Copy of ${p.name}`);
        return json({ page: card(c, 'owner'), open: openInfo(c, 'owner'), from: openInfo(p, (await roleOf(p, user))) }, 201);
      }

      // ---- the live room --------------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/relay$/))) {
        if (req.headers.get('Upgrade') !== 'websocket') fail(426, 'Expected a WebSocket');
        need();
        if (await limited('ws', 'u' + user.id)) fail(429, 'Too many connections. Try again in a minute.');
        const { p, role } = await access(m[1]);
        return enter(p, role, user, '');
      }
      if ((m = path.match(/^\/l\/([A-Za-z0-9_-]{20,64})\/relay$/))) {
        // Opened by link without an account (view only, when allowed).
        if (req.headers.get('Upgrade') !== 'websocket') fail(426, 'Expected a WebSocket');
        if (!cfg.linkSharing || !cfg.linkAnonymousView) fail(401, 'Sign in to open this page');
        if (await limited('ws', 'ip' + ip)) fail(429, 'Too many connections. Try again in a minute.');
        const l = await linkOf(m[1]);
        const p = await page(l.page_id);
        await linkAlive(l, p);
        if (p.blocked) fail(451, 'This page was taken down');
        // A random id per visit: everyone on the page sees it, so not the IP.
        return enter(p, 'viewer', { id: 'anon:' + randomHex(8), name: 'Visitor' }, l.id);
      }

      // ---- chat (the messages themselves travel on the live socket) -------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/chat$/))) {
        need();
        if (!cfg.chat) fail(404, 'Chat is off on this site');
        if (req.method === 'GET') {
          // Data export: everyone on the page may take what was said (while
          // chat is on; owners always).
          const { p, role } = await access(m[1]);
          const r = await (await roomCall(p.id, 'chat-export', { for: user.id }, { 'x-paper-owner': p.owner_id })).json();
          if (!r.settings.on && role !== 'owner') fail(403, 'Chat is off on this page');
          return json({ page: { id: p.id, name: p.name }, exported: Date.now(), ...r });
        }
        if (req.method === 'PATCH') {
          // Owners (and co-owners) only: on/off, kept or ephemeral.
          const { p } = await access(m[1], 'owner');
          const b = await body();
          const set = {};
          if (typeof b.on === 'boolean') set.on = b.on;
          if (typeof b.keep === 'boolean') set.keep = b.keep;
          if (!Object.keys(set).length) fail(400, 'Say on and/or keep (true or false)');
          return json(await (await roomCall(p.id, 'chat-settings', set, { 'x-paper-owner': p.owner_id })).json());
        }
        if (req.method === 'DELETE') {
          const { p } = await access(m[1], 'owner');
          return json(await (await roomCall(p.id, 'chat-clear', {}, { 'x-paper-owner': p.owner_id })).json());
        }
      }

      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/chat\/report$/)) && req.method === 'POST') {
        // Report one message: the room's copy of it and a few around it are
        // kept with the report (chat may be ephemeral and gone by the time
        // the admin looks).
        need();
        if (!cfg.chat) fail(404, 'Chat is off on this site');
        const { p } = await access(m[1]);
        const b = await body();
        if (await limited('create', 'u' + user.id)) fail(429, 'Too many reports at once');
        const snap = await (await roomCall(p.id, 'chat-around', { id: Number(b.id) }, { 'x-paper-owner': p.owner_id })).json();
        if (!snap.message) fail(404, 'That message is gone');
        if (snap.message.user === user.id) fail(409, 'That is your own message');
        const reason = cleanText(b.reason, cfg.maxReason).text || 'Reported from the chat';
        const id = await fileReport(db, { page: p.id, reporter: user.id, reason, kind: 'chat', subject: snap.message.user, context: snap }, env, waitUntil);
        return json({ ok: true, id }, 201);
      }

      // ---- blocking people (their chat isn't shown to me; they can't share with me or offer me pages) ----
      if (path === '/blocks') {
        need();
        if (req.method === 'GET') {
          const rows = (await db.prepare('SELECT blocked_id, created_at FROM paper_blocks WHERE user_id = ?1 ORDER BY created_at').bind(user.id).all()).results;
          const who = await names(rows.map(r => r.blocked_id));
          return json({ blocks: rows.map(r => ({ id: r.blocked_id, name: who[r.blocked_id] || null, at: r.created_at })) });
        }
        if (req.method === 'POST') {
          const b = await body();
          const uid = String(b.user ?? '').slice(0, 80);
          if (!uid || uid.startsWith('anon:')) fail(400, 'Whom? Give their account id');
          if (uid === user.id) fail(400, 'You can\'t block yourself');
          const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_blocks WHERE user_id = ?1').bind(user.id).first();
          if (n.n >= cfg.maxBlocks) fail(409, `You can block at most ${cfg.maxBlocks} people`);
          await db.prepare('INSERT OR IGNORE INTO paper_blocks (user_id, blocked_id, created_at) VALUES (?1, ?2, ?3)').bind(user.id, uid, Date.now()).run();
          await blocksChanged(user.id, uid);
          return json({ ok: true, blocked: { id: uid, name: (await names([uid]))[uid] || null } });
        }
      }
      if ((m = path.match(/^\/blocks\/([^/]{1,80})$/)) && req.method === 'DELETE') {
        need();
        const uid = decodeURIComponent(m[1]);
        await db.prepare('DELETE FROM paper_blocks WHERE user_id = ?1 AND blocked_id = ?2').bind(user.id, uid).run();
        await blocksChanged(user.id, uid);
        return json({ ok: true });
      }

      // ---- sharing ------------------------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/members$/)) && req.method === 'POST') {
        need();
        const { p } = await access(m[1], 'owner');
        const b = await body();
        const role = ROLES.includes(b.role) ? b.role : 'viewer';
        const who = String(b.who || '').trim();
        if (!who) fail(400, 'Who? Give a username or an email');
        // By email, the answer is the same whether or not an account uses it
        // (nobody learns who signs in with an address, or their username).
        const byEmail = who.includes('@');
        const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_members WHERE page_id = ?1').bind(p.id).first();
        if (byEmail) {
          if (await limited('find', 'find:u' + user.id)) fail(429, 'Too many lookups by email. Try again later.');
          if (n.n >= cfg.maxMembers) fail(409, `A page can be shared with at most ${cfg.maxMembers} people`);
        }
        const target = typeof auth.find === 'function' ? await auth.find(env, who) : null;
        const sent = () => json({ ok: true, email: true, member: { id: null, name: null, role } });
        if (!target) return byEmail ? sent() : fail(404, `No account called “${cleanName(who, 60)}”`);
        target.id = String(target.id);
        const had = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, target.id).first();
        if (!had && n.n >= cfg.maxMembers) fail(409, `A page can be shared with at most ${cfg.maxMembers} people`);
        if (target.id === p.owner_id) return byEmail ? sent() : fail(409, 'That is the page\'s owner');
        if (await blocked(target.id, user.id)) return byEmail ? sent() : fail(403, 'You can\'t share pages with this person');
        await db.prepare(`INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
          ON CONFLICT (page_id, user_id) DO UPDATE SET role = excluded.role, via_link = NULL`).bind(p.id, target.id, role, user.id, Date.now()).run();
        if (had && rank(role) < rank(had.role)) await kick(p.id, { user: target.id, why: 'Your access to this page changed' });
        if (byEmail) return sent();
        return json({ ok: true, member: { id: target.id, name: cleanName(target.name, 40) || null, role } });
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/members\/([^/]{1,80})$/))) {
        need();
        const uid = decodeURIComponent(m[2]);
        if (req.method === 'DELETE' && uid === user.id) {
          // Leaving a page shared with me (a co-owner's links go too, as when removed).
          const { p, role } = await access(m[1]);
          if (p.owner_id === user.id) fail(409, 'The owner cannot leave: delete the page or give it to someone first');
          const theirs = role === 'owner'
            ? (await db.prepare('SELECT id FROM paper_links WHERE page_id = ?1 AND created_by = ?2').bind(p.id, uid).all()).results.map(l => l.id)
            : [];
          await db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).run();
          await kick(p.id, { user: uid, why: 'You left this page' });
          await dropLinks(p, theirs, 'The person who made that link left this page');
          return json({ ok: true, linksOff: theirs.length });
        }
        const { p } = await access(m[1], 'owner');
        if (uid === p.owner_id) fail(409, 'Give the page to someone else first');
        const had = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).first();
        if (!had) fail(404, 'Not on this page');
        // A co-owner who stops being one takes the links they made with them.
        const linksOf = async () => had.role === 'owner'
          ? (await db.prepare('SELECT id FROM paper_links WHERE page_id = ?1 AND created_by = ?2').bind(p.id, uid).all()).results.map(l => l.id)
          : [];
        if (req.method === 'DELETE') {
          const theirs = await linksOf();
          await db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).run();
          await kick(p.id, { user: uid, why: 'You no longer have access to this page' });
          await dropLinks(p, theirs, 'The person who made that link no longer owns this page');
          return json({ ok: true, linksOff: theirs.length });
        }
        if (req.method === 'PATCH') {
          const b = await body();
          if (!ROLES.includes(b.role)) fail(400, 'Role is owner, editor or viewer');
          const theirs = b.role === 'owner' ? [] : await linksOf();
          await db.prepare('UPDATE paper_members SET role = ?1, via_link = NULL WHERE page_id = ?2 AND user_id = ?3').bind(b.role, p.id, uid).run();
          if (rank(b.role) < rank(had.role)) await kick(p.id, { user: uid, why: 'Your access to this page changed' });
          await dropLinks(p, theirs, 'The person who made that link no longer owns this page');
          return json({ ok: true, linksOff: theirs.length });
        }
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/links$/)) && req.method === 'POST') {
        need();
        if (!cfg.linkSharing) fail(403, 'Link sharing is off on this site');
        const { p } = await access(m[1], 'owner');
        const b = await body();
        const role = b.role === 'editor' ? 'editor' : 'viewer';
        const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_links WHERE page_id = ?1').bind(p.id).first();
        if (n.n >= cfg.maxLinks) fail(409, `At most ${cfg.maxLinks} links per page: revoke one first`);
        const token = b64url(crypto.getRandomValues(new Uint8Array(24)));
        const id = randomHex(6);
        await db.prepare('INSERT INTO paper_links (id, token_hash, page_id, role, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)')
          .bind(id, await sha256(token), p.id, role, user.id, Date.now()).run();
        return json({ id, role, token }, 201);
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/links\/([0-9a-f]{12})$/)) && req.method === 'DELETE') {
        need();
        const { p } = await access(m[1], 'owner');
        await dropLinks(p, [m[2]], 'That link was turned off');
        return json({ ok: true });
      }
      if ((m = path.match(/^\/links\/([A-Za-z0-9_-]{20,64})$/))) {
        if (!cfg.linkSharing) fail(404, 'No such link');
        const l = await linkOf(m[1]);
        const p = await page(l.page_id);
        await linkAlive(l, p);
        if (p.blocked) fail(451, 'This page was taken down');
        if (req.method === 'GET') return json({ name: p.name, role: l.role, signIn: !user, anonymousView: cfg.linkAnonymousView });
        if (req.method === 'POST') {
          need();
          // Opening the link adds me to the page (it then shows under Shared
          // with me); revoking the link takes me off again.
          const had = await roleOf(p, user);
          if (!had) {
            const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_members WHERE page_id = ?1').bind(p.id).first();
            if (n.n >= cfg.maxMembers) fail(409, 'This page is shared with as many people as it can be');
            await db.prepare('INSERT INTO paper_members (page_id, user_id, role, via_link, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)')
              .bind(p.id, user.id, l.role, l.id, l.created_by, Date.now()).run();
          } else if (rank(had) < rank(l.role)) {
            await db.prepare('UPDATE paper_members SET role = ?1, via_link = ?2 WHERE page_id = ?3 AND user_id = ?4').bind(l.role, l.id, p.id, user.id).run();
          }
          const role = (await roleOf(p, user));
          return json({ page: card(p, role, await names([p.owner_id])), open: openInfo(p, role) });
        }
      }
      // ---- giving a page away: an offer the recipient accepts -----------------
      // Nothing moves (no bytes, no access) until they accept, so nobody can
      // push pages into someone else's account.
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/transfer$/))) {
        need();
        const { p } = await access(m[1], 'owner');
        if (p.owner_id !== user.id && !isAdmin) fail(403, 'Only the billing owner can give the page away');
        if (req.method === 'DELETE') {
          await db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(p.id).run();
          return json({ ok: true });
        }
        if (req.method !== 'POST') fail(405, 'Use POST or DELETE');
        const b = await body();
        // Only an account the site knows by name or email (never a raw id).
        const who = String(b.who || '').trim();
        if (!who) fail(400, 'Who? Give a username or an email');
        // By email, the answer is the same whether or not an account uses it.
        const byEmail = who.includes('@');
        if (byEmail && await limited('find', 'find:u' + user.id)) fail(429, 'Too many lookups by email. Try again later.');
        if (await limited('create', 'u' + user.id)) fail(429, 'Too many offers at once. Try again in a minute.');
        const now = Date.now();
        const sent = () => json({ ok: true, email: true, offer: { to: { id: null, name: null }, created: now, leave: !!b.leave } }, 201);
        const target = typeof auth.find === 'function' ? await auth.find(env, who) : null;
        if (!target || target.id == null) return byEmail ? sent() : fail(404, `No account called “${cleanName(who, 60)}”`);
        const to = String(target.id);
        if (to === p.owner_id) return byEmail ? sent() : fail(409, 'They already own it');
        if (await blocked(to, user.id)) return byEmail ? sent() : fail(403, 'You can\'t offer pages to this person');
        const waiting = await db.prepare('SELECT COUNT(*) AS n FROM paper_transfers WHERE to_id = ?1 AND page_id != ?2 AND created_at >= ?3')
          .bind(to, p.id, now - cfg.offerDays * DAY).first();
        if (waiting.n >= cfg.maxOffersTo) return byEmail ? sent() : fail(409, 'They have too many offers waiting already');
        await db.prepare(`INSERT INTO paper_transfers (page_id, from_id, to_id, leave, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
          ON CONFLICT (page_id) DO UPDATE SET from_id = excluded.from_id, to_id = excluded.to_id, leave = excluded.leave, created_at = excluded.created_at`)
          .bind(p.id, p.owner_id, to, b.leave ? 1 : 0, now).run();
        if (byEmail) return sent();
        return json({ ok: true, offer: { to: { id: to, name: cleanName(target.name, 40) || null }, created: now, leave: !!b.leave } }, 201);
      }
      if ((m = path.match(/^\/offers\/([0-9a-f]{32})\/(accept|decline)$/)) && req.method === 'POST') {
        need();
        const t = await db.prepare('SELECT * FROM paper_transfers WHERE page_id = ?1 AND to_id = ?2').bind(m[1], user.id).first();
        if (!t || t.created_at < Date.now() - cfg.offerDays * DAY) fail(404, 'That offer was withdrawn or has lapsed');
        if (m[2] === 'decline') {
          await db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(t.page_id).run();
          return json({ ok: true });
        }
        const p = await page(t.page_id);
        if (p.owner_id !== t.from_id) {
          await db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(t.page_id).run();
          fail(404, 'That offer was withdrawn or has lapsed');
        }
        if (p.blocked) fail(451, 'This page was taken down');
        const acct = await account(db, user.id, cfg.freeQuota);
        if (acct.blocked) fail(403, 'This account cannot keep pages');
        const owned = await db.prepare('SELECT COUNT(*) AS n FROM paper_pages WHERE owner_id = ?1').bind(user.id).first();
        if (owned.n >= cfg.maxPages) fail(409, `You have ${owned.n} pages, the most an account can have. Delete some (and empty them from the trash) to take this one.`, { full: 'pages' });
        // One step in the room, between kept frames: settle with the old
        // owner, move what the page keeps to my account, charge me from now.
        const h = await roomCall(p.id, 'handover', { to: user.id }, { 'x-paper-owner': p.owner_id });
        const r = await h.json().catch(() => ({}));
        if (h.status === 409) fail(409, 'Your storage is too full to take this page. Free some space first.', { full: 'bytes' });
        if (h.status !== 200) fail(500, 'Something went wrong');
        const bytes = Number(r.bytes) || 0;
        const now = Date.now();
        await db.batch([
          db.prepare('UPDATE paper_pages SET owner_id = ?1, bytes = ?2, updated_at = ?3 WHERE id = ?4').bind(user.id, bytes, now, p.id),
          db.prepare(`INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, 'owner', ?3, ?4)
            ON CONFLICT (page_id, user_id) DO UPDATE SET role = 'owner', via_link = NULL`).bind(p.id, user.id, t.from_id, now),
          t.leave
            ? db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, t.from_id)
            : db.prepare('UPDATE paper_members SET role = \'owner\' WHERE page_id = ?1 AND user_id = ?2').bind(p.id, t.from_id),
          db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(p.id),
        ]);
        if (t.leave) await kick(p.id, { user: t.from_id, why: 'You gave this page away' });
        const mine = await page(p.id);
        return json({ ok: true, page: card(mine, 'owner'), open: openInfo(mine, 'owner') });
      }

      // ---- reports and the admin ---------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/report$/)) && req.method === 'POST') {
        need();
        await access(m[1]);
        const b = await body();
        // Plain text, like chat: no control or direction-override characters.
        const reason = cleanText(b.reason, cfg.maxReason).text;
        if (!reason) fail(400, 'Say what is wrong');
        if (await limited('create', 'u' + user.id)) fail(429, 'Too many reports at once');
        const id = await fileReport(db, { page: m[1], reporter: user.id, reason }, env, waitUntil);
        return json({ ok: true, id }, 201);
      }
      if (path.startsWith('/admin/')) {
        if (!isAdmin) fail(404, 'Not found');
        if (path === '/admin/reports' && req.method === 'GET') {
          const all = url.searchParams.get('all') === '1';
          const rows = (await db.prepare(`SELECT r.*, p.name AS page_name, p.owner_id, p.blocked FROM paper_reports r LEFT JOIN paper_pages p ON p.id = r.page_id
            ${all ? '' : 'WHERE r.resolved_at IS NULL'} ORDER BY r.created_at DESC LIMIT 200`).all()).results;
          return json({ reports: rows });
        }
        if ((m = path.match(/^\/admin\/reports\/(\d+)$/)) && req.method === 'POST') {
          const b = await body();
          await db.prepare('UPDATE paper_reports SET resolved_at = ?1, resolution = ?2 WHERE id = ?3').bind(Date.now(), String(b.resolution || 'resolved').slice(0, 200), Number(m[1])).run();
          return json({ ok: true });
        }
        if ((m = path.match(/^\/admin\/pages\/([0-9a-f]{32})$/))) {
          const p = await page(m[1], { trashed: true });
          if (req.method === 'GET') {
            // The admin may look (view only) to judge a report.
            const stats = await (await room(p.id).fetch(new Request('https://room/stats', { headers: { 'x-paper-page': p.id } }))).json();
            const members = (await db.prepare('SELECT user_id, role FROM paper_members WHERE page_id = ?1').bind(p.id).all()).results;
            return json({ page: card(p, 'viewer'), blocked: !!p.blocked, deleted: p.deleted_at, members, stats, open: openInfo(p, 'viewer') });
          }
          if (req.method === 'DELETE') return json(await deletePage(p));
        }
        if ((m = path.match(/^\/admin\/pages\/([0-9a-f]{32})\/(history|restore)$/))) {
          // Roll a page back after someone replaced it with a bad full copy
          // (the admin may take the owner past their quota doing it).
          const p = await page(m[1], { trashed: true });
          const b = m[2] === 'restore' ? await body() : {};
          return json(await (await roomCall(p.id, m[2], { since: Math.max(0, Number(b.since) || 0), only: b.only === true || b.only === 1, force: true }, { 'x-paper-owner': p.owner_id })).json());
        }
        if ((m = path.match(/^\/admin\/pages\/([0-9a-f]{32})\/chat(?:\/(\d+))?$/))) {
          // A page's chat, to judge a report: read it, clear it, or take one message down.
          const p = await page(m[1], { trashed: true });
          const own = { 'x-paper-owner': p.owner_id };
          if (req.method === 'GET' && !m[2]) return json({ page: { id: p.id, name: p.name }, ...(await (await roomCall(p.id, 'chat-export', {}, own)).json()) });
          if (req.method === 'DELETE' && !m[2]) return json(await (await roomCall(p.id, 'chat-clear', {}, own)).json());
          if (req.method === 'DELETE') {
            const r = await (await roomCall(p.id, 'chat-remove', { id: Number(m[2]) }, own)).json();
            if (!r.removed) fail(404, 'No such message');
            return json(r);
          }
        }
        if ((m = path.match(/^\/admin\/pages\/([0-9a-f]{32})\/block$/)) && req.method === 'POST') {
          const p = await page(m[1]);
          const b = await body();
          await db.prepare('UPDATE paper_pages SET blocked = ?1 WHERE id = ?2').bind(b.blocked === false ? 0 : 1, p.id).run();
          if (b.blocked !== false) await kick(p.id, { why: 'This page was taken down' });
          return json({ ok: true });
        }
        if ((m = path.match(/^\/admin\/accounts\/([^/]{1,80})$/)) && req.method === 'POST') {
          const uid = decodeURIComponent(m[1]);
          const b = await body();
          let quota = null;
          if (b.quota != null) {
            quota = Math.floor(Number(b.quota));
            if (!Number.isSafeInteger(quota) || quota < 0) fail(400, 'Quota is a whole number of bytes');
          }
          await account(db, uid, cfg.freeQuota);
          if (quota != null) await db.prepare('UPDATE paper_accounts SET quota = ?1 WHERE user_id = ?2').bind(quota, uid).run();
          if (b.blocked != null) await db.prepare('UPDATE paper_accounts SET blocked = ?1 WHERE user_id = ?2').bind(b.blocked ? 1 : 0, uid).run();
          // Blocked: out of every page it has open, at once.
          if (b.blocked) {
            for (const r of (await db.prepare('SELECT page_id FROM paper_members WHERE user_id = ?1').bind(uid).all()).results) {
              await kick(r.page_id, { user: uid, why: 'This account was blocked' });
            }
          }
          return json(await db.prepare('SELECT * FROM paper_accounts WHERE user_id = ?1').bind(uid).first());
        }
        if ((m = path.match(/^\/admin\/accounts\/([^/]{1,80})\/recount$/)) && req.method === 'POST') {
          // Set used to what the account's pages keep (after rooms settle up).
          const uid = decodeURIComponent(m[1]);
          const owned = (await db.prepare('SELECT id FROM paper_pages WHERE owner_id = ?1').bind(uid).all()).results;
          let total = 0;
          for (const o of owned) total += Number((await (await roomCall(o.id, 'flush', {}, { 'x-paper-owner': uid })).json()).bytes) || 0;
          await db.prepare('UPDATE paper_accounts SET used = ?1 WHERE user_id = ?2').bind(total, uid).run();
          return json({ used: total });
        }
        fail(404, 'Not found');
      }
      fail(404, 'Not found');
    } catch (e) {
      if (e instanceof HttpError) return json({ error: e.message, ...e.extra }, e.status);
      console.error('og-paper api', e);
      return json({ error: 'Something went wrong' }, 500);
    }

    async function linkOf(token) {
      return (await db.prepare('SELECT * FROM paper_links WHERE token_hash = ?1').bind(await sha256(token)).first()) || fail(404, 'That link was turned off or never existed');
    }

    /** A link works only while the person who made it still owns the page. */
    async function linkAlive(l, p) {
      if (l.created_by === p.owner_id) return;
      const r = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, l.created_by).first();
      if (r?.role !== 'owner') fail(404, 'That link was turned off or never existed');
    }

    /** Whether `who` has blocked `whom`. */
    async function blocked(who, whom) {
      return !!(await db.prepare('SELECT 1 AS x FROM paper_blocks WHERE user_id = ?1 AND blocked_id = ?2').bind(String(who), String(whom)).first());
    }

    /** `me` blocked or unblocked `other`: tell the rooms of pages they are both on. */
    async function blocksChanged(me, other) {
      const list = (await db.prepare('SELECT blocked_id FROM paper_blocks WHERE user_id = ?1 LIMIT ?2').bind(me, cfg.maxBlocks).all()).results.map(r => r.blocked_id);
      const pages = (await db.prepare(`SELECT a.page_id FROM paper_members a JOIN paper_members b ON b.page_id = a.page_id
        WHERE a.user_id = ?1 AND b.user_id = ?2`).bind(me, String(other)).all()).results;
      for (const r of pages) await roomCall(r.page_id, 'blocks', { user: me, list }).catch(() => {});
    }

    async function enter(p, role, u, via) {
      const h = new Headers(req.headers);
      h.set('x-paper-page', p.id);
      h.set('x-paper-owner', p.owner_id);
      h.set('x-paper-user', u.id);
      h.set('x-paper-name', String(u.name || '').slice(0, 40));
      h.set('x-paper-role', role);
      if (via) h.set('x-paper-via', via); else h.delete('x-paper-via');
      h.delete('x-paper-blocks');
      if (!u.id.startsWith('anon:')) {
        const list = (await db.prepare('SELECT blocked_id FROM paper_blocks WHERE user_id = ?1 LIMIT ?2').bind(u.id, cfg.maxBlocks).all()).results.map(r => r.blocked_id);
        h.set('x-paper-blocks', JSON.stringify(list));
      }
      return room(p.id).fetch(new Request('https://room/relay', { headers: h }));
    }

  };
}
