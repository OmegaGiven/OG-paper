// Records the OG Paper "endless street" ad: headless Brave over CDP,
// screencast frames with timestamps, a drawn cursor for every action.
// node rec.mjs OUTDIR [checkpoints]
import { writeFileSync, mkdirSync } from 'fs';

const OUT = process.argv[2];
const CHECK = process.argv[3] === 'check';
mkdirSync(`${OUT}/frames`, { recursive: true });
// A phone in portrait: 540 x 960 CSS px at 2x, so 1080 x 1920 video.
const CW = 540, CH = 960, DPR = 2;
const W = CW * DPR, H = CH * DPR;

const targets = await (await fetch('http://127.0.0.1:9334/json')).json();
const ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
const frames = [];
let recording = false;
ws.onmessage = e => {
  const m = JSON.parse(e.data);
  if (m.id && pend.has(m.id)) { pend.get(m.id)(m.result); pend.delete(m.id); }
  if (m.method === 'Page.screencastFrame') {
    const { data, metadata, sessionId } = m.params;
    if (recording) {
      const n = frames.length;
      const f = `${OUT}/frames/f${String(n).padStart(5, '0')}.jpg`;
      writeFileSync(f, Buffer.from(data, 'base64'));
      frames.push({ f, t: metadata.timestamp });
    }
    ws.send(JSON.stringify({ id: ++id, method: 'Page.screencastFrameAck', params: { sessionId } }));
  }
  if (m.method === 'Runtime.exceptionThrown') console.log('exception', m.params.exceptionDetails.exception?.description);
};
await new Promise(r => (ws.onopen = r));
const send = (method, params = {}) =>
  new Promise(r => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const sleep = ms => new Promise(r => setTimeout(r, ms));
const ev = expression => send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
let shotN = 0;
const T0 = Date.now();
const shot = async name => {
  console.log(name, ((Date.now() - T0) / 1000).toFixed(1));
  if (!CHECK) return;
  const s = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(`${OUT}/check-${String(++shotN).padStart(2, '0')}-${name}.png`, Buffer.from(s.data, 'base64'));
};

await send('Runtime.enable');
await send('Emulation.setDeviceMetricsOverride', { width: CW, height: CH, deviceScaleFactor: DPR, mobile: false });
await send('Page.navigate', { url: 'http://127.0.0.1:8990/app/' });
await sleep(3000);
await ev('localStorage.setItem("og-prefs","radialbar=off\\nhints=off\\n");location.reload()');
await sleep(7000);

// ---- the cursor: drawn over the page, follows every pointer action ----
await ev(`(() => {
  const d = document.createElement('div');
  d.id = 'adcursor';
  d.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647;pointer-events:none;will-change:transform';
  const arrow = '<svg width="34" height="34" viewBox="0 0 24 24"><path d="M3 2 L3 19 L7.5 14.8 L10.6 21.5 L13.6 20.2 L10.6 13.6 L16.8 13.6 Z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>';
  const pen = '<svg width="34" height="34" viewBox="0 0 24 24" style="transform:translate(0,-30px)"><path d="M2 22 L4 15 L16 3 L21 8 L9 20 Z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/><path d="M4 15 L9 20" stroke="#111" stroke-width="1.4"/><path d="M2 22 L3.2 18" stroke="#c82850" stroke-width="2.2"/></svg>';
  d.innerHTML = '<div id="adarrow">' + arrow + '</div><div id="adpen" style="display:none">' + pen + '</div>' +
    '<div id="adwheel" style="display:none;position:absolute;left:30px;top:30px;width:22px;height:34px;border:2.5px solid #111;border-radius:12px;background:#fff">' +
    '<div id="adwheeldot" style="position:absolute;left:7px;top:6px;width:4px;height:9px;border-radius:2px;background:#c82850"></div></div>';
  document.body.append(d);
  const style = document.createElement('style');
  style.textContent = '@keyframes adripple{from{transform:translate(-50%,-50%) scale(.2);opacity:.8}to{transform:translate(-50%,-50%) scale(1);opacity:0}}' +
    '.adripple{position:fixed;width:56px;height:56px;border-radius:50%;border:3px solid #c82850;pointer-events:none;z-index:2147483646;animation:adripple .45s ease-out forwards}';
  document.head.append(style);
  const cap = document.createElement('div');
  cap.id = 'adcap';
  cap.style.cssText = 'position:fixed;left:50%;top:84px;transform:translateX(-50%);z-index:2147483645;pointer-events:none;' +
    'font:700 26px system-ui,sans-serif;color:#fff;background:rgba(28,28,36,.86);padding:10px 20px;border-radius:14px;' +
    'opacity:0;transition:opacity .35s;white-space:nowrap;letter-spacing:.2px';
  document.body.append(cap);
  window.adCap = t => { if (t) { cap.textContent = t; cap.style.opacity = 1; } else cap.style.opacity = 0; };
  window.adMove = (x, y) => { d.style.transform = 'translate(' + x + 'px,' + y + 'px)'; };
  window.adMode = m => {
    document.getElementById('adarrow').style.display = m === 'pen' ? 'none' : '';
    document.getElementById('adpen').style.display = m === 'pen' ? '' : 'none';
    document.getElementById('adwheel').style.display = m === 'wheel' ? '' : 'none';
  };
  window.adWheel = dir => { document.getElementById('adwheeldot').style.top = (dir < 0 ? 4 : 14) + 'px'; };
  window.adRipple = (x, y) => {
    const r = document.createElement('div'); r.className = 'adripple';
    r.style.left = x + 'px'; r.style.top = y + 'px';
    document.body.append(r); setTimeout(() => r.remove(), 600);
  };
})()`);

let cur = [400, 760];
const REST = [400, 760];
const mouse = (type, x, y, b = 0) =>
  send('Input.dispatchMouseEvent', { type, x, y, button: type === 'mouseMoved' && !b ? 'none' : 'left', buttons: b, clickCount: 1 });
const ease = t => (t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2);
// Glide the cursor (and the real pointer) to (x, y), taking `ms`.
async function glide(x, y, ms = 600, held = false) {
  const [x0, y0] = cur;
  const t0 = Date.now();
  for (;;) {
    const t = Math.min(1, (Date.now() - t0) / ms);
    const e = ease(t);
    const p = [x0 + (x - x0) * e, y0 + (y - y0) * e];
    await ev(`adMove(${p[0]},${p[1]})`);
    await mouse('mouseMoved', p[0], p[1], held ? 1 : 0);
    if (t >= 1) break;
    await sleep(8);
  }
  cur = [x, y];
}
async function click(x, y, ms = 600) {
  await glide(x, y, ms);
  await sleep(120);
  await ev(`adRipple(${x},${y})`);
  await mouse('mousePressed', x, y, 1);
  await sleep(70);
  await mouse('mouseReleased', x, y, 0);
  await sleep(250);
}
async function drag(a, b, ms = 900) {
  await glide(a[0], a[1], 600);
  await sleep(120);
  await ev(`adRipple(${a[0]},${a[1]})`);
  await mouse('mousePressed', a[0], a[1], 1);
  await sleep(60);
  await glide(b[0], b[1], ms, true);
  await sleep(60);
  await mouse('mouseReleased', b[0], b[1], 0);
  await sleep(250);
}
async function wheel(total, ms, dir) {
  // Smooth, eased in and out, exactly `total` over `ms`.
  await ev(`adMode('wheel');adWheel(${dir})`);
  const F = t => (1 - Math.cos(Math.PI * t)) / 2;
  const t0 = Date.now();
  let prev = 0, k = 0;
  for (;;) {
    const t = Math.min(1, (Date.now() - t0) / ms);
    const d = total * (F(t) - F(prev));
    prev = t;
    if (d) await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: cur[0], y: cur[1], deltaX: 0, deltaY: d });
    if (++k % 4 === 0) await ev(`adWheel(${k % 8 === 0 ? dir : -dir})`);
    if (t >= 1) break;
    await sleep(12);
  }
  await ev(`adMode('arrow')`);
}

// ---- the street (same geometry as the try-mode demo, screen-sized) ----
const K = 0.25;
const at = (a, b, t) => [a * t * W * 0.5, b * t * H * 0.5];
const C = { INK: '#1c1c24', MUTED: '#6e6e7d', ACCENT: '#c8285a', BLUE: '#1e5ac8', GREEN: '#1e965a', ORANGE: '#eb7814', PURPLE: '#783cbe' };
const wpx = t => 0.0035 * t * H;
const ground = 0.45;
// Each element: commands, and the path the pen follows while drawing it.
const elements = [];
const rayEl = (a, b, t0, t1, width, color) => {
  const cmds = [];
  let t = t1;
  while (t > t0) {
    const u = Math.max(t / 1.25, t0);
    cmds.push({ add: 'stroke', points: [at(a, b, t), at(a, b, u)], width: width * H * Math.sqrt(t * u), color });
    t = u;
  }
  elements.push({ cmds, path: [at(a, b, Math.min(t1, 2.1)), at(a, b, t0)] });
};
// Rays reach just past the frame (t = 1 / max(|a|, |b|) there): all the
// portal shows.
const T_OUT = 2.25;
for (const a of [-0.28, 0.28]) rayEl(a, ground, K, T_OUT, 0.0035, C.MUTED);
for (const a of [-0.62, 0.62]) rayEl(a, ground, K, 1.65, 0.004, C.INK);
{
  const cmds = [];
  const d = 6;
  for (let i = 2 * d - 1; i >= -d; i--) {
    const t0 = K ** ((i / d) * 0.5), t1 = K ** (((i + 0.45) / d) * 0.5);
    const lo = Math.min(t0, t1), hi = Math.max(t0, t1);
    if (hi > K && lo < T_OUT) cmds.push({ add: 'stroke', points: [at(0, ground, lo), at(0, ground, hi)], width: wpx(hi) * 1.3, color: C.ORANGE });
  }
  cmds.reverse();
  elements.push({ cmds, path: [at(0, ground, 2.1), at(0, ground, K)] });
}
const n = 4;
const sides = [
  [-0.62, [-0.55, -0.85, -0.4, -0.7], [C.BLUE, C.ACCENT, C.GREEN, C.PURPLE]],
  [0.62, [-0.75, -0.45, -0.9, -0.6], [C.ORANGE, C.BLUE, C.ACCENT, C.GREEN]],
];
const buildings = [];
for (const [a, heights, colors] of sides) {
  for (let i = -n; i < n; i++) {
    const j = ((i % n) + n) % n;
    const hi = K ** (i / n), lo = K ** ((i + 1) / n);
    if (lo >= 1.65 || hi <= K * 0.999) continue;
    const roof = heights[j], col = colors[j];
    const gap = (hi / lo) ** 0.06;
    const front = hi / gap, back = lo * gap;
    const cmds = [
      { add: 'stroke', points: [at(a, ground, front), at(a, roof, front), at(a, roof, back), at(a, ground, back)], width: wpx(front), color: col },
    ];
    const rows = Math.floor((ground - roof) / 0.14);
    for (let r = 0; r < Math.max(rows, 1) - 1; r++) {
      const b0 = roof + 0.06 + r * 0.14, b1 = b0 + 0.07;
      for (let c = 0; c < 2; c++) {
        const f0 = 0.18 + c * 0.42, f1 = f0 + 0.24;
        const t0 = front * (back / front) ** f0, t1 = front * (back / front) ** f1;
        cmds.push({ add: 'stroke', points: [at(a, b0, t0), at(a, b0, t1), at(a, b1, t1), at(a, b1, t0), at(a, b0, t0)], width: wpx(t0) * 0.6, color: col });
      }
    }
    const t0 = front * (back / front) ** 0.4, t1 = front * (back / front) ** 0.6;
    cmds.push({ add: 'stroke', points: [at(a, ground, t0), at(a, ground - 0.12, t0), at(a, ground - 0.12, t1), at(a, ground, t1)], width: wpx(t0) * 0.7, color: C.INK });
    buildings.push({ i, side: a, cmds, path: [at(a, ground, front), at(a, roof, front), at(a, roof, back), at(a, ground, back)] });
  }
}
// Near to far, alternating sides.
buildings.sort((p, q) => p.i - q.i || p.side - q.side);
elements.push(...buildings);
const toScreen = p => [(p[0] + W / 2) / DPR, (p[1] + H / 2) / DPR];
const onScreen = p => [Math.min(CW - 20, Math.max(60, p[0])), Math.min(CH - 20, Math.max(20, p[1]))];

// ---- record ----
// Start with the hand (no tool panel), like the end.
const mouseClick = async (x, y) => { await mouse('mouseMoved', x, y); await sleep(80); await mouse('mousePressed', x, y, 1); await mouse('mouseReleased', x, y, 0); await sleep(400); };
await mouseClick(508, 927);
await mouseClick(507, 684);
await mouse('mouseMoved', cur[0], cur[1]);
await ev(`adMove(${cur[0]},${cur[1]})`);
await sleep(500);
await send('Page.startScreencast', { format: 'jpeg', quality: 92, everyNthFrame: 1 });
recording = true;
await sleep(1200);
await shot('start');

// 1. Draw the street.
await ev(`adCap('Draw a street')`);
await ev(`adMode('pen')`);
for (const e of elements) {
  const path = e.path.map(toScreen).map(onScreen);
  const steps = Math.min(Math.max(e.cmds.length, 4), 7);
  const per = Math.ceil(e.cmds.length / steps);
  let k = 0;
  // Move to the start, then along the path while the strokes appear.
  const [sx, sy] = path[0];
  await glide(sx, sy, 110);
  for (let s = 0; s < steps; s++) {
    const t = (s + 1) / steps;
    const seg = t * (path.length - 1);
    const a = path[Math.min(Math.floor(seg), path.length - 2)], b = path[Math.min(Math.floor(seg) + 1, path.length - 1)];
    const f = seg - Math.floor(seg) || (t >= 1 ? 1 : 0);
    const p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
    await ev(`adMove(${p[0]},${p[1]})`);
    cur = p;
    const batch = e.cmds.slice(k, k + per);
    k += per;
    if (batch.length) await ev(`ogPaper.run(${JSON.stringify(batch)})`);
    await sleep(26);
  }
}
await ev(`adMode('arrow')`);
await glide(REST[0], REST[1], 500);
await sleep(700);
await shot('street');

// 2. The portal: Portal tool, show this view, rectangle, no outline, draw it.
await ev(`adCap('Add a portal at the end')`);
await click(508, 927, 700);
await sleep(350);
await shot('fan');
await click(264, 928, 500);
await sleep(300);
await click(33, 927, 600);
await sleep(400);
await shot('portal-panel');
await click(59, 804, 600);
await sleep(300);
await click(113, 847, 400);
await sleep(200);
await click(29, 921, 400);
await sleep(300);
await shot('portal-set');
await drag([CW / 2 - CW * K / 2, CH / 2 - CH * K / 2], [CW / 2 + CW * K / 2, CH / 2 + CH * K / 2], 1100);
await sleep(700);
await shot('portal-drawn');
// Back to the hand.
await click(508, 927, 700);
await sleep(300);
await click(507, 684, 500);
await sleep(500);

// 3. Zoom down the street, through the portal again and again.
await ev(`adCap('Zoom in forever')`);
await glide(CW / 2, CH / 2, 700);
await sleep(500);
await wheel(-8200, 10000, -1);
await sleep(500);
await shot('zoomed');

// 4. Zoom out to see it all.
await wheel(900, 1800, 1);
await sleep(500);
await shot('out');

// 5. Select everything and delete it.
await ev(`adCap('Start over')`);
await click(508, 927, 700);
await sleep(300);
await click(392, 811, 500);
await sleep(300);
await drag([70, 40], [CW - 20, CH - 70], 900);
await sleep(600);
await shot('selected');
await click(124, 880, 700);
await sleep(600);
await shot('deleted');
// 6. Home, the hand, and the cursor back where it started.
await click(508, 32, 700);
await sleep(300);
await click(328, 498, 500);
await sleep(500);
await shot('bookmarks');
await click(207, 260, 500);
await sleep(400);
if ((await ev(`!!document.querySelector('.og-card:not([hidden]) h2, .og-card.open')`)).result?.value) await click(500, 122, 400);
await sleep(1600);
await shot('home');
await click(508, 927, 700);
await sleep(300);
await click(507, 684, 500);
await sleep(300);
await ev(`adCap('')`);
await glide(REST[0], REST[1], 800);
// Let messages fade, then hold on the same frame the video began with.
await sleep(3000);
await shot('end');
recording = false;
await send('Page.stopScreencast');
await send('Browser.close').catch(() => {});

// ---- assemble: frames at their own times, then constant 30 fps ----
const end = frames[frames.length - 1].t + 0.5;
let list = '';
for (let i = 0; i < frames.length; i++) {
  const d = (i + 1 < frames.length ? frames[i + 1].t : end) - frames[i].t;
  list += `file '${frames[i].f}'\nduration ${Math.max(d, 0.001).toFixed(4)}\n`;
}
list += `file '${frames[frames.length - 1].f}'\n`;
writeFileSync(`${OUT}/list.txt`, list);
console.log('frames', frames.length, 'seconds', (end - frames[0].t).toFixed(1));
process.exit(0);
