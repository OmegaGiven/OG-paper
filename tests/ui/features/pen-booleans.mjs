// The pen (Shapes › Pen (curves)): tap corners, press-drag a smooth point,
// tap the first point to close. Then two shapes selected: Union makes one
// shape of them; Subtract cuts the top one out of the lower.
export default {
  name: 'pen-booleans',
  features: ['DRAW-12', 'EDIT-13'],
  sizes: ['desktop', 'phone'],
  title: 'Pen paths and shape booleans',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    const D = t.phone ? 2 : 1;
    const s = t.phone ? 0.6 : 1;
    const px = (x, y) => [t.w / 2 + x / D, t.h / 2 + y / D];
    const panel = async () => {
      if (!(await t.has('Hide settings'))) {
        const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
        if (show) await t.tap(show);
      }
    };
    await t.caption('Shapes › Pen (curves)');
    await t.tap('Slot 8');
    await panel();
    await t.tap('Pen (curves)');
    if (await t.has('Hide settings')) await t.tap('Hide settings');

    await t.caption('Tap a corner, drag a curve, tap a corner, tap the first to close');
    const [ax, ay] = px(-200 * s, 60 * s);
    await t.tap({ x: ax, y: ay });
    const [bx, by] = px(-60 * s, -120 * s);
    await t.drag([[bx, by], [bx + 30, by], [bx + 60, by + 10]], 500);
    const [cx, cy] = px(40 * s, 80 * s);
    await t.tap({ x: cx, y: cy });
    await t.shot('drawing');
    await t.tap({ x: ax + 1, y: ay + 1 });
    await t.wait(async () => (await objects()).length === 1, 'one shape made');
    const o = (await objects())[0];
    t.check(o.kind === 'shape' && o.shape === 'path', `a pen path (${o.kind} ${o.shape})`);
    await t.shot('closed');

    await t.caption('Two squares: select both, Union');
    await run([
      { add: 'shape', kind: 'rect', x: 120 * s, y: -100 * s, w: 100 * s, h: 100 * s, fill: '#2050d0' },
      { add: 'shape', kind: 'rect', x: 170 * s, y: -50 * s, w: 100 * s, h: 100 * s, fill: '#d03030' },
    ]);
    await t.tap('Slot 6');
    await t.drag([px(100 * s, -130 * s), px(200 * s, 0), px(300 * s, 80 * s)]);
    await t.wait(async () => (await t.state()).selected === 2, 'both selected');
    await panel();
    await t.tap('Union');
    await t.wait(async () => (await objects()).length === 2, 'two squares became one shape');
    const u = (await objects()).find(x => x.shape === 'path' && x.x > 0);
    t.check(u && Math.abs(u.w - 150 * s) < 4 && Math.abs(u.h - 150 * s) < 4, `the union spans both (${u?.w} x ${u?.h})`);
    await t.shot('union');
  },
};
