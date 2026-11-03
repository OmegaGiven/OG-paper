// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// A page's room: one Durable Object per page. It is a relay (the same
// protocol as `og-paper --relay`, so every OG Paper app can use it) that
// knows who is connected and what they may do, keeps the page's log in its
// own SQLite storage (big frames in R2 when bound), charges what it keeps to
// the page owner's quota, and passes live frames between the people on the
// page. It never reads the canvas: frames are sealed end to end.
//
// Only the API Worker reaches a room (it is not routable from outside), and
// it tells the room who each connection is with x-paper-* headers after
// checking the session and the page's permissions.

import { configFrom } from './config.js';
import { Chat, cleanName } from './chat.js';
import { Lease, charge } from './quota.js';
import { KIND, READY, TAG, parseFrame, seqFrame, wants } from './protocol.js';

const EDIT = new Set(['owner', 'editor']);
const IDLE_MS = 30_000;
const SYNC_MS = 60_000;
const DAY = 86_400_000;

/** Seams for tests (Node has no WebSocketPair and no 101 responses). */
export const rt = {
  pair: () => { const p = new WebSocketPair(); return [p[0], p[1]]; },
  upgraded: client => new Response(null, { status: 101, webSocket: client }),
};

const json = (data, status = 200) => new Response(JSON.stringify(data), { status, headers: { 'content-type': 'application/json' } });

class Bucket {
  constructor(rate, burst) { this.rate = rate; this.burst = burst; this.level = burst; this.at = Date.now(); }
  take(n = 1) {
    const now = Date.now();
    this.level = Math.min(this.burst, this.level + ((now - this.at) / 1000) * this.rate);
    this.at = now;
    if (this.level < n) return false;
    this.level -= n;
    return true;
  }
}

export class PaperRoom {
  constructor(ctx, env, config = {}) {
    this.ctx = ctx;
    this.env = env;
    this.cfg = configFrom(env, { ...(env.PAPER_CONFIG || {}), ...config });
    this.sql = ctx.storage.sql;
    this.sql.exec(`CREATE TABLE IF NOT EXISTS frames (
      seq INTEGER PRIMARY KEY AUTOINCREMENT, kind INTEGER NOT NULL, tiles TEXT NOT NULL DEFAULT '',
      author TEXT NOT NULL, at INTEGER NOT NULL, size INTEGER NOT NULL, body BLOB, store TEXT NOT NULL DEFAULT '')`);
    // What a full copy replaced is kept keepReplacedDays (gone: when, by: the
    // copy's seq) so an owner or the admin can bring it back; rooms made
    // before this get the two columns here.
    try { this.sql.exec('SELECT gone, by FROM frames LIMIT 0'); } catch (e) {
      this.sql.exec('ALTER TABLE frames ADD COLUMN gone INTEGER');
      this.sql.exec('ALTER TABLE frames ADD COLUMN by INTEGER');
    }
    this.sql.exec('CREATE TABLE IF NOT EXISTS chunks (seq INTEGER NOT NULL, i INTEGER NOT NULL, body BLOB NOT NULL, PRIMARY KEY (seq, i))');
    this.sql.exec('CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT)');
    // Tile subscriptions per socket (a socket's attachment holds at most 2 KB).
    this.sql.exec('CREATE TABLE IF NOT EXISTS subs (sock TEXT NOT NULL, tile TEXT NOT NULL, PRIMARY KEY (sock, tile))');
    this.lease = new Lease(env.DB, this.cfg.leaseChunk,
      () => ({ owner: this.meta('owner'), lease: Number(this.meta('lease') || 0) }),
      (owner, lease) => { this.setMeta('owner', owner || ''); this.setMeta('lease', String(lease)); },
      acct => this.usageToOwner(acct));
    // Kept frames are handled one at a time, in arrival order.
    this.queue = Promise.resolve();
    this.buckets = new Map();
    this.subCache = new Map();
    this.chat = new Chat(this);
  }

  /** A socket's tiles, or null when it takes the whole page. */
  subsOf(a) {
    if (!a.tiled) return null;
    let t = this.subCache.get(a.id);
    if (!t) {
      t = this.sql.exec('SELECT tile FROM subs WHERE sock = ?', a.id).toArray().map(r => r.tile);
      this.subCache.set(a.id, t);
    }
    return t;
  }
  setSubs(a, tiles) {
    this.sql.exec('DELETE FROM subs WHERE sock = ?', a.id);
    for (const t of tiles) this.sql.exec('INSERT OR IGNORE INTO subs (sock, tile) VALUES (?, ?)', a.id, t);
    this.subCache.set(a.id, tiles);
  }

  meta(k) { return this.sql.exec('SELECT v FROM meta WHERE k = ?', k).toArray()[0]?.v ?? null; }
  setMeta(k, v) { this.sql.exec('INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT (k) DO UPDATE SET v = excluded.v', k, v); }
  /** What the page keeps (and its owner pays for): not what full copies replaced. */
  bytes() { return Number(this.sql.exec('SELECT COALESCE(SUM(size), 0) AS n FROM frames WHERE gone IS NULL').toArray()[0].n); }
  replacedBytes() { return Number(this.sql.exec('SELECT COALESCE(SUM(size), 0) AS n FROM frames WHERE gone IS NOT NULL').toArray()[0].n); }
  head() { return Number(this.sql.exec('SELECT COALESCE(MAX(seq), 0) AS n FROM frames').toArray()[0].n); }
  sockets() { return this.ctx.getWebSockets(); }
  att(ws) { return ws.deserializeAttachment() || {}; }
  setAtt(ws, a) { ws.serializeAttachment(a); }

  async fetch(req) {
    const url = new URL(req.url);
    const h = req.headers;
    if (h.get('x-paper-owner')) this.setMeta('owner', this.meta('owner') || h.get('x-paper-owner'));
    if (h.get('x-paper-page')) this.setMeta('page', h.get('x-paper-page'));
    switch (url.pathname) {
      case '/relay': return this.connect(req);
      case '/kick': return this.kick(await req.json());
      case '/flush': return this.flush(await req.json().catch(() => ({})));
      case '/destroy': return this.destroy();
      case '/pin': { const b = await req.json().catch(() => ({})); return json({ ok: this.pinRoom(hexBytes(String(b.room || '')), { role: 'owner' }) === true }); }
      case '/trash': return this.trash(await req.json().catch(() => ({})));
      case '/untrash': this.setMeta('trashed', ''); return json({ ok: true });
      case '/history': return json(this.history());
      case '/restore': return this.restore(await req.json().catch(() => ({})));
      case '/chat-settings': return json(this.chat.set(await req.json().catch(() => ({}))));
      case '/chat-clear': return json(this.chat.clear());
      case '/chat-export': return json(this.chat.export());
      case '/chat-get': return json(this.chat.welcome());
      case '/stats': return json({ chat: this.chat.count(), bytes: this.bytes(), replaced: this.replacedBytes(), frames: this.frameCount(), sockets: this.sockets().length, lease: Number(this.meta('lease') || 0), head: this.head() });
      default: return json({ error: 'not found' }, 404);
    }
  }

  frameCount() { return Number(this.sql.exec('SELECT COUNT(*) AS n FROM frames WHERE gone IS NULL').toArray()[0].n); }

  connect(req) {
    if (req.headers.get('Upgrade') !== 'websocket') return json({ error: 'expected a WebSocket' }, 426);
    const h = req.headers;
    const user = h.get('x-paper-user') || '';
    const role = h.get('x-paper-role') || '';
    if (!['owner', 'editor', 'viewer'].includes(role)) return json({ error: 'no access' }, 403);
    if (this.sockets().length >= this.cfg.maxSockets) return json({ error: 'This page has too many people on it right now' }, 503);
    const [client, server] = rt.pair();
    const via = h.get('x-paper-via') || '';
    this.ctx.acceptWebSocket(server, ['u:' + user, ...(via ? ['v:' + via] : [])]);
    this.setAtt(server, { user, name: cleanName(h.get('x-paper-name'), 40), role, via, mode: null, tiled: false, ready: 0, seen: 0, id: crypto.randomUUID() });
    return rt.upgraded(client);
  }

  // ---- messages ---------------------------------------------------------

  async webSocketMessage(ws, msg) {
    if (typeof msg === 'string') return this.control(ws, msg);
    const b = new Uint8Array(msg);
    const a = this.att(ws);
    const f = parseFrame(b);
    if (!f) return this.refuse(ws, 'That is not something this page understands');
    if (f.tag === TAG.ROOM) {
      if (a.mode) return;
      const pin = this.pinRoom(f.room, a);
      if (pin !== true) return this.refuse(ws, pin || 'This link\'s key does not belong to this page', true);
      a.mode = 'relay';
      this.setAtt(ws, a);
      return this.join(ws, a, 0);
    }
    if (!a.mode) return this.refuse(ws, 'Say which room first', true);
    // drop: a change was not kept (the page then knows it is not saved).
    if (!this.rate(ws, b.length)) return this.send(ws, { t: 'error', msg: 'Slow down: too many changes at once', drop: f.tag !== TAG.EPH });
    if (f.tag === TAG.EPH) {
      if (f.body.length > this.cfg.maxEphBytes) return;
      for (const o of this.sockets()) if (o !== ws && this.att(o).mode) this.sendRaw(o, f.body);
      return;
    }
    if (!EDIT.has(a.role)) return this.send(ws, { t: 'denied', msg: 'You can only view this page', drop: true });
    const ckpt = f.tag === TAG.CKPT || f.tag === TAG.TCKP;
    // A copy that sees only some tiles cannot stand for the whole page.
    if (f.tag === TAG.CKPT && a.tiled) return this.send(ws, { t: 'denied', msg: 'A full copy needs the whole page: subscribe to everything first', drop: true });
    const cap = ckpt ? this.cfg.maxCheckpointBytes : this.cfg.maxOpBytes;
    if (f.body.length > cap) return this.send(ws, { t: 'error', msg: `That change is too big to keep (${f.body.length} bytes, the most is ${cap})`, drop: true });
    const body = f.body.slice();
    const job = this.queue.then(() => this.keep(ws, a, f.tag, f.tiles || [], body));
    this.queue = job.catch(e => console.error('og-paper room', e));
    return job;
  }

  control(ws, text) {
    if (text.length > 64 * 1024) return this.refuse(ws, 'Too big');
    let m;
    try { m = JSON.parse(text); } catch (e) { return this.refuse(ws, 'Bad message'); }
    const a = this.att(ws);
    if (m.t === 'hello' && !a.mode) {
      const pin = m.room ? this.pinRoom(hexBytes(String(m.room)), a) : true;
      if (pin !== true) return this.refuse(ws, pin || 'This link\'s key does not belong to this page', true);
      a.mode = m.seq ? 'seq' : 'relay';
      a.tiled = Array.isArray(m.tiles);
      if (a.tiled) this.setSubs(a, m.tiles.slice(0, this.cfg.maxSubs).map(t => String(t).slice(0, 255)));
      this.setAtt(ws, a);
      return this.join(ws, a, Math.max(0, Number(m.since) || 0));
    }
    if (m.t === 'subs' && a.mode) {
      // More tiles came into view: send what they hold (only those).
      const had = this.subsOf(a) || [];
      const add = (Array.isArray(m.add) ? m.add : []).map(t => String(t).slice(0, 255)).filter(t => !had.includes(t));
      const remove = new Set((Array.isArray(m.remove) ? m.remove : []).map(String));
      a.tiled = true;
      this.setSubs(a, [...had.filter(t => !remove.has(t)), ...add].slice(0, this.cfg.maxSubs));
      this.setAtt(ws, a);
      if (add.length) return this.backlog(ws, a, 0, add, false);
      return;
    }
    if (m.t === 'ping') return this.send(ws, { t: 'pong' });
    if (a.mode && (m.t === 'chat' || m.t === 'chat-del' || m.t === 'chat-react' || m.t === 'typing')) {
      if (!this.rate(ws, text.length)) return this.send(ws, { t: 'chat-error', msg: 'Slow down: too many messages at once' });
      return this.chat.handle(ws, a, m);
    }
  }

  /**
   * The room is named by the key's room (`Keys::room`). The first one an
   * owner or editor brings pins it (a viewer can't, so nobody with only a
   * view link can brick a new page), and an owner may re-pin a room that
   * has never kept anything. true, or why not.
   */
  pinRoom(room, a) {
    if (!room || room.length !== 32) return false;
    const hex = [...room].map(x => x.toString(16).padStart(2, '0')).join('');
    const pinned = this.meta('room');
    if (pinned === hex) return true;
    if (!pinned) {
      if (!EDIT.has(a.role)) return 'This page has not been opened by its owner yet: try again once they have';
      this.setMeta('room', hex);
      return true;
    }
    if (a.role === 'owner' && Number(this.sql.exec('SELECT COUNT(*) AS n FROM frames').toArray()[0].n) === 0) {
      this.setMeta('room', hex);
      return true;
    }
    return false;
  }

  async join(ws, a, since) {
    await this.backlog(ws, a, since, this.subsOf(a), true);
    a = this.att(ws);
    a.ready = this.head();
    a.seen = Math.max(a.seen, a.ready);
    this.setAtt(ws, a);
    if (a.mode === 'seq') this.send(ws, { t: 'ready', head: a.ready, role: a.role });
    else this.sendRaw(ws, READY);
    // chat: { on, keep, max, … } when the site has chat (null: none; older
    // rooms send nothing, and the page then shows no chat at all).
    this.send(ws, { t: 'welcome', role: a.role, name: a.name, user: a.user, head: a.ready, bytes: this.bytes(), chat: this.chat.welcome() });
    this.chat.joined(ws);
    if (a.user === this.meta('owner') && this.env.DB) {
      this.usageToOwner(await this.env.DB.prepare('SELECT used, quota FROM paper_accounts WHERE user_id = ?1').bind(a.user).first());
    }
    this.peers();
  }

  /** Send kept frames after `since` (only those touching `tiles`, if given). */
  async backlog(ws, a, since, tiles, all) {
    const rows = this.sql.exec('SELECT seq, kind, tiles, size, body, store FROM frames WHERE seq > ? AND gone IS NULL ORDER BY seq', since).toArray();
    let seen = a.seen;
    for (const r of rows) {
      const t = r.tiles ? r.tiles.split('\n') : [];
      // Untagged frames go to everyone once (on join), not again per tile.
      if (!all && t.length === 0) continue;
      if (!wants(tiles, t)) continue;
      const body = await this.body(r);
      if (!body) continue;
      if (a.mode === 'seq') this.sendRaw(ws, seqFrame(r.seq, body));
      else this.sendRaw(ws, body);
      seen = Math.max(seen, r.seq);
    }
    const now = this.att(ws);
    now.seen = Math.max(now.seen || 0, seen);
    this.setAtt(ws, now);
  }

  async body(r) {
    if (r.store === '') return new Uint8Array(r.body);
    if (r.store === 'chunks') {
      const parts = this.sql.exec('SELECT body FROM chunks WHERE seq = ? ORDER BY i', r.seq).toArray().map(c => new Uint8Array(c.body));
      const out = new Uint8Array(r.size);
      let at = 0;
      for (const p of parts) { out.set(p, at); at += p.length; }
      return out;
    }
    if (r.store === 'r2' && this.env.PAPER_BUCKET) {
      const o = await this.env.PAPER_BUCKET.get(this.r2Key(r.seq));
      return o ? new Uint8Array(await o.arrayBuffer()) : null;
    }
    return null;
  }

  r2Key(seq) { return `og-paper/${this.meta('page') || 'page'}/${seq}`; }

  /** Keep a change or a full copy, charge it to the owner, pass it on. */
  async keep(ws, a, tag, tiles, body) {
    a = { ...a, ...this.att(ws) };
    let kind = tag === TAG.CKPT ? KIND.CKPT : tag === TAG.TCKP ? KIND.TCKP : KIND.PUT;
    // What a full copy stands for: everything its sender had when making it.
    const upTo = kind === KIND.PUT ? 0 : (a.mode === 'relay' ? a.ready : a.seen);
    let covered = [];
    if (kind === KIND.CKPT) {
      covered = this.sql.exec('SELECT seq, size, store FROM frames WHERE seq <= ? AND gone IS NULL', upTo).toArray();
    } else if (kind === KIND.TCKP) {
      covered = this.sql.exec('SELECT seq, size, store FROM frames WHERE seq <= ? AND tiles = ? AND gone IS NULL', upTo, tiles[0]).toArray();
    }
    let freed = covered.reduce((n, r) => n + Number(r.size), 0);
    // A copy far smaller than what it would replace, from someone who isn't
    // an owner (an editor, an edit link, a broken app), replaces nothing: it
    // is kept as an ordinary change, so it can't empty the page.
    if (covered.length && a.role !== 'owner' && freed >= this.cfg.tinyCheckpointMin && body.length < freed * this.cfg.tinyCheckpointRatio) {
      covered = []; freed = 0; kind = KIND.PUT;
    }
    const net = body.length - freed;
    if (net > 0 && !(await this.lease.spend(net))) {
      const owner = this.meta('owner');
      const acct = await this.env.DB.prepare('SELECT quota, used FROM paper_accounts WHERE user_id = ?1').bind(owner).first();
      return this.send(ws, {
        t: 'full', drop: true, owner: owner === a.user,
        used: acct?.used ?? null, quota: acct?.quota ?? null,
        msg: owner === a.user
          ? 'Your storage is full: this change was not saved to your account. Export or delete pages to free space.'
          : 'The owner\'s storage is full: this change was not saved. Ask them to free space, or make a copy into your own account.',
      });
    }
    let seq;
    const store = body.length > this.cfg.chunkBytes ? (this.env.PAPER_BUCKET ? 'r2' : 'chunks') : '';
    const now = Date.now();
    this.ctx.storage.transactionSync(() => {
      this.sql.exec('INSERT INTO frames (kind, tiles, author, at, size, body, store) VALUES (?, ?, ?, ?, ?, ?, ?)',
        kind, tiles.join('\n'), a.user, now, body.length, store === '' ? body : null, store);
      seq = Number(this.sql.exec('SELECT last_insert_rowid() AS s').toArray()[0].s);
      // Replaced, not deleted: kept keepReplacedDays (see restore()).
      for (const r of covered) this.sql.exec('UPDATE frames SET gone = ?, by = ? WHERE seq = ?', now, seq, r.seq);
      if (store === 'chunks') {
        for (let i = 0, at = 0; at < body.length; i++, at += this.cfg.chunkBytes) {
          this.sql.exec('INSERT INTO chunks (seq, i, body) VALUES (?, ?, ?)', seq, i, body.slice(at, at + this.cfg.chunkBytes));
        }
      }
    });
    if (store === 'r2') await this.env.PAPER_BUCKET.put(this.r2Key(seq), body);
    if (net < 0) await this.lease.free(-net);
    if (covered.length) await this.alarmIn(this.cfg.keepReplacedDays * DAY, false);
    if (!this.dirty) { this.dirty = true; this.setMeta('dirty', '1'); await this.alarmIn(SYNC_MS, false); }
    // Pass it on (full copies too: they may hold what others missed).
    for (const o of this.sockets()) {
      const oa = this.att(o);
      if (!oa.mode) continue;
      if (o === ws) {
        // Kept: the sender's page can tell its changes are safe in the account.
        if (oa.mode === 'seq') { this.send(o, { t: 'ack', seq }); oa.seen = Math.max(oa.seen, seq); this.setAtt(o, oa); }
        else this.send(o, { t: 'kept', seq });
        continue;
      }
      if (!wants(this.subsOf(oa), tiles)) continue;
      if (oa.mode === 'seq') { this.sendRaw(o, seqFrame(seq, body)); oa.seen = Math.max(oa.seen, seq); this.setAtt(o, oa); }
      else this.sendRaw(o, body);
    }
    if (kind === KIND.PUT && this.frameCount() > this.cfg.maxFramesBeforeCheckpoint && a.mode === 'seq') {
      this.send(ws, { t: 'compact' });
    }
  }

  /** Tell the owner's open sockets how full their storage is. */
  usageToOwner(acct) {
    if (!acct || acct.used == null) return;
    const owner = this.meta('owner');
    for (const o of this.ctx.getWebSockets('u:' + owner)) if (this.att(o).mode) this.send(o, { t: 'usage', used: acct.used, quota: acct.quota });
  }

  // ---- connections ------------------------------------------------------

  rate(ws, n) {
    let b = this.buckets.get(ws);
    if (!b) { b = [new Bucket(this.cfg.frameRate, this.cfg.frameBurst), new Bucket(this.cfg.byteRate, this.cfg.byteBurst)]; this.buckets.set(ws, b); }
    return b[0].take(1) && b[1].take(n);
  }

  send(ws, m) { try { ws.send(JSON.stringify(m)); } catch (e) { /* closed */ } }
  sendRaw(ws, b) { try { ws.send(b); } catch (e) { /* closed */ } }
  refuse(ws, msg, close = false) {
    this.send(ws, { t: 'error', msg });
    if (close) try { ws.close(4400, msg.slice(0, 120)); } catch (e) {}
  }

  /** Who is here (checked identities, for the page's people list). */
  peers() {
    const seen = new Map();
    for (const o of this.sockets()) {
      const a = this.att(o);
      if (a.mode && !seen.has(a.user)) seen.set(a.user, { user: a.user, name: a.name, role: a.role });
    }
    const list = [...seen.values()];
    for (const o of this.sockets()) if (this.att(o).mode) this.send(o, { t: 'peers', list });
  }

  forget(ws) {
    this.buckets.delete(ws);
    const a = this.att(ws);
    if (a.id) { this.sql.exec('DELETE FROM subs WHERE sock = ?', a.id); this.subCache.delete(a.id); }
  }

  async webSocketClose(ws, code, reason) {
    this.forget(ws);
    try { ws.close(code, reason); } catch (e) {}
    this.afterLeave(ws);
  }
  async webSocketError(ws) { this.forget(ws); this.afterLeave(ws); }

  afterLeave(ws) {
    const left = this.sockets().filter(o => o !== ws);
    if (left.length === 0) { this.chat.empty(); this.alarmIn(IDLE_MS, true); }
    else {
      const list = [];
      const seen = new Set();
      for (const o of left) { const a = this.att(o); if (a.mode && !seen.has(a.user)) { seen.add(a.user); list.push({ user: a.user, name: a.name, role: a.role }); } }
      for (const o of left) if (this.att(o).mode) this.send(o, { t: 'peers', list });
    }
  }

  /** Close the connections of a user, of a link, or everyone's. */
  kick({ user, via, why }) {
    const tag = user != null ? 'u:' + user : via ? 'v:' + via : undefined;
    const list = tag ? this.ctx.getWebSockets(tag) : this.sockets();
    for (const ws of list) {
      this.forget(ws);
      this.send(ws, { t: 'kicked', msg: why || 'Your access to this page changed' });
      try { ws.close(4003, 'access changed'); } catch (e) {}
    }
    return json({ kicked: list.length });
  }

  // ---- bookkeeping ------------------------------------------------------

  async alarmIn(ms, force) {
    const at = await this.ctx.storage.getAlarm();
    if (force || at == null || at <= Date.now() || at > Date.now() + ms) await this.ctx.storage.setAlarm(Date.now() + ms);
  }

  async alarm() {
    if (await this.purgeTrash()) return;
    await this.syncPage();
    await this.purgeReplaced();
    if (this.sockets().length === 0) { this.chat.empty(); await this.lease.release(); }
    else if (this.meta('dirty') === '1') await this.alarmIn(SYNC_MS, true);
    await this.laterAlarm();
  }

  /** The next thing due: the trash running out, or replaced frames ageing out. */
  async laterAlarm() {
    const due = [];
    const t = Number(this.meta('trash-until') || 0);
    if (this.meta('trashed') && t) due.push(t);
    const g = this.sql.exec('SELECT MIN(gone) AS g FROM frames WHERE gone IS NOT NULL').toArray()[0].g;
    if (g != null) due.push(Number(g) + this.cfg.keepReplacedDays * DAY);
    if (due.length) await this.alarmIn(Math.max(1000, Math.min(...due) - Date.now()), false);
  }

  /** Drop what full copies replaced more than keepReplacedDays ago, and the oldest beyond keepReplacedBytes. */
  async purgeReplaced(now = Date.now()) {
    const cutoff = now - this.cfg.keepReplacedDays * DAY;
    const drop = this.sql.exec('SELECT seq, size, store FROM frames WHERE gone IS NOT NULL AND gone <= ?', cutoff).toArray();
    let left = Number(this.sql.exec('SELECT COALESCE(SUM(size), 0) AS n FROM frames WHERE gone > ?', cutoff).toArray()[0].n);
    if (left > this.cfg.keepReplacedBytes) {
      for (const r of this.sql.exec('SELECT seq, size, store FROM frames WHERE gone > ? ORDER BY gone, seq', cutoff).toArray()) {
        if (left <= this.cfg.keepReplacedBytes) break;
        drop.push(r);
        left -= Number(r.size);
      }
    }
    if (!drop.length) return 0;
    this.ctx.storage.transactionSync(() => {
      for (const r of drop) { this.sql.exec('DELETE FROM frames WHERE seq = ?', r.seq); this.sql.exec('DELETE FROM chunks WHERE seq = ?', r.seq); }
    });
    if (this.env.PAPER_BUCKET) for (const r of drop) if (r.store === 'r2') await this.env.PAPER_BUCKET.delete(this.r2Key(r.seq));
    return drop.length;
  }

  /** The full copies whose replaced frames are still kept, newest first. */
  history() {
    const rows = this.sql.exec(`SELECT f.by AS seq, MIN(f.gone) AS at, COUNT(*) AS frames, SUM(f.size) AS bytes, c.author AS author, c.size AS size
      FROM frames f LEFT JOIN frames c ON c.seq = f.by WHERE f.gone IS NOT NULL GROUP BY f.by ORDER BY f.by DESC`).toArray();
    return {
      keepDays: this.cfg.keepReplacedDays,
      bytes: this.bytes(),
      replaced: rows.map(r => ({
        seq: Number(r.seq), at: Number(r.at), frames: Number(r.frames), bytes: Number(r.bytes),
        by: r.author ?? null, size: r.size == null ? null : Number(r.size), until: Number(r.at) + this.cfg.keepReplacedDays * DAY,
      })),
    };
  }

  /**
   * Bring back what full copies at seq >= since replaced. Nothing is taken
   * away (pages only add), so the page ends up with both; the owner is
   * charged for it again (even past the quota: getting a page back matters
   * more), and everyone on the page gets the frames now.
   */
  async restore({ since = 0 }) {
    await this.queue;
    const rows = this.sql.exec('SELECT seq, size FROM frames WHERE gone IS NOT NULL AND by >= ? ORDER BY seq', since).toArray();
    if (!rows.length) return json({ restored: 0, bytes: 0 });
    const bytes = rows.reduce((n, r) => n + Number(r.size), 0);
    this.ctx.storage.transactionSync(() => {
      for (const r of rows) this.sql.exec('UPDATE frames SET gone = NULL, by = NULL WHERE seq = ?', r.seq);
    });
    const owner = this.meta('owner');
    if (owner && this.env.DB) await charge(this.env.DB, owner, bytes);
    this.setMeta('dirty', '1');
    this.dirty = true;
    await this.syncPage();
    for (const r of rows) {
      const f = this.sql.exec('SELECT seq, kind, tiles, size, body, store FROM frames WHERE seq = ?', r.seq).toArray()[0];
      const body = f && await this.body(f);
      if (!body) continue;
      const t = f.tiles ? f.tiles.split('\n') : [];
      for (const o of this.sockets()) {
        const oa = this.att(o);
        if (!oa.mode || !wants(this.subsOf(oa), t)) continue;
        this.sendRaw(o, oa.mode === 'seq' ? seqFrame(f.seq, body) : body);
      }
    }
    return json({ restored: rows.length, bytes });
  }

  /** In the trash: everyone out; gone for good at `until` unless restored first. */
  async trash({ at, until }) {
    this.setMeta('trashed', String(at || Date.now()));
    this.setMeta('trash-until', String(until || Date.now() + this.cfg.trashDays * DAY));
    this.kick({ why: 'This page was moved to the trash' });
    await this.alarmIn(Math.max(1000, Number(this.meta('trash-until')) - Date.now()), false);
    return json({ ok: true });
  }

  /** The trash ran out: delete the page for good (room and directory), if it is still in the trash. */
  async purgeTrash(now = Date.now()) {
    const until = Number(this.meta('trash-until') || 0);
    if (!this.meta('trashed') || !until || now < until) return false;
    const page = this.meta('page');
    const db = this.env.DB;
    if (page && db) {
      const p = await db.prepare('SELECT deleted_at FROM paper_pages WHERE id = ?1').bind(page).first();
      if (p && p.deleted_at == null) { this.setMeta('trashed', ''); return false; }  // restored meanwhile
      await db.batch([
        db.prepare('DELETE FROM paper_members WHERE page_id = ?1').bind(page),
        db.prepare('DELETE FROM paper_links WHERE page_id = ?1').bind(page),
        db.prepare('DELETE FROM paper_transfers WHERE page_id = ?1').bind(page),
        db.prepare('DELETE FROM paper_pages WHERE id = ?1').bind(page),
      ]);
    }
    await this.destroy();
    return true;
  }

  /** Tell the directory what the page keeps and when it last changed. */
  async syncPage() {
    const page = this.meta('page');
    if (!page || !this.env.DB) return;
    if (this.meta('dirty') !== '1') return;
    await this.env.DB.prepare('UPDATE paper_pages SET bytes = ?1, updated_at = ?2 WHERE id = ?3').bind(this.bytes(), Date.now(), page).run();
    this.setMeta('dirty', '0');
    this.dirty = false;
  }

  /** Before the page changes billing owner: settle up with the old one. */
  async flush(body) {
    await this.queue;
    this.setMeta('dirty', '1');
    this.dirty = true;
    await this.syncPage();
    await this.lease.release();
    if (body.owner) this.setMeta('owner', String(body.owner));
    this.lease.left = null;
    return json({ bytes: this.bytes() });
  }

  /** Delete everything the page keeps, giving its bytes back to the owner. */
  async destroy() {
    await this.queue;
    const bytes = this.bytes();
    await this.lease.release();
    await this.lease.free(bytes);
    for (const ws of this.sockets()) {
      this.send(ws, { t: 'kicked', msg: 'This page was deleted' });
      try { ws.close(4004, 'deleted'); } catch (e) {}
    }
    if (this.env.PAPER_BUCKET) {
      for (const r of this.sql.exec("SELECT seq FROM frames WHERE store = 'r2'").toArray()) await this.env.PAPER_BUCKET.delete(this.r2Key(r.seq));
    }
    await this.ctx.storage.deleteAlarm();
    await this.ctx.storage.deleteAll();
    return json({ bytes });
  }
}

function hexBytes(s) {
  if (!/^[0-9a-f]{64}$/i.test(s)) return null;
  const out = new Uint8Array(32);
  for (let i = 0; i < 32; i++) out[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
  return out;
}

