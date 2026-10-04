// Draw with the brush, then undo and redo from the tool fan.
export default {
  name: 'draw-undo',
  features: ['DRAW-01', 'EDIT-01', 'UI-02'],
  sizes: ['desktop', 'phone'],
  title: 'Draw, then undo and redo',
  async run(t) {
    await t.open();
    const s0 = await t.state();
    await t.caption('Draw anywhere with the brush');
    await t.scribble(t.w * 0.55, t.h * 0.42, t.w * 0.5);
    await t.wait(async () => (await t.state()).strokes > s0.strokes, 'a stroke is drawn');
    const s1 = await t.state();
    t.check(s1.strokes === s0.strokes + 1, 'one stroke was drawn');
    await t.shot('drawn');

    await t.caption('Open the tool fan: undo and redo lead it');
    await t.tap('Tools');
    await t.tap('Undo');
    await t.wait(async () => (await t.state()).undos > s1.undos, 'the stroke is undone');
    await t.shot('undone');
    await t.tap('Redo');
    await t.wait(async () => (await t.state()).undos === s1.undos || (await t.state()).strokes === s1.strokes, 'the stroke is back');
    await t.shot('redone');
  },
};
