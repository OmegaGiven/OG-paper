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

import { DEFAULTS } from './config.js';
import { Lease } from './quota.js';
import { KIND, READY, TAG, parseFrame, seqFrame, wants } from './protocol.js';

const EDIT = new Set(['owner', 'editor']);
const IDLE_MS = 30_000;
const SYNC_MS = 60_000;

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
    this.cfg = { ...DEFAULTS, ...(env.PAPER_CONFIG || {}), ...config };
    this.sql = ctx.storage.sql;
    this.sql.exec(`CREATE TABLE IF NOT EXISTS frames (
      seq INTEGER PRIMARY KEY AUTOINCREMENT, kind INTEGER NOT NULL, tiles TEXT NOT NULL DEFAULT '',
      author TEXT NOT NULL, at INTEGER NOT NULL, size INTEGER NOT NULL, body BLOB, store TEXT NOT NULL DEFAULT '')`);
    this.sql.exec('CREATE TABLE IF NOT EXISTS chunks (seq INTEGER NOT NULL, i INTEGER NOT NULL, body BLOB NOT NULL, PRIMARY KEY (seq, i))');
    this.sql.exec('CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT)');
    this.lease = new Lease(env.DB, this.cfg.leaseChunk,
      () => ({ owner: this.meta('owner'), lease: Number(this.meta('lease') || 0) }),
      (owner, lease) => { this.setMeta('owner', owner || ''); this.setMeta('lease', String(lease)); });
    // Kept frames are handled one at a time, in arrival order.
    this.queue = Promise.resolve();
    this.buckets = new Map();
  }

  meta(k) { return this.sql.exec('SELECT v FROM meta WHERE k = ?', k).toArray()[0]?.v ?? null; }
  setMeta(k, v) { this.sql.exec('INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT (k) DO UPDATE SET v = excluded.v', k, v); }
  bytes() { return Number(this.sql.exec('SELECT COALESCE(SUM(size), 0) AS n FROM frames').toArray()[0].n); }
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
      case '/stats': return json({ bytes: this.bytes(), frames: this.frameCount(), sockets: this.sockets().length, lease: Number(this.meta('lease') || 0), head: this.head() });
      default: return json({ error: 'not found' }, 404);
    }
  }

  frameCount() { return Number(this.sql.exec('SELECT COUNT(*) AS n FROM frames').toArray()[0].n); }

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
    this.setAtt(server, { user, name: h.get('x-paper-name') || '', role, via, mode: null, subs: null, ready: 0, seen: 0 });
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
      if (!this.pinRoom(f.room)) return this.refuse(ws, 'This link\'s key does not belong to this page', true);
      a.mode = 'relay';
      this.setAtt(ws, a);
      return this.join(ws, a, 0);
    }
    if (!a.mode) return this.refuse(ws, 'Say which room first', true);
    if (!this.rate(ws, b.length)) return this.send(ws, { t: 'error', msg: 'Slow down: too many changes at once' });
    if (f.tag === TAG.EPH) {
      if (f.body.length > this.cfg.maxEphBytes) return;
      for (const o of this.sockets()) if (o !== ws && this.att(o).mode) this.sendRaw(o, f.body);
      return;
    }
    if (!EDIT.has(a.role)) return this.send(ws, { t: 'denied', msg: 'You can only view this page' });
    const ckpt = f.tag === TAG.CKPT || f.tag === TAG.TCKP;
    const cap = ckpt ? this.cfg.maxCheckpointBytes : this.cfg.maxOpBytes;
    if (f.body.length > cap) return this.send(ws, { t: 'error', msg: `That change is too big to keep (${f.body.length} bytes, the most is ${cap})` });
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
      if (m.room && !this.pinRoom(hexBytes(String(m.room)))) return this.refuse(ws, 'This link\'s key does not belong to this page', true);
      a.mode = m.seq ? 'seq' : 'relay';
      a.subs = Array.isArray(m.tiles) ? m.tiles.slice(0, this.cfg.maxSubs).map(String) : null;
      this.setAtt(ws, a);
      return this.join(ws, a, Math.max(0, Number(m.since) || 0));
    }
    if (m.t === 'subs' && a.mode) {
      // More tiles came into view: send what they hold (only those).
      const add = (Array.isArray(m.add) ? m.add : []).map(String).filter(t => !(a.subs || []).includes(t));
      const remove = new Set((Array.isArray(m.remove) ? m.remove : []).map(String));
      a.subs = [...(a.subs || []).filter(t => !remove.has(t)), ...add].slice(0, this.cfg.maxSubs);
      this.setAtt(ws, a);
      if (add.length) return this.backlog(ws, a, 0, add, false);
      return;
    }
    if (m.t === 'ping') return this.send(ws, { t: 'pong' });
  }

  /** The room is named by the key's room (`Keys::room`); the first one in pins it. */
  pinRoom(room) {
    if (!room || room.length !== 32) return false;
    const hex = [...room].map(x => x.toString(16).padStart(2, '0')).join('');
    const pinned = this.meta('room');
    if (!pinned) { this.setMeta('room', hex); return true; }
    return pinned === hex;
  }

  async join(ws, a, since) {
    await this.backlog(ws, a, since, a.subs, true);
    a = this.att(ws);
    a.ready = this.head();
    a.seen = Math.max(a.seen, a.ready);
    this.setAtt(ws, a);
    if (a.mode === 'seq') this.send(ws, { t: 'ready', head: a.ready, role: a.role });
    else this.sendRaw(ws, READY);
    this.send(ws, { t: 'welcome', role: a.role, name: a.name, head: a.ready, bytes: this.bytes() });
    this.peers();
  }

  /** Send kept frames after `since` (only those touching `tiles`, if given). */
  async backlog(ws, a, since, tiles, all) {
    const rows = this.sql.exec('SELECT seq, kind, tiles, size, body, store FROM frames WHERE seq > ? ORDER BY seq', since).toArray();
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
    const kind = tag === TAG.CKPT ? KIND.CKPT : tag === TAG.TCKP ? KIND.TCKP : KIND.PUT;
    // What a full copy stands for: everything its sender had when making it.
    const upTo = kind === KIND.PUT ? 0 : (a.mode === 'relay' ? a.ready : a.seen);
    let covered = [];
    if (kind === KIND.CKPT) {
      covered = this.sql.exec('SELECT seq, size, store FROM frames WHERE seq <= ?', upTo).toArray();
    } else if (kind === KIND.TCKP) {
      covered = this.sql.exec('SELECT seq, size, store FROM frames WHERE seq <= ? AND tiles = ?', upTo, tiles[0]).toArray();
    }
    const freed = covered.reduce((n, r) => n + Number(r.size), 0);
    const net = body.length - freed;
    if (net > 0 && !(await this.lease.spend(net))) {
      const owner = this.meta('owner');
      const acct = await this.env.DB.prepare('SELECT quota, used FROM paper_accounts WHERE user_id = ?1').bind(owner).first();
      return this.send(ws, {
        t: 'full', owner: owner === a.user,
        used: acct?.used ?? null, quota: acct?.quota ?? null,
        msg: owner === a.user
          ? 'Your storage is full: this change was not saved to your account. Export or delete pages to free space.'
          : 'The owner\'s storage is full: this change was not saved. Ask them to free space, or make a copy into your own account.',
      });
    }
    let seq;
    const store = body.length > this.cfg.chunkBytes ? (this.env.PAPER_BUCKET ? 'r2' : 'chunks') : '';
    this.ctx.storage.transactionSync(() => {
      for (const r of covered) {
        this.sql.exec('DELETE FROM frames WHERE seq = ?', r.seq);
        this.sql.exec('DELETE FROM chunks WHERE seq = ?', r.seq);
      }
      this.sql.exec('INSERT INTO frames (kind, tiles, author, at, size, body, store) VALUES (?, ?, ?, ?, ?, ?, ?)',
        kind, tiles.join('\n'), a.user, Date.now(), body.length, store === '' ? body : null, store);
      seq = Number(this.sql.exec('SELECT last_insert_rowid() AS s').toArray()[0].s);
      if (store === 'chunks') {
        for (let i = 0, at = 0; at < body.length; i++, at += this.cfg.chunkBytes) {
          this.sql.exec('INSERT INTO chunks (seq, i, body) VALUES (?, ?, ?)', seq, i, body.slice(at, at + this.cfg.chunkBytes));
        }
      }
    });
    if (store === 'r2') await this.env.PAPER_BUCKET.put(this.r2Key(seq), body);
    for (const r of covered) if (r.store === 'r2' && this.env.PAPER_BUCKET) await this.env.PAPER_BUCKET.delete(this.r2Key(r.seq));
    if (net < 0) await this.lease.free(-net);
    this.setMeta('dirty', '1');
    await this.alarmIn(SYNC_MS, false);
    // Pass it on (full copies too: they may hold what others missed).
    for (const o of this.sockets()) {
      const oa = this.att(o);
      if (!oa.mode) continue;
      if (o === ws) {
        if (oa.mode === 'seq') { this.send(o, { t: 'ack', seq }); oa.seen = Math.max(oa.seen, seq); this.setAtt(o, oa); }
        continue;
      }
      if (!wants(oa.subs, tiles)) continue;
      if (oa.mode === 'seq') { this.sendRaw(o, seqFrame(seq, body)); oa.seen = Math.max(oa.seen, seq); this.setAtt(o, oa); }
      else this.sendRaw(o, body);
    }
    if (kind === KIND.PUT && this.frameCount() > this.cfg.maxFramesBeforeCheckpoint && a.mode === 'seq') {
      this.send(ws, { t: 'compact' });
    }
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

  async webSocketClose(ws, code, reason) {
    this.buckets.delete(ws);
    try { ws.close(code, reason); } catch (e) {}
    this.afterLeave(ws);
  }
  async webSocketError(ws) { this.buckets.delete(ws); this.afterLeave(ws); }

  afterLeave(ws) {
    const left = this.sockets().filter(o => o !== ws);
    if (left.length === 0) this.alarmIn(IDLE_MS, true);
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
      this.send(ws, { t: 'kicked', msg: why || 'Your access to this page changed' });
      try { ws.close(4003, 'access changed'); } catch (e) {}
    }
    return json({ kicked: list.length });
  }

  // ---- bookkeeping ------------------------------------------------------

  async alarmIn(ms, force) {
    const at = await this.ctx.storage.getAlarm();
    if (force || at == null || at > Date.now() + ms) await this.ctx.storage.setAlarm(Date.now() + ms);
  }

  async alarm() {
    await this.syncPage();
    if (this.sockets().length === 0) await this.lease.release();
    else if (this.meta('dirty') === '1') await this.alarmIn(SYNC_MS, true);
  }

  /** Tell the directory what the page keeps and when it last changed. */
  async syncPage() {
    const page = this.meta('page');
    if (!page || !this.env.DB) return;
    if (this.meta('dirty') !== '1') return;
    await this.env.DB.prepare('UPDATE paper_pages SET bytes = ?1, updated_at = ?2 WHERE id = ?3').bind(this.bytes(), Date.now(), page).run();
    this.setMeta('dirty', '0');
  }

  /** Before the page changes billing owner: settle up with the old one. */
  async flush(body) {
    await this.queue;
    this.setMeta('dirty', '1');
    await this.syncPage();
    await this.lease.release();
    if (body.owner) this.setMeta('owner', String(body.owner));
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

