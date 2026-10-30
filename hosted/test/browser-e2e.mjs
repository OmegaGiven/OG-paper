// The web app itself (the Rust app in a browser) against a hosted site in
// workerd: alice signs in and sees her account panel, makes a page and
// writes on it, shares it with bob; bob sees it under Shared with me, opens
// it, gets alice's writing and writes back live.
//   BASE=http://127.0.0.1:8799 PW=/path/with/node_modules/playwright node hosted/test/browser-e2e.mjs

import assert from 'node:assert/strict';

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const BASE = process.env.BASE || 'http://127.0.0.1:8799';
const wait = ms => new Promise(r => setTimeout(r, ms));
// SHOTS=dir: keep screenshots of the warnings there.
const shot = (p, name) => process.env.SHOTS ? p.screenshot({ path: `${process.env.SHOTS}/${name}.png` }) : null;

const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });

async function person(name) {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  page.on('pageerror', e => console.log(`[${name}] page error:`, e.message));
  page.on('console', m => { if (m.type() === 'error') console.log(`[${name}]`, m.text()); });
  page.on('dialog', d => d.accept(d.type() === 'prompt' ? 'E2E page' : undefined));
  await page.goto(`${BASE}/dev/login?as=${name}`);
  await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
  return page;
}

async function texts(page) {
  await page.evaluate(() => window.ogPaper.run([{ get: 'texts' }]));
  for (let i = 0; i < 50; i++) {
    const r = await page.evaluate(() => window.ogPaper.results());
    if (r) return JSON.stringify(r);
    await wait(100);
  }
  return '';
}
async function until(f, ms = 30000) {
  const t = Date.now();
  for (;;) { if (await f()) return; if (Date.now() - t > ms) throw new Error('timed out'); await wait(300); }
}

const alice = await person('alice');
// Signed in, the account panel shows by itself.
await alice.waitForSelector('section[data-name="My account"]:not([hidden])', { timeout: 15000 });
assert.match(await alice.textContent('section[data-name="My account"]'), /Signed in as alice/);
console.log('alice: account panel on sign-in');
await alice.click('section[data-name="My account"] [data-act=new]');
await until(async () => (await alice.evaluate(() => fetch('/paper/api/pages').then(r => r.json()))).mine.length === 1);
await alice.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 0, text: 'hello-from-alice' }]));
const list = await alice.evaluate(() => fetch('/paper/api/pages').then(r => r.json()));
const id = list.mine[0].id;
assert.equal(list.mine[0].name, 'E2E page');
const shared = await alice.evaluate(id => fetch(`/paper/api/pages/${id}/members`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ who: 'bob', role: 'editor' }) }).then(r => r.status), id);
assert.equal(shared, 200);
await until(async () => (await alice.evaluate(() => window.ogPaper.cloud())) === 'saved');
console.log('alice: page made, written on, shared; saved to the account');

const bob = await person('bob');
await bob.waitForSelector('section[data-name="My account"]:not([hidden])', { timeout: 15000 });
await bob.waitForSelector(`[data-list=shared] li[data-id="${id}"] .go`);
await bob.click(`[data-list=shared] li[data-id="${id}"] .go`);
await until(async () => (await texts(bob)).includes('hello-from-alice'));
console.log('bob: opened from Shared with me, got alice\'s writing');
await bob.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 80, text: 'hi-from-bob' }]));
await until(async () => (await texts(alice)).includes('hi-from-bob'));
console.log('alice: got bob\'s writing live');

// Bob makes a copy into his own account; carol, given the copy, sees it all.
await bob.evaluate(() => window.ogPaper.account());
await bob.waitForSelector(`[data-list=shared] li[data-id="${id}"] [data-act=share]`);
await bob.click(`[data-list=shared] li[data-id="${id}"] [data-act=share]`);
await bob.waitForSelector('section[data-name="Page"] [data-act=copy]');
await bob.click('section[data-name="Page"] [data-act=copy]');
let copyId = null;
await until(async () => { const l = await bob.evaluate(() => fetch('/paper/api/pages').then(r => r.json())); copyId = l.mine[0]?.id; return !!copyId; });
assert.equal((await bob.evaluate(() => fetch('/paper/api/pages').then(r => r.json()))).mine[0].name, 'Copy of E2E page');
await until(async () => { const t = await texts(bob); return t.includes('hello-from-alice') && t.includes('hi-from-bob'); });
await wait(1500);
assert.equal(await bob.evaluate(id => fetch(`/paper/api/pages/${id}/members`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ who: 'carol', role: 'viewer' }) }).then(r => r.status), copyId), 200);
const carol = await person('carol');
await carol.waitForSelector(`[data-list=shared] li[data-id="${copyId}"] .go`);
await carol.click(`[data-list=shared] li[data-id="${copyId}"] .go`);
await until(async () => { const t = await texts(carol); return t.includes('hello-from-alice') && t.includes('hi-from-bob'); });
console.log('bob: made a copy into his account (carol opened it and got everything)');

// Reload: alice's page comes back from the account by itself.
await alice.reload();
await alice.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
await until(async () => { const t = await texts(alice); return t.includes('hi-from-bob') && t.includes('hello-from-alice'); });
console.log('alice: reopened from the account after reload');
// Storage nearly full, then full: warnings, the offer of more storage, and
// the browser asking before the tab closes with edits not in the account.
const admin = await person('admin');
assert.equal(await admin.evaluate(() => fetch('/paper/api/admin/accounts/dave', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ quota: 60 * 1024 }) }).then(r => r.status)), 200);
const dave = await person('dave');
await dave.waitForSelector('section[data-name="My account"]:not([hidden])', { timeout: 15000 });
assert.match(await dave.textContent('section[data-name="My account"]'), /Get more storage/);
await dave.click('section[data-name="My account"] [data-act=new]');
await until(async () => (await dave.evaluate(() => window.ogPaper.cloud())) === 'saved');
let sawNearly = false;
const levels = [];
for (let i = 0; i < 200; i++) {
  await dave.evaluate(i => {
    const pts = Array.from({ length: 200 }, (_, k) => [k * 2 + i * 7, Math.sin(k / 9 + i) * 300]);
    window.ogPaper.run([{ add: 'stroke', points: pts, width: 3, color: '#1e5ac8' }]);
  }, i);
  await wait(250);
  if (!sawNearly && await dave.isVisible('section[data-name="Storage nearly full"]')) { sawNearly = true; await shot(dave, 'storage-nearly-full'); }
  levels.push(await dave.evaluate(() => localStorage.getItem('og-quota-warned')));
  if (await dave.isVisible('section[data-name="Storage full"]')) break;
}
sawNearly ||= levels.some(l => l && l.includes('nearly'));
assert.ok(sawNearly, 'warned when nearly full: ' + levels.filter(Boolean).slice(-3).join(' | '));
await dave.waitForSelector('section[data-name="Storage full"]:not([hidden])');
assert.match(await dave.textContent('section[data-name="Storage full"]'), /not saved to your account/);
assert.equal(await dave.getAttribute('section[data-name="Storage full"] [data-act=upgrade]', 'href'), '/dev/storage');
assert.equal(await dave.evaluate(() => window.ogPaper.cloud()), 'unsaved');
await dave.waitForSelector('.og-cloud.warn:not([hidden])');
await shot(dave, 'storage-full');
assert.match(await dave.textContent('.og-cloud'), /Not saved to your account/);
// The browser only asks after the person has touched the page (a click
// may draw a dot: wait for it to be saved when that is the point).
const asked = async (p, settle = false) => {
  await p.click('canvas', { position: { x: 3, y: 300 } });
  if (settle) await until(async () => (await p.evaluate(() => window.ogPaper.cloud())) === 'saved');
  let kind = null;
  p.removeAllListeners('dialog');
  p.on('dialog', d => { kind = d.type(); d.dismiss(); });
  await p.close({ runBeforeUnload: true });
  await wait(1000);
  return kind;
};
assert.equal(await asked(dave), 'beforeunload', 'closing asks first');
console.log('dave: nearly-full and full warnings, more-storage offer, close asks first');

// Signed out on the site: work is only in this browser, and closing asks.
const evePage = await (await browser.newContext()).newPage();
await evePage.goto(`${BASE}/paper/`);
await evePage.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
await evePage.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 0, text: 'mine only' }]));
await until(async () => (await evePage.evaluate(() => window.ogPaper.cloud())) === 'local');
await evePage.waitForSelector('.og-cloud:not([hidden])');
assert.match(await evePage.textContent('.og-cloud'), /sign in to save to the cloud/);
await shot(evePage, 'signed-out-chip');
assert.equal(await asked(evePage), 'beforeunload');
// And an account page that is saved closes without asking.
assert.equal(await alice.evaluate(() => window.ogPaper.cloud()), 'saved');
assert.equal(await asked(alice, true), null, 'saved: no question');
console.log('signed out: only in this browser, close asks; saved pages close quietly');

await browser.close();
console.log('BROWSER E2E OK');
process.exit(0);
