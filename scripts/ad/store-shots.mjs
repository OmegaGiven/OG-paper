// App Store screenshots from the web app (the same app as the native
// builds), with a caption bar. Needs a local server (scripts/serve-web.py)
// and a headless Chromium-family browser on port 9334.
//   node store-shots.mjs mac|iphone OUTDIR
import { writeFileSync, mkdirSync } from 'fs';

const KIND = process.argv[2];
const OUT = process.argv[3];
mkdirSync(OUT, { recursive: true });
const SIZES = {
  mac: { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false },
  iphone: { width: 440, height: 956, deviceScaleFactor: 3, mobile: true },
};
const size = SIZES[KIND];

const targets = await (await fetch('http://127.0.0.1:9334/json')).json();
const ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
ws.onmessage = e => { const m = JSON.parse(e.data); if (m.id && pend.has(m.id)) { pend.get(m.id)(m.result); pend.delete(m.id); } };
await new Promise(r => (ws.onopen = r));
const send = (method, params = {}) => new Promise(r => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = ms => new Promise(r => setTimeout(r, ms));
const ev = expression => send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }).then(r => r?.result?.value);

await send('Emulation.setDeviceMetricsOverride', size);
if (size.mobile) await send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });

const key = async (k, code, mods = 0) => {
  const vk = k.length === 1 ? k.toUpperCase().charCodeAt(0) : 0;
  if (mods) await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Control', code: 'ControlLeft', modifiers: 2, windowsVirtualKeyCode: 17 });
  await send('Input.dispatchKeyEvent', { type: mods ? 'rawKeyDown' : 'keyDown', key: k, code, modifiers: mods, windowsVirtualKeyCode: vk, text: mods ? undefined : k });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, modifiers: mods, windowsVirtualKeyCode: vk });
  if (mods) await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Control', code: 'ControlLeft', windowsVirtualKeyCode: 17 });
  await sleep(250);
};
const tap = async (x, y) => {
  if (size.mobile) {
    await send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
    await sleep(50);
    await send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  } else {
    await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 });
  }
  await sleep(400);
};
const wheel = async (x, y, dy, n) => {
  await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  for (let i = 0; i < n; i++) { await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: dy }); await sleep(30); }
  await sleep(400);
};
// Two-finger pinch about (x, y), from `a` to `b` px apart.
const pinch = async (x, y, a, b) => {
  const pts = d => [{ x: x - d / 2, y, id: 1 }, { x: x + d / 2, y, id: 2 }];
  await send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: pts(a) });
  for (let i = 1; i <= 12; i++) {
    await send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: pts(a + (b - a) * i / 12) });
    await sleep(20);
  }
  await send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await sleep(500);
};
// Fly to a try-mode bookmark by name (the bookmarks card is page HTML).
const bookmark = async name => {
  await ev(`(() => { const b = [...document.querySelectorAll('button,li,a')].find(e => e.textContent.trim().startsWith(${JSON.stringify(name)}) && e.closest('.og-card')); return !!b; })()`);
  await ev(`window.__ogOpenBookmarks && window.__ogOpenBookmarks()`);
};

async function fresh(prefs) {
  await send('Page.navigate', { url: 'http://127.0.0.1:8990/try/' });
  await sleep(2500);
  await ev(`localStorage.clear(); localStorage.setItem("og-prefs", ${JSON.stringify(prefs)}); indexedDB.deleteDatabase("og-paper"); location.reload()`);
  await sleep(7000);
  // The tour card and toasts out of the way.
  await ev(`document.querySelectorAll('.og-card').forEach(c => c.hidden = true)`);
}
async function caption(text, short) {
  if (KIND !== 'mac' && short) text = short;
  await ev(`(() => {
    let c = document.getElementById('shotcap');
    if (!c) { c = document.createElement('div'); c.id = 'shotcap'; document.body.append(c); }
    c.style.cssText = 'position:fixed;left:50%;transform:translateX(-50%);z-index:2147483647;pointer-events:none;white-space:nowrap;' +
      'font:700 ${KIND === 'mac' ? 30 : 22}px system-ui,-apple-system,sans-serif;color:#fff;background:rgba(28,28,36,.88);' +
      'padding:${KIND === 'mac' ? '12px 26px' : '9px 18px'};border-radius:16px;letter-spacing:.2px;top:${KIND === 'mac' ? '22px' : '96px'}';
    c.textContent = ${JSON.stringify(text)};
  })()`);
  await sleep(300);
}
let n = 0;
async function shot(name) {
  await sleep(600);
  const r = await send('Page.captureScreenshot', { format: 'png' });
  const f = `${OUT}/${String(++n).padStart(2, '0')}-${name}.png`;
  writeFileSync(f, Buffer.from(r.data, 'base64'));
  console.log(f);
}
async function flyTo(name) {
  // Gear > Bookmarks via its hotkey, then the bookmark's button.
  await key('b', 'KeyB', 2);
  await sleep(600);
  const ok = await ev(`(() => { const b = [...document.querySelectorAll('.og-card:not([hidden]) button')].find(e => e.textContent.trim().startsWith(${JSON.stringify(name)})); if (!b) return false; b.click(); return true; })()`);
  await sleep(6500);
  await ev(`document.querySelectorAll('.og-card').forEach(c => c.hidden = true)`);
  return ok;
}
const W = size.width, H = size.height;

// 1. The endless street.
await fresh('radialbar=off\nhints=off\n');
await key('h', 'KeyH');
console.log('street', await flyTo('Endless street'));
if (KIND !== 'mac') { await pinch(W / 2, H / 2, 80, 200); await pinch(W / 2, H / 2, 100, 150); }
await caption('Zoom into the street. It never ends.', 'A street that never ends');
await shot('street');

// 2. Home: the canvas with tools.
await fresh('radialbar=off\nhints=off\n');
await key('h', 'KeyH');
await caption('Sketch, write and paste anything', 'Sketch, write, paste');
await shot('home');

// 3. Deep: a world a trillion trillion times smaller.
await key('h', 'KeyH');
console.log('deep', await flyTo('Zoom 1024'));
await caption('Zoom 10²⁴ times deeper. Still sharp.', '10²⁴ times deeper. Sharp.');
await shot('deep');

// 4. Toolbars and inventory.
await fresh('radialbar=off\nhints=off\n');
await key('h', 'KeyH');
if (KIND === 'mac') await tap(W / 2 - 200, H - 40);
else await tap(36, 290);
await caption('Keep your tools, exactly as you set them', 'Your tools, kept');
await shot('tools');

// 5. Settings in sections.
await fresh('radialbar=off\nhints=off\n');
await key('h', 'KeyH');
await tap(W - 40, 32);
await caption('Pages, sharing, plugins and more', 'Pages, sharing, plugins');
await shot('settings');

ws.close();
process.exit(0);
