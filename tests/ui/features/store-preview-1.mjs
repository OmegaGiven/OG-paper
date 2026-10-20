// App Store preview 1 (iPhone 886 x 1920, iPad 1200 x 1600): draw, then a
// diagram whose arrows follow a box as it moves. Record with
// `node tests/ui/run.mjs --demo store-preview-1`; cut with
// scripts/ad/store-cut.sh.
import { run, scale, diagram } from '../store-scenes.mjs';

export default {
  name: 'store-preview-1',
  features: [],
  demoOnly: true,
  sizes: ['iphone-preview', 'ipad-preview'],
  title: 'OG Paper',
  async run(t) {
    await t.open();
    if (await t.has('Hide settings')) await t.tap('Hide settings');
    const k = scale(t);
    const c = [t.w / 2, t.h / 2];
    const at = (x, y) => [c[0] + x * k, c[1] + y * k];
    await t.caption('Draw anything');
    const heart = [];
    for (let i = 0; i <= 40; i++) {
      const a = i / 40 * Math.PI * 2;
      heart.push(at(16 * Math.sin(a) ** 3 * 5, -60 - (13 * Math.cos(a) - 5 * Math.cos(2 * a) - 2 * Math.cos(3 * a) - Math.cos(4 * a)) * 5));
    }
    await t.drag(heart, 1300);
    await t.beat(400);
    await run(t, [{ get: 'objects' }]).then(r => run(t, [{ delete: r[0].objects.map(o => o.id) }]));
    await t.caption('Diagrams that stay connected');
    await t.tap('Menu');
    await t.tap('Diagram');
    if (await t.maybe('Views', 300)) await t.tap('Menu');
    const d = await diagram(t, { step: 250 });
    // Drag the middle box by its middle: its arrows follow.
    await t.tap('Slot 6');
    const b = d.sketch;
    const p0 = [c[0] + b.x + b.w * 0.5, c[1] + b.y + b.h * 0.5];
    await t.tap({ x: p0[0], y: p0[1] });
    await t.drag([p0, [p0[0] + 40 * k, p0[1] + 6 * k], [p0[0] + 90 * k, p0[1] + 12 * k]], 1100);
    await t.beat(900);
  },
};
