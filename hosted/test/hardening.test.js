// The launch-audit hardening (docs/LAUNCH-AUDIT.md H1, H2, H4, M3, M4, L3):
// give-away is an offer, full copies can't wipe a page, the trash, room
// pinning, links of removed co-owners, and names.

import test from 'node:test';
import assert from 'node:assert/strict';
import { site, pair } from './fakes.js';
import { client, newPage } from './helpers.js';
import { TAG, withTag } from '../src/protocol.js';
import { cleanName } from '../src/chat.js';

const KB = 1024;
const DAY = 86_400_000;
const used = async (s, as) => (await s.call(as, 'GET', '/me')).body.used;
const ids = list => list.map(p => p.id);
// The runtime clears an alarm as it fires it.
const fire = room => { room.ctx.storage.alarmAt = null; return room.alarm(); };

// ---- H2: give-away is an offer --------------------------------------------------

test('giving a page away is an offer: nothing moves until they accept', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  const page = await newPage(s, 1, 'Gift');
  const a = await client(s, 1, page);
  a.log.add('s', 't', 'g'.repeat(4 * KB)); await a.push();
  const bytes = s.room(page).bytes();
  const o = await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'bob', leave: true });
  assert.equal(o.status, 201);
  assert.deepEqual(o.body.offer.to, { id: '2', name: 'bob' });
  // Bob sees an offer, not a page; he can't open it; nothing counts to him yet.
  const bob = (await s.call(2, 'GET', '/pages')).body;
  assert.deepEqual(bob.mine, []);
  assert.deepEqual(bob.offers.map(x => [x.page.id, x.page.name, x.from.name, x.leave]), [[page, 'Gift', 'alice', true]]);
  assert.equal(await used(s, 2), 0);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/open`)).status, 404);
  // Alice still owns it and sees it is waiting.
  const alice = (await s.call(1, 'GET', '/pages')).body;
  assert.deepEqual(ids(alice.mine), [page]);
  assert.equal(alice.mine[0].offer.to.name, 'bob');
  assert.equal((await s.call(1, 'GET', `/pages/${page}`)).body.offer.to.name, 'bob');
  // Someone else can't accept it.
  assert.equal((await s.call(3, 'POST', `/offers/${page}/accept`)).status, 404);
  // Bob accepts: the page and its bytes move; alice leaves (she asked to).
  const acc = await s.call(2, 'POST', `/offers/${page}/accept`);
  assert.equal(acc.status, 200);
  assert.match(acc.body.open.link, /\?k=e/);
  assert.deepEqual(ids((await s.call(2, 'GET', '/pages')).body.mine), [page]);
  assert.equal(await used(s, 2), bytes);
  assert.equal(await used(s, 1), 0);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/open`)).status, 404);
  assert.equal(a.ws, null, 'the giver is shown the door');
  assert.equal((await s.call(2, 'POST', `/offers/${page}/accept`)).status, 404, 'an offer is used once');
});

test('offers can be declined, withdrawn, and lapse; nobody gets pages pushed at them', async () => {
  const s = site({ config: { maxOffersTo: 2 } });
  const p1 = await newPage(s, 1, 'One');
  await s.call(1, 'POST', `/pages/${p1}/transfer`, { who: 'bob' });
  assert.equal((await s.call(2, 'POST', `/offers/${p1}/decline`)).status, 200);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.offers, []);
  assert.deepEqual(ids((await s.call(1, 'GET', '/pages')).body.mine), [p1], 'still alice\'s');
  // Withdrawn by the giver.
  await s.call(1, 'POST', `/pages/${p1}/transfer`, { who: 'bob' });
  assert.equal((await s.call(1, 'DELETE', `/pages/${p1}/transfer`)).status, 200);
  assert.equal((await s.call(2, 'POST', `/offers/${p1}/accept`)).status, 404);
  // Lapsed after offerDays.
  await s.call(1, 'POST', `/pages/${p1}/transfer`, { who: 'bob' });
  s.env.DB.db.prepare('UPDATE paper_transfers SET created_at = ?').run(Date.now() - 31 * DAY);
  assert.equal((await s.call(2, 'POST', `/offers/${p1}/accept`)).status, 404);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.offers, [], 'lapsed offers are cleared');
  // At most maxOffersTo waiting for one person.
  const p2 = await newPage(s, 1), p3 = await newPage(s, 1), p4 = await newPage(s, 1);
  assert.equal((await s.call(1, 'POST', `/pages/${p2}/transfer`, { who: 'bob' })).status, 201);
  assert.equal((await s.call(1, 'POST', `/pages/${p3}/transfer`, { who: 'bob' })).status, 201);
  assert.equal((await s.call(1, 'POST', `/pages/${p4}/transfer`, { who: 'bob' })).status, 409);
  // A page deleted while offered: the offer goes with it.
  assert.equal((await s.call(1, 'DELETE', `/pages/${p2}`)).status, 200);
  assert.equal((await s.call(2, 'POST', `/offers/${p2}/accept`)).status, 404);
});

test('give-away only to accounts the site knows by name or email, never a raw id', async () => {
  const s = site();
  const page = await newPage(s, 1);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/transfer`, { to: '2' })).status, 400);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/transfer`, { to: 'no_such_user', who: 'no_such_user' })).status, 404);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'alice' })).status, 409, 'not to myself');
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_transfers').n, 0);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'bob@example.com' })).status, 201);
});

// ---- H1: full copies can't wipe a page --------------------------------------------

async function filled(s, n = 5, size = 200) {
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' });
  const a = await client(s, 1, page);
  for (let i = 0; i < n; i++) { a.log.add('s' + i, 't', 'x'.repeat(size)); await a.push(); }
  return { page, a };
}

async function tinyCheckpoint(s, as, page) {
  const ws = await s.dial(as, page)();
  ws.onmessage = () => {};
  await ws.send(JSON.stringify({ t: 'hello', since: 0, seq: true }));
  await ws.send(withTag(TAG.CKPT, new Uint8Array(8)));
  await s.room(page).queue;
  return ws;
}

test('an editor\'s tiny full copy replaces nothing (audit probe 4)', async () => {
  const s = site();
  const { page } = await filled(s);
  await tinyCheckpoint(s, 2, page);
  const room = s.room(page);
  assert.equal(room.frameCount(), 6, 'kept as an ordinary change: the 5 are still there');
  assert.equal(room.replacedBytes(), 0);
  const late = await client(s, 3, page);
  assert.equal(late.shown().size, 5, 'a new viewer still gets the whole page');
});

test('a full copy keeps what it replaced; owners see it and bring it back', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  const { page } = await filled(s);
  const before = await used(s, 1);
  // The owner's own tiny copy is allowed (it's their page) but nothing is lost.
  await tinyCheckpoint(s, 1, page);
  const room = s.room(page);
  assert.equal(room.frameCount(), 1);
  assert.equal(room.replacedBytes() > 900, true);
  assert.ok(await used(s, 1) < before, 'what was replaced no longer counts');
  const late = await client(s, 3, page);
  assert.equal(late.shown().size, 0, 'a new device sees only the copy');
  // History: owners only.
  assert.equal((await s.call(2, 'GET', `/pages/${page}/history`)).status, 403);
  const h = (await s.call(1, 'GET', `/pages/${page}/history`)).body;
  assert.equal(h.keepDays, 30);
  assert.equal(h.replaced.length, 1);
  assert.equal(h.replaced[0].frames, 5);
  assert.equal(h.replaced[0].by, '1');
  assert.equal(h.replaced[0].size, 8);
  // Bring it back: everyone on the page gets it at once, new devices too.
  assert.equal((await s.call(2, 'POST', `/pages/${page}/restore`, {})).status, 403);
  const r = await s.call(1, 'POST', `/pages/${page}/restore`, { since: h.replaced[0].seq });
  assert.deepEqual([r.status, r.body.restored], [200, 5]);
  assert.equal(late.shown().size, 5, 'pushed to the people on the page');
  assert.equal((await client(s, 3, page)).shown().size, 5);
  assert.equal(room.replacedBytes(), 0);
  assert.equal(await used(s, 1), room.bytes(), 'charged again');
  assert.deepEqual((await s.call(1, 'GET', `/pages/${page}/history`)).body.replaced, []);
});

test('the admin can roll a page back too', async () => {
  const s = site();
  const { page } = await filled(s);
  await tinyCheckpoint(s, 1, page);
  assert.equal((await s.call(1, 'POST', `/admin/pages/${page}/restore`, {})).status, 404, 'not for owners');
  const r = await s.call(9, 'POST', `/admin/pages/${page}/restore`, {});
  assert.equal(r.body.restored, 5);
});

test('replaced frames go after keepReplacedDays, and beyond keepReplacedBytes', async () => {
  const s = site({ config: { keepReplacedBytes: 600 } });
  const { page } = await filled(s);
  await tinyCheckpoint(s, 1, page);
  const room = s.room(page);
  assert.ok(room.ctx.storage.alarmAt != null, 'an alarm is set');
  // Over the byte cap: the oldest go first, down to the cap.
  await fire(room);
  assert.ok(room.replacedBytes() <= 600 && room.replacedBytes() > 0, String(room.replacedBytes()));
  // Past the days: all of it.
  room.sql.exec('UPDATE frames SET gone = ? WHERE gone IS NOT NULL', Date.now() - 31 * DAY);
  await fire(room);
  assert.equal(room.replacedBytes(), 0);
  assert.equal(room.frameCount(), 1);
});

// ---- H4: the trash ----------------------------------------------------------------

test('deleting puts a page in the trash for 30 days: hidden, restorable, then gone', async () => {
  const s = site({ config: { leaseChunk: 1 } });
  const { page, a } = await filled(s, 3, 1000);
  const bytes = s.room(page).bytes();
  const del = await s.call(1, 'DELETE', `/pages/${page}`);
  assert.equal(del.status, 200);
  assert.equal(del.body.purge - del.body.trashed, 30 * DAY);
  assert.equal(a.ws, null, 'everyone is shown the door');
  // Gone for everyone; in alice's trash.
  const l = (await s.call(1, 'GET', '/pages')).body;
  assert.deepEqual(l.mine, []);
  assert.deepEqual(ids(l.trash), [page]);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.shared, []);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/open`)).status, 404);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/open`)).status, 404);
  assert.equal((await s.dial(1, page)()).status, 404);
  assert.equal(await used(s, 1), bytes, 'still counts while in the trash');
  // Only the owner restores it; then it's all back, shares too.
  assert.equal((await s.call(2, 'POST', `/pages/${page}/untrash`)).status, 404);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/untrash`)).status, 200);
  assert.deepEqual(ids((await s.call(2, 'GET', '/pages')).body.shared), [page]);
  assert.equal((await client(s, 3, page)).shown().size, 3);
  // Trash again, and let the 30 days run out: the room's alarm deletes it for good.
  await s.call(1, 'DELETE', `/pages/${page}`);
  const room = s.room(page);
  // Sooner alarms (sync, idle) each set the next; the last is the end of the trash.
  for (let i = 0; i < 4 && room.ctx.storage.alarmAt < Date.now() + DAY; i++) await fire(room);
  assert.ok(room.ctx.storage.alarmAt >= Date.now() + 29 * DAY, String(room.ctx.storage.alarmAt - Date.now()));
  room.setMeta('trash-until', String(Date.now() - 1));
  await fire(room);
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_pages WHERE id = ?', page).n, 0);
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_members WHERE page_id = ?', page).n, 0);
  assert.equal(await used(s, 1), 0, 'its space comes back');
});

test('a restored page is not purged by an old trash alarm; delete forever from the trash', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'DELETE', `/pages/${page}`);
  await s.call(1, 'POST', `/pages/${page}/untrash`);
  const room = s.room(page);
  room.setMeta('trash-until', String(Date.now() - 1));
  await fire(room);
  assert.deepEqual(ids((await s.call(1, 'GET', '/pages')).body.mine), [page]);
  // Forever only from the trash.
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}?forever=1`)).status, 200, 'first: to the trash');
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}?forever=1`)).status, 200, 'then: for good');
  assert.equal(s.env.DB.row('SELECT COUNT(*) AS n FROM paper_pages WHERE id = ?', page).n, 0);
});

test('the page list purges trash past 30 days even if the room never woke', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'DELETE', `/pages/${page}`);
  s.env.DB.db.prepare('UPDATE paper_pages SET deleted_at = ?').run(Date.now() - 31 * DAY);
  const l = (await s.call(1, 'GET', '/pages')).body;
  assert.deepEqual(l.trash, []);
  assert.equal(l.usage.pages, 0);
});

// ---- M3: room pinning ---------------------------------------------------------------

test('a viewer cannot pin a fresh page to the wrong room (audit probe 6)', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' });
  const bogus = 'b'.repeat(64), right = 'a'.repeat(64);
  const v = await client(s, 3, page, { connect: false });
  v.room = bogus;
  await v.connect();
  assert.equal(v.ws, null, 'refused');
  assert.match(v.notes.find(n => n.t === 'error').msg, /not been opened by its owner/);
  const o = await client(s, 1, page, { connect: false });
  o.room = right;
  await o.connect();
  assert.ok(o.ws && o.live, 'the owner gets in and pins the right room');
  const v2 = await client(s, 3, page, { connect: false });
  v2.room = right;
  await v2.connect();
  assert.ok(v2.live, 'then viewers with the right key get in');
});

test('an owner may re-pin a room that keeps nothing yet; pinning at creation', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  const e = await client(s, 2, page, { connect: false });
  e.room = 'c'.repeat(64);
  await e.connect();
  e.disconnect();
  const o = await client(s, 1, page, { connect: false });
  o.room = 'd'.repeat(64);
  await o.connect();
  assert.ok(o.live, 're-pinned: nothing was kept under the other key');
  // With POST /pages { room }: pinned before anyone joins.
  const { keys } = await import('./fakes.js');
  const r = await s.call(1, 'POST', '/pages', { name: 'Pinned', room: 'e'.repeat(64), ...keys() });
  assert.equal(s.room(r.body.page.id).meta('room'), 'e'.repeat(64));
});

// ---- M4: removed co-owners' links ---------------------------------------------------

test('removing or demoting a co-owner turns off the links they made (audit probe 9)', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'owner' });
  const mine = (await s.call(1, 'POST', `/pages/${page}/links`, { role: 'viewer' })).body;
  const his = (await s.call(2, 'POST', `/pages/${page}/links`, { role: 'editor' })).body;
  const his2 = (await s.call(2, 'POST', `/pages/${page}/links`, { role: 'viewer' })).body;
  // Dave joins through bob's edit link and is on the page.
  assert.equal((await s.call(4, 'POST', `/links/${his.token}`)).status, 200);
  const d = await client(s, 4, page);
  const links = (await s.call(1, 'GET', `/pages/${page}`)).body.links;
  assert.deepEqual(links.map(l => l.by.name).sort(), ['alice', 'bob', 'bob'], 'owners see who made each link');
  // Alice demotes bob to editor: his links go, and dave with them.
  const r = await s.call(1, 'PATCH', `/pages/${page}/members/2`, { role: 'editor' });
  assert.equal(r.body.linksOff, 2);
  assert.equal((await s.call(4, 'POST', `/links/${his.token}`)).status, 404);
  assert.equal((await s.call(4, 'GET', `/links/${his2.token}`)).status, 404);
  assert.equal(d.ws, null, 'dave is shown the door');
  assert.deepEqual((await s.call(4, 'GET', '/pages')).body.shared, []);
  assert.equal((await s.call(3, 'POST', `/links/${mine.token}`)).status, 200, 'alice\'s link still works');
  // Removing works the same, and a link whose maker is no longer an owner is dead anyway.
  await s.call(1, 'PATCH', `/pages/${page}/members/2`, { role: 'owner' });
  const again = (await s.call(2, 'POST', `/pages/${page}/links`, { role: 'editor' })).body;
  s.env.DB.db.prepare("UPDATE paper_members SET role = 'editor' WHERE user_id = '2'").run();
  assert.equal((await s.call(4, 'POST', `/links/${again.token}`)).status, 404, 'checked on use too');
  s.env.DB.db.prepare("UPDATE paper_members SET role = 'owner' WHERE user_id = '2'").run();
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/members/2`)).body.linksOff, 1);
});

// ---- L3: names ----------------------------------------------------------------------

test('page and people names lose markup, controls and bidi tricks', async () => {
  assert.equal(cleanName('  <img src=x onerror=alert(1)>  '), 'img src=x onerror=alert(1)');
  assert.equal(cleanName('a‮gnp.exe\u0000​ b\n\nc'), 'agnp.exe b c');
  assert.equal(cleanName('👍🏽'.repeat(5), 3), '👍🏽👍');
  const s = site();
  const r = await s.call(1, 'POST', '/pages', { name: '<b>bold</b>⁦x', ...(await import('./fakes.js')).keys() });
  assert.equal(r.body.page.name, 'bbold/bx');
  const page = r.body.page.id;
  assert.equal((await s.call(1, 'PATCH', `/pages/${page}`, { name: '<script>' })).body.name, 'script');
  assert.equal((await s.call(1, 'PATCH', `/pages/${page}`, { name: '<>' })).status, 400, 'nothing left: refused');
  assert.equal((await s.call(1, 'POST', '/pages', { name: '‮', ...(await import('./fakes.js')).keys() })).body.page.name, 'Untitled');
});

test('names from the site are cleaned too (me, owners, the room\'s people list)', async () => {
  const s = site();
  s.users[1] = '<i>al‮ice</i>';
  const me = (await s.call(1, 'GET', '/me')).body;
  assert.equal(me.user.name, 'ialice/i');
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  assert.equal((await s.call(2, 'GET', '/pages')).body.shared[0].owner.name, me.user.name);
  const a = await client(s, 1, page);
  assert.equal(a.peers[0].name, me.user.name);
});

test('the directory upgrades itself (no new D1 migration for the site)', async () => {
  const s = site();
  const cols = s.env.DB.db.prepare("SELECT name FROM pragma_table_info('paper_pages')").all().map(c => c.name);
  assert.ok(!cols.includes('deleted_at'), 'a fresh 0001 database');
  await s.call(1, 'GET', '/me');
  const after = s.env.DB.db.prepare("SELECT name FROM pragma_table_info('paper_pages')").all().map(c => c.name);
  assert.ok(after.includes('deleted_at'));
  assert.ok(s.env.DB.row("SELECT name FROM sqlite_master WHERE name = 'paper_transfers'"));
  // A room from before keeps working with its old frames.
  void pair;
});
