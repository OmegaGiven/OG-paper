// The tool panel: a sheet along the bottom on phones (tucked into a strip
// until tapped), a window that can be dragged on wider screens. Width and
// opacity stay in view; the rest sits behind tabs.
export default {
  name: 'tool-panel',
  features: ['UI-16'],
  sizes: ['desktop', 'phone'],
  title: 'The tool panel',
  async run(t) {
    await t.open();
    if (t.phone) {
      await t.caption('On a phone the settings tuck into a strip');
      await t.shot('strip');
      await t.tap('Brush settings');
    }
    await t.find('Width');
    await t.find('Opacity');
    t.check(true, 'width and opacity in view');
    await t.caption(t.phone ? 'Tap it for a sheet over the bottom half' : 'A window you can move anywhere');
    await t.shot('simple');

    await t.caption('Style: pressure and dashes');
    await t.tap('Style');
    await t.find('Width follows pressure');
    t.check(true, 'the Style tab');

    await t.caption('Advanced: the brush engine, a tab at a time');
    await t.tap('Advanced');
    await t.tap('Dynamics');
    await t.find('taper start');
    t.check(true, 'the Dynamics tab');
    await t.shot('advanced');
    await t.tap('Look');
    await t.tap('Watercolor');
    await t.scribble(t.w * 0.55, t.h * 0.2, t.w * 0.3);
    await t.tap('Simple');

    if (!t.phone) {
      await t.caption('Drag it by any free space');
      const at = await t.find('Brush');
      const [x, y] = [at.x + at.w / 2, at.y + at.h / 2];
      await t.drag([[x, y], [x + 140, y - 80], [x + 420, y - 240]]);
      await t.beat(300);
      const moved = await t.find('Brush');
      t.check(Math.abs(moved.x - at.x - 420) < 30, 'the window moved');
      await t.shot('moved');
    } else {
      await t.caption('Swipe it down to tuck it away');
      await t.tap('Hide settings');
      await t.find('Brush settings');
      t.check(true, 'back to the strip');
    }
  },
};
