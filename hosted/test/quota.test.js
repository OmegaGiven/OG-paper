// Quotas: each account keeps at most its quota, enforced by the rooms, and
// gets it back when pages are deleted, compacted or given away.

import test from 'node:test';
import assert from 'node:assert/strict';
import { site } from './fakes.js';
import { client, last, newPage } from './helpers.js';

const KB = 1024;
const used = async (s, as) => (await s.call(as, 'GET', '/me')).body;

test('usage shows, and a full account cannot keep more', async () => {
  const s = site({ quota: 64 * KB, config: { leaseChunk: 16 * KB } });
  const page = await newPage(s, 1);
  const me0 = await used(s, 1);
  assert.equal(me0.quota, 64 * KB);
  assert.equal(me0.used, 0);
  const a = await client(s, 1, page);
  let kept = 0;
  for (let i = 0; i < 40; i++) {
    a.log.add('big' + i, 't', 'x'.repeat(4 * KB));
    await a.push();
    if (last(a, 'full')) break;
    kept++;
  }
  const full = last(a, 'full');
  assert.ok(full, 'told when full');
  assert.match(full.msg, /Export or delete pages to free space/);
  assert.ok(kept > 5 && kept < 40);
  const room = s.room(page);
  const me = await used(s, 1);
  assert.ok(me.used <= me.quota, `never over: ${me.used} <= ${me.quota}`);
  assert.ok(room.bytes() <= me.quota);
  // The refused change was not kept and not passed on.
  const b = await client(s, 1, page);
  assert.equal(b.shown().size, kept);
  // A new page can't be made either.
  const fullNow = await s.call(1, 'POST', '/pages', { name: 'more', canvas: 'a'.repeat(32), edit: 'e' + 'A'.repeat(43), view: 'v' + 'B'.repeat(86) });
  if (me.used >= me.quota) assert.equal(fullNow.status, 409);
});

test('the owner pays: an editor\'s changes count toward the owner, and they are told', async () => {
  const s = site({ quota: 16 * KB, config: { leaseChunk: 4 * KB } });
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  const b = await client(s, 2, page);
  for (let i = 0; i < 10 && !last(b, 'full'); i++) { b.log.add('s' + i, 't', 'y'.repeat(3 * KB)); await b.push(); }
  assert.match(last(b, 'full').msg, /owner's storage is full/);
  assert.equal((await used(s, 2)).used, 0);
  assert.ok((await used(s, 1)).used > 8 * KB);
});

test('deleting a page gives its space back', async () => {
  const s = site({ quota: 32 * KB, config: { leaseChunk: 8 * KB } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  for (let i = 0; i < 4; i++) { a.log.add('s' + i, 't', 'z'.repeat(4 * KB)); await a.push(); }
  assert.ok((await used(s, 1)).used >= 16 * KB);
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}`)).status, 200);
  assert.equal((await used(s, 1)).used, 0);
  assert.equal(a.ws, null, 'people on it are disconnected');
});

test('quiet rooms hand back quota taken ahead; used = what pages keep', async () => {
  const s = site({ quota: 1024 * KB, config: { leaseChunk: 256 * KB } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  a.log.add('s', 't', 'small'); await a.push();
  const busy = (await used(s, 1)).used;
  assert.ok(busy >= 256 * KB, 'a busy room takes a chunk ahead');
  a.disconnect();
  await s.room(page).alarm();
  const room = s.room(page);
  assert.equal((await used(s, 1)).used, room.bytes());
  // The page list shows what the page keeps.
  assert.equal((await s.call(1, 'GET', '/pages')).body.mine[0].bytes, room.bytes());
});

test('a checkpoint frees what it replaces', async () => {
  const s = site({ quota: 1024 * KB, config: { leaseChunk: 1 } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  for (let i = 0; i < 30; i++) { a.log.add('s' + i, 't', 'w'.repeat(100)); await a.push(); }
  const before = (await used(s, 1)).used;
  await a.checkpoint();
  const after = (await used(s, 1)).used;
  assert.ok(after < before, `${after} < ${before}`);
  assert.equal(after, s.room(page).bytes());
});

test('make a copy: it is mine and counts toward my quota', async () => {
  const s = site({ quota: 64 * KB, config: { leaseChunk: 1 } });
  const page = await newPage(s, 1, 'Original');
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  const a = await client(s, 1, page);
  for (let i = 0; i < 3; i++) { a.log.add('s' + i, 't', 'v'.repeat(2 * KB)); await a.push(); }
  const { canvas, edit, view } = { canvas: 'c'.repeat(32), edit: 'e' + 'C'.repeat(43), view: 'v' + 'D'.repeat(86) };
  const copy = await s.call(2, 'POST', `/pages/${page}/copy`, { canvas, edit, view });
  assert.equal(copy.status, 201);
  assert.equal(copy.body.page.name, 'Copy of Original');
  assert.match(copy.body.open.link, /\?k=e/, 'I can edit my copy');
  assert.match(copy.body.from.link, /\?k=v/, 'and read the original to fill it');
  // The app fills the copy from the original (client side: the room cannot read it).
  const src = await client(s, 2, page);
  const dst = await client(s, 2, copy.body.page.id);
  dst.log.merge(src.log.since({}));
  await dst.push();
  assert.deepEqual([...dst.shown()].sort(), [...a.shown()].sort());
  const bob = await used(s, 2);
  assert.ok(bob.used >= 6 * KB, 'counted to bob');
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.mine.map(p => p.name), ['Copy of Original']);
});

test('giving a page away moves its bytes; refused if they have no room', async () => {
  const s = site({ quota: 32 * KB, config: { leaseChunk: 1 } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  for (let i = 0; i < 4; i++) { a.log.add('s' + i, 't', 'u'.repeat(4 * KB)); await a.push(); }
  const bytes = s.room(page).bytes();
  // Fill carol up first.
  const cp = await newPage(s, 3);
  const c = await client(s, 3, cp);
  for (let i = 0; i < 7; i++) { c.log.add('c' + i, 't', 'q'.repeat(4 * KB)); await c.push(); }
  assert.equal((await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'carol' })).status, 409);
  assert.equal((await used(s, 1)).used, bytes);
  const t = await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'bob' });
  assert.equal(t.status, 200);
  assert.equal((await used(s, 1)).used, 0);
  assert.equal((await used(s, 2)).used, bytes);
  // New changes now count toward bob.
  a.log.add('later', 't', 'n'.repeat(KB)); await a.push();
  assert.ok((await used(s, 2)).used > bytes);
  assert.equal((await used(s, 1)).used, 0);
});

test('caps: a change too big for one frame is refused, as is a flood', async () => {
  const s = site({ config: { maxOpBytes: 8 * KB, frameBurst: 5, frameRate: 0.001 } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  a.log.add('huge', 't', 'h'.repeat(16 * KB)); await a.push();
  assert.match(last(a, 'error').msg, /too big/);
  assert.equal(s.room(page).frameCount(), 0);
  a.log = new a.log.constructor(a.log.peer);
  for (let i = 0; i < 10; i++) { a.log.add('f' + i, 't', i); a.their = {}; await a.push(); }
  assert.match(last(a, 'error').msg, /Slow down/);
  assert.ok(s.room(page).frameCount() <= 5);
});

test('admins can raise a quota', async () => {
  const s = site({ quota: 10 * KB });
  await used(s, 1);
  assert.equal((await s.call(9, 'POST', '/admin/accounts/1', { quota: 100 * KB })).status, 200);
  assert.equal((await used(s, 1)).quota, 100 * KB);
  assert.equal((await s.call(1, 'POST', '/admin/accounts/1', { quota: 1e9 })).status, 404);
});

test('the owner hears how full their storage is as they draw; others do not', async () => {
  const s = site({ quota: 64 * KB, config: { leaseChunk: 8 * KB, upgradeUrl: '/account/storage' } });
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  assert.equal((await used(s, 1)).upgrade, '/account/storage');
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  assert.equal(last(a, 'usage').used, 0, 'on joining');
  for (let i = 0; i < 6; i++) { a.log.add('s' + i, 't', 'p'.repeat(4 * KB)); await a.push(); }
  const u = last(a, 'usage');
  assert.equal(u.quota, 64 * KB);
  assert.ok(u.used >= 24 * KB, `grows as quota is taken: ${u.used}`);
  assert.equal(last(b, 'usage'), undefined, 'an editor never sees the owner\'s usage');
});
