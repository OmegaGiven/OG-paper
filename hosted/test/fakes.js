// Stand-ins for the Workers runtime, enough to run the API and rooms in
// Node: D1 and Durable Object storage on node:sqlite, WebSocket pairs that
// deliver in memory, Durable Object namespaces, and an auth adapter.

import { DatabaseSync } from 'node:sqlite';
import { readFileSync } from 'node:fs';
import { createPaperApi, PaperRoom } from '../src/index.js';
import { rt } from '../src/room.js';

const MIGRATION = readFileSync(new URL('../migrations/0001_og_paper.sql', import.meta.url), 'utf8');

const plain = v => (v instanceof Uint8Array ? v : v);

export class FakeD1 {
  constructor() {
    this.db = new DatabaseSync(':memory:');
    this.db.exec('PRAGMA foreign_keys = ON');
    this.db.exec(MIGRATION);
  }
  prepare(sql) {
    const db = this.db;
    const make = args => ({
      args,
      sql,
      bind: (...a) => make(a),
      first: async () => db.prepare(sql).get(...args.map(plain)) ?? null,
      all: async () => ({ results: db.prepare(sql).all(...args.map(plain)) }),
      run: async () => { const r = db.prepare(sql).run(...args.map(plain)); return { meta: { changes: Number(r.changes), last_row_id: Number(r.lastInsertRowid) } }; },
      _run: () => db.prepare(sql).run(...args.map(plain)),
    });
    return make([]);
  }
  async batch(stmts) {
    this.db.exec('BEGIN');
    try { for (const s of stmts) s._run(); this.db.exec('COMMIT'); } catch (e) { this.db.exec('ROLLBACK'); throw e; }
    return stmts.map(() => ({ meta: {} }));
  }
  row(sql, ...a) { return this.db.prepare(sql).get(...a); }
}

class Cursor {
  constructor(rows) { this.rows = rows; }
  toArray() { return this.rows; }
  one() { return this.rows[0]; }
  [Symbol.iterator]() { return this.rows[Symbol.iterator](); }
}

export class FakeStorage {
  constructor() {
    this.db = new DatabaseSync(':memory:');
    this.alarmAt = null;
    const db = this.db;
    this.sql = {
      exec(q, ...params) {
        const st = db.prepare(q);
        const p = params.map(v => (v instanceof Uint8Array ? v : v));
        if (st.columns().length) return new Cursor(st.all(...p));
        st.run(...p);
        return new Cursor([]);
      },
    };
  }
  transactionSync(fn) {
    this.db.exec('BEGIN');
    try { const r = fn(); this.db.exec('COMMIT'); return r; } catch (e) { this.db.exec('ROLLBACK'); throw e; }
  }
  async getAlarm() { return this.alarmAt; }
  async setAlarm(t) { this.alarmAt = t; }
  async deleteAlarm() { this.alarmAt = null; }
  async deleteAll() {
    for (const t of this.db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'").all()) this.db.exec(`DROP TABLE "${t.name}"`);
  }
}

/** One end of an in-memory WebSocket pair. */
class End {
  constructor() { this.peer = null; this.closed = false; this.attachment = null; this.onmessage = null; this.onclose = null; this.sent = []; this.tags = []; }
  send(data) {
    if (this.closed) throw new Error('closed');
    const copy = typeof data === 'string' ? data : new Uint8Array(data).slice().buffer;
    this.sent.push(copy);
    return this.peer.deliver(copy);
  }
  deliver(data) { if (!this.closed && this.onmessage) return this.onmessage(data); }
  close(code = 1000, reason = '') {
    if (this.closed) return;
    this.closed = true;
    this.code = code;
    this.peer.peerClosed(code, reason);
  }
  peerClosed(code, reason) {
    if (this.closed) return;
    this.closed = true;
    this.code = code;
    if (this.onclose) this.onclose({ code, reason });
  }
  serializeAttachment(a) { this.attachment = JSON.parse(JSON.stringify(a)); }
  deserializeAttachment() { return this.attachment; }
}

export function pair() {
  const a = new End(), b = new End();
  a.peer = b; b.peer = a;
  return [a, b];
}

class FakeState {
  constructor() {
    this.storage = new FakeStorage();
    this.sockets = new Set();
  }
  acceptWebSocket(ws, tags = []) {
    ws.tags = tags;
    this.sockets.add(ws);
    ws.onmessage = data => this.room.webSocketMessage(ws, data);
    ws.onclose = ({ code, reason }) => { this.sockets.delete(ws); return this.room.webSocketClose(ws, code, reason); };
    const close = ws.close.bind(ws);
    ws.close = (c, r) => { this.sockets.delete(ws); close(c, r); };
  }
  getWebSockets(tag) { return [...this.sockets].filter(w => !w.closed && (tag == null || w.tags.includes(tag))); }
}

export class FakeNamespace {
  constructor(env, config) { this.env = env; this.config = config; this.rooms = new Map(); }
  idFromName(name) { return name; }
  get(id) {
    return { fetch: req => this.room(id).fetch(req) };
  }
  room(id) {
    if (!this.rooms.has(id)) {
      const state = new FakeState();
      const room = new PaperRoom(state, this.env, this.config);
      state.room = room;
      this.rooms.set(id, room);
    }
    return this.rooms.get(id);
  }
}

// Node cannot make WebSocketPairs or 101 responses.
rt.pair = pair;
rt.upgraded = client => ({ status: 101, webSocket: client });

/**
 * A site: the API mounted at /paper/api with accounts alice, bob, carol,
 * dave (ids 1-4; admin: 9). The signed-in account is the x-test-user header.
 */
export function site({ config = {}, quota } = {}) {
  const users = { 1: 'alice', 2: 'bob', 3: 'carol', 4: 'dave', 9: 'admin' };
  const emails = { 'bob@example.com': 2 };
  const env = { DB: new FakeD1() };
  if (quota != null) config.freeQuota = quota;
  env.PAPER_ROOMS = new FakeNamespace(env, config);
  const reports = [];
  const auth = {
    user: async req => {
      const id = req.headers.get('x-test-user');
      return id && users[id] ? { id, name: users[id] } : null;
    },
    find: async (_env, who) => {
      const id = who.includes('@') ? emails[who.toLowerCase()] : Object.keys(users).find(k => users[k] === who.toLowerCase());
      return id ? { id: String(id), name: users[id] } : null;
    },
    names: async (_env, ids) => Object.fromEntries(ids.map(i => [i, users[i] || null])),
    isAdmin: u => u.id === '9',
  };
  const api = createPaperApi({ auth, config, onReport: r => reports.push(r) });
  const ORIGIN = 'https://omegagiven.com';
  const call = async (as, method, path, body, headers = {}) => {
    const h = { Origin: ORIGIN, ...headers };
    if (as) h['x-test-user'] = String(as);
    if (body !== undefined) h['content-type'] = 'application/json';
    const res = await api(new Request(ORIGIN + '/paper/api' + path, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, { waitUntil() {} });
    if (!res) return { status: 0, body: null };
    if (res.status === 101) return res;
    return { status: res.status, body: await res.json() };
  };
  const dial = (as, pageId, headers = {}) => async () => {
    const h = { Origin: ORIGIN, Upgrade: 'websocket', ...headers };
    if (as) h['x-test-user'] = String(as);
    const res = await api(new Request(`${ORIGIN}/paper/api/pages/${pageId}/relay`, { headers: h }), env, {});
    return res.status === 101 ? res.webSocket : { status: res.status, body: await res.json() };
  };
  return { env, api, call, dial, users, reports, room: id => env.PAPER_ROOMS.room(id) };
}

// Page keys in the shapes the API checks (the room never reads them).
let n = 0;
export function keys() {
  n++;
  const hex = n.toString(16).padStart(32, '0');
  return { canvas: hex, edit: 'e' + 'A'.repeat(39) + String(n).padStart(4, '0'), view: 'v' + 'B'.repeat(82) + String(n).padStart(4, '0') };
}
