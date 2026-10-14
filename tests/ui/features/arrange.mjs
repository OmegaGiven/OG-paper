// Select several things: the panel lines them up (left, centre, right,
// top, middle, bottom) and spreads them out evenly. Dragging a selection
// snaps its edges and centre to what is around, with a guide.
export default {
  name: 'arrange',
  features: ['EDIT-11'],
  sizes: ['desktop', 'phone'],
  title: 'Align, spread out, snap',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const s = t.phone ? 0.7 : 1;   // phones: the same layout, smaller
    const D = t.phone ? 2 : 1;      // points per CSS pixel
    const shapes = async () => (await run([{ get: 'objects', kinds: ['shape'] }]))[0].objects;
    await run([
      { add: 'shape', kind: 'rect', x: -200 * s, y: -150 * s, w: 80 * s, h: 40 * s },
      { add: 'shape', kind: 'rect', x: -130 * s, y: -60 * s, w: 60 * s, h: 50 * s },
      { add: 'shape', kind: 'rect', x: -60 * s, y: 100 * s, w: 100 * s, h: 30 * s },
    ]);
    const px = (x, y) => [t.w / 2 + x / D, t.h / 2 + y / D];
    // On a phone the selection's panel is a sheet: open it.
    const sheet = async () => {
      if (await t.has('Align left')) return;
      const open = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (open) await t.tap(open);
    };
    await t.caption('Box-select the three, then Align left');
    await t.tap('Slot 6');
    await t.drag([px(-260 * s, -200 * s), px(0, 0), px(80 * s, 170 * s)]);
    await t.wait(async () => (await t.state()).selected === 3, 'three selected');
    await sheet();
    await t.tap('Align left');
    let b = await shapes();
    t.check(b.every(o => Math.abs(o.x - b[0].x) < 4), `lined up on the left (${b.map(o => o.x).join(', ')})`);
    await t.shot('aligned');

    await t.caption('Spread them out down: even gaps');
    await sheet();
    await t.tap('Spread out down');
    b = (await shapes()).sort((p, q) => p.y - q.y);
    const g1 = b[1].y - (b[0].y + b[0].h), g2 = b[2].y - (b[1].y + b[1].h);
    t.check(Math.abs(g1 - g2) < 1.5, `equal gaps (${g1.toFixed(1)}, ${g2.toFixed(1)})`);

    await t.caption('Drag one: it snaps to the others, with a guide');
    // Select the middle one alone and drag it right by 40 points less a
    // hair: its left edge snaps back onto the others' left edge... then
    // across to line up its right edge with the widest one's.
    const m = b[1];
    if (t.phone && await t.has('Hide settings')) await t.tap('Hide settings');
    // Let go of the three (tap empty canvas), then tap the one's edge.
    await t.tap({ x: t.w * 0.85, y: t.h * 0.2 });
    await t.wait(async () => (await t.state()).selected === 0, 'nothing selected');
    const [tx, ty] = px(m.x + m.w / 2, m.y + 1);
    await t.tap({ x: tx, y: ty });
    await t.wait(async () => (await t.state()).selected === 1, 'one selected');
    const wide = b.reduce((p, q) => (q.w > p.w ? q : p));
    const dx = ((wide.x + wide.w) - (m.x + m.w) + 3) / D;   // 3 points past
    const a = px(m.x + m.w / 2, m.y + m.h / 2);
    await t.drag([a, [a[0] + dx / 2, a[1] + 2], [a[0] + dx, a[1]]], 700);
    b = await shapes();
    const moved = b.find(o => Math.abs(o.w - m.w) < 1 && Math.abs(o.h - m.h) < 1);
    t.check(Math.abs((moved.x + moved.w) - (wide.x + wide.w)) < 0.6 && Math.abs(moved.y - m.y) < 0.6,
      `snapped: right edges meet, height kept (${(moved.x + moved.w).toFixed(1)} vs ${(wide.x + wide.w).toFixed(1)})`);
    await t.shot('snapped');
  },
};
