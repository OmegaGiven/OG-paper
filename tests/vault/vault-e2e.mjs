// OG Paper's web app in vault mode, against a throwaway OG Vault (tests/vault/run-e2e.sh):
// - signed out, the account panel offers OG Studio and Vault; Vault has this vault's address
//   filled in (the copy is served by the vault), Continue goes to the vault's own sign-in page
//   (PKCE) and comes back signed in as "alice @ <vault>";
// - alice makes a page and writes on it; a second device of hers signs in the same way, finds
//   the page under My pages, opens it and gets the writing; both write live;
// - the second device goes offline, writes, comes back: the room gets it, and what the first
//   device wrote meanwhile reaches it (offline edits merge);
// - after a reload the profile and the page are still there; Switch lists the vault profile;
// - bob (another person on the vault) gets the page shared with him and sees it.
//   VAULT=http://127.0.0.1:18792 ALICE=<session> BOB=<session> PW=<dir with node_modules/playwright> node tests/vault/vault-e2e.mjs

import assert from 'node:assert/strict';

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const VAULT = process.env.VAULT || 'http://127.0.0.1:18792';
const APP = `${VAULT}/paper/`;
const wait = ms => new Promise(r => setTimeout(r, ms));
const shot = (p, name) => process.env.SHOTS ? p.screenshot({ path: `${process.env.SHOTS}/${name}.png` }) : null;
const browser = await chromium.launch({
  executablePath: process.env.OGP_BROWSER || undefined,
  args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'],
});

async function until(f, ms = 30000, what = 'condition') {
  const t = Date.now();
  for (;;) { if (await f()) return; if (Date.now() - t > ms) throw new Error('timed out: ' + what); await wait(300); }
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
const account = 'section[data-name="My account"]';

/** A device: a browser of its own, signed in to the vault (its session cookie). */
async function device(name, session, size) {
  const ctx = await browser.newContext(size ? { viewport: size, isMobile: true, hasTouch: true } : {});
  await ctx.addCookies([{ name: 'ogv_session', value: session, url: VAULT }]);
  const page = await ctx.newPage();
  page.on('pageerror', e => console.log(`[${name}] page error:`, e.message));
  page.on('console', m => { if ((process.env.DEBUG || m.type() === 'error') && !/ERR_INTERNET_DISCONNECTED|net::ERR/.test(m.text())) console.log(`[${name}]`, m.text()); });
  if (process.env.DEBUG) {
    page.on('websocket', ws => { console.log(`[${name}] ws`, ws.url().replace(/k=[^&]+/, 'k=…')); ws.on('close', () => console.log(`[${name}] ws closed`)); ws.on('socketerror', e => console.log(`[${name}] ws error`, e)); });
    page.on('response', r => { if (r.status() >= 400) console.log(`[${name}] ${r.status()} ${r.request().method()} ${r.url()}`); });
  }
  page.on('dialog', d => d.accept(d.type() === 'prompt' ? 'Vault page' : undefined));
  await page.goto(APP);
  await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
  return { ctx, page };
}

async function signIn(page, who) {
  await page.evaluate(() => window.ogPaper.account());
  await page.waitForSelector(`${account} [data-act=vault]`);
  assert.match(await page.textContent(`${account}`), /OG Studio/);
  await page.click(`${account} [data-act=vault]`);
  assert.equal(await page.inputValue(`${account} [data-addr]`), VAULT, 'served by the vault: its address is filled in');
  await shot(page, `${who}-choice`);
  await Promise.all([page.waitForURL(/\/auth\/vault\/continue\//), page.click(`${account} [data-vault] button[type=submit]`)]);
  // The vault's own page: already signed in there, "Continue as …".
  await page.waitForSelector('form#go:not([hidden]) button.primary');
  assert.match(await page.textContent('body'), /OG Paper/);
  await shot(page, `${who}-vault-page`);
  await Promise.all([page.waitForURL(u => u.pathname === '/paper/' && !u.search.includes('code=')), page.click('form#go button.primary')]);
  await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
  await page.evaluate(() => window.ogPaper.account());
  await until(async () => /Signed in as/.test(await page.textContent(account)), 20000, `${who} signed in`);
  assert.match(await page.textContent(account), new RegExp(`Signed in as ${who}[^@]* @ `));
  await shot(page, `${who}-signed-in`);
}

// ---- alice, first device -------------------------------------------------------------------
const a = await device('alice-1', process.env.ALICE);
// Signed out: no chip, no prompt to leave.
assert.equal(await a.page.evaluate(() => window.ogPaper.cloud?.()), 'empty');
await signIn(a.page, 'alice');
console.log('alice: signed in through the vault sign-in page (PKCE), back in OG Paper');
await a.page.click(`${account} [data-act=new]`);
await until(async () => { const c = await a.page.evaluate(() => window.ogPaper.cloud()); if (process.env.DEBUG) console.log('cloud', c, await a.page.textContent('body').then(t => t.slice(0, 300))); return c === 'saved'; }, 20000, 'new page saved');
await a.page.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 0, text: 'hello-from-device-1' }]));
await until(async () => (await a.page.evaluate(() => window.ogPaper.cloud())) === 'saved', 20000, 'writing saved');
console.log('alice: new page in the vault, written on, kept by the room');

// ---- alice, second device (a phone) ----------------------------------------------------------
const b = await device('alice-2', process.env.ALICE, { width: 390, height: 844 });
await signIn(b.page, 'alice');
await b.page.waitForSelector(`${account} [data-list=mine] li .go`);
assert.match(await b.page.textContent(`${account} [data-list=mine]`), /Vault page/);
await b.page.click(`${account} [data-list=mine] li .go`);
await until(async () => (await texts(b.page)).includes('hello-from-device-1'), 30000, 'device 2 gets the page');
console.log('alice phone: the page is under My pages and opens with device 1\'s writing');
await b.page.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 80, text: 'live-from-device-2' }]));
await until(async () => (await texts(a.page)).includes('live-from-device-2'), 30000, 'live to device 1');
console.log('alice: device 2\'s writing arrives live on device 1');

// ---- offline edits -------------------------------------------------------------------------
await b.ctx.setOffline(true);
await b.page.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 160, text: 'offline-on-device-2' }]));
await a.page.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 240, text: 'meanwhile-on-device-1' }]));
await wait(3000);
const offState = await b.page.evaluate(() => window.ogPaper.cloud());
console.log('alice phone offline: its page is', offState);
assert.ok(!(await texts(a.page)).includes('offline-on-device-2'), 'nothing went up while offline');
await b.ctx.setOffline(false);
await until(async () => (await texts(a.page)).includes('offline-on-device-2'), 45000, 'offline edit arrives');
await until(async () => (await texts(b.page)).includes('meanwhile-on-device-1'), 45000, 'meanwhile edit arrives');
await until(async () => (await b.page.evaluate(() => window.ogPaper.cloud())) === 'saved', 30000, 'phone saved again');
console.log('alice: offline edits merged both ways on reconnect');

// ---- a reload keeps the profile and the page -------------------------------------------------
await b.page.reload();
await b.page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
await until(async () => { const t = await texts(b.page); return t.includes('offline-on-device-2') && t.includes('meanwhile-on-device-1'); }, 30000, 'after reload');
await b.page.evaluate(() => window.ogPaper.account());
await until(async () => /Signed in as alice/.test(await b.page.textContent(account)), 20000, 'still signed in');
await b.page.click(`${account} [data-act=accounts]`);
await b.page.waitForSelector('section[data-name="Accounts"] li[data-to^="vault:"]');
assert.match(await b.page.textContent('section[data-name="Accounts"]'), /✓/);
await shot(b.page, 'alice-phone-accounts');
console.log('alice phone: after a reload, still signed in with the page; Switch lists the profile');

// ---- sharing with another person on the vault ----------------------------------------------
if (process.env.BOB) {
  const bob = await device('bob', process.env.BOB);
  await signIn(bob.page, 'bob');
  const bobName = (await bob.page.textContent(`${account}`)).match(/Signed in as (bob-[0-9a-f]+)/)[1];
  const id = await a.page.evaluate(async () => {
    const r = await window.ogPaper.account();
    return document.querySelector('section[data-name="My account"] [data-list=mine] li')?.dataset.id;
  });
  await a.page.click(`${account} [data-list=mine] li[data-id="${id}"] [data-act=share]`);
  await a.page.waitForSelector('section[data-name="Page"] select[data-who] option:nth-child(2)', { state: 'attached' });
  const bobId = await a.page.evaluate(n => [...document.querySelectorAll('section[data-name="Page"] select[data-who] option')].find(o => o.textContent.trim() === n)?.value, bobName);
  assert.ok(bobId, 'bob is in the vault\'s people');
  assert.match(await a.page.textContent('section[data-name="Page"]'), /Yours/, 'alice owns it');
  await a.page.selectOption('section[data-name="Page"] select[data-who]', bobId);
  await a.page.click('section[data-name="Page"] [data-act=add]');
  await wait(1000);
  await shot(a.page, 'alice-share'); if (process.env.DEBUG) console.log('page card:', await a.page.innerHTML('section[data-name="Page"]'));
  await until(async () => {
    await bob.page.evaluate(() => window.ogPaper.account());
    await wait(1000);
    const t = await bob.page.textContent(`${account}`);
    if (process.env.DEBUG) console.log('bob panel:', t.replace(/\s+/g, ' ').slice(0, 400));
    return /Vault page/.test(await bob.page.textContent(`${account} [data-list=shared]`));
  }, 20000, 'shared with bob');
  await bob.page.click(`${account} [data-list=shared] li .go`);
  await until(async () => (await texts(bob.page)).includes('offline-on-device-2'), 30000, 'bob sees it');
  console.log('bob: the page shared with him opens with everything on it');
}

await browser.close();
console.log('vault e2e: all passed');
