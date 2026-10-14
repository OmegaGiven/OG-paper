// Layers: Menu › Layers lists them top first. New layer draws above the
// rest; hide one and it isn't drawn; lock one and nothing new goes on it;
// move one up and its drawing goes above.
export default {
  name: 'layers',
  features: ['DRAW-10'],
  sizes: ['desktop', 'phone'],
  title: 'Layers',
  async run(t) {
    await t.open();
    await t.caption('Draw on the first layer');
    await t.scribble(t.w * 0.45, t.h * 0.35, t.w * 0.3);
    await t.caption('Menu › Layers › + New: draw on it');
    await t.tap('Menu');
    await t.tap('Layers');
    await t.find('Layer Layer 1');
    await t.tap('New layer');
    await t.find('Layer Layer 2');
    await t.scribble(t.w * 0.45, t.h * 0.55, t.w * 0.3);
    const drawn = async () => (await t.state()).shown;
    await t.wait(async () => (await t.state()).strokes === 2, 'two strokes');
    const both = await drawn();
    await t.shot('two-layers');

    await t.caption('Hide the first layer');
    await t.tap('Hide Layer 1');
    await t.wait(async () => (await drawn()) < both, 'its drawing is hidden');
    await t.shot('hidden');
    await t.tap('Show Layer 1');
    await t.wait(async () => (await drawn()) === both, 'and back');

    await t.caption('Lock the layer in use: nothing new goes on it');
    await t.tap('Lock Layer 2');
    await t.scribble(t.w * 0.45, t.h * 0.7, t.w * 0.2);
    t.check((await t.state()).strokes === 2, 'no new stroke on a locked layer');
    await t.tap('Unlock Layer 2');

    await t.caption('Move the first layer up');
    await t.tap('Move Layer 1 up');
    const rows = (await t.nodes()).filter(n => n.w > 0 && /^Layer Layer \d$/.test(n.label || '')).sort((a, b) => a.y - b.y);
    t.check(rows[0]?.label === 'Layer Layer 1', `Layer 1 is on top now (${rows.map(r => r.label).join(', ')})`);
    await t.shot('moved');
  },
};
