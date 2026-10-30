// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// The hosted OG Paper API: page lists, sharing, quotas, reports, and the
// door to each page's room. Mounted by the host site at a prefix
// (default /paper/api). Who is signed in comes from the host site through
// an auth adapter (see index.js); there is no user system here.

import { configFrom } from './config.js';
import { account, give, take } from './quota.js';

const ROLES = ['viewer', 'editor', 'owner'];
const rank = r => ROLES.indexOf(r);
const HEX32 = /^[0-9a-f]{32}$/;
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

export function createApi({ auth, config = {}, roomBinding = 'PAPER_ROOMS', onReport } = {}) {
  if (!auth || typeof auth.user !== 'function') throw new Error('og-paper hosted: an auth adapter with user(req, env) is needed');

  return async function handle(req, env, ctx) {
    const cfg = configFrom(env, config);
    const url = new URL(req.url);
    if (!url.pathname.startsWith(cfg.prefix + '/') && url.pathname !== cfg.prefix) return null;
    const path = url.pathname.slice(cfg.prefix.length) || '/';
    const db = env.DB;
    const rooms = env[roomBinding];
    const room = id => rooms.get(rooms.idFromName(id));
    const roomCall = (id, op, body, headers = {}) =>
      room(id).fetch(new Request('https://room/' + op, { method: 'POST', body: JSON.stringify(body || {}), headers: { 'x-paper-page': id, ...headers } }));
    const limited = async (kind, key) => typeof auth.limit === 'function' && await auth.limit(env, kind, key);
    const wsOrigin = (cfg.origin || url.origin).replace(/^http/, 'ws');

    try {
      // Same-site only for anything that changes things or opens a room (the
      // session is a cookie: no other site may use it).
      const origin = req.headers.get('Origin');
      const allowed = [url.origin, ...(cfg.origins || [])];
      if ((req.method !== 'GET' || req.headers.get('Upgrade') === 'websocket') && origin && !allowed.includes(origin)) fail(403, 'Bad origin');
      if (req.method !== 'GET' && req.headers.get('Sec-Fetch-Site') && !['same-origin', 'none'].includes(req.headers.get('Sec-Fetch-Site'))) fail(403, 'Bad origin');

      const user = await auth.user(req, env);
      if (user) user.id = String(user.id);
      const ip = req.headers.get('CF-Connecting-IP') || 'local';
      if (await limited(req.method === 'GET' ? 'api' : 'write', user ? 'u' + user.id : 'ip' + ip)) fail(429, 'Too many requests. Please slow down and try again in a minute.');
      const isAdmin = !!(user && typeof auth.isAdmin === 'function' && await auth.isAdmin(user, env));
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
        return (await auth.names(env, ids)) || {};
      };

      const page = async id => {
        if (!HEX32.test(id)) fail(404, 'No such page');
        return (await db.prepare('SELECT * FROM paper_pages WHERE id = ?1').bind(id).first()) || fail(404, 'No such page');
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
        const role = await roleOf(p, user);
        if (p.blocked && !isAdmin) fail(451, 'This page was taken down');
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
        name = String(name || 'Untitled').trim().slice(0, cfg.maxName) || 'Untitled';
        await db.batch([
          db.prepare('INSERT INTO paper_pages (id, owner_id, name, canvas, edit_key, view_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)')
            .bind(id, user.id, name, canvas, b.edit, b.view, now),
          db.prepare('INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, \'owner\', ?2, ?3)').bind(id, user.id, now),
        ]);
        return page(id);
      };
      const kick = (id, what) => roomCall(id, 'kick', what).catch(() => {});

      let m;
      // ---- the signed-in account ------------------------------------------------
      if (path === '/me' && req.method === 'GET') {
        if (!user) return json({ user: null, linkSharing: cfg.linkSharing });
        return json({ user: { id: user.id, name: user.name || null }, admin: isAdmin, ...(await usage()), maxOpBytes: cfg.maxOpBytes, linkSharing: cfg.linkSharing });
      }

      // ---- page lists -------------------------------------------------------------
      if (path === '/pages' && req.method === 'GET') {
        need();
        const mine = (await db.prepare('SELECT p.* FROM paper_pages p WHERE p.owner_id = ?1 ORDER BY p.updated_at DESC').bind(user.id).all()).results;
        const shared = (await db.prepare(`SELECT p.*, m.role AS my_role FROM paper_members m JOIN paper_pages p ON p.id = m.page_id
          WHERE m.user_id = ?1 AND p.owner_id != ?1 AND p.blocked = 0 ORDER BY p.updated_at DESC`).bind(user.id).all()).results;
        const owners = await names(shared.map(p => p.owner_id));
        return json({
          mine: mine.map(p => ({ ...card(p, 'owner'), blocked: !!p.blocked })),
          shared: shared.map(p => card(p, p.my_role, owners)),
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
            out.links = (await db.prepare('SELECT id, role, created_at FROM paper_links WHERE page_id = ?1 ORDER BY created_at').bind(p.id).all()).results
              .map(l => ({ id: l.id, role: l.role, created: l.created_at }));
          }
          return json(out);
        }
        if (req.method === 'PATCH') {
          const { p } = await access(m[1], 'editor');
          const b = await body();
          const name = String(b.name || '').trim().slice(0, cfg.maxName);
          if (!name) fail(400, 'A page needs a name');
          await db.prepare('UPDATE paper_pages SET name = ?1, updated_at = ?2 WHERE id = ?3').bind(name, Date.now(), p.id).run();
          return json({ ok: true, name });
        }
        if (req.method === 'DELETE') {
          const p = await page(m[1]);
          if (p.owner_id !== user.id && !isAdmin) {
            await access(m[1]);
            fail(403, 'Only the page\'s owner can delete it (you can leave it instead)');
          }
          return json(await deletePage(p));
        }
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
        if (p.blocked) fail(451, 'This page was taken down');
        return enter(p, 'viewer', { id: 'anon:' + ip, name: 'Visitor' }, l.id);
      }

      // ---- sharing ------------------------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/members$/)) && req.method === 'POST') {
        need();
        const { p } = await access(m[1], 'owner');
        const b = await body();
        const role = ROLES.includes(b.role) ? b.role : 'viewer';
        const who = String(b.who || '').trim();
        if (!who) fail(400, 'Who? Give a username or an email');
        const target = typeof auth.find === 'function' ? await auth.find(env, who) : null;
        if (!target) fail(404, `No account called “${who.slice(0, 60)}”`);
        target.id = String(target.id);
        const n = await db.prepare('SELECT COUNT(*) AS n FROM paper_members WHERE page_id = ?1').bind(p.id).first();
        const had = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, target.id).first();
        if (!had && n.n >= cfg.maxMembers) fail(409, `A page can be shared with at most ${cfg.maxMembers} people`);
        if (target.id === p.owner_id) fail(409, 'That is the page\'s owner');
        await db.prepare(`INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
          ON CONFLICT (page_id, user_id) DO UPDATE SET role = excluded.role, via_link = NULL`).bind(p.id, target.id, role, user.id, Date.now()).run();
        if (had && rank(role) < rank(had.role)) await kick(p.id, { user: target.id, why: 'Your access to this page changed' });
        return json({ ok: true, member: { id: target.id, name: target.name || null, role } });
      }
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/members\/([^/]{1,80})$/))) {
        need();
        const uid = decodeURIComponent(m[2]);
        if (req.method === 'DELETE' && uid === user.id) {
          // Leaving a page shared with me.
          const { p } = await access(m[1]);
          if (p.owner_id === user.id) fail(409, 'The owner cannot leave: delete the page or give it to someone first');
          await db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).run();
          await kick(p.id, { user: uid, why: 'You left this page' });
          return json({ ok: true });
        }
        const { p } = await access(m[1], 'owner');
        if (uid === p.owner_id) fail(409, 'Give the page to someone else first');
        const had = await db.prepare('SELECT role FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).first();
        if (!had) fail(404, 'Not on this page');
        if (req.method === 'DELETE') {
          await db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, uid).run();
          await kick(p.id, { user: uid, why: 'You no longer have access to this page' });
          return json({ ok: true });
        }
        if (req.method === 'PATCH') {
          const b = await body();
          if (!ROLES.includes(b.role)) fail(400, 'Role is owner, editor or viewer');
          await db.prepare('UPDATE paper_members SET role = ?1, via_link = NULL WHERE page_id = ?2 AND user_id = ?3').bind(b.role, p.id, uid).run();
          if (rank(b.role) < rank(had.role)) await kick(p.id, { user: uid, why: 'Your access to this page changed' });
          return json({ ok: true });
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
        const via = (await db.prepare('SELECT user_id FROM paper_members WHERE page_id = ?1 AND via_link = ?2').bind(p.id, m[2]).all()).results;
        await db.batch([
          db.prepare('DELETE FROM paper_links WHERE page_id = ?1 AND id = ?2').bind(p.id, m[2]),
          db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND via_link = ?2').bind(p.id, m[2]),
        ]);
        // Out go the people the link brought in, and anyone on it without an account.
        for (const v of via) await kick(p.id, { user: v.user_id, why: 'That link was turned off' });
        await kick(p.id, { via: m[2], why: 'That link was turned off' });
        return json({ ok: true });
      }
      if ((m = path.match(/^\/links\/([A-Za-z0-9_-]{20,64})$/))) {
        if (!cfg.linkSharing) fail(404, 'No such link');
        const l = await linkOf(m[1]);
        const p = await page(l.page_id);
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
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/transfer$/)) && req.method === 'POST') {
        need();
        const { p } = await access(m[1], 'owner');
        if (p.owner_id !== user.id && !isAdmin) fail(403, 'Only the billing owner can give the page away');
        const b = await body();
        let to = b.to != null ? String(b.to) : null;
        if (!to && b.who && typeof auth.find === 'function') to = (await auth.find(env, String(b.who)))?.id;
        if (!to) fail(404, 'No such account');
        to = String(to);
        if (to === p.owner_id) fail(409, 'They already own it');
        const acct = await account(db, to, cfg.freeQuota);
        const owned = await db.prepare('SELECT COUNT(*) AS n FROM paper_pages WHERE owner_id = ?1').bind(to).first();
        if (owned.n >= cfg.maxPages) fail(409, 'They have as many pages as an account can have');
        // Settle the room with the old owner, then move its bytes over.
        const r = await (await roomCall(p.id, 'flush', {}, { 'x-paper-owner': p.owner_id })).json();
        const bytes = Number(r.bytes) || 0;
        if (acct.blocked || !(await take(db, to, bytes))) fail(409, 'Their storage is too full to take this page', { full: 'bytes' });
        await give(db, p.owner_id, bytes);
        await db.batch([
          db.prepare('UPDATE paper_pages SET owner_id = ?1, bytes = ?2, updated_at = ?3 WHERE id = ?4').bind(to, bytes, Date.now(), p.id),
          db.prepare(`INSERT INTO paper_members (page_id, user_id, role, added_by, added_at) VALUES (?1, ?2, 'owner', ?3, ?4)
            ON CONFLICT (page_id, user_id) DO UPDATE SET role = 'owner', via_link = NULL`).bind(p.id, to, user.id, Date.now()),
          b.leave
            ? db.prepare('DELETE FROM paper_members WHERE page_id = ?1 AND user_id = ?2').bind(p.id, p.owner_id)
            : db.prepare('UPDATE paper_members SET role = \'owner\' WHERE page_id = ?1 AND user_id = ?2').bind(p.id, p.owner_id),
        ]);
        await roomCall(p.id, 'flush', { owner: to });
        if (b.leave) await kick(p.id, { user: p.owner_id, why: 'You gave this page away' });
        return json({ ok: true, owner: to });
      }

      // ---- reports and the admin ---------------------------------------------------
      if ((m = path.match(/^\/pages\/([0-9a-f]{32})\/report$/)) && req.method === 'POST') {
        need();
        await access(m[1]);
        const b = await body();
        const reason = String(b.reason || '').trim().slice(0, cfg.maxReason);
        if (!reason) fail(400, 'Say what is wrong');
        if (await limited('create', 'u' + user.id)) fail(429, 'Too many reports at once');
        const r = await db.prepare('INSERT INTO paper_reports (page_id, reporter_id, reason, created_at) VALUES (?1, ?2, ?3, ?4) RETURNING id')
          .bind(m[1], user.id, reason, Date.now()).first();
        const hook = onReport && onReport({ id: r.id, page: m[1], reporter: user.id, reason }, env);
        if (hook && ctx?.waitUntil) ctx.waitUntil(Promise.resolve(hook).catch(() => {}));
        return json({ ok: true, id: r.id }, 201);
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
          const p = await page(m[1]);
          if (req.method === 'GET') {
            // The admin may look (view only) to judge a report.
            const stats = await (await room(p.id).fetch(new Request('https://room/stats', { headers: { 'x-paper-page': p.id } }))).json();
            const members = (await db.prepare('SELECT user_id, role FROM paper_members WHERE page_id = ?1').bind(p.id).all()).results;
            return json({ page: card(p, 'viewer'), blocked: !!p.blocked, members, stats, open: openInfo(p, 'viewer') });
          }
          if (req.method === 'DELETE') return json(await deletePage(p));
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
          await account(db, uid, cfg.freeQuota);
          if (b.quota != null) await db.prepare('UPDATE paper_accounts SET quota = ?1 WHERE user_id = ?2').bind(Math.max(0, Number(b.quota) | 0), uid).run();
          if (b.blocked != null) await db.prepare('UPDATE paper_accounts SET blocked = ?1 WHERE user_id = ?2').bind(b.blocked ? 1 : 0, uid).run();
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

    async function enter(p, role, u, via) {
      const h = new Headers(req.headers);
      h.set('x-paper-page', p.id);
      h.set('x-paper-owner', p.owner_id);
      h.set('x-paper-user', u.id);
      h.set('x-paper-name', String(u.name || '').slice(0, 40));
      h.set('x-paper-role', role);
      if (via) h.set('x-paper-via', via); else h.delete('x-paper-via');
      return room(p.id).fetch(new Request('https://room/relay', { headers: h }));
    }

    async function deletePage(p) {
      const r = await (await roomCall(p.id, 'destroy', {}, { 'x-paper-owner': p.owner_id })).json().catch(() => ({}));
      await db.batch([
        db.prepare('DELETE FROM paper_members WHERE page_id = ?1').bind(p.id),
        db.prepare('DELETE FROM paper_links WHERE page_id = ?1').bind(p.id),
        db.prepare('DELETE FROM paper_pages WHERE id = ?1').bind(p.id),
      ]);
      return { ok: true, freed: r.bytes || 0 };
    }
  };
}
