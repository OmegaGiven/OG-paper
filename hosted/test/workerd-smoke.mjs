// The real thing: run against `wrangler dev` (workerd, real Durable Objects,
// D1 and WebSockets), where the unit tests use stand-ins.
//   cd hosted/example && npx wrangler d1 migrations apply DB --local && npx wrangler dev --port 8799 &
//   BASE=http://localhost:8799 node hosted/test/workerd-smoke.mjs

import assert from 'node:assert/strict';
import { HostedClient } from '../client/oplog.js';
import { TAG, withTag, READY } from '../src/protocol.js';

const BASE = process.env.BASE || 'http://localhost:8799';
const WS = BASE.replace(/^http/, 'ws');

async function login(as) {
  const r = await fetch(`${BASE}/dev/login?as=${as}`, { redirect: 'manual' });
  return r.headers.get('set-cookie').split(';')[0];
}
async function call(cookie, method, path, body) {
  const r = await fetch(`${BASE}/paper/api${path}`, {
    method, headers: { Cookie: cookie, Origin: BASE, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  return { status: r.status, body: await r.json() };
}
function dialer(cookie, page) {
  return () => new Promise(ok => {
    const w = new WebSocket(`${WS}/paper/api/pages/${page}/relay`, { headers: { Cookie: cookie, Origin: BASE } });
    w.binaryType = 'arraybuffer';
    const wrap = {
      onmessage: null, onclose: null, closed: false,
      send: d => { w.send(d); return wait(60); },
      close: () => w.close(),
    };
    w.onopen = () => ok(wrap);
    w.onerror = () => ok({ status: 'refused' });
    w.onmessage = e => wrap.onmessage?.(e.data);
    w.onclose = e => { wrap.closed = true; wrap.code = e.code; wrap.onclose?.(e); };
  });
}
const wait = ms => new Promise(r => setTimeout(r, ms));
const until = async (f, ms = 3000) => { const t = Date.now(); while (!f()) { if (Date.now() - t > ms) throw new Error('timed out'); await wait(20); } };
let n = 0;
const keys = () => { n++; return { canvas: (Date.now().toString(16) + n).padStart(32, '0').slice(-32), edit: 'e' + 'A'.repeat(39) + String(n).padStart(4, '0'), view: 'v' + 'B'.repeat(82) + String(n).padStart(4, '0') }; };

// Fresh accounts each run (the dev server keeps its data between runs).
const run = Date.now().toString(36).slice(-6);
const [ALICE, BOB, CAROL] = ['alice', 'bob', 'carol'].map(n => `${n}_${run}`);
const alice = await login(ALICE), bob = await login(BOB), carol = await login(CAROL);
const me = await call(alice, 'GET', '/me');
assert.equal(me.status, 200);
assert.equal(me.body.user.id, ALICE);
console.log('me', me.body.used, '/', me.body.quota);

const made = await call(alice, 'POST', '/pages', { name: 'Smoke', ...keys() });
assert.equal(made.status, 201, JSON.stringify(made.body));
const page = made.body.page.id;
assert.equal((await call(alice, 'POST', `/pages/${page}/members`, { who: BOB, role: 'editor' })).status, 200);
assert.equal((await call(alice, 'POST', `/pages/${page}/members`, { who: CAROL, role: 'viewer' })).status, 200);

// Two editors and a viewer, live.
const A = new HostedClient({ peer: 'pa', dial: dialer(alice, page) });
const B = new HostedClient({ peer: 'pb', dial: dialer(bob, page) });
const C = new HostedClient({ peer: 'pc', dial: dialer(carol, page) });
for (const c of [A, B, C]) { assert.ok(await c.connect(), 'connects'); }
await until(() => A.live && B.live && C.live);
A.log.add('a1', 'T1', 'one'); await A.push();
B.log.add('b1', 'T2', 'two'); await B.push();
await until(() => A.shown().has('b1') && B.shown().has('a1') && C.shown().has('a1') && C.shown().has('b1'));
C.log.add('c1', 'T1', 'nope'); await C.push();
await until(() => C.notes.some(m => m.t === 'denied'));
await until(() => A.notes.some(m => m.t === 'peers' && m.list.length === 3));
console.log('live editing, viewer refused, presence: ok');

// Offline and back.
B.disconnect();
A.log.add('a2', 'T1', 'while bob is away'); await A.push();
B.log.add('b2', 'T2', 'bob offline');
const before = B.received;
assert.ok(await B.connect());
await until(() => B.live && A.shown().has('b2') && B.shown().has('a2'));
assert.equal(B.received - before, 1, 'resumed: only what came after');
console.log('offline merge and resume: ok');

// The Rust app's relay frames, unchanged.
const raw = await dialer(alice, page)();
const got = [];
raw.onmessage = d => got.push(d);
raw.send(withTag(TAG.ROOM, new Uint8Array(32).fill(3)));
await until(() => got.some(d => typeof d !== 'string' && new Uint8Array(d).length === READY.length));
assert.ok(got.filter(d => typeof d !== 'string').length >= 4);
raw.close();
console.log('relay protocol: ok');

// Revoke: bob is shown the door.
assert.equal((await call(alice, 'DELETE', `/pages/${page}/members/${BOB}`)).status, 200);
await until(() => B.notes.some(m => m.t === 'kicked'));
const again = await dialer(bob, page)();
assert.equal(again.status, 'refused');
assert.equal((await call(bob, 'POST', `/pages/${page}/open`)).status, 404);
console.log('revoke: ok');

// Quota: alice's usage grew, and deleting gives it back.
const used = (await call(alice, 'GET', '/me')).body.used;
assert.ok(used > 0);
A.disconnect(); C.disconnect();
assert.equal((await call(alice, 'DELETE', `/pages/${page}`)).status, 200);
assert.equal((await call(alice, 'GET', '/me')).body.used, 0);
console.log('quota accounting: ok');

// The web app is served with its API named.
const html = await (await fetch(`${BASE}/paper/`)).text();
assert.match(html, /og-paper-api/);
console.log('SMOKE OK');
process.exit(0);
