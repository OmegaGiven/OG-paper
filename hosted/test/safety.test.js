// The second hosted review (2026-11-07): history that an editor can't
// flush, leases that survive hibernation, restore and quotas, lookups by
// email, anonymous ids, give-away billing, sockets per person, leaving
// co-owners, taken-down pages, account deletion, and chat safety (reports,
// the admin, blocked accounts, blocking people, the word filter).

import test from 'node:test';
import assert from 'node:assert/strict';
import { site } from './fakes.js';
import { client, last, newPage } from './helpers.js';
import { TAG, withTag } from '../src/protocol.js';
import { PaperRoom } from '../src/room.js';
import { deleteAccount } from '../src/index.js';
import { DEFAULT_WORDS, filterWords } from '../src/chat.js';

const KB = 1024;
const DAY = 86_400_000;
const ORIGIN = 'https://omegagiven.com';
const used = async (s, as) => (await s.call(as, 'GET', '/me')).body.used;
const fire = room => { room.ctx.storage.alarmAt = null; return room.alarm(); };
const said = c => c.notes.filter(n => n.t === 'chat').map(n => n.msg.text);

async function filled(s, n = 5, size = 400) {
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' });
  const a = await client(s, 1, page);
  for (let i = 0; i < n; i++) { a.log.add('s' + i, 't', 'x'.repeat(size)); await a.push(); }
  return { page, a };
}

/** A raw socket on a page, joined (seq mode). */
async function raw(s, as, page) {
  const ws = await s.dial(as, page)();
  ws.got = [];
  ws.onmessage = m => { if (typeof m === 'string') ws.got.push(JSON.parse(m)); };
  await ws.send(JSON.stringify({ t: 'hello', since: 0, seq: true }));
  return ws;
}

async function sharedChat(s) {
  const page = await newPage(s, 1, 'Talk');
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' });
  return page;
}

// ---- H3: history an editor can't flush ----------------------------------------------

test('an editor sending copy after copy cannot push the page as it was out of its history', async () => {
  const s = site({ config: { keepReplacedBytes: 3000, leaseChunk: 1 } });
  const { page } = await filled(s);
  const room = s.room(page);
  const original = room.frameCount();
  const bob = await raw(s, 2, page);
  // Each copy is big enough (over 10% of what it replaces) to replace the last.
  for (let i = 0; i < 6; i++) { await bob.send(withTag(TAG.CKPT, new Uint8Array(1500).fill(i + 1))); await room.queue; }
  assert.equal(room.history().replaced.length, 6);
  assert.ok(room.replacedBytes() > 3000);
  await fire(room);
  // Bob's own newer copies went; the set holding the page as it was stays.
  const h = room.history().replaced;
  assert.equal(h.length, 1);
  assert.equal(h[0].frames, original);
  assert.ok(room.replacedBytes() <= 3000);
  const r = await s.call(1, 'POST', `/pages/${page}/restore`, { since: h[0].seq });
  assert.deepEqual([r.status, r.body.restored], [200, original]);
  assert.equal((await client(s, 3, page)).shown().size, 5, 'the page as it was is back');
});

test('a page bigger than keepReplacedBytes still keeps its last replaced set', async () => {
  const s = site({ config: { keepReplacedBytes: 500 } });
  const { page } = await filled(s);
  const room = s.room(page);
  const bob = await raw(s, 2, page);
  await bob.send(withTag(TAG.CKPT, new Uint8Array(400)));
  await room.queue;
  await fire(room);
  assert.equal(room.history().replaced.length, 1);
  assert.ok(room.replacedBytes() > 500);
});

// ---- M1: the lease survives hibernation ------------------------------------------------

test('what is left of a lease is stored with every kept frame', async () => {
  const s = site({ config: { leaseChunk: 16 * KB } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  for (let i = 0; i < 4; i++) { a.log.add('s' + i, 't', 'x'.repeat(500)); await a.push(); }
  const room = s.room(page);
  assert.equal(Number(room.meta('lease')), room.lease.left);
  // The room hibernates (a new object over the same storage) and goes quiet:
  // it hands back only what it did not spend.
  a.disconnect();
  const woke = new PaperRoom(room.ctx, s.env, {});
  await woke.lease.release();
  assert.equal(await used(s, 1), room.bytes());
});

// ---- M2: restore counts toward the quota -------------------------------------------------

test('bringing back an earlier version is refused when the owner has no room (the admin may)', async () => {
  const s = site({ quota: 4 * KB, config: { leaseChunk: 1 } });
  const { page } = await filled(s, 5, 600);
  const room = s.room(page);
  const ws = await raw(s, 1, page);
  await ws.send(withTag(TAG.CKPT, new Uint8Array(8)));
  await room.queue;
  // Fill the freed space again.
  const a = await client(s, 1, page);
  for (let i = 0; i < 5; i++) { a.log.add('n' + i, 't', 'z'.repeat(600)); await a.push(); }
  const before = await used(s, 1);
  const r = await s.call(1, 'POST', `/pages/${page}/restore`, {});
  assert.equal(r.status, 409);
  assert.equal(r.body.full, 'bytes');
  assert.equal(await used(s, 1), before, 'nothing charged');
  assert.equal(room.history().replaced.length, 1, 'still there to bring back later');
  const admin = await s.call(9, 'POST', `/admin/pages/${page}/restore`, {});
  assert.equal(admin.body.restored, 5);
  assert.ok(await used(s, 1) > 4 * KB, 'the admin may take it past the quota');
});

test('bringing back just one earlier version (only), not the newer ones too', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  const { page, a } = await filled(s, 3, 300);
  const room = s.room(page);
  const ws = await raw(s, 1, page);
  await ws.send(withTag(TAG.CKPT, new Uint8Array(8)));
  await room.queue;
  for (let i = 0; i < 2; i++) { a.log.add('n' + i, 't', 'z'.repeat(300)); await a.push(); }
  await ws.send(withTag(TAG.CKPT, new Uint8Array(8)));
  await room.queue;
  const h = (await s.call(1, 'GET', `/pages/${page}/history`)).body.replaced;
  assert.equal(h.length, 2);
  const older = h[1];
  const r = await s.call(1, 'POST', `/pages/${page}/restore`, { since: older.seq, only: true });
  assert.deepEqual([r.status, r.body.restored], [200, older.frames]);
  const left = (await s.call(1, 'GET', `/pages/${page}/history`)).body.replaced;
  assert.deepEqual(left.map(x => x.seq), [h[0].seq], 'the newer copy\'s set is still there');
});

// ---- M3: quotas over 2 GiB ------------------------------------------------------------

test('admin quotas over 2 GiB are kept whole; nonsense is refused', async () => {
  const s = site();
  const r = await s.call(9, 'POST', '/admin/accounts/1', { quota: 3 * 1024 ** 3 + 0.5 });
  assert.equal(r.status, 200);
  assert.equal(r.body.quota, 3 * 1024 ** 3);
  assert.equal((await s.call(9, 'POST', '/admin/accounts/1', { quota: 1e30 })).status, 400);
  assert.equal((await s.call(9, 'POST', '/admin/accounts/1', { quota: -5 })).status, 400);
  assert.equal((await s.call(9, 'POST', '/admin/accounts/1', { quota: 'lots' })).status, 400);
});

// ---- M4: lookups by email don't say who --------------------------------------------------

test('sharing or giving by email answers the same whether or not the address has an account', async () => {
  const kinds = [];
  const s = site({ limit: async (_env, kind) => { kinds.push(kind); return false; } });
  const page = await newPage(s, 1);
  const known = await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob@example.com', role: 'viewer' });
  const unknown = await s.call(1, 'POST', `/pages/${page}/members`, { who: 'nobody@example.com', role: 'viewer' });
  assert.equal(known.status, 200);
  assert.deepEqual(known.body, unknown.body);
  assert.equal(JSON.stringify(known.body).includes('bob'), false);
  assert.ok(kinds.includes('find'), 'lookups by email have their own (tighter) limit');
  // bob did get it.
  assert.equal((await s.call(2, 'GET', '/pages')).body.shared.length, 1);
  // By username it still says who (and no such account is a 404).
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' })).body.member.name, 'carol');
  // Give-away the same way.
  const p2 = await newPage(s, 1);
  const k = await s.call(1, 'POST', `/pages/${p2}/transfer`, { who: 'bob@example.com' });
  const u = await s.call(1, 'POST', `/pages/${p2}/transfer`, { who: 'nobody@example.com' });
  assert.deepEqual([k.status, u.status], [201, 201]);
  assert.equal(k.body.offer.to.name, null);
  assert.deepEqual(Object.keys(k.body).sort(), Object.keys(u.body).sort());
  // When the email lookup limit is hit, it says so.
  const t = site({ limit: async (_env, kind) => kind === 'find' });
  const p3 = await newPage(t, 1);
  assert.equal((await t.call(1, 'POST', `/pages/${p3}/members`, { who: 'bob@example.com' })).status, 429);
});

// ---- M5: anonymous visitors aren't named by their IP --------------------------------------

test('anonymous link visitors get a random id, not their IP', async () => {
  const s = site({ config: { linkAnonymousView: true } });
  const page = await newPage(s, 1);
  const a = await client(s, 1, page);
  const token = (await s.call(1, 'POST', `/pages/${page}/links`, { role: 'viewer' })).body.token;
  const open = async () => {
    const res = await s.api(new Request(`${ORIGIN}/paper/api/l/${token}/relay`, { headers: { Origin: ORIGIN, Upgrade: 'websocket', 'CF-Connecting-IP': '203.0.113.7' } }), s.env, {});
    assert.equal(res.status, 101);
    const ws = res.webSocket;
    ws.onmessage = () => {};
    await ws.send(JSON.stringify({ t: 'hello', since: 0, seq: true }));
  };
  await open(); await open();
  const anons = last(a, 'peers').list.filter(p => p.user.startsWith('anon:'));
  assert.equal(anons.length, 2, 'two visits, two ids');
  for (const p of anons) assert.ok(!p.user.includes('203.0.113.7'), p.user);
});

// ---- M6: give-away billing in one step ------------------------------------------------------

test('accepting a page settles its bytes in one step, even while someone draws', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  const { page, a } = await filled(s, 3, 500);
  const b = await client(s, 2, page);
  await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'carol', leave: false });
  // bob draws while carol accepts.
  b.log.add('late', 't', 'q'.repeat(700));
  const [acc] = await Promise.all([s.call(3, 'POST', `/offers/${page}/accept`), b.push()]);
  assert.equal(acc.status, 200);
  await s.room(page).queue;
  b.log.add('later', 't', 'r'.repeat(300)); await b.push();
  await s.room(page).alarm();
  const room = s.room(page);
  assert.equal(await used(s, 1), 0, 'the old owner pays for nothing');
  assert.equal(await used(s, 3), room.bytes(), 'the new owner pays for all of it');
  assert.ok(a);
});

// ---- M7: sockets per person -------------------------------------------------------------------

test('one person cannot take every place in a room; owners always get in', async () => {
  const s = site({ config: { maxSockets: 6, maxSocketsPerUser: 2 } });
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'editor' });
  for (let i = 0; i < 2; i++) assert.equal(typeof (await s.dial(2, page)()).send, 'function');
  const third = await s.dial(2, page)();
  assert.equal(third.status, 429);
  for (let i = 0; i < 2; i++) assert.equal(typeof (await s.dial(3, page)()).send, 'function');
  for (let i = 0; i < 2; i++) assert.equal(typeof (await s.dial(1, page)()).send, 'function');
  // The room is full for everyone else, not for its owner.
  assert.equal((await s.dial(1, page)()).send != null, true);
});

// ---- L1, L3, L4, L8 ---------------------------------------------------------------------------------

test('a co-owner who leaves takes the links they made with them', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'owner' });
  const token = (await s.call(2, 'POST', `/pages/${page}/links`, { role: 'editor' })).body.token;
  assert.equal((await s.call(4, 'POST', `/links/${token}`)).status, 200);
  const r = await s.call(2, 'DELETE', `/pages/${page}/members/2`);
  assert.deepEqual([r.status, r.body.linksOff], [200, 1]);
  assert.equal((await s.call(3, 'GET', `/links/${token}`)).status, 404);
  assert.equal((await s.call(4, 'POST', `/pages/${page}/open`)).status, 404, 'who came in by it is out');
});

test('a page taken down cannot be deleted for good by its owner (the admin can)', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(9, 'POST', `/admin/pages/${page}/block`, { blocked: true });
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}`)).status, 200, 'to the trash');
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}?forever=1`)).status, 403);
  // Nor does the trash running out take it.
  s.env.DB.db.prepare('UPDATE paper_pages SET deleted_at = ?').run(Date.now() - 31 * DAY);
  await s.call(1, 'GET', '/pages');
  assert.ok(s.env.DB.row('SELECT id FROM paper_pages WHERE id = ?', page));
  const room = s.room(page);
  room.setMeta('trash-until', String(Date.now() - 1000));
  await fire(room);
  assert.ok(s.env.DB.row('SELECT id FROM paper_pages WHERE id = ?', page), 'kept for the admin');
  assert.equal((await s.call(9, 'DELETE', `/admin/pages/${page}`)).status, 200);
  assert.equal(s.env.DB.row('SELECT id FROM paper_pages WHERE id = ?', page), undefined);
});

test('purging the trash survives a broken room; a room can be destroyed twice', async () => {
  const s = site();
  const p1 = await newPage(s, 1), p2 = await newPage(s, 1);
  await s.call(1, 'DELETE', `/pages/${p1}`);
  await s.call(1, 'DELETE', `/pages/${p2}`);
  s.env.DB.db.prepare('UPDATE paper_pages SET deleted_at = ?').run(Date.now() - 31 * DAY);
  const r1 = s.room(p1);
  const real = r1.destroy.bind(r1);
  r1.destroy = async () => { throw new Error('boom'); };
  const l = await s.call(1, 'GET', '/pages');
  assert.equal(l.status, 200);
  assert.deepEqual(l.body.trash.map(p => p.id), [p1], 'the other one still went');
  r1.destroy = real;
  const once = await (await r1.fetch(new Request('https://room/destroy', { method: 'POST' }))).json();
  const twice = await r1.fetch(new Request('https://room/destroy', { method: 'POST' }));
  assert.equal(twice.status, 200);
  assert.equal((await twice.json()).bytes, 0);
  assert.ok(once.bytes >= 0);
  assert.equal((await (await r1.fetch(new Request('https://room/stats'))).json()).frames, 0);
});

test('report reasons are cleaned like chat', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  const r = await s.call(2, 'POST', `/pages/${page}/report`, { reason: 'bad‮txt.exe\u0007 page' });
  assert.equal(r.status, 201);
  assert.equal(s.env.DB.row('SELECT reason FROM paper_reports').reason, 'badtxt.exe page');
  assert.equal(s.reports[0].reason, 'badtxt.exe page');
  assert.equal((await s.call(2, 'POST', `/pages/${page}/report`, { reason: '​\u0001' })).status, 400);
});

// ---- H1: deleting an account -------------------------------------------------------------------------

test('deleteAccount removes everything OG Paper keeps about an account', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  // bob's own pages (one in the trash).
  const mine = await newPage(s, 2, 'Bob page');
  const gone = await newPage(s, 2, 'Bob trash');
  const bc = await client(s, 2, mine);
  bc.log.add('s', 't', 'x'.repeat(300)); await bc.push();
  await s.call(2, 'DELETE', `/pages/${gone}`);
  // carol's page: bob is a co-owner there, made a link someone used, chatted and reacted.
  const theirs = await newPage(s, 3, 'Carol page');
  await s.call(3, 'POST', `/pages/${theirs}/members`, { who: 'bob', role: 'owner' });
  await s.call(3, 'PATCH', `/pages/${theirs}/chat`, { keep: true });
  const token = (await s.call(2, 'POST', `/pages/${theirs}/links`, { role: 'viewer' })).body.token;
  await s.call(4, 'POST', `/links/${token}`);
  const c = await client(s, 3, theirs);
  const b = await client(s, 2, theirs);
  await c.say('hello bob');
  await b.say('hi carol');
  await b.control({ t: 'chat-react', id: last(b, 'chat').msg.id - 1, emoji: '👍' });
  // An offer to bob, a report by bob, a block by bob.
  const offered = await newPage(s, 1, 'For bob');
  await s.call(1, 'POST', `/pages/${offered}/transfer`, { who: 'bob' });
  await s.call(2, 'POST', `/pages/${theirs}/report`, { reason: 'test' });
  await s.call(2, 'POST', '/blocks', { user: '1' });
  assert.ok(s.env.DB.row('SELECT * FROM paper_chatters WHERE user_id = ?', '2'));

  const out = await deleteAccount(s.env, '2');
  assert.deepEqual([out.pages, out.left, out.links], [2, 1, 1]);
  const count = (sql, ...a) => s.env.DB.row(sql, ...a).n;
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_pages WHERE owner_id = ?', '2'), 0);
  assert.equal((await (await s.room(mine).fetch(new Request('https://room/stats'))).json()).frames, 0, 'the room is emptied');
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_members WHERE user_id IN (?, ?)', '2', '4'), 0, 'bob, and who came in by his link');
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_links WHERE created_by = ?', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_transfers WHERE to_id = ? OR from_id = ?', '2', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_accounts WHERE user_id = ?', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_blocks WHERE user_id = ? OR blocked_id = ?', '2', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_chatters WHERE user_id = ?', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_reports WHERE reporter_id = ?', '2'), 0);
  assert.equal(count('SELECT COUNT(*) AS n FROM paper_reports'), 1, 'the report stays, without who made it');
  // His chat on carol's page is gone (and carol was told), his reaction too.
  const x = (await s.call(3, 'GET', `/pages/${theirs}/chat`)).body.messages;
  assert.deepEqual(x.map(m => [m.name, m.text]), [['carol', 'hello bob']]);
  assert.deepEqual(x[0].reacts, {});
  assert.ok(c.notes.some(n => n.t === 'chat-del'));
  assert.equal(b.ws, null, 'shown the door');
});

// ---- H2: chat safety ---------------------------------------------------------------------------------------

test('reporting a chat message keeps a copy with what was said around it, even once chat is gone', async () => {
  const s = site();
  const page = await sharedChat(s);
  const a = await client(s, 1, page), b = await client(s, 2, page), c = await client(s, 3, page);
  await a.say('one'); await b.say('nasty'); await a.say('three');
  const id = c.notes.find(n => n.t === 'chat' && n.msg.text === 'nasty').msg.id;
  assert.equal((await s.call(2, 'POST', `/pages/${page}/chat/report`, { id })).status, 409, 'not your own');
  assert.equal((await s.call(4, 'POST', `/pages/${page}/chat/report`, { id })).status, 404, 'not for outsiders');
  const r = await s.call(3, 'POST', `/pages/${page}/chat/report`, { id, reason: 'rude‮' });
  assert.equal(r.status, 201);
  assert.equal(s.reports.at(-1).kind, 'chat');
  assert.equal(s.reports.at(-1).subject, '2');
  // Everyone leaves: the (ephemeral) chat goes, the report's copy stays.
  for (const x of [a, b, c]) x.disconnect();
  assert.equal(s.room(page).chat.count(), 0);
  const rep = (await s.call(9, 'GET', '/admin/reports')).body.reports.find(x => x.kind === 'chat');
  assert.equal(rep.reason, 'rude');
  const ctx = JSON.parse(rep.context);
  assert.equal(ctx.message.text, 'nasty');
  assert.deepEqual([ctx.before.map(m => m.text), ctx.after.map(m => m.text)], [['one'], ['three']]);
  assert.equal((await s.call(3, 'POST', `/pages/${page}/chat/report`, { id })).status, 404, 'gone now');
});

test('the admin reads a page\'s chat, takes a message or all of it down, and can look at the page live', async () => {
  const s = site();
  const page = await sharedChat(s);
  await s.call(1, 'PATCH', `/pages/${page}/chat`, { keep: true });
  const a = await client(s, 1, page);
  await a.say('fine'); await a.say('bad');
  assert.equal((await s.call(1, 'GET', `/admin/pages/${page}/chat`)).status, 404, 'admins only');
  const x = await s.call(9, 'GET', `/admin/pages/${page}/chat`);
  assert.deepEqual(x.body.messages.map(m => m.text), ['fine', 'bad']);
  const bad = x.body.messages[1].id;
  assert.equal((await s.call(9, 'DELETE', `/admin/pages/${page}/chat/${bad}`)).status, 200);
  assert.ok(a.notes.some(n => n.t === 'chat-del' && n.id === bad));
  assert.equal((await s.call(9, 'DELETE', `/admin/pages/${page}/chat/${bad}`)).status, 404);
  assert.equal((await s.call(9, 'DELETE', `/admin/pages/${page}/chat`)).body.cleared, 1);
  // The look link the admin gets opens the page, view only.
  const look = await s.call(9, 'GET', `/admin/pages/${page}`);
  assert.match(look.body.open.link, /\?k=v/);
  const admin = await client(s, 9, page);
  assert.equal(last(admin, 'welcome').role, 'viewer');
  admin.log.add('x', 't', 'nope'); await admin.push();
  assert.ok(admin.notes.some(n => n.t === 'denied'));
  // Other people still can't.
  assert.equal((await s.dial(4, page)()).status, 404);
});

test('an account the admin blocks is shown the door and can\'t write, open pages or chat', async () => {
  const s = site();
  const page = await sharedChat(s);
  const a = await client(s, 1, page), b = await client(s, 2, page);
  assert.equal((await s.call(9, 'POST', '/admin/accounts/2', { blocked: true })).status, 200);
  assert.equal(b.ws, null, 'kicked at once');
  assert.ok(b.notes.some(n => n.t === 'kicked'));
  assert.equal((await s.dial(2, page)()).status, 403);
  assert.equal((await s.call(2, 'PATCH', `/pages/${page}`, { name: 'x' })).status, 403);
  assert.equal((await s.call(2, 'POST', '/pages', { name: 'n' })).status, 403);
  // Reading (and downloading) still works.
  assert.equal((await s.call(2, 'GET', '/pages')).status, 200);
  assert.equal((await s.call(2, 'GET', `/pages/${page}/chat`)).status, 200);
  await s.call(9, 'POST', '/admin/accounts/2', { blocked: false });
  assert.equal(typeof (await s.dial(2, page)()).send, 'function');
  assert.ok(a);
});

test('blocking someone: their chat isn\'t shown to me, and they can\'t share with me or offer me pages', async () => {
  const s = site();
  const page = await sharedChat(s);
  const b = await client(s, 2, page), c = await client(s, 3, page);
  await b.say('before');
  // carol blocks bob (from the chat's person menu).
  const r = await s.call(3, 'POST', '/blocks', { user: '2' });
  assert.deepEqual([r.status, r.body.blocked.name], [200, 'bob']);
  assert.deepEqual(last(c, 'blocks').list, ['2'], 'carol\'s open page is told');
  assert.equal((await s.call(3, 'POST', '/blocks', { user: '3' })).status, 400);
  await b.say('can you see me');
  await b.control({ t: 'typing', on: true });
  assert.ok(!said(c).includes('can you see me'));
  assert.ok(!c.notes.some(n => n.t === 'typing'));
  assert.ok(said(b).includes('can you see me'), 'bob is not told');
  // Nor in history when she comes back, nor in her download.
  const c2 = await client(s, 3, page);
  assert.ok(!last(c2, 'chat-history').list.some(m => m.user === '2'));
  assert.deepEqual(last(c2, 'blocks').list, ['2']);
  assert.ok(!(await s.call(3, 'GET', `/pages/${page}/chat`)).body.messages.some(m => m.user === '2'));
  // bob can't add her to his pages or offer her one (by name: told; by email: same answer as always).
  const his = await newPage(s, 2);
  assert.equal((await s.call(2, 'POST', `/pages/${his}/members`, { who: 'carol' })).status, 403);
  assert.equal((await s.call(2, 'POST', `/pages/${his}/transfer`, { who: 'carol' })).status, 403);
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_members WHERE page_id = ? AND user_id = ?', his, '3').n, 0);
  assert.deepEqual((await s.call(3, 'GET', '/blocks')).body.blocks.map(x => [x.id, x.name]), [['2', 'bob']]);
  // Unblocked: all as before.
  assert.equal((await s.call(3, 'DELETE', '/blocks/2')).status, 200);
  assert.deepEqual(last(c2, 'blocks').list, []);
  await b.say('back');
  assert.ok(said(c2).includes('back'));
  assert.equal((await s.call(2, 'POST', `/pages/${his}/members`, { who: 'carol' })).status, 200);
});

test('the word filter masks listed words and tells the admin', async () => {
  const s = site({ config: { chatWords: ['badword', 'two words'] } });
  const page = await sharedChat(s);
  const a = await client(s, 1, page), b = await client(s, 2, page);
  await b.say('you BadWord, not badwords, two words');
  assert.deepEqual(said(a), ['you *******, not badwords, *********']);
  const r = s.reports.at(-1);
  assert.equal(r.kind, 'chat-auto');
  assert.equal(r.subject, '2');
  assert.equal(r.context.message.text, 'you BadWord, not badwords, two words');
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_reports WHERE kind = ?', 'chat-auto').n, 1);
  await b.say('badword again');
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_reports WHERE kind = ?', 'chat-auto').n, 1, 'one report a minute per person');
  // Clean messages pass untouched; the built-in list is there by default.
  await b.say('all fine');
  assert.equal(said(a).at(-1), 'all fine');
  assert.ok(DEFAULT_WORDS.length >= 10);
  assert.equal(filterWords('A classic pass', ['ass']).text, 'A classic pass');
  assert.equal(filterWords('ok', []).hits.length, 0);
});
