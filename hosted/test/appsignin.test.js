// Signing in from the native apps, device tokens, and deleting an account.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { site } from './fakes.js';
import { newPage } from './helpers.js';
import { pairCode } from '../src/oauth.js';

const ORIGIN = 'https://omegagiven.com';
const challengeOf = v => createHash('sha256').update(v).digest('base64url');
let n = 0;
const verifier = () => 'v'.repeat(39) + String(++n).padStart(4, '0');
const params = (v, over = {}) => ({
  client_id: 'og-paper-app', code_challenge: challengeOf(v), code_challenge_method: 'S256', device: 'iPad', ...over,
});

const raw = (s, path, init = {}) => s.api(new Request(ORIGIN + '/paper/api' + path, init), s.env, { waitUntil() {} });
const authorizeUrl = (v, over) => '/oauth/authorize?' + new URLSearchParams(params(v, over));
const agree = (s, as, v, over = {}, choice = 'allow') => raw(s, '/oauth/authorize', {
  method: 'POST',
  headers: { Origin: ORIGIN, 'x-test-user': String(as), 'content-type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ ...params(v, over), choice }).toString(),
});
const post = (s, form) => raw(s, '/oauth/token', {
  method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'og-paper-app', ...form }).toString(),
});
const poll = (s, v) => post(s, { grant_type: 'poll', code_verifier: v });

async function signIn(s, as = 1, device = 'iPad') {
  const v = verifier();
  await agree(s, as, v, { device });
  const res = await poll(s, v);
  assert.equal(res.status, 200);
  return res.json();
}
const app = (s, t, method, path, body) => raw(s, path, {
  method, headers: { Authorization: 'Bearer ' + t, ...(body ? { 'content-type': 'application/json' } : {}) },
  body: body ? JSON.stringify(body) : undefined,
});

test('signed out, the browser goes to the site to sign in and comes back', async () => {
  const s = site();
  const res = await raw(s, authorizeUrl(verifier(), { device: 'd'.repeat(24) }));
  assert.equal(res.status, 302);
  const to = new URL(res.headers.get('location'), ORIGIN);
  assert.equal(to.pathname, '/account/');
  const next = to.searchParams.get('next');
  assert.ok(next.startsWith('/paper/api/oauth/authorize?'));
  assert.ok(next.length < 300, 'the site refuses a next= of 300 characters or more');
});

test('signed in, the person is always asked, with the code the app shows', async () => {
  const s = site();
  const v = verifier();
  const res = await raw(s, authorizeUrl(v, { device: '<script>x</script>' }), { headers: { 'x-test-user': '1' } });
  assert.equal(res.status, 200);
  const html = await res.text();
  assert.ok(html.includes('Sign in to OG Paper?'));
  assert.ok(html.includes(pairCode(challengeOf(v))));
  assert.ok(!html.includes('<script>'));
  assert.match(res.headers.get('content-security-policy'), /frame-ancestors 'none'/);
  assert.equal((await (await poll(s, v)).json()).error, 'authorization_pending');
});

test('the pair code matches the app (same vector as account.rs)', () => {
  assert.equal(pairCode('a-b_c1d2e3f4'), 'ABC1D2');
});

test('saying no ends it', async () => {
  const s = site();
  const v = verifier();
  assert.ok((await (await agree(s, 1, v, {}, 'deny')).text()).includes('Nothing was signed in'));
  assert.equal((await (await poll(s, v)).json()).error, 'access_denied');
});

test('a bad link is refused', async () => {
  const s = site();
  const v = verifier();
  for (const over of [{ client_id: 'other' }, { code_challenge_method: 'plain' }, { code_challenge: 'short' }]) {
    assert.equal((await raw(s, authorizeUrl(v, over), { headers: { 'x-test-user': '1' } })).status, 400, JSON.stringify(over));
  }
});

test('another site can\'t agree on someone\'s behalf', async () => {
  const s = site();
  const res = await raw(s, '/oauth/authorize', {
    method: 'POST', headers: { Origin: 'https://evil.example', 'x-test-user': '1', 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ ...params(verifier()), choice: 'allow' }).toString(),
  });
  assert.equal(res.status, 403);
});

test('only the verifier collects the sign-in, once, and the first answer stands', async () => {
  const s = site();
  const v = verifier();
  await agree(s, 1, v);
  // Someone else agreeing to the same challenge doesn't replace it.
  await agree(s, 2, v);
  assert.equal((await (await poll(s, verifier())).json()).error, 'authorization_pending');
  assert.equal((await post(s, { grant_type: 'poll', code_verifier: 'short' })).status, 400);
  const got = await poll(s, v);
  assert.equal(got.status, 200);
  const me = await (await app(s, (await got.json()).access_token, 'GET', '/me')).json();
  assert.equal(me.user.name, 'alice');
  assert.equal((await (await poll(s, v)).json()).error, 'authorization_pending');
});

test('an answer not collected in time is gone', async () => {
  const s = site();
  const v = verifier();
  await agree(s, 1, v);
  s.env.DB.db.prepare('UPDATE paper_app_codes SET expires_at = 1').run();
  assert.equal((await (await poll(s, v)).json()).error, 'expired_token');
});

test('a token gets the person\'s pages, without Origin, and the room', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Notes');
  const t = await signIn(s);
  const list = await app(s, t.access_token, 'GET', '/pages');
  assert.deepEqual((await list.json()).mine.map(p => p.name), ['Notes']);
  const open = await app(s, t.access_token, 'POST', `/pages/${page}/open`);
  assert.equal(open.status, 200);
  const ws = await s.dial(null, page, { Authorization: 'Bearer ' + t.access_token })();
  assert.equal(ws.status, undefined, 'the room lets the app in');
  assert.equal(s.room(page).ctx.getWebSockets().length, 1);
  // Others' pages stay hidden.
  const other = await newPage(s, 2, 'Private');
  assert.equal((await app(s, t.access_token, 'POST', `/pages/${other}/open`)).status, 404);
});

test('a bad token gets 401 and never falls back on a cookie', async () => {
  const s = site();
  const res = await raw(s, '/pages', { headers: { Authorization: 'Bearer nope', 'x-test-user': '1' } });
  assert.equal(res.status, 401);
  assert.equal((await res.json()).token, 'invalid');
});

test('a token carries no admin rights, and writes without a token need an origin', async () => {
  const s = site();
  const t = await signIn(s, 9, 'Admin phone');
  assert.equal((await s.call(9, 'GET', '/admin/reports')).status, 200);
  assert.notEqual((await app(s, t.access_token, 'GET', '/admin/reports')).status, 200);
  assert.equal((await raw(s, '/pages', { method: 'POST', headers: { 'x-test-user': '1' }, body: '{}' })).status, 403);
});

test('refresh tokens rotate; a stale one ends the sign-in', async () => {
  const s = site();
  const t1 = await signIn(s);
  const refresh = rt => post(s, { grant_type: 'refresh_token', refresh_token: rt });
  const r2 = await refresh(t1.refresh_token);
  assert.equal(r2.status, 200);
  const t2 = await r2.json();
  assert.notEqual(t2.refresh_token, t1.refresh_token);
  assert.equal((await app(s, t1.access_token, 'GET', '/pages')).status, 401);
  // The answer got lost on a bad network: the old one works for a moment.
  const r3 = await refresh(t1.refresh_token);
  assert.equal(r3.status, 200);
  const t3 = await r3.json();
  // Long after, it means the token was copied.
  s.env.DB.db.prepare('UPDATE paper_app_grants SET rotated_at = 1').run();
  assert.equal((await refresh(t1.refresh_token)).status, 400);
  assert.equal((await app(s, t3.access_token, 'GET', '/pages')).status, 401);
  assert.equal((await refresh(t3.refresh_token)).status, 400);
  assert.equal((await post(s, { grant_type: 'password' })).status, 400);
});

test('devices are listed, and revoking one ends it at once, live sockets too', async () => {
  const s = site();
  const page = await newPage(s, 1, 'Notes');
  const phone = await signIn(s, 1, 'Phone');
  const mac = await signIn(s, 1, 'Mac');
  const list = await (await app(s, mac.access_token, 'GET', '/devices')).json();
  assert.deepEqual(list.devices.map(d => [d.name, d.current]).sort(), [['Mac', true], ['Phone', false]]);
  const id = list.devices.find(d => d.name === 'Phone').id;
  // Someone else can't touch it.
  assert.equal((await s.call(2, 'DELETE', `/devices/${id}`)).status, 200);
  assert.equal((await app(s, phone.access_token, 'GET', '/pages')).status, 200);
  const ws = await s.dial(null, page, { Authorization: 'Bearer ' + phone.access_token })();
  assert.equal(ws.status, undefined, 'the phone is on the page');
  assert.equal((await s.call(1, 'DELETE', `/devices/${id}`)).status, 200);
  assert.equal((await app(s, phone.access_token, 'GET', '/pages')).status, 401);
  assert.equal(s.room(page).ctx.getWebSockets().length, 0);
  // Signing out from the app itself.
  assert.equal((await app(s, mac.access_token, 'POST', '/oauth/revoke')).status, 200);
  assert.equal((await app(s, mac.access_token, 'GET', '/pages')).status, 401);
});

test('deleting an account removes its pages and sign-ins, keeps co-owned pages', async () => {
  const s = site();
  const gone = await newPage(s, 1, 'Mine');
  const kept = await newPage(s, 1, 'Ours');
  const theirs = await newPage(s, 2, 'Theirs');
  await s.call(1, 'POST', `/pages/${kept}/members`, { who: 'bob', role: 'editor' });
  await s.call(1, 'PATCH', `/pages/${kept}/members/2`, { role: 'owner' });
  await s.call(2, 'POST', `/pages/${theirs}/members`, { who: 'alice', role: 'viewer' });
  const t = await signIn(s);
  assert.equal((await s.call(1, 'DELETE', '/account', {})).status, 400);
  const res = await app(s, t.access_token, 'DELETE', '/account', { confirm: 'delete' });
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { ok: true, deleted: 1, handed: 1 });
  const db = s.env.DB;
  assert.equal(db.row('SELECT COUNT(*) AS n FROM paper_pages WHERE id = ?', gone).n, 0);
  assert.equal(db.row('SELECT owner_id FROM paper_pages WHERE id = ?', kept).owner_id, '2');
  for (const table of ['paper_members', 'paper_app_grants', 'paper_accounts']) {
    assert.equal(db.row(`SELECT COUNT(*) AS n FROM ${table} WHERE user_id = '1'`).n, 0, table);
  }
  assert.equal((await app(s, t.access_token, 'GET', '/pages')).status, 401);
  assert.deepEqual((await s.call(2, 'GET', '/pages')).body.mine.map(p => p.name).sort(), ['Ours', 'Theirs']);
});
