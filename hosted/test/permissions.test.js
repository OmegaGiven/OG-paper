// Who may see and change a page: owners, editors, viewers, links, and
// what happens when access is taken away.

import test from 'node:test';
import assert from 'node:assert/strict';
import { site } from './fakes.js';
import { client, last, newPage, sorted } from './helpers.js';

test('my pages show on sign-in; others see nothing, not even that they exist', async () => {
  const s = site();
  const p1 = await newPage(s, 1, 'Notes');
  const p2 = await newPage(s, 1, 'Sketches');
  const mine = await s.call(1, 'GET', '/pages');
  assert.deepEqual(mine.body.mine.map(p => p.name).sort(), ['Notes', 'Sketches']);
  assert.deepEqual(mine.body.shared, []);
  assert.equal((await s.call(null, 'GET', '/pages')).status, 401);
  const bob = await s.call(2, 'GET', '/pages');
  assert.deepEqual(bob.body.mine, []);
  assert.deepEqual(bob.body.shared, []);
  // A page bob has no access to looks exactly like a page that doesn't exist.
  const r1 = await s.call(2, 'GET', `/pages/${p1}`);
  const r2 = await s.call(2, 'GET', `/pages/${'f'.repeat(32)}`);
  assert.equal(r1.status, 404);
  assert.deepEqual(r1.body, r2.body);
  assert.equal((await s.call(2, 'POST', `/pages/${p2}/open`)).status, 404);
  assert.equal((await s.dial(2, p1)()).status, 404);
  assert.equal((await s.dial(null, p1)()).status, 401);
});

test('sharing by username or email puts the page under Shared with me', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Plans');
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob@example.com', role: 'viewer' })).status, 200);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'editor' })).status, 200);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/members`, { who: 'nobody', role: 'editor' })).status, 404);
  const bob = await s.call(2, 'GET', '/pages');
  assert.deepEqual(bob.body.shared.map(p => [p.name, p.role, p.owner.name]), [['Plans', 'viewer', 'alice']]);
  // The owner sees who has it; others don't get the list.
  const detail = await s.call(1, 'GET', `/pages/${page}`);
  assert.deepEqual(detail.body.members.map(m => [m.name, m.role]).sort(), [['alice', 'owner'], ['bob', 'viewer'], ['carol', 'editor']]);
  assert.equal((await s.call(3, 'GET', `/pages/${page}`)).body.members, undefined);
  // Viewers get the view key only; editors the edit key.
  assert.match((await s.call(2, 'POST', `/pages/${page}/open`)).body.link, /\?k=v/);
  assert.match((await s.call(3, 'POST', `/pages/${page}/open`)).body.link, /\?k=e/);
  assert.match((await s.call(3, 'POST', `/pages/${page}/open`)).body.link, /^wss:\/\/omegagiven\.com\/paper\/api\/pages\/[0-9a-f]{32}\/relay\?k=e.*&c=[0-9a-f]{32}$/);
});

test('a viewer cannot write: changes are refused and never reach anyone', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  const a = await client(s, 1, page);
  const v = await client(s, 2, page);
  a.log.add('by-owner', 't', 1); await a.push();
  assert.ok(v.shown().has('by-owner'), 'viewers see changes live');
  v.log.add('by-viewer', 't', 1); await v.push();
  assert.equal(last(v, 'denied').msg, 'You can only view this page');
  assert.ok(!a.shown().has('by-viewer'));
  assert.equal(s.room(page).frameCount(), 1);
  // Nor checkpoints (which would wipe the page).
  await v.checkpoint();
  assert.equal(s.room(page).frameCount(), 1);
  // Viewers can't rename, share, link or delete either.
  assert.equal((await s.call(2, 'PATCH', `/pages/${page}`, { name: 'mine now' })).status, 403);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'editor' })).status, 403);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/links`, { role: 'editor' })).status, 403);
  assert.equal((await s.call(2, 'DELETE', `/pages/${page}`)).status, 403);
});

test('an editor can draw and rename but not share or delete', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  const e = await client(s, 2, page);
  e.log.add('x', 't', 1); await e.push();
  assert.equal(s.room(page).frameCount(), 1);
  assert.equal((await s.call(2, 'PATCH', `/pages/${page}`, { name: 'Renamed' })).status, 200);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' })).status, 403);
  assert.equal((await s.call(2, 'DELETE', `/pages/${page}`)).status, 403);
});

test('a revoked person is disconnected at once and loses all access', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  const a = await client(s, 1, page);
  const b = await client(s, 2, page);
  assert.ok(b.live);
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/members/2`)).status, 200);
  assert.equal(b.ws, null, 'kicked from the live room');
  assert.equal(last(b, 'kicked').msg, 'You no longer have access to this page');
  assert.ok(a.live, 'others stay');
  assert.ok(!(await b.connect()), 'cannot come back');
  assert.equal(b.refused.status, 404);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/open`)).status, 404);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.shared, []);
});

test('demoting an editor to viewer takes effect immediately', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'editor' });
  const b = await client(s, 2, page);
  assert.equal((await s.call(1, 'PATCH', `/pages/${page}/members/2`, { role: 'viewer' })).status, 200);
  assert.equal(b.ws, null);
  await b.connect();
  b.log.add('nope', 't', 1); await b.push();
  assert.ok(last(b, 'denied'));
  assert.equal(s.room(page).frameCount(), 0);
});

test('links: anyone signed in with the link gets in; revoking takes them out', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Open house');
  const link = await s.call(1, 'POST', `/pages/${page}/links`, { role: 'editor' });
  assert.equal(link.status, 201);
  assert.equal((await s.call(null, 'GET', `/links/${link.body.token}`)).body.signIn, true);
  assert.equal((await s.call(null, 'POST', `/links/${link.body.token}`)).status, 401);
  const joined = await s.call(4, 'POST', `/links/${link.body.token}`);
  assert.equal(joined.status, 200);
  assert.equal(joined.body.page.role, 'editor');
  assert.deepEqual((await s.call(4, 'GET', '/pages')).body.shared.map(p => p.name), ['Open house']);
  const d = await client(s, 4, page);
  d.log.add('via-link', 't', 1); await d.push();
  assert.equal(s.room(page).frameCount(), 1);
  // Revoke: dave is disconnected, removed, and the link is dead.
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/links/${link.body.id}`)).status, 200);
  assert.equal(d.ws, null);
  assert.deepEqual((await s.call(4, 'GET', '/pages')).body.shared, []);
  assert.equal((await s.call(4, 'POST', `/links/${link.body.token}`)).status, 404);
  assert.ok(!(await d.connect()));
});

test('links can be switched off for the whole site', async () => {
  const s = site({ config: { linkSharing: false } });
  const page = await newPage(s, 1);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/links`, { role: 'viewer' })).status, 403);
});

test('co-owners can share; ownership can be given away', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Ours');
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'owner' });
  assert.equal((await s.call(2, 'POST', `/pages/${page}/members`, { who: 'carol', role: 'viewer' })).status, 200);
  // A co-owner can't delete it or give it away; the billing owner can.
  assert.equal((await s.call(2, 'DELETE', `/pages/${page}`)).status, 403);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/transfer`, { who: 'carol' })).status, 403);
  const t = await s.call(1, 'POST', `/pages/${page}/transfer`, { who: 'bob', leave: true });
  assert.equal(t.status, 200);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.mine.map(p => p.name), ['Ours']);
  assert.deepEqual((await s.call(1, 'GET', '/pages')).body.mine, []);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/open`)).status, 404);
});

test('leaving a shared page', async () => {
  const s = site();
  const page = await newPage(s, 1);
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  assert.equal((await s.call(2, 'DELETE', `/pages/${page}/members/2`)).status, 200);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.shared, []);
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/members/1`)).status, 409, 'the owner cannot just leave');
});

test('another site cannot use the session (origin checks)', async () => {
  const s = site();
  const page = await newPage(s, 1);
  const evil = { Origin: 'https://evil.example' };
  assert.equal((await s.call(1, 'POST', '/pages', { name: 'x' }, evil)).status, 403);
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}`, undefined, evil)).status, 403);
  assert.equal((await s.dial(1, page, evil)()).status, 403);
  assert.equal((await s.call(1, 'POST', `/pages/${page}/open`, undefined, { 'Sec-Fetch-Site': 'cross-site' })).status, 403);
});

test('reports reach the admin, who can take a page down', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Bad stuff');
  await s.call(1, 'POST', `/pages/${page}/members`, { who: 'bob', role: 'viewer' });
  const b = await client(s, 2, page);
  assert.equal((await s.call(2, 'POST', `/pages/${page}/report`, { reason: 'spam' })).status, 201);
  assert.equal(s.reports.length, 1, 'the site hears of it');
  assert.equal((await s.call(3, 'POST', `/pages/${page}/report`, { reason: 'x' })).status, 404, 'only people who can see it');
  assert.equal((await s.call(1, 'GET', '/admin/reports')).status, 404, 'not for non-admins');
  const reports = await s.call(9, 'GET', '/admin/reports');
  assert.deepEqual(reports.body.reports.map(r => [r.page_name, r.reason]), [['Bad stuff', 'spam']]);
  const look = await s.call(9, 'GET', `/admin/pages/${page}`);
  assert.match(look.body.open.link, /\?k=v/, 'the admin may look, view only');
  assert.equal((await s.call(9, 'POST', `/admin/pages/${page}/block`, { blocked: true })).status, 200);
  assert.equal(b.ws, null, 'everyone is disconnected');
  assert.equal((await s.call(1, 'POST', `/pages/${page}/open`)).status, 451);
  assert.equal((await s.dial(2, page)()).status, 451);
  assert.equal((await s.call(9, 'DELETE', `/admin/pages/${page}`)).status, 200);
  assert.deepEqual((await s.call(1, 'GET', '/pages')).body.mine, []);
  void sorted;
});
