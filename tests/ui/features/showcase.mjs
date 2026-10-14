// The showcase demo: one page built live through the real app — a painting,
// a circuit with connectors that follow its parts, a mechanical sketch,
// business notes, a timeline made by the commands an AI assistant uses —
// then the camera pulls back to show it all on one endless page.
//
//   node tests/ui/run.mjs showcase --demo     → out/demos/showcase-desktop.mp4
//
// Laid out for the 1920 x 1200 demo size; at test size everything scales
// down with the window (`k`), so `node tests/ui/run.mjs showcase` checks it
// quickly.

const sleep = ms => new Promise(r => setTimeout(r, ms));

// The color dial's presets (see ui.rs BASIC and EXTRA).
const C = {
  black: '#1c1c24', red: '#d2283c', orange: '#f08214', yellow: '#fad21e', green: '#148c50',
  blue: '#1e5ac8', purple: '#823cbe', white: '#fafafa', grey: '#5a5a64', lgrey: '#a0a0a8',
  brown: '#784628', pink: '#dc3c96', rose: '#ff96aa', peach: '#ffc88c', lime: '#aac83c',
  teal: '#00a0aa', sky: '#78c8ff', navy: '#14286e', lav: '#bea0f0', maroon: '#6e1428',
};

/** A small seeded random source, so the "hand" wobbles the same each run. */
function rng(seed) {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}
const R = rng(42);
const wob = (a = 1) => (R() - 0.5) * 2 * a;

/** Points along a polyline, every `step`, nudged like a hand would. */
function hand(pts, step = 14, wobble = 1.5) {
  const out = [];
  for (let i = 0; i + 1 < pts.length; i++) {
    const [a, b] = [pts[i], pts[i + 1]];
    const n = Math.max(1, Math.round(Math.hypot(b[0] - a[0], b[1] - a[1]) / step));
    for (let j = 0; j < n; j++) {
      const t = j / n;
      out.push([a[0] + (b[0] - a[0]) * t + wob(wobble), a[1] + (b[1] - a[1]) * t + wob(wobble)]);
    }
  }
  out.push(pts[pts.length - 1]);
  return out;
}
/** A wavy horizontal pass from x0 to x1 around y. */
const wave = (x0, x1, y, amp = 12, f = 0.012, ph = 0) => {
  const pts = [];
  for (let x = x0; x <= x1; x += 18) pts.push([x, y + Math.sin(x * f + ph) * amp + wob(2)]);
  return pts;
};
/** A spiral (sun washes, scribbled foliage). */
const spiral = (cx, cy, r0, r1, turns = 3, step = 0.18) => {
  const pts = [];
  const n = Math.round((turns * Math.PI * 2) / step);
  for (let i = 0; i <= n; i++) {
    const a = i * step, r = r0 + ((r1 - r0) * i) / n;
    pts.push([cx + Math.cos(a) * r + wob(2), cy + Math.sin(a) * r + wob(2)]);
  }
  return pts;
};
const circle = (cx, cy, r, from = 0, to = Math.PI * 2, n = 48) =>
  Array.from({ length: n + 1 }, (_, i) => {
    const a = from + ((to - from) * i) / n;
    return [cx + Math.cos(a) * r, cy + Math.sin(a) * r];
  });
/** A gear's outline: `teeth` teeth between radius r0 and r1. */
function gear(cx, cy, r0, r1, teeth) {
  const pts = [];
  for (let i = 0; i < teeth; i++) {
    const a = (i / teeth) * Math.PI * 2, w = (Math.PI * 2) / teeth;
    for (const [f, r] of [[0, r0], [0.18, r1], [0.5, r1], [0.68, r0], [1, r0]]) {
      pts.push([cx + Math.cos(a + w * f) * r, cy + Math.sin(a + w * f) * r]);
    }
  }
  return hand(pts, 10, 0.8);
}

export default {
  name: 'showcase',
  features: [],
  sizes: ['desktop'],
  demoOnly: true,
  title: 'OG Paper — one endless page',
  async run(t) {
    await t.open();
    // Design space: 1920 x 1200 points at the home view; `k` fits it to
    // the window. `cam` is how far the view has been panned (design units).
    const k = t.w / 1920;
    const cam = [0, 0];
    const S = (x, y) => [t.w / 2 + (x - cam[0]) * k, t.h / 2 + (y - cam[1]) * k];
    const SS = pts => pts.map(p => S(p[0], p[1]));
    const run = async cmds => {
      const scaled = JSON.parse(JSON.stringify(cmds), (key, v) =>
        typeof v === 'number' && ['x', 'y', 'w', 'h', 'size', 'width'].includes(key) ? v * k : v);
      for (const c of scaled) if (c.points) c.points = c.points.map(p => [p[0] * k, p[1] * k]);
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(scaled)})`);
      await sleep(120);
      return t.b.eval('window.ogPaper.results()');
    };
    const pause = ms => t.beat(ms);

    // ---- input helpers -------------------------------------------------
    /** Draw through screen points in `ms` (a hand's pace, not a robot's). */
    const ink = async (pts, ms = 900) => {
      const [a, ...rest] = pts;
      await t.glide(a[0], a[1], t.demo ? 220 : 1);
      await t.mouse('mousePressed', a[0], a[1], 1);
      const dt = rest.length ? ms / rest.length : 0;
      for (const p of rest) {
        await t.mouse('mouseMoved', p[0], p[1], 1);
        if (t.demo) { await t.b.eval(`window.ogDemo?.move(${p[0]},${p[1]})`); await sleep(dt); }
      }
      const z = pts[pts.length - 1];
      await t.mouse('mouseReleased', z[0], z[1], 0);
      t.cur = [z[0], z[1]];
      await sleep(t.demo ? 60 : 30);
    };
    const draw = (pts, ms) => ink(SS(pts), ms);
    /** Pan the view by (dx, dy) design units with right-drags. */
    const pan = async (dx, dy, ms = 1400) => {
      const chunks = Math.max(1, Math.ceil(Math.max(Math.abs(dx), Math.abs(dy)) * k / (t.h * 0.6)));
      for (let c = 0; c < chunks; c++) {
        const [x0, y0] = [t.w / 2 + (dx * k) / chunks / 2, t.h / 2 + (dy * k) / chunks / 2];
        const [x1, y1] = [x0 - (dx * k) / chunks, y0 - (dy * k) / chunks];
        await t.glide(x0, y0, t.demo ? 200 : 1);
        await t.b.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: x0, y: y0, button: 'right', buttons: 2, clickCount: 1 });
        const n = 30;
        for (let i = 1; i <= n; i++) {
          const e = i / n, s = e < 0.5 ? 2 * e * e : 1 - (-2 * e + 2) ** 2 / 2;
          const p = [x0 + (x1 - x0) * s, y0 + (y1 - y0) * s];
          await t.b.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: p[0], y: p[1], button: 'right', buttons: 2 });
          if (t.demo) { await t.b.eval(`window.ogDemo?.move(${p[0]},${p[1]})`); await sleep(ms / chunks / n); }
        }
        await t.b.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: x1, y: y1, button: 'right', buttons: 0, clickCount: 1 });
        t.cur = [x1, y1];
      }
      cam[0] += dx;
      cam[1] += dy;
      await pause(300);
    };
    const enter = async () => {
      await t.b.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
      await t.b.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
    };
    /** Type like a person (punctuation and symbols included). */
    const type = async text => {
      for (const ch of text) {
        const vk = /[a-z0-9]/i.test(ch) ? ch.toUpperCase().charCodeAt(0) : ch === ' ' ? 32 : 0;
        await t.b.send('Input.dispatchKeyEvent', { type: 'keyDown', key: ch, text: ch, windowsVirtualKeyCode: vk });
        await t.b.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch, windowsVirtualKeyCode: vk });
        await sleep(t.demo ? 38 : 4);
      }
      await t.beat(150);
    };
    /** Type lines into a new text at (x, y); finished by picking `then`. */
    const write = async (x, y, lines, then = 'Slot 6') => {
      await t.tap('Slot 9');
      await t.tap({ x: S(x, y)[0], y: S(x, y)[1] });
      for (let i = 0; i < lines.length; i++) {
        if (i) await enter();
        await type(lines[i]);
      }
      await t.tap(then);
    };
    /** The settings panel, open. */
    const panel = async () => {
      if (!(await t.has('Hide settings'))) {
        const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
        if (show) await t.tap(show);
      }
    };
    const hidePanel = async () => { if (await t.has('Hide settings')) await t.tap('Hide settings'); };
    /** The width slider dragged to `want` px: it's logarithmic, so feel
     *  for it along its rail, reading the value it shows. */
    const setWidth = async want => {
      const spinNode = async () => (await t.nodes()).find(n => n.w > 0 && n.role === 'SpinButton' && / px$/.test(n.value || ''));
      const s0 = await spinNode();
      if (!s0) return;
      const z = t.demo || t.w > 1500 ? 1.5 : 1;
      const y = s0.y + s0.h / 2;
      let hi = s0.x - 8 * z, lo = hi - 128 * z;
      await t.glide((lo + hi) / 2, y, t.demo ? 250 : 1);
      for (let i = 0; i < 8; i++) {
        const x = (lo + hi) / 2;
        await t.mouse('mousePressed', x - 2, y, 1);
        await t.mouse('mouseMoved', x, y, 1);
        await t.mouse('mouseReleased', x, y, 0);
        if (t.demo) await t.b.eval(`window.ogDemo?.move(${x},${y})`);
        await sleep(t.demo ? 160 : 140);
        const v = parseFloat((await spinNode()).value);
        if (Math.abs(v - want) / want < 0.06) break;
        if (v < want) lo = x; else hi = x;
      }
      t.cur = [(lo + hi) / 2, y];
    };
    /** A brush: look (Advanced), color, width (px). */
    const brush = async ({ look, color, width }) => {
      await t.tap('Slot 1');
      await panel();
      if (look) {
        if (await t.has('Advanced')) await t.tap('Advanced');
        await t.tap('Look', { role: 'Button' });
        await t.tap(look);
      }
      if (color) {
        await t.tap('Color', { role: 'Button' });
        await t.tap(`Color ${color}`);
      }
      if (width) await setWidth(width * k);
      await hidePanel();
    };
    /** The shapes tool with a kind (Rectangle, Ellipse, Line, Arrow, …). */
    const shapeTool = async kind => {
      await t.tap('Slot 8');
      await panel();
      await t.tap(kind);
      await hidePanel();
    };
    const box = async (x, y, w, h) => draw([[x, y], [x + w * 0.5, y + h * 0.5], [x + w, y + h]], 500);

    // ==== 1. A painting ====================================================
    await t.caption('One endless page. First, a painting — by hand');
    await brush({ look: 'Watercolor', color: C.sky, width: 70 });
    for (const y of [-400, -340, -280, -220]) await draw(wave(-760, 760, y, 10, 0.01, y), 1100);
    await brush({ color: C.lav, width: 50 });
    for (const y of [-170, -130]) await draw(wave(-760, 760, y, 8, 0.015, y), 900);
    await brush({ color: C.yellow, width: 46 });
    await draw(spiral(420, -310, 6, 70, 3), 1400);
    await brush({ color: C.orange, width: 14 });
    await draw(circle(420, -310, 86).map(p => [p[0] + wob(3), p[1] + wob(3)]), 900);

    await t.caption('Charcoal mountains');
    await brush({ look: 'Charcoal', color: C.navy, width: 10 });
    const ridge = [[-760, -60], [-560, -250], [-420, -140], [-180, -310], [40, -120], [240, -220], [470, -80], [620, -160], [760, -90]];
    await draw(hand(ridge, 12, 2.5), 2200);
    await brush({ color: C.purple, width: 5 });
    for (let x = -700; x < 740; x += 46) {
      // Shade below the ridge: short slanted hatching.
      const top = ridge.reduce((acc, p, i) => (i && x >= ridge[i - 1][0] && x <= p[0]
        ? ridge[i - 1][1] + ((p[1] - ridge[i - 1][1]) * (x - ridge[i - 1][0])) / (p[0] - ridge[i - 1][0]) : acc), -60);
      await draw([[x, top + 20], [x + 30, top + 70 + wob(10)]], 120);
    }

    await t.caption('Watercolor hills, a lake');
    await brush({ look: 'Watercolor', color: C.lime, width: 90 });
    for (const y of [-20, 50]) await draw(wave(-760, 760, y, 22, 0.008, y), 1100);
    await brush({ look: 'Watercolor', color: C.green, width: 44 });
    for (const y of [140, 200]) await draw(wave(-760, 760, y, 16, 0.01, y), 1000);
    await brush({ color: C.teal, width: 46 });
    for (const y of [270, 320, 370]) await draw(wave(-200, 760, y, 6, 0.02, y), 900);
    await brush({ look: 'Ink', color: C.white, width: 3 });
    for (let i = 0; i < 10; i++) {
      const [x, y] = [-120 + R() * 820, 260 + R() * 120];
      await draw([[x, y], [x + 40 + R() * 40, y + wob(2)]], 160);
    }

    await t.caption('Trees with a bristle brush, foliage scribbled');
    await brush({ look: 'Bristle brush', color: C.brown, width: 18 });
    await draw(hand([[-560, 250], [-552, 120], [-560, 20]], 10, 2), 700);
    await draw(hand([[-400, 280], [-396, 180], [-404, 110]], 10, 2), 600);
    await brush({ look: 'Sketchy', color: C.green, width: 10 });
    await draw(spiral(-556, -10, 10, 85, 4, 0.3), 1700);
    await draw(spiral(-400, 80, 8, 55, 3.5, 0.3), 1300);
    await brush({ look: 'Rainbow', width: 26 });
    await draw(circle(-60, 160, 420, Math.PI * 1.12, Math.PI * 1.88, 60), 1600);

    await t.caption('Birds, flowers, a signature');
    await brush({ look: 'Ink', color: C.black, width: 4 });
    for (const [bx, by, s] of [[60, -420, 1], [130, -390, 0.8], [10, -370, 0.7], [200, -440, 0.6]]) {
      await draw(hand([[bx - 22 * s, by - 8 * s], [bx - 8 * s, by - 2 * s], [bx, by + 6 * s], [bx + 8 * s, by - 2 * s], [bx + 22 * s, by - 10 * s]], 5, 0.6), 280);
    }
    for (const [fx, fy, col] of [[-250, 110, C.pink], [-180, 150, C.yellow], [-90, 100, C.rose], [-320, 170, C.red], [20, 140, C.pink]]) {
      await brush({ color: col, width: 10 });
      await draw(circle(fx, fy, 9, 0, Math.PI * 2, 10), 200);
    }
    await brush({ look: 'Calligraphy', color: C.black, width: 9 });
    // A loopy flourish of a signature.
    const sig = [];
    for (let i = 0; i <= 90; i++) {
      const u = i / 90 * 4 * Math.PI;
      sig.push([520 + u * 13 + Math.sin(u) * 16, 410 - Math.cos(u) * 16 - Math.sin(u / 4) * 10]);
    }
    await draw(sig, 1300);
    await draw([[515, 440], [720, 432]], 400);
    await t.shot('painting');
    await pause(1200);

    // ---- zoom in: a climber on the highest peak -----------------------------
    await t.caption('Zoom in — someone made it to the summit');
    const peak = [-180, -318];
    await pan(peak[0] - cam[0], peak[1] - cam[1], 900);
    const Z = 8;
    await t.wheel(t.w / 2, t.h / 2, -120, Z);
    await pause(500);
    // Screen points around the summit, in demo pixels.
    const P = (dx, dy) => [t.w / 2 + dx * k, t.h / 2 + (dy + 58) * k];
    const PP = pts => pts.map(p => P(p[0], p[1]));
    const fig = async (pts, ms) => ink(PP(hand(pts, 6, 0.7)), ms);
    await brush({ look: 'Pencil', color: C.brown, width: 4 });
    // The rope, trailing down the left slope.
    const rope = [];
    for (let i = 0; i <= 30; i++) rope.push([-30 - i * 11 + Math.sin(i * 0.9) * 6, -95 + i * 8 + Math.cos(i * 0.7) * 5]);
    await ink(PP(rope), 900);
    await brush({ look: 'Ink', color: C.black, width: 4 });
    await fig([[48, -40], [48, -300]], 400);                       // flag pole
    await brush({ color: C.red, width: 5 });
    await fig([[48, -300], [170, -268], [48, -236]], 500);          // flag
    await fig([[60, -290], [140, -268], [60, -248], [110, -268], [60, -282]], 400);
    await brush({ color: C.navy, width: 11 });
    await fig([[-28, -45], [-10, -120]], 300);                      // legs
    await fig([[16, -45], [-4, -120]], 300);
    await brush({ color: C.orange, width: 16 });
    await fig([[-40, -195], [-36, -135]], 300);                     // backpack
    await brush({ color: C.red, width: 14 });
    await fig([[-6, -120], [0, -200]], 300);                        // jacket
    await brush({ color: C.red, width: 8 });
    await fig([[0, -185], [26, -240], [44, -262]], 350);             // arm up to the flag
    await fig([[-2, -178], [-46, -150], [-58, -132]], 350);          // arm with the axe
    await brush({ color: C.grey, width: 4 });
    await fig([[-58, -128], [-68, -92]], 200);                       // ice axe
    await fig([[-82, -132], [-44, -140]], 200);
    await brush({ color: C.peach, width: 22 });
    await fig([[4, -226], [6, -224]], 150);                          // face
    await brush({ color: C.yellow, width: 8 });
    await ink(PP(circle(4, -228, 17, Math.PI * 1.05, Math.PI * 1.95, 12)), 300); // helmet
    await t.shot('summit');
    await pause(1600);
    await t.wheel(t.w / 2, t.h / 2, 120, Z);
    await pause(400);
    await pan(-cam[0], -cam[1], 900);

    // ==== 2. A headset, on paper ============================================
    await t.caption('Next door: an open-source headset — a theoretical block diagram');
    await pan(2100, 0);
    await t.tap('Menu');
    await t.tap('Diagram');
    if (await t.has('Views')) await t.tap('Menu');
    const E = (x, y) => [2100 + x, y];
    await write(...E(-880, -575), ['OG Headset — multipoint Bluetooth + dongle + mixer (theoretical)'], 'Slot 8');
    await t.caption('Diagram mode: blocks, then wires that snap to green connection points');
    await shapeTool('Rectangle');
    const blocks = {
      btA: [-560, -470, 220, 110], btB: [-560, -320, 220, 110], r24: [-560, -150, 220, 110],
      dongle: [-880, -150, 220, 110], dsp: [-220, -470, 300, 330], pmic: [-220, -60, 300, 110],
      codec: [200, -470, 270, 110], mic: [200, -320, 270, 110], ctl: [200, -170, 270, 110],
      spk: [600, -470, 230, 110],
    };
    for (const b of Object.values(blocks)) await box(...E(b[0], b[1]), b[2], b[3]);
    const labels = {
      btA: ['BT radio A', 'LE Audio'], btB: ['BT radio B', 'A2DP · HFP'], r24: ['2.4G radio', 'nRF52840'],
      dongle: ['USB dongle', '2.4 GHz'], dsp: ['DSP mixer', '3 in, 2 out', 'EQ · sidetone'],
      pmic: ['Battery + PMIC', 'USB-C charge'], codec: ['Codec + amp', '24-bit DAC'],
      mic: ['Mic array', '2 × MEMS'], ctl: ['Controls', 'vol · mute'], spk: ['Drivers', '40 mm L/R'],
    };
    for (const [key, lines] of Object.entries(labels)) {
      const b = blocks[key];
      await write(...E(b[0] + 22, b[1] + 16), lines, 'Slot 8');
    }
    await shapeTool('Line');
    // Side middles and quarters of the blocks: the connection points.
    const mid = (b, side, q = 0) => {
      const [x, y, w, h] = b;
      return {
        r: [x + w, y + h / 2 + (q * h) / 2], l: [x, y + h / 2 + (q * h) / 2],
        t: [x + w / 2 + (q * w) / 2, y], b: [x + w / 2 + (q * w) / 2, y + h],
      }[side];
    };
    const wire = async (a, b) => {
      const [p, q] = [E(...a), E(...b)];
      // End a few points off the dots: the snapping finds them.
      await draw([[p[0] + 3, p[1] + 2], [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2], [q[0] - 3, q[1] - 2]], 650);
    };
    const B_ = blocks;
    await wire(mid(B_.btA, 'r'), mid(B_.dsp, 'l', -0.5));
    await wire(mid(B_.btB, 'r'), mid(B_.dsp, 'l', 0));
    await wire(mid(B_.r24, 'r'), mid(B_.dsp, 'l', 0.5));
    await wire(mid(B_.dsp, 'r', -0.5), mid(B_.codec, 'l'));
    await wire(mid(B_.codec, 'r'), mid(B_.spk, 'l'));
    await wire(mid(B_.mic, 'l'), mid(B_.dsp, 'r', 0));
    await wire(mid(B_.ctl, 'l'), mid(B_.dsp, 'r', 0.5));
    await wire(mid(B_.pmic, 't'), mid(B_.dsp, 'b'));

    await t.caption('Hand-drawn: antennas, the wireless link, a speaker');
    await brush({ look: 'Ink', color: C.black, width: 4 });
    for (const b of [B_.btA, B_.r24]) {
      const x = b[0] + b[2] - 30;
      await draw([E(x, b[1]), E(x, b[1] - 34)], 200);
      await draw([E(x - 14, b[1] - 46), E(x, b[1] - 30), E(x + 14, b[1] - 46)], 250);
    }
    await brush({ color: C.blue, width: 4 });
    for (let i = 0; i < 3; i++) {
      const pts = [];
      for (let x = -655; x <= -565; x += 6) pts.push(E(x, -95 + i * 14 + Math.sin(x * 0.25) * 5));
      await draw(pts, 350);
    }
    await brush({ color: C.black, width: 4 });
    await draw([E(855, -440), E(875, -440), E(875, -400), E(855, -400), E(855, -440)], 300);
    await draw([E(875, -440), E(905, -462), E(905, -378), E(875, -400)], 350);
    for (const r of [18, 32]) await draw(circle(...E(910, -420), r, -0.7, 0.7, 10), 200);

    await t.caption('Move a block — every wire stays attached');
    const m = B_.ctl;
    // Box-select the block and its label, then drag it down.
    await t.tap('Slot 6');
    await draw([E(m[0] - 25, m[1] - 25), E(m[0] + m[2] + 25, m[1] + m[3] + 25)], 500);
    await pause(400);
    await draw([E(m[0] + 60, m[1] + 80), E(m[0] + 90, m[1] + 160), E(m[0] + 110, m[1] + 230)], 1200);
    await t.shot('circuit');
    await pause(800);

    // ==== 3. A mechanical sketch ==========================================
    await t.caption('Below it: a mechanical sketch');
    await pan(0, 900);
    await brush({ look: 'Pencil', color: C.grey, width: 3 });
    const G = (x, y) => [2100 + x, 900 + y];
    await draw(gear(...G(-300, 0), 150, 178, 14), 3200);
    await draw(circle(...G(-300, 0), 40).map(p => [p[0] + wob(1), p[1] + wob(1)]), 600);
    await draw(gear(...G(-30, -10), 82, 108, 8), 2000);
    await draw(circle(...G(-30, -10), 24), 400);
    await brush({ look: 'Ink', color: C.blue, width: 3 });
    await draw([G(-300, -260), G(-300, 260)], 400);
    await draw([G(-560, 0), G(140, 0)], 400);
    await shapeTool('Arrow');
    await draw([G(-478, 240), G(-122, 240)], 500);
    await write(...G(-360, 255), ['Ø 356  ·  14 T  ·  m 4'], 'Slot 8');
    await shapeTool('Rectangle');
    await box(...G(260, -180), 340, 360);
    await brush({ look: 'Ink', color: C.black, width: 3 });
    for (const [hx, hy] of [[310, -130], [550, -130], [310, 130], [550, 130]]) await draw(circle(...G(hx, hy), 16, 0, Math.PI * 2, 16), 300);
    await write(...G(270, 200), ['Bracket · 6061-T6', '4 × M8 clearance'], 'Slot 6');
    await t.shot('mechanical');
    await pause(1000);

    // ==== 4. Business notes ================================================
    await t.caption('Back to work: notes, the way a team plans');
    await pan(-4200, -900, 2200);
    const B = (x, y) => [-2100 + x, y];
    // Two boards up top (your Main page's style: a rounded box, a heading,
    // dashed lists); the pasted table lands in the middle below them.
    await run([
      { add: 'shape', kind: 'rect', x: B(-880, 0)[0], y: -500, w: 820, h: 380, round: true, width: 4, color: C.black },
      { add: 'shape', kind: 'rect', x: B(0, 0)[0], y: -500, w: 880, h: 380, round: true, width: 4, color: C.black },
    ]);
    await write(...B(-880, -585), ['Q4 Launch — OG Paper'], 'Slot 6');
    await write(...B(-850, -480), ['Goals'], 'Slot 6');
    await write(...B(-850, -415), [
      '- Ship 1.0 on web, desktop, Android, iOS',
      '- 3 smart-board pilots (schools)',
      '- 500 paying users',
      '- AI assistants via MCP',
    ], 'Slot 6');
    await write(...B(30, -480), ['Workflow'], 'Slot 6');
    const steps = ['Design', 'Build', 'Test', 'Launch'];
    const stepBoxes = steps.map((_, i) => [B(40 + i * 210, 0)[0], -370, 160, 84]);
    await run(stepBoxes.map(([x, y, w, h]) => ({ add: 'shape', kind: 'rect', x, y, w, h, round: true, fill: C.peach, color: C.brown, width: 3 })));
    for (let i = 0; i < steps.length; i++) await write(stepBoxes[i][0] + 28, -350, [steps[i]], 'Slot 6');
    await shapeTool('Arrow');
    for (let i = 0; i + 1 < steps.length; i++) {
      const [a, b] = [stepBoxes[i], stepBoxes[i + 1]];
      await draw([[a[0] + a[2] + 3, -328 + 2], [b[0] - 3, -328 - 1]], 450);
    }
    await write(...B(30, -230), ['Owner: Omega  ·  review every Friday'], 'Slot 6');
    await t.caption('Paste a table straight from a spreadsheet');
    await t.tap('Slot 6');
    await t.glide(...S(...B(-60, 20)), t.demo ? 500 : 1);
    await t.paste('Task\tOwner\tDue\nMCP connector\tOmega\tOct 20\nSymbol packs\tClaude\tOct 31\nPartner pitch\tOmega\tNov 15');
    await pause(700);
    await t.tap('Slot 3');
    await draw(hand([B(-835, -316), B(-600, -316)], 20, 1), 600);
    await t.shot('notes');
    await pause(800);

    await t.caption('Detail lives deeper: zoom into a note');
    await run([{ add: 'shape', kind: 'ellipse', x: B(-860, 0)[0], y: -60, w: 40, h: 40, fill: C.peach, color: C.red, width: 1.5 }]);
    await write(...B(-805, -58), ['← pilot details, inside'], 'Slot 6');
    // Bring the dot to the middle, then dive in.
    const dot = B(-840, -40);
    const back = [cam[0] - dot[0], cam[1] - dot[1]];
    await pan(dot[0] - cam[0], dot[1] - cam[1], 900);
    await t.wheel(t.w / 2, t.h / 2, -120, 10);
    await pause(400);
    await run([
      { add: 'text', x: dot[0] - 13, y: dot[1] - 12, text: 'Pilot schools', size: 2.2, color: C.red },
      { add: 'text', x: dot[0] - 13, y: dot[1] - 6.5, text: '- Lincoln High: 2 boards\n- Westview: 1 board\n- call Tue 10am', size: 1.3, color: C.black },
    ]);
    await t.shot('deeper');
    await pause(1800);
    await t.wheel(t.w / 2, t.h / 2, 120, 10);
    await pause(500);
    await pan(back[0], back[1], 900);

    // ==== 5. An AI assistant ===============================================
    await t.caption('Claude (over MCP): “Turn the plan into a timeline below the boards”');
    await pan(0, 0);
    const tl = [-800, 330];
    const ms = [['Oct 20', 'MCP connector', C.blue], ['Oct 31', 'Symbol packs', C.green], ['Nov 15', 'Partner pitch', C.purple], ['Dec 1', 'Ship 1.0', C.red]];
    await run([{ add: 'shape', kind: 'arrow', x: B(tl[0], 0)[0], y: tl[1], w: 1620, h: 0, color: C.black, width: 4 }]);
    await pause(500);
    for (let i = 0; i < ms.length; i++) {
      const x = B(tl[0] + 80 + i * 400, 0)[0];
      await run([
        { add: 'shape', kind: 'ellipse', x: x - 14, y: tl[1] - 14, w: 28, h: 28, fill: ms[i][2], color: ms[i][2], width: 2 },
        { add: 'text', x: x - 40, y: tl[1] - 64, text: ms[i][0], size: 20, color: ms[i][2] },
        { add: 'shape', kind: 'rect', x: x - 90, y: tl[1] + 40, w: 190, h: 70, round: true, fill: '#fff4d6', color: ms[i][2], width: 2 },
        { add: 'text', x: x - 76, y: tl[1] + 60, text: ms[i][1], size: 18 },
      ]);
      await pause(600);
    }
    await t.shot('timeline');
    await pause(1200);

    // ==== 6. All of it =====================================================
    await t.caption('And all of it is one page');
    await t.tap('Menu');
    await t.tap('Home');
    await pause(1500);
    cam[0] = 0; cam[1] = 0;
    await pan(0, 340, 900);
    for (let i = 0; i < 4; i++) { await t.wheel(t.w / 2, t.h / 2, 120, 1); await pause(450); }
    await pause(2500);
    await t.caption('OG Paper — open source, endless, yours');
    await pause(3000);
    await t.shot('everything');
  },
};
