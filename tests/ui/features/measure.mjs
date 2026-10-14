// The selection's X, Y, W, H and turn in the selection panel: type a
// number and the selection moves, resizes or turns to it. Menu › Rulers
// shows rulers along the top and left, in the same units.
export default {
  name: 'measure',
  features: ['EDIT-14', 'NAV-11'],
  sizes: ['desktop'],
  title: 'Numbers and rulers',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const shape = async () => (await run([{ get: 'objects', kinds: ['shape'] }]))[0].objects[0];
    await run([{ add: 'shape', kind: 'rect', x: -100, y: -60, w: 120, h: 80, neat: true, width: 2 }]);
    await t.tap('Slot 6');
    await t.tap({ x: t.w / 2 - 100, y: t.h / 2 - 20 });
    await t.wait(async () => (await t.state()).selected === 1, 'selected');
    if (!(await t.has('Width'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    }
    const set = async (name, v) => {
      const f = await t.find(name);
      await t.tap({ x: f.x + f.w / 2, y: f.y + f.h / 2 });
      await t.beat(400);
      await t.type(String(v));
      await t.key('Enter');
      await t.beat(400);
    };
    await t.caption('Type X 50: it moves there');
    await set('X (home view points)', 50);
    let o = await shape();
    t.check(Math.abs(o.x - 50) < 3, `x is 50 (${o.x})`);
    await t.caption('Type Width 300: it grows from its left edge');
    await set('Width', 300);
    o = await shape();
    t.check(Math.abs(o.w - 300) < 6 && Math.abs(o.x - 50) < 3, `w is 300 (${o.x}, ${o.w})`);
    await t.caption('Menu › Rulers');
    await t.tap('Menu');
    await t.tap('Rulers');
    await t.beat(300);
    await t.shot('rulers');
    // The top ruler is drawn: a light band with ticks.
    // The top ruler is drawn: a light band with dark ticks across it.
    const row = [];
    for (let x = 100; x < t.w - 100; x += 7) row.push((await t.pixel(x, 12))[0]);
    const light = row.filter(v => v > 235).length, dark = row.filter(v => v < 150).length;
    t.check(light > row.length / 2 && dark >= 2, `the ruler band, with ticks (${light} light, ${dark} dark)`);
  },
};
