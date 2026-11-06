// Signing in from the native apps, device tokens, and deleting an account.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { site } from './fakes.js';
import { newPage } from './helpers.js';

const ORIGIN = 'https://omegagiven.com';
const VERIFIER = 'v'.repeat(43);
const challenge = v => createHash('sha256').update(v).digest('base64url');
const REDIRECT = 'ogpaper://signed-in';
const params = (over = {}) => ({
  client_id: 'og-paper-app', redirect_uri: REDIRECT, code_challenge: challenge(VERIFIER),
  code_challenge_method: 'S256', state: 'abc', device: 'iPad', ...over,
});

const raw = (s, path, init = {}) => s.api(new Request(ORIGIN + '/paper/api' + path, init), s.env, { waitUntil() {} });
const authorizeUrl = over => '/oauth/authorize?' + new URLSearchParams(params(over));
const agree = (s, as, over = {}, choice = 'allow') => raw(s, '/oauth/authorize', {
  method: 'POST',
  headers: { Origin: ORIGIN, 'x-test-user': String(as), 'content-type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ ...params(over), choice }).toString(),
});
const post = (s, form) => raw(s, '/oauth/token', {
  method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'og-paper-app', ...form }).toString(),
});
const codeOf = res => new URL(res.headers.get('location')).searchParams.get('code');
const exchange = (s, code, over = {}) => post(s, { grant_type: 'authorization_code', code, code_verifier: VERIFIER, redirect_uri: REDIRECT, ...over });

async function signIn(s, as = 1, device = 'iPad') {
  const res = await exchange(s, codeOf(await agree(s, as, { device })));
  assert.equal(res.status, 200);
  return res.json();
}
const app = (s, t, method, path, body) => raw(s, path, {
  method, headers: { Authorization: 'Bearer ' + t, ...(body ? { 'content-type': 'application/json' } : {}) },
  body: body ? JSON.stringify(body) : undefined,
});

test('signed out, the browser goes to the site to sign in and comes back', async () => {
  const s = site();
  const res = await raw(s, authorizeUrl());
  assert.equal(res.status, 302);
  const to = new URL(res.headers.get('location'), ORIGIN);
  assert.equal(to.pathname, '/account/');
  const next = to.searchParams.get('next');
  assert.ok(next.startsWith('/paper/api/oauth/authorize?'));
  // The site refuses a next= of 300 characters or more; loopback is the longest.
  const long = await raw(s, authorizeUrl({ redirect_uri: 'http://127.0.0.1:54321/signed-in', state: 'a'.repeat(22), device: 'd'.repeat(24) }));
  assert.ok(new URL(long.headers.get('location'), ORIGIN).searchParams.get('next').length < 300);
});

test('signed in, the person is always asked, and the page is safe', async () => {
  const s = site();
  const res = await raw(s, authorizeUrl({ device: '<script>x</script>' }), { headers: { 'x-test-user': '1' } });
  assert.equal(res.status, 200);
  const html = await res.text();
  assert.ok(html.includes('Sign in to OG Paper?'));
  assert.ok(!html.includes('<script>'));
  assert.match(res.headers.get('content-security-policy'), /frame-ancestors 'none'/);
  assert.match(res.headers.get('content-security-policy'), /form-action 'self' ogpaper:/);
  const no = await agree(s, 1, {}, 'deny');
  const back = new URL(no.headers.get('location'));
  assert.equal(back.searchParams.get('error'), 'access_denied');
  assert.equal(back.searchParams.get('code'), null);
});

test('only the app\'s own addresses get a code', async () => {
  const s = site();
  for (const redirect_uri of ['https://evil.example/signed-in', 'ogpaper://evil', 'http://127.0.0.1.evil.example:1/signed-in', 'http://localhost:1/signed-in']) {
    assert.equal((await raw(s, authorizeUrl({ redirect_uri }), { headers: { 'x-test-user': '1' } })).status, 400, redirect_uri);
    assert.equal((await agree(s, 1, { redirect_uri })).status, 400, redirect_uri);
  }
  assert.equal((await raw(s, authorizeUrl({ client_id: 'other' }), { headers: { 'x-test-user': '1' } })).status, 400);
  assert.equal((await raw(s, authorizeUrl({ code_challenge_method: 'plain' }), { headers: { 'x-test-user': '1' } })).status, 400);
});

test('another site can\'t agree on someone\'s behalf', async () => {
  const s = site();
  const res = await raw(s, '/oauth/authorize', {
    method: 'POST', headers: { Origin: 'https://evil.example', 'x-test-user': '1', 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ ...params(), choice: 'allow' }).toString(),
  });
  assert.equal(res.status, 403);
});

test('a code needs the verifier, the same address, and works once', async () => {
  const s = site();
  const res = await agree(s, 1);
  const back = new URL(res.headers.get('location'));
  assert.equal(back.searchParams.get('state'), 'abc');
  assert.equal(back.searchParams.get('iss'), ORIGIN);
  const code = codeOf(res);
  assert.equal((await exchange(s, code, { code_verifier: 'w'.repeat(43) })).status, 400);
  // That wrong try spent it.
  assert.equal((await exchange(s, code)).status, 400);
  const good = codeOf(await agree(s, 1));
  assert.equal((await exchange(s, good, { redirect_uri: 'http://127.0.0.1:9/signed-in' })).status, 400);
  const again = codeOf(await agree(s, 1));
  assert.equal((await exchange(s, again)).status, 200);
  assert.equal((await exchange(s, again)).status, 400);
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
