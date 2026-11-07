// The launch-audit hardening in the web app, against a hosted site in
// workerd: a big photo is made to fit before it goes up, give-away is an
// offer bob accepts, a tiny full copy is undone from "Earlier versions",
// delete goes to the trash and comes back (and takes the page's copy off
// the device), an expired session keeps the pages and offline edits, signing
// out here asks about unsynced work and then takes the account's pages
// (links with keys, offline copies, the name shown) off the device, and
// another account signing in is shown under its own name.
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
const alice = await person('hana');
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
const bob = await person('hiro');
alice.answer = 'hiro';
await alice.evaluate(() => window.ogPaper.account());
await alice.waitForSelector(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await alice.click(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await alice.waitForSelector('section[data-name="Page"] [data-act=transfer]');
await alice.click('section[data-name="Page"] [data-act=transfer]');
await alice.waitForSelector('section[data-name="Page"] [data-act=withdraw]');
assert.match(await alice.textContent('section[data-name="Page"]'), /Offered to hiro/);
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
const carolShare = await api(bob, 'POST', `/pages/${id}/members`, { who: 'hope', role: 'viewer' });
assert.equal(carolShare.status, 200);
const carol = await person('hope');
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

// ---- deleting a page takes its copy off the device at once ------------------------
const local = p => p.evaluate(() => ({
  map: JSON.parse(localStorage.getItem('og-hosted-pages') || 'null'),
  owner: localStorage.getItem('og-hosted-user'),
  pages: localStorage.getItem('og-pages') || '',
  prefs: localStorage.getItem('og-prefs') || '',
  sync: JSON.parse(localStorage.getItem('og-hosted-sync') || '{}'),
}));
const hasCopy = (p, c) => p.evaluate(c => new Promise(ok => {
  const r = indexedDB.open('og-paper');
  r.onsuccess = () => { const q = r.result.transaction('canvases').objectStore('canvases').get('page:' + c); q.onsuccess = () => ok(!!q.result); };
}), c);
const canvasOf = async (p, pid) => (await local(p)).map?.[pid]?.canvas;
const newPage = async (p, name, text) => {
  const had = (await api(p, 'GET', '/pages')).body.mine.map(x => x.id);
  p.answer = name;
  await p.evaluate(() => window.ogPaper.account());
  await p.waitForSelector('section[data-name="My account"] [data-act=new]');
  await p.click('section[data-name="My account"] [data-act=new]');
  let pid;
  await until(async () => (pid = (await api(p, 'GET', '/pages')).body.mine.map(x => x.id).find(x => !had.includes(x))), 15000, 'no new page');
  await until(async () => !!(await canvasOf(p, pid)), 15000, 'new page not noted on the device');
  await wait(1500);
  await p.evaluate(t => window.ogPaper.run([{ add: 'text', x: 0, y: 0, text: t }]), text);
  await until(async () => (await p.evaluate(() => window.ogPaper.cloud())) === 'saved', 30000, `${name} not saved`);
  const c = await canvasOf(p, pid);
  await until(() => hasCopy(p, c), 15000, `no offline copy of ${name}`);
  await wait(2500);  // the copy here has the text too (autosave)
  return { pid, c };
};
const before = await local(carol);
assert.equal(Object.keys(before.map).length, 1);
assert.equal(before.owner, 'hope');
assert.match(before.prefs, /^name=hope$/m, 'others see her account name');
const canvas = Object.values(before.map)[0].canvas;
await until(() => hasCopy(carol, canvas), 15000, 'no offline copy to begin with');
const gone = await newPage(carol, 'Carol gone', 'carol-gone-text');
await carol.evaluate(() => window.ogPaper.account());
await carol.waitForSelector(`[data-list=mine] li[data-id="${gone.pid}"] [data-act=share]`);
await carol.click(`[data-list=mine] li[data-id="${gone.pid}"] [data-act=share]`);
await carol.waitForSelector('section[data-name="Page"] [data-act=delete]');
await carol.click('section[data-name="Page"] [data-act=delete]');
await until(async () => !(await local(carol)).map?.[gone.pid], 15000, 'the deleted page is still noted');
await until(async () => !(await hasCopy(carol, gone.c)), 15000, 'the deleted page\'s copy is still here');
await wait(3000);  // autosave must not bring it back
assert.equal(await hasCopy(carol, gone.c), false, 'the deleted page\'s copy stays gone');
assert.ok(!(await local(carol)).pages.includes(gone.c), 'the deleted page is not under On this device');
console.log('delete: the page\'s copy left the device with it');

// ---- an expired session keeps the pages (and offline edits) ---------------------------
const two = await newPage(carol, 'Carol two', 'carol-two-text');
await carol.context().clearCookies();
await carol.reload();
await ready(carol);
await carol.waitForSelector('section[data-name="My account"] [data-act=forget-device]', { timeout: 15000 });
await shots(carol, 'session-ended');
let now = await local(carol);
assert.equal(now.owner, 'hope', 'still known whose pages these are');
assert.equal(Object.keys(now.map || {}).length, 2, 'links kept');
assert.ok(await hasCopy(carol, canvas) && await hasCopy(carol, two.c), 'offline copies kept');
await until(async () => (await texts(carol)).includes('carol-two-text'), 20000, 'the page is not on screen any more');
await carol.evaluate(() => window.ogPaper.run([{ add: 'text', x: 0, y: 300, text: 'offline-edit' }]));
await until(async () => { const r = (await local(carol)).sync[two.c]; return r && r.n !== r.synced; }, 15000, 'the offline edit is not noted as unsynced');
await wait(2500);  // autosaved here
await carol.reload();
await ready(carol);
await carol.waitForSelector('section[data-name="My account"] [data-act=forget-device]', { timeout: 15000 });
await until(async () => (await texts(carol)).includes('offline-edit'), 20000, 'the offline edit did not survive a reload while signed out');
console.log('expired session: pages, offline copies and an offline edit all kept; sign-in prompt shown');

// ---- signing out here: asks about unsynced work, then takes everything off -------------
await carol.click('section[data-name="My account"] [data-act=forget-device]');
await carol.waitForSelector('section[data-name="Not in your account yet"]:not([hidden]) [data-act=remove-anyway]', { timeout: 15000 });
assert.match(await carol.textContent('section[data-name="Not in your account yet"]'), /Carol two/);
await shots(carol, 'signout-unsynced');
assert.equal(Object.keys((await local(carol)).map).length, 2, 'nothing removed before the answer');
await carol.click('section[data-name="Not in your account yet"] [data-act=remove-anyway]');
await until(async () => (await local(carol)).map === null, 15000, 'links with keys left on the device');
for (const c of [canvas, two.c, gone.c]) assert.equal(await hasCopy(carol, c), false, `the offline copy ${c} is gone`);
now = await local(carol);
assert.equal(now.owner, null);
assert.ok(![canvas, two.c, gone.c].some(c => now.pages.includes(c)), 'not listed under On this device');
assert.doesNotMatch(now.prefs, /^name=/m, 'the account name is not left for the next person');
let after = '';
await until(async () => { after = await texts(carol); return !/keep-me-alice|carol-two-text|offline-edit/.test(after); }, 15000, 'the pages are still on screen').catch(e => { console.log('on screen:', after); throw e; });
console.log('sign-out: asked first (unsynced edit), then links (keys), offline copies and the name removed');

// ---- another account signing in: the name others see follows -------------------------
assert.match((await local(bob)).prefs, /^name=hiro$/m);
await bob.goto(`${BASE}/dev/login?as=iris`);
await ready(bob);
await until(async () => /^name=iris$/m.test((await local(bob)).prefs), 15000, 'the name is still the last account\'s');
now = await local(bob);
assert.doesNotMatch(now.prefs, /^name=hiro$/m);
assert.equal(now.owner, 'iris');
assert.ok(!now.map || !now.map[id], 'hiro\'s page is not on the device for iris');
console.log('switching account: hiro\'s pages removed, others now see "iris"');

await browser.close();
console.log('HARDENING E2E OK');
process.exit(0);
