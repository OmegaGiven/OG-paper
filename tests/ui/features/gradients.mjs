// Shapes › Fill › Gradient: the fill runs from the fill color to the
// "Fill end" color. Under every color dial, Swatches: + keeps the color,
// a tap takes it back.
export default {
  name: 'gradients',
  features: ['DRAW-13', 'UI-20'],
  sizes: ['desktop'],
  title: 'Gradients and swatches',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const px = (x, y) => [t.w / 2 + x, t.h / 2 + y];
    await t.caption('A gradient square, red to blue, left to right');
    await run([{ add: 'shape', kind: 'rect', x: -100, y: -100, w: 200, h: 200, fill: '#e02020', neat: true,
      gradient: { to: '#2040e0', angle: 0 } }]);
    await t.beat(300);
    const l = await t.pixel(...px(-80, 0)), r = await t.pixel(...px(80, 0)), m = await t.pixel(...px(0, 0));
    t.check(l[0] > 180 && l[2] < 100, `red on the left (${l})`);
    t.check(r[2] > 180 && r[0] < 100, `blue on the right (${r})`);
    t.check(m[0] > 80 && m[2] > 80, `between in the middle (${m})`);
    await t.shot('gradient');

    await t.caption('The shapes tool: Fill › Gradient, then Fill end');
    await t.tap('Slot 8');
    if (!(await t.has('Hide settings'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    }
    await t.tap('Gradient');
    await t.find('Gradient angle');
    await t.tap('Color', { role: 'Button' });
    await t.tap('Fill end');
    await t.caption('Swatches: + keeps the color');
    const before = (await t.nodes()).filter(n => /^Swatch #/.test(n.label || '')).length;
    await t.tap('Save swatch');
    await t.wait(async () => (await t.nodes()).filter(n => /^Swatch #/.test(n.label || '')).length === before + 1, 'a new swatch');
    await t.shot('swatches');
  },
};
