// Select several things and Group them: a tap on any one picks them all,
// and they move as one. Ungroup splits them again.
export default {
  name: 'groups',
  features: ['EDIT-12'],
  sizes: ['desktop', 'phone'],
  title: 'Group and ungroup',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const s = t.phone ? 0.7 : 1;
    const D = t.phone ? 2 : 1;
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    await run([
      { add: 'shape', kind: 'rect', x: -200 * s, y: -120 * s, w: 80 * s, h: 40 * s },
      { add: 'shape', kind: 'ellipse', x: -60 * s, y: -100 * s, w: 60 * s, h: 60 * s },
      { add: 'text', text: 'label', x: -180 * s, y: 0, size: 24 * s },
    ]);
    const px = (x, y) => [t.w / 2 + x / D, t.h / 2 + y / D];
    const sheet = async label => {
      if (await t.has(label)) return;
      const open = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (open) await t.tap(open);
    };
    await t.caption('Box-select the three, then Group');
    await t.tap('Slot 6');
    await t.drag([px(-260 * s, -180 * s), px(0, 0), px(60 * s, 80 * s)]);
    await t.wait(async () => (await t.state()).selected === 3, 'three selected');
    await sheet('Group');
    await t.tap('Group');
    await t.wait(async () => (await t.state()).selected === 1, 'one group selected');
    let o = await objects();
    t.check(o.length === 1 && o[0].kind === 'group' && o[0].members === 3, `one group of three (${o.map(x => x.kind).join(', ')})`);
    await t.shot('grouped');

    await t.caption('Tap the circle alone: the whole group is picked; drag moves it all');
    if (t.phone && await t.has('Hide settings')) await t.tap('Hide settings');
    await t.tap({ x: t.w * 0.85, y: t.h * 0.2 });
    await t.wait(async () => (await t.state()).selected === 0, 'nothing selected');
    const [cx, cy] = px(-30 * s, -100 * s);
    await t.tap({ x: cx, y: cy });
    await t.wait(async () => (await t.state()).selected === 1, 'group picked');
    const g0 = (await objects())[0];
    const a = px(-30 * s, -70 * s);
    await t.drag([a, [a[0] + 30, a[1] + 20], [a[0] + 60, a[1] + 40]], 700);
    const g1 = (await objects())[0];
    t.check(g1.kind === 'group' && Math.abs(g1.x - g0.x - 60 * D) < 8 && Math.abs(g1.y - g0.y - 40 * D) < 8,
      `moved together (${(g1.x - g0.x).toFixed(0)}, ${(g1.y - g0.y).toFixed(0)})`);
    await t.shot('moved');

    await t.caption('Ungroup: three things again');
    await sheet('Ungroup');
    await t.tap('Ungroup');
    await t.wait(async () => (await t.state()).selected === 3, 'three selected again');
    o = await objects();
    t.check(o.length === 3 && !o.some(x => x.kind === 'group'), `three apart (${o.map(x => x.kind).join(', ')})`);
  },
};
