// The web app itself (the Rust app in a browser) against a hosted site in
// workerd: alice signs in and sees her account panel, makes a page and
// writes on it, shares it with bob; bob sees it under Shared with me, opens
// it, gets alice's writing and writes back live.
//   BASE=http://127.0.0.1:8799 PW=/path/with/node_modules/playwright node hosted/test/browser-e2e.mjs

import assert from 'node:assert/strict';

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const BASE = process.env.BASE || 'http://127.0.0.1:8799';
const wait = ms => new Promise(r => setTimeout(r, ms));

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
console.log('alice: page made, written on, shared');

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
await browser.close();
console.log('BROWSER E2E OK');
process.exit(0);
