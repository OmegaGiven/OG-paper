// App Store preview 2: gradients and curves, then a zoom through worlds
// inside worlds. See store-preview-1.
import { run, scale, design } from '../store-scenes.mjs';

export default {
  name: 'store-preview-2',
  features: [],
  demoOnly: true,
  sizes: ['iphone-preview', 'ipad-preview'],
  title: 'OG Paper',
  async run(t) {
    await t.open();
    if (await t.has('Hide settings')) await t.tap('Hide settings');
    const k = scale(t);
    const c = [t.w / 2, t.h / 2];
    await t.caption('Gradients, curves, words on a path');
    await design(t, { step: 450 });
    await t.beat(2600);
    await run(t, [{ get: 'objects' }]).then(r => run(t, [{ delete: r[0].objects.map(o => o.id) }]));
    await t.caption('Zoom in forever');
    // Worlds inside worlds, each a hundred times smaller than the last,
    // each in the middle of the one before.
    const cmds = [];
    const colors = ['#c8285a', '#1e5ac8', '#1f8a4c', '#e0a100', '#7b3fd6'];
    const fills = ['#ffd6e2', '#d6e6ff', '#d6f5e2', '#fff1c2', '#eadcff'];
    const words = ['Zoom in', 'deeper', 'deeper still', 'and deeper', 'no bottom'];
    for (let i = 0; i < 5; i++) {
      const s = Math.pow(1e-2, i) * k;
      cmds.push({ add: 'shape', kind: 'ellipse', x: -150 * s, y: -150 * s, w: 300 * s, h: 300 * s, neat: true, width: 6 * s,
        color: colors[i], fill: fills[i] });
      cmds.push({ add: 'text', text: words[i], x: -110 * s, y: -95 * s, size: 30 * s, color: colors[i], font: 'Nunito' });
    }
    await run(t, cmds);
    await t.beat(700);
    // Wheel zoom centres on the pointer: put it in the middle.
    await t.mouse('mouseMoved', c[0], c[1]);
    for (let i = 0; i < 60; i++) {
      const z = (await t.state()).zoom;
      if (z >= 8.2) break;
      await t.wheel(c[0], c[1], -60, 3);
    }
    await t.beat(1200);
  },
};
