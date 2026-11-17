// What someone drew before signing in goes into their account by itself on the
// first sign-in on that browser, and stays in the browser too. An empty canvas
// makes no page. Run by run-browser-e2e.sh.

import assert from 'node:assert/strict';


const BASE = process.env.BASE;

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const wait = ms => new Promise(r => setTimeout(r, ms));
const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });
const ready = page => page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
const pages = page => page.evaluate(() => fetch('/paper/api/pages').then(r => r.json()));
async function until(f, ms = 30000) {
  const t = Date.now();
  for (;;) { if (await f()) return; if (Date.now() - t > ms) throw new Error('timed out'); await wait(300); }
}

// Drew something, then signed in.
const ctx = await browser.newContext();
const page = await ctx.newPage();
page.on('pageerror', e => console.log('page error:', e.message));
await page.goto(`${BASE}/paper/`);
await ready(page);
await page.evaluate(() => window.ogPaper.run([{ add: 'stroke', points: [[0, 0], [100, 40]], width: 4, color: '#c82850' }]));
await wait(3000);
await page.goto(`${BASE}/dev/login?as=erin`);
await ready(page);
await until(async () => (await pages(page)).mine?.length === 1);
assert.equal((await pages(page)).mine[0].role, 'owner');
await until(async () => (await page.evaluate(() => window.ogPaper.cloud())) === 'saved');
console.log('erin: the drawing is in the account');

// Signing in again doesn't make another copy.
await page.reload();
await ready(page);
await wait(3000);
assert.equal((await pages(page)).mine.length, 1);
console.log('erin: no second copy on the next visit');

// Nothing drawn, nothing made.
const empty = await (await browser.newContext()).newPage();
await empty.goto(`${BASE}/dev/login?as=frank`);
await ready(empty);
await wait(3000);
assert.equal((await pages(empty)).mine.length, 0);
console.log('frank: an empty canvas makes no page');

await browser.close();
