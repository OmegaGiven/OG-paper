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

    await t.caption('Advanced by default: the brush engine, a tab at a time');
    await t.tap('Dynamics');
    await t.find('taper start');
    t.check(true, 'Advanced is the default, with its Dynamics tab');
    await t.shot('advanced');
    await t.tap('Look');
    await t.tap('Watercolor');
    await t.scribble(t.w * 0.55, t.h * 0.2, t.w * 0.3);

    await t.caption('Simple: a plain line with pressure and dashes');
    await t.tap('Simple');
    await t.tap('Style');
    await t.find('Width follows pressure');
    t.check(true, 'the Simple Style tab');
    await t.tap('Advanced');

    if (!t.phone) {
      await t.caption('Drag the corner to resize; wide, the color gets its own column');
      const grip = await t.find('Resize settings');
      const [gx, gy] = [grip.x + grip.w / 2, grip.y + grip.h / 2];
      await t.drag([[gx, gy], [gx + 150, gy], [gx + 300, gy]]);
      await t.beat(300);
      const after = await t.find('Resize settings');
      t.check(after.x - grip.x > 250, 'the window got wider');
      const colorTabs = (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Color')).length;
      t.check(colorTabs === 0, 'wide: no Color tab (the color has its own column)');
      await t.shot('wide');

      await t.caption('Drag it by any free space');
      const at = await t.find('Brush');
      const [x, y] = [at.x + at.w / 2, at.y + at.h / 2];
      await t.drag([[x, y], [x + 100, y - 60], [x + 250, y - 150]]);
      await t.beat(300);
      const moved = await t.find('Brush');
      t.check(Math.abs(moved.x - at.x - 250) < 30, 'the window moved');
      await t.shot('moved');
    } else {
      await t.caption('Tap the handle for a taller sheet');
      const h0 = await t.find('Sheet handle');
      await t.tap('Sheet handle');
      await t.beat(300);
      const h1 = await t.find('Sheet handle');
      t.check(h0.y - h1.y > t.h * 0.2, 'the sheet got taller');
      await t.shot('tall');
      await t.caption('Tuck it away');
      await t.tap('Hide settings');
      await t.find('Brush settings');
      t.check(true, 'back to the strip');
    }
  },
};
