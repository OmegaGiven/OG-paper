// Scenes for the App Store screenshots and previews (store-ios,
// store-preview): built with the app's own commands, scaled to the screen.

/** Run commands in the page; their results. */
export async function run(t, cmds) {
  await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
  await t.beat(200);
  return t.b.eval('window.ogPaper.results()');
}

/** The scale of a scene designed in a 440 x 900 point portrait box. */
export const scale = t => Math.min(t.w / 440, t.h / 1000);

/** A caption pill near the top (page HTML over the canvas). */
export async function caption(t, text) {
  const k = t.w / 440;
  await t.b.eval(`(() => {
    let c = document.getElementById('storecap');
    if (!c) { c = document.createElement('div'); c.id = 'storecap'; document.body.append(c); }
    c.style.cssText = 'position:fixed;left:50%;transform:translateX(-50%);z-index:2147483647;pointer-events:none;white-space:nowrap;' +
      'font:700 ${Math.round(23 * Math.min(k, 3.2))}px system-ui,-apple-system,sans-serif;color:#fff;background:rgba(28,28,36,.9);' +
      'padding:${Math.round(10 * k)}px ${Math.round(22 * k)}px;border-radius:${Math.round(18 * k)}px;top:${Math.round(t.h * 0.075)}px';
    c.textContent = ${JSON.stringify(text)};
  })()`);
}

export async function uncaption(t) {
  await t.b.eval(`document.getElementById('storecap')?.remove()`);
}

/** A symbol centred on (cx, cy): its box. */
async function symbol(t, name, cx, cy, size, color = '#24242c') {
  const r = await run(t, [{ add: 'symbol', name, cx, cy, size, color }]);
  return r[0];
}

/** A symbol with words in it, grouped (picked and moved as one). */
async function boxed(t, name, cx, cy, size, color, words, k) {
  const b = await symbol(t, name, cx, cy, size, color);
  const w = (await run(t, [label(words, 0, 0, 15 * k, color)]))[0];
  const m = await run(t, [{ move: w.id, dx: cx - (w.x + w.w / 2), dy: cy - (w.y + w.h / 2) }]);
  const g = await run(t, [{ group: [b.id, m[0].ids[0]] }]);
  return { ...b, id: g[0].ids[0] };
}

const mid = b => [b.x + b.w / 2, b.y + b.h / 2];
const left = b => [b.x, b.y + b.h / 2];
const right = b => [b.x + b.w, b.y + b.h / 2];
const top = b => [b.x + b.w / 2, b.y];
const bottom = b => [b.x + b.w / 2, b.y + b.h];

/** A wire through points (neat). */
const wire = (pts, k, color = '#24242c') => ({ add: 'shape', kind: 'path', points: pts, width: 2.2 * k, color, neat: true });
const label = (text, x, y, size, color = '#24242c') => ({ add: 'text', text, x, y, size, color, font: 'Nunito' });

/** A flowchart over a small circuit, its parts joined. Returns the boxes
 *  (for a preview to drag one). */
export async function diagram(t, { step = 0 } = {}) {
  const k = scale(t);
  const u = 10 * k;
  const out = {};
  const pause = async () => { if (step) await t.beat(step); };
  // Flowchart.
  const fy = -300 * k;
  out.start = await boxed(t, 'terminator', 0, fy, u, '#c8285a', 'Idea', k);
  await pause();
  out.sketch = await boxed(t, 'process', 0, fy + 110 * k, u, '#1e5ac8', 'Sketch it', k);
  await run(t, [{ add: 'shape', kind: 'arrow', x: 0, y: bottom(out.start)[1], w: 0, h: top(out.sketch)[1] - bottom(out.start)[1], width: 2.2 * k, neat: true }]);
  await pause();
  out.ok = await boxed(t, 'decision', 0, fy + 235 * k, u, '#1f8a4c', 'Works?', k);
  await run(t, [{ add: 'shape', kind: 'arrow', x: 0, y: bottom(out.sketch)[1], w: 0, h: top(out.ok)[1] - bottom(out.sketch)[1], width: 2.2 * k, neat: true }]);
  // "No" loops back up the left side.
  const L = out.ok.x - 34 * k;
  // One elbow arrow from the diamond's left back into the box's left: its
  // ends stay on both as they move.
  const pts = [left(out.ok), [L, left(out.ok)[1]], [L, left(out.sketch)[1]], left(out.sketch)];
  await run(t, [
    { add: 'shape', kind: 'arrow', points: pts, width: 2.2 * k, neat: true },
    label('no', L - 22 * k, left(out.ok)[1] - 22 * k, 12 * k, '#5d5d68'),
  ]);
  await pause();
  // Circuit.
  const cy = 170 * k, gap = 110 * k;
  out.res = await symbol(t, 'resistor', -95 * k, cy, u);
  out.led = await symbol(t, 'led', 95 * k, cy, u, '#c8285a');
  out.bat = await symbol(t, 'battery', -95 * k, cy + gap, u);
  out.sw = await symbol(t, 'switch', 95 * k, cy + gap, u);
  await pause();
  const xl = out.res.x - 30 * k, xr = out.led.x + out.led.w + 30 * k;
  await run(t, [
    wire([right(out.res), left(out.led)], k),
    wire([right(out.led), [xr, right(out.led)[1]], [xr, right(out.sw)[1]], right(out.sw)], k),
    wire([left(out.sw), right(out.bat)], k),
    wire([left(out.bat), [xl, left(out.bat)[1]], [xl, left(out.res)[1]], left(out.res)], k),
    label('220 Ω', out.res.x + 6 * k, out.res.y - 28 * k, 13 * k, '#5d5d68'),
    label('9 V', out.bat.x + 14 * k, out.bat.y - 26 * k, 13 * k, '#5d5d68'),
  ]);
  return out;
}

/** Gradients, pen curves and words on a path. */
export async function design(t, { step = 0 } = {}) {
  const k = scale(t);
  const pause = async () => { if (step) await t.beat(step); };
  await run(t, [
    { add: 'shape', kind: 'rect', x: -190 * k, y: -330 * k, w: 380 * k, h: 200 * k, round: true, neat: true, width: 0.1,
      color: '#c8285a', fill: '#ff8a5c', gradient: { to: '#c8285a', angle: 135 } },
    { add: 'text', text: 'Endless', x: -160 * k, y: -300 * k, size: 46 * k, color: '#ffffff', font: 'Nunito' },
    { add: 'text', text: 'ideas, one canvas', x: -158 * k, y: -215 * k, size: 17 * k, color: '#fff3ee', font: 'Nunito', letter_spacing: 0.15 },
  ]);
  await pause();
  await run(t, [
    { add: 'shape', kind: 'ellipse', x: -190 * k, y: -90 * k, w: 170 * k, h: 170 * k, neat: true, width: 0.1, color: '#1e5ac8',
      fill: '#9fd3ff', gradient: { to: '#1e5ac8', radial: true } },
    { add: 'shape', kind: 'star', x: 20 * k, y: -95 * k, w: 175 * k, h: 175 * k, neat: true, width: 3 * k, color: '#e0a100',
      fill: '#ffe066', gradient: { to: '#ff9f1c', angle: 90 } },
  ]);
  await pause();
  await run(t, [
    { add: 'shape', kind: 'path', points: [[-190 * k, 210 * k], [-100 * k, 130 * k], [0, 210 * k], [100 * k, 290 * k], [190 * k, 210 * k]],
      smooth: true, width: 9 * k, color: '#7b3fd6', neat: true },
  ]);
  await pause();
  await run(t, [
    { add: 'text', text: 'words that follow any curve', size: 15 * k, color: '#24242c', font: 'Nunito',
      path: [[-190 * k, 330 * k], [-90 * k, 270 * k], [10 * k, 320 * k], [110 * k, 370 * k], [190 * k, 310 * k]], smooth: true, align: 'center' },
  ]);
}

/** A loose hand sketch (for behind the tools). */
export async function sketch(t) {
  const k = scale(t);
  const pts = [];
  for (let i = 0; i <= 60; i++) {
    const a = i / 60 * Math.PI * 2;
    pts.push([Math.cos(a) * (120 + 18 * Math.sin(5 * a)) * k, Math.sin(a) * (120 + 18 * Math.sin(5 * a)) * k - 40 * k]);
  }
  await run(t, [
    { add: 'stroke', points: pts, width: 5 * k, color: '#c8285a' },
    { add: 'text', text: 'Bloom', x: -48 * k, y: -60 * k, size: 34 * k, color: '#24242c' },
  ]);
}
