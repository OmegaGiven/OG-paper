// Select things, Make component: they become a component's main.
// Duplicate it: a copy. Change the main (recolor): the copy follows.
export default {
  name: 'components',
  features: ['EDIT-16'],
  sizes: ['desktop'],
  title: 'Components',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const groups = async () => (await run([{ get: 'objects' }]))[0].objects.filter(o => o.kind === 'group');
    await run([
      { add: 'shape', kind: 'rect', x: -260, y: -60, w: 120, h: 50, fill: '#2050d0', neat: true },
      { add: 'text', text: 'Button', x: -245, y: -48, size: 20, color: '#ffffff' },
    ]);
    const panel = async name => {
      if (await t.has(name)) return;
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    };
    await t.caption('Select both, Make component');
    await t.tap('Slot 6');
    await t.drag([[t.w / 2 - 290, t.h / 2 - 90], [t.w / 2 - 200, t.h / 2], [t.w / 2 - 120, t.h / 2 + 20]]);
    await t.wait(async () => (await t.state()).selected === 2, 'both selected');
    await panel('Make component');
    await t.tap('Make component');
    await t.wait(async () => (await groups()).some(g => g.component && g.main), 'a component main');
    await t.caption('Duplicate: a copy');
    await t.tap('Duplicate');
    await t.wait(async () => (await groups()).some(g => g.component && !g.main), 'a copy');
    // Move the copy away so both show.
    await run([{ move: [(await groups()).find(g => !g.main).id], dx: 260, dy: 80 }]);
    await t.shot('copy');
    await t.caption('Recolor the main: the copy follows');
    const main = (await groups()).find(g => g.main);
    await run([{ set: main.id, color: '#d03030' }]);
    const copy = (await groups()).find(g => !g.main);
    await t.shot('followed');
    // Across the copy's left edge: some red outline.
    let red = null;
    for (let dx = -3; dx <= 6 && !red; dx++) {
      const p = await t.pixel(t.w / 2 + copy.x + dx, t.h / 2 + copy.y + copy.h / 2);
      if (p[0] > 150 && p[2] < 120) red = p;
    }
    t.check(!!red, `the copy's outline turned red too (${red})`);
  },
};
