// The launch-audit hardening in the web app, against a hosted site in
// workerd: a big photo is made to fit before it goes up, give-away is an
// offer bob accepts, a tiny full copy is undone from "Earlier versions",
// delete goes to the trash and comes back, and signing out takes the
// account's pages (links with keys, offline copies) off the device.
// SHOTS=dir keeps phone and desktop screenshots of the new panels.
//   BASE=http://127.0.0.1:8799 PW=/path/with/node_modules/playwright node hosted/test/hardening-e2e.mjs

import assert from 'node:assert/strict';

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const BASE = process.env.BASE || 'http://127.0.0.1:8799';
const wait = ms => new Promise(r => setTimeout(r, ms));
const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });
const PHONE = { width: 390, height: 844 }, DESKTOP = { width: 1280, height: 800 };

async function shots(p, name) {
  if (!process.env.SHOTS) return;
  const was = p.viewportSize();
  for (const [k, v] of [['phone', PHONE], ['desktop', DESKTOP]]) {
    await p.setViewportSize(v);
    await wait(400);
    await p.screenshot({ path: `${process.env.SHOTS}/${name}-${k}.png` });
  }
  await p.setViewportSize(was);
}

async function person(name) {
  const ctx = await browser.newContext({ viewport: DESKTOP });
  const page = await ctx.newPage();
  page.on('pageerror', e => console.log(`[${name}] page error:`, e.message));
  page.on('console', m => { if (m.type() === 'error') console.log(`[${name}]`, m.text()); });
  page.on('dialog', d => d.accept(d.type() === 'prompt' ? (page.answer ?? 'Hardening page') : undefined));
  await page.goto(`${BASE}/dev/login?as=${name}`);
  await ready(page);
  return page;
}
const ready = p => p.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
async function texts(page) {
  await page.evaluate(() => window.ogPaper.run([{ get: 'texts' }]));
  for (let i = 0; i < 50; i++) {
    const r = await page.evaluate(() => window.ogPaper.results());
    if (r) return JSON.stringify(r);
    await wait(100);
  }
  return '';
}
async function until(f, ms = 30000, what = 'timed out') {
  const t = Date.now();
  for (;;) { if (await f()) return; if (Date.now() - t > ms) throw new Error(what); await wait(300); }
}
const api = (p, method, path, body) => p.evaluate(([method, path, body]) => fetch('/paper/api' + path, {
  method, headers: body ? { 'content-type': 'application/json' } : {}, body: body ? JSON.stringify(body) : undefined,
}).then(async r => ({ status: r.status, body: await r.json().catch(() => null) })), [method, path, body]);
const toasts = p => p.evaluate(() => [...document.querySelectorAll('.og-toast, .og-say, [role=status]')].map(e => e.textContent).join(' | '));

// ---- alice: a page, and a 3+ MB photo dropped on it --------------------------------
const alice = await person('alice');
await alice.waitForSelector('section[data-name="My account"]:not([hidden])', { timeout: 15000 });
await alice.click('section[data-name="My account"] [data-act=new]');
await until(async () => (await api(alice, 'GET', '/pages')).body.mine.length === 1);
const id = (await api(alice, 'GET', '/pages')).body.mine[0].id;
await alice.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 0, text: 'keep-me-alice' }]));
await until(async () => (await alice.evaluate(() => window.ogPaper.cloud())) === 'saved');
const photo = await alice.evaluate(async () => {
  // A noisy "photo": incompressible enough to be several MB as PNG.
  const c = document.createElement('canvas');
  c.width = 2400; c.height = 1600;
  const g = c.getContext('2d');
  const img = g.createImageData(c.width, c.height);
  let x = 12345;
  for (let i = 0; i < img.data.length; i += 4) {
    x = (x * 1103515245 + 12345) & 0x7fffffff;
    const k = (i / 4) % c.width;
    img.data[i] = (k / 10 + (x & 63)) & 255; img.data[i + 1] = ((i >> 12) + (x >> 8 & 63)) & 255; img.data[i + 2] = (x >> 16) & 255; img.data[i + 3] = 255;
  }
  g.putImageData(img, 0, 0);
  const blob = await new Promise(r => c.toBlob(r, 'image/png'));
  const dt = new DataTransfer();
  dt.items.add(new File([blob], 'photo.png', { type: 'image/png' }));
  const ev = new DragEvent('drop', { dataTransfer: dt, clientX: 600, clientY: 400, bubbles: true, cancelable: true });
  document.dispatchEvent(ev);
  return blob.size;
});
assert.ok(photo > 2 * 1024 * 1024, `the test photo is big (${photo} B)`);
await until(async () => /made smaller/.test(await alice.evaluate(() => document.body.innerText)), 30000, 'no "made smaller" note');
await until(async () => (await alice.evaluate(() => window.ogPaper.cloud())) === 'saved', 30000, 'photo not saved to the account');
const admin = await person('admin');
const stats = (await api(admin, 'GET', `/admin/pages/${id}`)).body.stats;
assert.ok(stats.bytes > 100 * 1024 && stats.bytes < 1.2 * 1024 * 1024, `the room keeps the shrunk photo (${stats.bytes} B)`);
console.log(`alice: a ${(photo / 1048576).toFixed(1)} MB photo went up as ${(stats.bytes / 1048576).toFixed(2)} MB, saved`);

// ---- give-away: an offer bob accepts ---------------------------------------------------
const bob = await person('bob');
alice.answer = 'bob';
await alice.evaluate(() => window.ogPaper.account());
await alice.waitForSelector(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await alice.click(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await alice.waitForSelector('section[data-name="Page"] [data-act=transfer]');
await alice.click('section[data-name="Page"] [data-act=transfer]');
await alice.waitForSelector('section[data-name="Page"] [data-act=withdraw]');
assert.match(await alice.textContent('section[data-name="Page"]'), /Offered to bob/);
await shots(alice, 'offer-made');
assert.deepEqual((await api(bob, 'GET', '/pages')).body.mine, [], 'nothing moved yet');
await bob.evaluate(() => window.ogPaper.account());
await bob.waitForSelector(`[data-list=offers] li[data-offer="${id}"]`);
await shots(bob, 'offer-received');
await bob.click(`[data-list=offers] li[data-offer="${id}"] [data-act=accept]`);
await until(async () => (await api(bob, 'GET', '/pages')).body.mine.some(p => p.id === id), 15000, 'bob did not get the page');
await bob.waitForSelector(`[data-list=mine] li[data-id="${id}"]`);
console.log('give-away: offered (nothing moved), bob accepted, it is his');

// ---- a tiny full copy, undone from Earlier versions ----------------------------------
await bob.click(`[data-list=mine] li[data-id="${id}"] .go`);
await until(async () => (await texts(bob)).includes('keep-me-alice'), 30000, 'bob cannot see the page');
await bob.evaluate(async id => {
  // A broken app (or the owner) replacing the page with an 8-byte copy.
  const w = new WebSocket(`${location.origin.replace(/^http/, 'ws')}/paper/api/pages/${id}/relay`);
  w.binaryType = 'arraybuffer';
  await new Promise(r => { w.onopen = r; });
  w.send(JSON.stringify({ t: 'hello', since: 0, seq: true }));
  await new Promise(r => { w.onmessage = e => { if (typeof e.data === 'string' && JSON.parse(e.data).t === 'ready') r(); }; });
  const f = new Uint8Array(12); f.set([67, 75, 80, 84]); // 'CKPT' + 8 bytes
  w.send(f);
  await new Promise(r => setTimeout(r, 800));
  w.close();
}, id);
const h = (await api(bob, 'GET', `/pages/${id}/history`)).body;
assert.equal(h.replaced.length, 1, 'the copy replaced the page');
await bob.evaluate(() => window.ogPaper.account());
await bob.waitForSelector(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await bob.click(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await bob.waitForSelector('section[data-name="Page"] [data-history] [data-act=restore]');
await shots(bob, 'earlier-versions');
await bob.click('section[data-name="Page"] [data-history] [data-act=restore]');
await until(async () => (await api(bob, 'GET', `/pages/${id}/history`)).body.replaced.length === 0, 15000, 'not restored');
// A new device gets the whole page again.
const carolShare = await api(bob, 'POST', `/pages/${id}/members`, { who: 'carol', role: 'viewer' });
assert.equal(carolShare.status, 200);
const carol = await person('carol');
await carol.waitForSelector(`[data-list=shared] li[data-id="${id}"] .go`);
await carol.click(`[data-list=shared] li[data-id="${id}"] .go`);
await until(async () => (await texts(carol)).includes('keep-me-alice'), 30000, 'carol did not get the restored page');
console.log('earlier versions: a tiny copy replaced the page; brought back; a new device gets it all');

// ---- trash ------------------------------------------------------------------------------
await bob.evaluate(() => window.ogPaper.account());
await bob.waitForSelector(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await bob.click(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await bob.waitForSelector('section[data-name="Page"] [data-act=delete]');
await bob.click('section[data-name="Page"] [data-act=delete]');
await until(async () => (await api(bob, 'GET', '/pages')).body.trash.some(p => p.id === id), 15000, 'not in the trash');
await bob.waitForSelector('section[data-name="Page"]', { state: 'hidden' });
await wait(500);
await bob.evaluate(() => window.ogPaper.account());
await bob.waitForSelector(`[data-list=trash] li[data-trash="${id}"]`);
await shots(bob, 'trash');
assert.equal((await api(carol, 'POST', `/pages/${id}/open`)).status, 404, 'gone for carol');
await bob.click(`[data-list=trash] li[data-trash="${id}"] [data-act=untrash]`);
await bob.waitForSelector(`[data-list=mine] li[data-id="${id}"]`);
assert.equal((await api(carol, 'POST', `/pages/${id}/open`)).status, 200, 'back for carol');
console.log('trash: deleted (gone for everyone), restored');

// ---- signing out takes the account's pages off the device ---------------------------
const before = await carol.evaluate(() => ({ map: Object.keys(JSON.parse(localStorage.getItem('og-hosted-pages') || '{}')).length, owner: localStorage.getItem('og-hosted-user') }));
assert.equal(before.map, 1);
assert.equal(before.owner, 'carol');
const canvas = await carol.evaluate(() => JSON.parse(localStorage.getItem('og-hosted-pages'))[Object.keys(JSON.parse(localStorage.getItem('og-hosted-pages')))[0]].canvas);
await until(async () => carol.evaluate(c => new Promise(ok => {
  const r = indexedDB.open('og-paper');
  r.onsuccess = () => { const q = r.result.transaction('canvases').objectStore('canvases').get('page:' + c); q.onsuccess = () => ok(!!q.result); };
}), canvas), 15000, 'no offline copy to begin with');
await carol.goto(`${BASE}/dev/login?as=`);
await ready(carol);
await until(async () => (await carol.evaluate(() => localStorage.getItem('og-hosted-pages'))) === null, 15000, 'links with keys left on the device');
const left = await carol.evaluate(c => new Promise(ok => {
  const r = indexedDB.open('og-paper');
  r.onsuccess = () => { const q = r.result.transaction('canvases').objectStore('canvases').get('page:' + c); q.onsuccess = () => ok(!!q.result); };
}), canvas);
assert.equal(left, false, 'the offline copy is gone');
assert.ok(!(await texts(carol)).includes('keep-me-alice'), 'the page is not on screen');
assert.ok(!(await carol.evaluate(() => localStorage.getItem('og-pages') || '')).includes(canvas), 'not listed under On this device');
console.log('sign-out: links (keys) and offline copies removed from the device');

await browser.close();
console.log('HARDENING E2E OK');
process.exit(0);
