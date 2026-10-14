// A layer's look (Menu › Layers, under the list, for the layer in use):
// its opacity, its blend mode, and Clip (show it only where the layer
// under it has something). Checked on the screen's own pixels.
export default {
  name: 'layer-looks',
  features: ['DRAW-11'],
  sizes: ['desktop'],
  title: 'Layer opacity, blend modes, clipping masks',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const px = (x, y) => [t.w / 2 + x, t.h / 2 + y];
    const at = async (x, y) => t.pixel(...px(x, y));
    await t.caption('A blue square on the first layer');
    await run([{ add: 'shape', kind: 'rect', x: -200, y: -60, w: 120, h: 120, fill: '#2050d0', color: '#2050d0', neat: true }]);
    await t.tap('Menu');
    await t.tap('Layers');
    await t.tap('New layer');
    await t.caption('A red bar on a new layer, across it');
    await run([{ add: 'shape', kind: 'rect', x: -170, y: -30, w: 260, h: 60, fill: '#d03030', color: '#d03030', neat: true }]);
    await t.beat(300);
    const over = await at(-130, 0), out = await at(40, 0);
    t.check(over[0] > 150 && over[2] < 90, `red over the blue (${over})`);
    t.check(out[0] > 150 && out[1] < 90, `red past it (${out})`);
    await t.shot('plain');

    await t.caption('Clip: the red shows only on the blue');
    await t.tap('Clip to the layer below');
    await t.wait(async () => (await at(40, 0))[1] > 200, 'red outside the blue is gone');
    const inside = await at(-130, 0);
    t.check(inside[0] > 150 && inside[2] < 90, `still red on the blue (${inside})`);
    await t.shot('clipped');
    await t.tap('Clip to the layer below');
    await t.wait(async () => (await at(40, 0))[1] < 90, 'unclipped');

    await t.caption('Multiply: red times blue is dark');
    await t.tap('Normal');
    await t.tap('Multiply');
    await t.wait(async () => { const c = await at(-130, 0); return c[0] < 120 && c[2] < 120; }, 'dark where they meet');
    const paper = await at(40, 0);
    t.check(paper[0] > 150 && paper[1] < 90, `red on paper stays red (${paper})`);
    await t.shot('multiply');

    await t.caption('Half opacity: see-through');
    const s = await t.find('Layer opacity');
    await t.tap({ x: s.x + s.w * 0.3, y: s.y + s.h / 2 });
    await t.wait(async () => { const c = await at(40, 0); return c[1] > 100 && c[1] < 220 && c[0] > 200; }, 'pink, not red');
    await t.shot('faded');
  },
};
