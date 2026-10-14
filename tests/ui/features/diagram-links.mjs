// Diagram mode: with the arrow tool, an object under the pointer shows its
// connection points (green); an arrow drawn onto them is linked, and
// dragging several objects at once carries every arrow linked to them,
// whether it is selected too or not.
export default {
  name: 'diagram-links',
  features: ['DRAW-09'],
  sizes: ['desktop'],
  title: 'Arrows stay joined',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(300);
      return t.b.eval('window.ogPaper.results()');
    };
    // Home-view points to screen.
    const px = (x, y) => [t.w / 2 + x, t.h / 2 + y];
    const arrows = async () => {
      const r = await run([{ get: 'objects', kinds: ['shape'] }]);
      return r[0].objects.filter(o => o.shape === 'arrow');
    };
    const near = (p, q) => Math.abs(p[0] - q[0]) < 1 && Math.abs(p[1] - q[1]) < 1;

    await t.caption('Diagram mode on');
    await t.tap('Menu');
    await t.tap('Diagram');
    if (await t.maybe('Views', 300)) await t.tap('Menu');
    // A and C on the left, one above the other; B on the right.
    await run([
      { add: 'shape', kind: 'rect', x: -300, y: -50, w: 100, h: 100 },
      { add: 'shape', kind: 'rect', x: 100, y: -50, w: 100, h: 100 },
      { add: 'shape', kind: 'rect', x: -300, y: 150, w: 100, h: 100 },
      // C's top to A's bottom.
      { add: 'shape', kind: 'arrow', x: -250, y: 150, w: 0, h: -100 },
    ]);

    await t.caption('The arrow tool shows connection points');
    await t.tap('Slot 8');
    await t.tap('Arrow');
    await t.glide(...px(-210, 6));
    await t.beat(300);
    await t.shot('ports');

    await t.caption('Draw an arrow from A to B, ending near their side middles');
    await t.drag([px(-205, 4), px(-50, 2), px(104, -3)], 700);
    let list = await arrows();
    const ab = list.find(a => a.from[0] > -240);
    t.check(ab && near(ab.from, [-200, 0]) && near(ab.to, [100, 0]),
      `snapped onto both connection points (${JSON.stringify(ab && [ab.from, ab.to])})`);

    await t.caption('Select A and C together and drag them down');
    await t.tap('Slot 6');
    await t.drag([px(-330, -80), px(-250, 100), px(-170, 280)]);
    await t.wait(async () => (await t.state()).selected >= 2, 'A and C selected');
    await t.drag([px(-250, 0), px(-250, 80), px(-250, 150)], 800);
    await t.shot('moved');

    list = await arrows();
    const ab2 = list.find(a => a.to[0] > 50);
    const ca2 = list.find(a => a.to[0] < -200);
    t.check(ab2 && near(ab2.from, [-200, 150]) && near(ab2.to, [100, 0]),
      `A to B: its start came along, its end stayed on B (${JSON.stringify(ab2 && [ab2.from, ab2.to])})`);
    t.check(ca2 && near(ca2.from, [-250, 300]) && near(ca2.to, [-250, 200]),
      `C to A: moved whole (${JSON.stringify(ca2 && [ca2.from, ca2.to])})`);
  },
};
