// OG Paper UI tests: drive the real web app in headless Brave/Chrome over
// the DevTools protocol. Every test is also a demo: run with --demo and the
// same steps play at a human pace with a drawn cursor and captions, and are
// recorded to a video (out/demos/<test>-<size>.mp4).
//
// Widgets are found by name: the app publishes its accessibility tree
// (og_ui_nodes, after og_test_mode(true)), so tests say "tap Bookmarks", not
// "tap (980, 183)", and survive layout changes.

import { spawn, execFileSync } from 'child_process';
import { mkdirSync, writeFileSync, rmSync, existsSync, mkdtempSync } from 'fs';
import { tmpdir } from 'os';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import net from 'net';

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const WEB = 'http://127.0.0.1:8990';
const sleep = ms => new Promise(r => setTimeout(r, ms));

export const SIZES = {
  desktop: { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false },
  phone: { width: 412, height: 860, deviceScaleFactor: 2, mobile: true },
};
/** Demos record at sizes made for publishing: 1920 x 1200 for desktop,
 *  1080 x 1920 (Shorts, TikTok, Reels) for phone. The page is laid out at
 *  that size and the UI zoomed (`zoom`) to look like the device, so every
 *  pixel is real and the screencast stays smooth. */
export const DEMO_SIZES = {
  desktop: { width: 1920, height: 1200, deviceScaleFactor: 1, mobile: false, zoom: 1.5 },
  phone: { width: 1080, height: 1920, deviceScaleFactor: 1, mobile: true, zoom: 1080 / 412 },
};
let SIZE_SET = SIZES;
export const sizeOf = size => SIZE_SET[size];

const freePort = () => new Promise(res => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => res(p)); });
});

const children = [];
process.on('exit', () => { for (const c of children) try { c.kill(); } catch (e) {} });

/** The web app on :8990 (scripts/serve-web.py), started if not running. */
export async function ensureWeb() {
  const up = () => fetch(`${WEB}/app/`).then(r => r.ok, () => false);
  if (await up()) return;
  const c = spawn('python3', [join(ROOT, 'scripts/serve-web.py'), '8990'], { stdio: 'ignore' });
  children.push(c);
  for (let i = 0; i < 50 && !(await up()); i++) await sleep(100);
  if (!(await up())) throw new Error('could not start scripts/serve-web.py');
}

/** A page server (workspace) in a temporary folder, for workspace tests.
 *  Needs a desktop build (cargo build -p og-paper). Returns its address and
 *  view link; stopped when the run ends. */
export async function startHub() {
  const bin = join(ROOT, 'target/debug/og-paper');
  if (!existsSync(bin)) throw new Error('build the desktop app first: cargo build -p og-paper');
  const dir = mkdtempSync(join(tmpdir(), 'ogp-hub-'));
  const port = await freePort();
  const c = spawn(bin, ['--serve-dir', dir, '--port', String(port)], { stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(c);
  let log = '';
  c.stdout.on('data', d => (log += d));
  // Ready once its web pages are up (printed after the links).
  for (let i = 0; i < 100 && !/In a browser:/.test(log); i++) await sleep(50);
  const m = log.match(/view only\):\s*(ws:\/\/\S+)/);
  if (!m) throw new Error('the page server did not start');
  const view = m[1].replace(/ws:\/\/[^/]+/, `ws://127.0.0.1:${port}`);
  const http = `http://127.0.0.1:${port}`;
  // Sign in to its console (admin / password) and read a page's link.
  const pageLink = async (name = 'First page') => {
    const r = await fetch(`${http}/login`, { method: 'POST', redirect: 'manual', body: 'user=admin&password=password', headers: { 'content-type': 'application/x-www-form-urlencoded' } });
    const cookie = (r.headers.get('set-cookie') || '').split(';')[0];
    const html = await (await fetch(`${http}/console`, { headers: { cookie } })).text();
    const row = html.split('<li class="page">').find(li => li.includes(`<b>${name}</b>`));
    const m = row && row.match(/data-copy="([^"]+)"/);
    if (!m) throw new Error(`no page named ${name} on the server`);
    return m[1].replace(/&amp;/g, '&').replace(/ws:\/\/[^/]+/, `ws://127.0.0.1:${port}`);
  };
  return { port, dir, view, url: `ws://127.0.0.1:${port}`, http, pageLink, log: () => log, stop: () => c.kill() };
}

function findBrowser() {
  for (const b of [process.env.BROWSER, 'brave', 'brave-browser', 'chromium', 'chromium-browser', 'google-chrome']) {
    if (!b) continue;
    try { execFileSync('which', [b], { stdio: 'ignore' }); return b; } catch (e) {}
  }
  throw new Error('no Chromium-family browser found (set BROWSER)');
}

/** A browser with one page, sized for `size`. */
export async function launch(size, { demo = false } = {}) {
  if (demo) SIZE_SET = DEMO_SIZES;
  const port = await freePort();
  const prof = mkdtempSync(join(tmpdir(), 'ogp-ui-'));
  const s = sizeOf(size);
  const c = spawn(findBrowser(), [
    '--headless=new', '--no-sandbox', `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`,
    `--window-size=${s.width},${s.height}`, '--enable-unsafe-webgpu', '--enable-features=Vulkan',
    '--use-angle=vulkan', '--ignore-gpu-blocklist', '--hide-scrollbars', '--mute-audio', 'about:blank',
  ], { stdio: 'ignore' });
  children.push(c);
  let targets;
  for (let i = 0; i < 100; i++) {
    try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (targets.some(t => t.type === 'page')) break; } catch (e) {}
    await sleep(100);
  }
  const page = targets.find(t => t.type === 'page');
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  const b = new Browser(ws, size, async () => {
    try { ws.close(); } catch (e) {}
    const gone = new Promise(r => c.once('exit', r));
    c.kill();
    await Promise.race([gone, sleep(3000)]);
    try { rmSync(prof, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch (e) {}
  });
  await b.send('Runtime.enable');
  await b.send('Page.enable');
  const { zoom, ...metrics } = s;
  await b.send('Emulation.setDeviceMetricsOverride', metrics);
  if (s.mobile) await b.send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  return b;
}

class Browser {
  constructor(ws, size, close) {
    this.ws = ws; this.size = size; this.close = close;
    this.id = 0; this.pend = new Map(); this.onFrame = null; this.errors = [];
    ws.onmessage = e => {
      const m = JSON.parse(e.data);
      if (m.id && this.pend.has(m.id)) { const [res, rej] = this.pend.get(m.id); this.pend.delete(m.id); m.error ? rej(new Error(m.error.message)) : res(m.result); }
      if (m.method === 'Page.screencastFrame') {
        this.onFrame?.(m.params);
        this.send('Page.screencastFrameAck', { sessionId: m.params.sessionId }).catch(() => {});
      }
      if (m.method === 'Runtime.exceptionThrown') this.errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
    };
  }
  send(method, params = {}) {
    return new Promise((res, rej) => { const i = ++this.id; this.pend.set(i, [res, rej]); this.ws.send(JSON.stringify({ id: i, method, params })); });
  }
  async eval(expression) {
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error('page: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
    return r.result.value;
  }
}

class Failure extends Error {}

/** What a widget can be called by: its label, or (text boxes) its hint. */
const names = n => [n.label, n.hint, n.role !== 'TextInput' && n.role !== 'MultilineTextInput' ? n.value : null].filter(Boolean);

/** What a test gets: the app on a page, and steps that read like a script. */
export class T {
  constructor(browser, { name, demo, out }) {
    this.b = browser; this.name = name; this.demo = demo; this.out = out;
    this.size = browser.size; this.phone = sizeOf(browser.size).mobile;
    this.w = sizeOf(this.size).width; this.h = sizeOf(this.size).height;
    this.cur = [this.w * 0.62, this.h * 0.72];
    this.steps = []; this.shots = [];
  }

  log(s) { this.steps.push(s); if (process.env.UI_VERBOSE) console.log(`    ${s}`); }
  fail(msg) { throw new Failure(msg); }
  check(cond, msg) { if (!cond) this.fail(msg); this.log(`✓ ${msg}`); }
  /** Pause in a demo (so a viewer can follow); short in a test. */
  async beat(ms = 600) { await sleep(this.demo ? ms : Math.min(ms, 150)); }

  /** Open the app with nothing saved (or `prefs`, the app's settings). */
  async open(path = '/app/', { prefs = 'hints=off\n', fresh = true, hash = '' } = {}) {
    if (fresh) {
      await this.b.send('Page.navigate', { url: `${WEB}/robots-none.txt` });
      await sleep(300);
      await this.b.eval(`(async () => {
        try { localStorage.clear(); sessionStorage.clear(); } catch (e) {}
        try { for (const d of await indexedDB.databases()) indexedDB.deleteDatabase(d.name); } catch (e) {}
        try { for (const k of await caches.keys()) await caches.delete(k); } catch (e) {}
        try { for (const r of await navigator.serviceWorker.getRegistrations()) await r.unregister(); } catch (e) {}
      })()`);
      const z = sizeOf(this.size).zoom;
      if (z) prefs = (prefs || '') + `uiscale=${z}\n`;
      if (prefs != null) await this.b.eval(`localStorage.setItem('og-prefs', ${JSON.stringify(prefs)})`);
    }
    await this.b.send('Page.navigate', { url: `${WEB}${path}${hash ? '#' + hash : ''}` });
    await this.wait(async () => (await this.stateOrNull())?.ready, 'the app starts', 30000);
    await this.b.eval(`import('/app/pkg/og_paper.js').then(m => m.og_test_mode(true))`);
    await this.wait(async () => (await this.nodes()).length > 0, 'the app lists its widgets', 5000);
    const z = sizeOf(this.size).zoom;
    if (z) await this.b.eval(`(() => { const st = document.createElement('style'); st.textContent = '.og-ui { zoom: ${z}; }'; document.head.append(st); })()`);
    if (this.demo) await this.installOverlay();
    this.log(`opened ${path}`);
    // A demo opens on its title.
    if (this.demo && this.title && !this.titled) { this.titled = true; await this.caption(this.title); }
  }

  async stateOrNull() {
    try { return JSON.parse(await this.b.eval(`import('/app/pkg/og_paper.js').then(m => m.og_status())`)); } catch (e) { return null; }
  }
  /** The app's status: zoom, strokes, undos, bookmarks, net, name, ... */
  async state() { const s = await this.stateOrNull(); if (!s) this.fail('the app gave no status'); return s; }
  async nodes() {
    try { return JSON.parse(await this.b.eval(`import('/app/pkg/og_paper.js').then(m => m.og_ui_nodes())`)); } catch (e) { return []; }
  }

  /** Wait until `fn` is truthy (polling); fail with `what` if it never is. */
  async wait(fn, what, ms = 4000) {
    const t0 = Date.now();
    let last;
    while (Date.now() - t0 < ms) {
      try { last = await fn(); if (last) return last; } catch (e) {}
      await sleep(60);
    }
    this.fail(`timed out waiting: ${what}`);
  }

  /** A widget by its name (exact, or a RegExp), optionally its role. */
  async find(label, { role, ms = 4000 } = {}) {
    const ok = n => names(n).some(x => (label instanceof RegExp ? label.test(x) : x === label)) && (!role || n.role === role) && n.w > 0;
    return this.wait(async () => (await this.nodes()).find(ok), `a widget named ${label}`, ms);
  }
  async has(label, opts = {}) {
    const ok = n => names(n).some(x => (label instanceof RegExp ? label.test(x) : x === label)) && (!opts.role || n.role === opts.role);
    return (await this.nodes()).some(ok);
  }
  /** A page element (HTML cards around the canvas) by CSS selector. */
  async el(sel, ms = 4000) {
    return this.wait(() => this.b.eval(`(() => { const e = document.querySelector(${JSON.stringify(sel)});
      if (!e || e.closest('[hidden]')) return null; const r = e.getBoundingClientRect();
      return r.width ? { x: r.left, y: r.top, w: r.width, h: r.height, label: e.textContent.trim() } : null; })()`), `the element ${sel}`, ms);
  }

  // ---- input -------------------------------------------------------------
  async installOverlay() {
    const z = sizeOf(this.size).zoom || 1;
    await this.b.eval(`(() => {
      if (window.ogDemo) return;
      const d = document.createElement('div');
      d.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647;pointer-events:none;will-change:transform';
      d.innerHTML = '<svg width="${30 * z}" height="${30 * z}" viewBox="0 0 24 24"><path d="M3 2 L3 19 L7.5 14.8 L10.6 21.5 L13.6 20.2 L10.6 13.6 L16.8 13.6 Z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>';
      document.body.append(d);
      const st = document.createElement('style');
      st.textContent = '@keyframes ogrip{from{transform:translate(-50%,-50%) scale(.2);opacity:.8}to{transform:translate(-50%,-50%) scale(1);opacity:0}}' +
        '.ogrip{position:fixed;width:${52 * z}px;height:${52 * z}px;border-radius:50%;border:${3 * z}px solid #c82850;pointer-events:none;z-index:2147483646;animation:ogrip .45s ease-out forwards}' +
        '#ogcap{position:fixed;left:0;right:0;margin:0 auto;width:fit-content;bottom:${24 * z}px;zoom:${z};z-index:2147483645;pointer-events:none;font:700 20px system-ui,sans-serif;color:#fff;' +
        'background:rgba(28,28,36,.88);padding:9px 18px;border-radius:12px;opacity:0;transition:opacity .3s;max-width:90vw;text-align:center}';
      document.head.append(st);
      const cap = document.createElement('div'); cap.id = 'ogcap'; document.body.append(cap);
      window.ogDemo = {
        move: (x, y) => { d.style.transform = 'translate(' + x + 'px,' + y + 'px)'; },
        ripple: (x, y) => { const r = document.createElement('div'); r.className = 'ogrip'; r.style.left = x + 'px'; r.style.top = y + 'px'; document.body.append(r); setTimeout(() => r.remove(), 600); },
        cap: t => { if (t) { cap.textContent = t; cap.style.opacity = 1; } else cap.style.opacity = 0; },
      };
      window.ogDemo.move(${this.cur[0]}, ${this.cur[1]});
    })()`);
  }
  /** A caption in the demo video (a step name in the test log). */
  async caption(text) {
    this.log(`— ${text}`);
    if (this.demo) { await this.b.eval(`window.ogDemo?.cap(${JSON.stringify(text)})`); await sleep(900); }
  }

  async mouse(type, x, y, buttons = 0) {
    await this.b.send('Input.dispatchMouseEvent', { type, x, y, button: type === 'mouseMoved' && !buttons ? 'none' : 'left', buttons, clickCount: 1 });
  }
  async touch(type, pts) { await this.b.send('Input.dispatchTouchEvent', { type, touchPoints: pts }); }
  /** Move the (drawn) pointer to (x, y); held = dragging. */
  async glide(x, y, ms = 550, held = false) {
    const [x0, y0] = this.cur;
    const steps = this.demo ? Math.max(8, Math.round(ms / 16)) : (held ? 12 : 1);
    for (let i = 1; i <= steps; i++) {
      const t = i / steps, e = t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
      const p = [x0 + (x - x0) * e, y0 + (y - y0) * e];
      if (this.demo) await this.b.eval(`window.ogDemo?.move(${p[0]},${p[1]})`);
      if (held) { if (this.phone) await this.touch('touchMove', [{ x: p[0], y: p[1] }]); else await this.mouse('mouseMoved', p[0], p[1], 1); }
      else if (!this.phone) await this.mouse('mouseMoved', p[0], p[1], 0);
      if (this.demo) await sleep(12);
    }
    this.cur = [x, y];
  }
  /** Tap a widget (by name or node) or a point {x, y}. */
  async tap(target, opts = {}) {
    const n = typeof target === 'string' || target instanceof RegExp ? await this.find(target, opts) : target;
    const x = n.x + (n.w ?? 0) / 2, y = n.y + (n.h ?? 0) / 2;
    await this.glide(x, y);
    if (this.demo) { await sleep(100); await this.b.eval(`window.ogDemo?.ripple(${x},${y})`); }
    if (this.phone) { await this.touch('touchStart', [{ x, y }]); await sleep(50); await this.touch('touchEnd', []); }
    else { await this.mouse('mousePressed', x, y, 1); await sleep(40); await this.mouse('mouseReleased', x, y, 0); }
    this.log(`tap ${n.label || n.hint || `${Math.round(x)},${Math.round(y)}`}`);
    await this.beat(450);
  }
  /** Draw (or drag) along points [[x, y], ...]. */
  async drag(points, ms = 900) {
    const [a, ...rest] = points;
    await this.glide(a[0], a[1]);
    if (this.demo) await this.b.eval(`window.ogDemo?.ripple(${a[0]},${a[1]})`);
    if (this.phone) await this.touch('touchStart', [{ x: a[0], y: a[1] }]); else await this.mouse('mousePressed', a[0], a[1], 1);
    for (const p of rest) await this.glide(p[0], p[1], ms / rest.length, true);
    const z = points[points.length - 1];
    if (this.phone) await this.touch('touchEnd', []); else await this.mouse('mouseReleased', z[0], z[1], 0);
    this.log(`drag through ${points.length} points`);
    await this.beat(400);
  }
  /** A wavy stroke across the middle of the screen. */
  async scribble(cx, cy, w = 260) {
    const pts = [];
    for (let i = 0; i <= 16; i++) pts.push([cx - w / 2 + (w * i) / 16, cy + Math.sin(i / 2.5) * 40]);
    await this.drag(pts, 900);
  }
  async wheel(x, y, dy, n = 10) {
    await this.glide(x, y);
    for (let i = 0; i < n; i++) { await this.b.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: dy }); await sleep(this.demo ? 40 : 15); }
    this.log(`wheel ${dy > 0 ? 'out' : 'in'} ×${n}`);
    await this.beat(400);
  }
  /** Two-finger pinch about (cx, cy) from `a` to `b` px apart (phone). */
  async pinch(cx, cy, a, b, ms = 700) {
    const pts = d => [{ x: cx - d / 2, y: cy, id: 1 }, { x: cx + d / 2, y: cy, id: 2 }];
    await this.touch('touchStart', pts(a));
    const n = this.demo ? Math.round(ms / 20) : 14;
    for (let i = 1; i <= n; i++) { await this.touch('touchMove', pts(a + ((b - a) * i) / n)); await sleep(this.demo ? 20 : 8); }
    await this.touch('touchEnd', []);
    this.log(`pinch ${a}→${b}`);
    await this.beat(400);
  }
  /** Zoom in (or out, with a negative amount) the way this device does. */
  async zoom(amount = 1) {
    const [cx, cy] = [this.w * 0.55, this.h * 0.45];
    if (this.phone) {
      for (let i = 0; i < Math.abs(amount); i++) await (amount > 0 ? this.pinch(cx, cy, 60, 300) : this.pinch(cx, cy, 300, 60));
    } else await this.wheel(cx, cy, amount > 0 ? -120 : 120, 8 * Math.abs(amount));
  }
  /** Type text into whatever has the keyboard (app text box or page input). */
  async type(text) {
    for (const ch of text) {
      const vk = ch.toUpperCase().charCodeAt(0);
      await this.b.send('Input.dispatchKeyEvent', { type: 'keyDown', key: ch, text: ch, windowsVirtualKeyCode: vk });
      await this.b.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch, windowsVirtualKeyCode: vk });
      await sleep(this.demo ? 70 : 10);
    }
    this.log(`type "${text}"`);
    await this.beat(200);
  }
  async key(name, mods = 0) {
    const vk = { Enter: 13, Escape: 27, Backspace: 8, Tab: 9, Delete: 46 }[name] ?? name.toUpperCase().charCodeAt(0);
    const text = name.length === 1 && !mods ? name : undefined;
    await this.b.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: name, code: name.length === 1 ? 'Key' + name.toUpperCase() : name, windowsVirtualKeyCode: vk, modifiers: mods });
    if (text) await this.b.send('Input.dispatchKeyEvent', { type: 'char', text, key: name });
    await this.b.send('Input.dispatchKeyEvent', { type: 'keyUp', key: name, code: name.length === 1 ? 'Key' + name.toUpperCase() : name, windowsVirtualKeyCode: vk, modifiers: mods });
    this.log(`key ${mods ? 'ctrl+' : ''}${name}`);
    await this.beat(250);
  }

  /** Paste text the way a browser does (Ctrl+V into what has focus). */
  async paste(text) {
    await this.b.eval(`(() => { const dt = new DataTransfer(); dt.setData('text/plain', ${JSON.stringify(text)});
      const tgt = document.activeElement && document.activeElement !== document.body ? document.activeElement : document.querySelector('canvas');
      tgt.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })); })()`);
    this.log(`paste "${text.slice(0, 40)}…"`);
    await this.beat(400);
  }

  /** Screenshot into out/shots. */
  async shot(label) {
    const s = await this.b.send('Page.captureScreenshot', { format: 'png' });
    const f = join(this.out, 'shots', `${this.name}-${this.size}-${String(this.shots.length + 1).padStart(2, '0')}-${label}.png`);
    mkdirSync(dirname(f), { recursive: true });
    writeFileSync(f, Buffer.from(s.data, 'base64'));
    this.shots.push(f);
  }
}

/** Record a demo while `fn` runs: screencast frames at their own times,
 *  then a constant 30 fps H.264 video (needs ffmpeg). */
export async function record(b, file, fn) {
  const dir = mkdtempSync(join(tmpdir(), 'ogp-demo-'));
  const frames = [];
  const s = sizeOf(b.size);
  b.onFrame = p => {
    const f = join(dir, `f${String(frames.length).padStart(6, '0')}.jpg`);
    writeFileSync(f, Buffer.from(p.data, 'base64'));
    frames.push({ f, t: p.metadata.timestamp });
  };
  await b.send('Page.startScreencast', { format: 'jpeg', quality: 90, maxWidth: s.width, maxHeight: s.height });
  let failed = null;
  try {
    await fn();
    await sleep(1200);
  } catch (e) {
    failed = e;
  } finally {
    await b.send('Page.stopScreencast').catch(() => {});
    b.onFrame = null;
  }
  if (frames.length < 2) throw failed || new Error('no frames recorded');
  const end = frames[frames.length - 1].t + 0.5;
  let list = '';
  frames.forEach((fr, i) => {
    const d = (i + 1 < frames.length ? frames[i + 1].t : end) - fr.t;
    list += `file '${fr.f}'\nduration ${Math.max(d, 0.001).toFixed(4)}\n`;
  });
  list += `file '${frames[frames.length - 1].f}'\n`;
  writeFileSync(join(dir, 'list.txt'), list);
  mkdirSync(dirname(file), { recursive: true });
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', join(dir, 'list.txt'),
    '-vf', 'fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p', '-c:v', 'libx264', '-preset', 'medium', '-crf', '20', '-movflags', '+faststart', file]);
  rmSync(dir, { recursive: true, force: true });
  // A failed demo still leaves its video, to see what went wrong.
  if (failed) throw failed;
  return file;
}

export { Failure };
