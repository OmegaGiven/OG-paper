// Several people on one page at once: changes reach everyone, rival edits
// settle the same way everywhere, offline work merges on reconnect, and a
// reconnect or a tile subscription only fetches what it needs.

import test from 'node:test';
import assert from 'node:assert/strict';
import { site } from './fakes.js';
import { client, last, newPage, sorted } from './helpers.js';
import { READY, TAG, enc, withTag } from '../src/protocol.js';
import { pair } from './fakes.js';

async function shared() {
  const s = site();
  const page = await newPage(s, 1);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' })).status, 200);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'editor' })).status, 200);
  return { s, page };
}

test('two people drawing at once both see everything', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  assert.ok(a.live && b.live);
  a.log.add('a1', 't0', 'stroke by alice');
  b.log.add('b1', 't0', 'stroke by bob');
  await Promise.all([a.push(), b.push()]);
  a.log.add('a2', 't1', 'another');
  await a.push();
  assert.deepEqual(sorted(a.shown()), ['a1', 'a2', 'b1']);
  assert.deepEqual(sorted(b.shown()), sorted(a.shown()));
  // Everyone on the page is listed, with names from their accounts.
  assert.deepEqual(last(a, 'peers').list.map(p => p.name).sort(), ['alice', 'bob']);
});

test('rival edits of one stroke settle the same on every copy', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  a.log.add('s', 't0', 'original');
  await a.push();
  assert.ok(b.shown().has('s'));
  // Both move the same stroke before hearing of the other's move.
  b.disconnect();
  a.log.replace('s', 's-moved-by-a', 't0', 'moved left');
  b.log.replace('s', 's-moved-by-b', 't0', 'moved right');
  await a.push();
  await b.connect();
  await a.push();
  const sa = a.shown(), sb = b.shown();
  assert.deepEqual(sorted(sa), sorted(sb), 'same result on both');
  assert.equal(sa.size, 1, 'one version shows, never both');
  assert.ok(!sa.has('s'));
  // The newest edit (bob's, made after alice's) wins; alice's stays in the log.
  assert.ok(sa.has('s-moved-by-b'));
  assert.ok(a.log.items.has('s-moved-by-a'));
});

test('delete against edit: the later one wins on both sides', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  a.log.add('x', 't0', 'x');
  await a.push();
  b.disconnect();
  b.log.replace('x', 'x2', 't0', 'x edited');
  a.log.remove('x');                 // later than bob's edit
  await a.push();
  await b.connect();
  await a.push();
  assert.deepEqual(sorted(a.shown()), sorted(b.shown()));
  assert.ok(!a.shown().has('x'));
  assert.ok(a.shown().has('x2'), 'an edit is a new stroke: deleting the old one leaves it');
});

test('offline edits merge on reconnect, fetching only what came after', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  for (let i = 0; i < 5; i++) { a.log.add('early' + i, 't0', i); await a.push(); }
  assert.equal(b.received, 5);
  b.disconnect();
  // Both work apart: alice live, bob offline.
  for (let i = 0; i < 3; i++) { a.log.add('a-later' + i, 't0', i); await a.push(); }
  b.log.add('b-offline1', 't0', 'on the train');
  b.log.add('b-offline2', 't0', 'still on the train');
  const before = b.received;
  await b.connect();
  // Bob got only alice's 3 newer frames (resumed from his last seq), then sent his work.
  assert.equal(b.received - before, 3);
  assert.deepEqual(sorted(a.shown()), sorted(b.shown()));
  assert.equal(a.shown().size, 10);
  // A third person joining later gets all of it.
  const c = await client(s, 3, page);
  assert.deepEqual(sorted(c.shown()), sorted(a.shown()));
});

test('three people, interleaved, with one dropping out twice', async () => {
  const { s, page } = await shared();
  const cs = [await client(s, 1, page), await client(s, 2, page), await client(s, 3, page)];
  for (let round = 0; round < 6; round++) {
    const off = cs[round % 3];
    off.disconnect();
    for (const [i, c] of cs.entries()) {
      c.log.add(`r${round}-${i}`, `t${round % 2}`, round);
      if (round > 0 && c.shown().has(`r${round - 1}-${i}`)) c.log.remove(`r${round - 1}-${i}`);
      await c.push();
    }
    await off.connect();
  }
  for (const c of cs) await c.push();
  const want = sorted(cs[0].shown());
  for (const c of cs) assert.deepEqual(sorted(c.shown()), want);
});

test('tiles: a viewer of one region gets that region, more as it comes into view', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  a.log.add('north', 'N', 1); await a.push();
  a.log.add('south', 'S', 1); await a.push();
  a.log.add('east', 'E', 1); await a.push();
  const b = await client(s, 2, page, { tiles: ['N'] });
  assert.deepEqual(sorted(b.shown()), ['north']);
  // Live changes: only those in view.
  a.log.add('north2', 'N', 2); await a.push();
  a.log.add('south2', 'S', 2); await a.push();
  assert.deepEqual(sorted(b.shown()), ['north', 'north2']);
  // Panning south: the south tile's frames come, nothing else.
  await b.subscribe(['S']);
  assert.deepEqual(sorted(b.shown()), ['north', 'north2', 'south', 'south2']);
  assert.ok(!b.shown().has('east'));
});

test('a checkpoint replaces what it covers and keeps what came after', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  for (let i = 0; i < 20; i++) { a.log.add('s' + i, 't0', 'x'.repeat(200)); await a.push(); }
  const room = s.room(page);
  const before = room.bytes();
  assert.equal(room.frameCount(), 20);
  await a.checkpoint();
  assert.equal(room.frameCount(), 1);
  assert.ok(room.bytes() < before, 'one copy is smaller than twenty changes');
  b.log.add('after', 't0', 'y'); await b.push();
  const c = await client(s, 3, page);
  assert.deepEqual(sorted(c.shown()), sorted(a.shown()));
  assert.equal(c.shown().size, 21);
});

test('the Rust app\'s relay protocol works unchanged (ROOM, PUT, READY)', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  a.log.add('one', '', 1); await a.push();
  // A plain relay client: binary ROOM first, then everything kept, then READY.
  const ws = await s.dial(2, page)();
  const got = [];
  ws.onmessage = d => got.push(d);
  await ws.send(withTag(TAG.ROOM, new Uint8Array(32).fill(7)));
  const bin = got.filter(d => typeof d !== 'string').map(d => new Uint8Array(d));
  assert.equal(bin.length, 2);
  assert.equal(JSON.parse(new TextDecoder().decode(bin[0])).kind, 'changes');
  assert.deepEqual(bin[1], READY);
  // Its changes reach hosted clients; a different key's room is refused.
  await ws.send(withTag(TAG.PUT, enc.encode(JSON.stringify({ kind: 'changes', events: [{ item: 'rust', alive: true, at: [9e12, 0, 'rust'] }], items: [['rust', { tile: '', data: 1 }]], replaces: [] }))));
  assert.ok(a.shown().has('rust'));
  const other = await s.dial(3, page)();
  const got2 = [];
  other.onmessage = d => got2.push(d);
  await other.send(withTag(TAG.ROOM, new Uint8Array(32).fill(8)));
  assert.ok(other.closed);
  assert.match(got2.find(d => typeof d === 'string'), /does not belong to this page/);
});

test('views and strokes in progress are passed on, never kept', async () => {
  const { s, page } = await shared();
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  const seen = [];
  const prev = b.ws.onmessage;
  b.ws.onmessage = d => { if (typeof d !== 'string') seen.push(d); return prev(d); };
  await a.ephemeral({ view: [1, 2, 3] });
  assert.equal(seen.length, 1);
  assert.equal(s.room(page).frameCount(), 0);
  void pair;
});
