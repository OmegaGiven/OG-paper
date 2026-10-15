// Menu › Symbols: flowchart, electrical and mechanical symbols. Placed,
// a symbol is a group with pins; in diagram mode a line drawn from a pin
// is joined to it, and stays on it when the symbol moves.
export default {
  name: 'symbols',
  features: ['DRAW-16'],
  sizes: ['desktop'],
  title: 'Diagram symbols',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(300);
      return t.b.eval('window.ogPaper.results()');
    };
    const px = (x, y) => [t.w / 2 + x, t.h / 2 + y];
    const near = (p, q) => Math.abs(p[0] - q[0]) < 1.5 && Math.abs(p[1] - q[1]) < 1.5;
    await t.caption('Diagram mode on, then Menu › Symbols › Electrical › Resistor');
    await t.tap('Menu');
    await t.tap('Diagram');
    if (await t.maybe('Views', 300)) await t.tap('Menu');
    await t.tap('Menu');
    await t.tap('Symbols');
    await t.tap('Electrical');
    await t.shot('panel');
    await t.tap('Symbol Resistor');
    await t.wait(async () => (await run([{ get: 'objects', kinds: ['group'] }]))[0].objects.length === 1, 'a resistor');
    const g = (await run([{ get: 'objects', kinds: ['group'] }]))[0].objects[0];
    const pin = [g.x + g.w, g.y + g.h / 2];
    // Away from the panel.
    await run([{ move: g.id, dx: 200 - pin[0], dy: -pin[1] }]);
    const g2 = (await run([{ get: 'objects', kinds: ['group'] }]))[0].objects[0];
    const pin2 = [g2.x + g2.w, g2.y + g2.h / 2];

    await t.caption('A line from its right pin');
    await t.tap('Slot 8');
    if (!(await t.has('Line'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    }
    await t.tap('Line');
    await t.drag([px(pin2[0] + 3, pin2[1] + 2), px(pin2[0] + 60, pin2[1] + 40), px(pin2[0] + 140, pin2[1] + 90)], 700);
    let line = (await run([{ get: 'objects', kinds: ['shape'] }]))[0].objects.find(o => o.shape === 'line');
    t.check(line && near(line.from, pin2), `joined at the pin (${JSON.stringify(line && line.from)} vs ${pin2})`);
    await t.shot('wired');

    await t.caption('Move the resistor: the wire stays on its pin');
    await run([{ move: g2.id, dx: 0, dy: 80 }]);
    line = (await run([{ get: 'objects', kinds: ['shape'] }]))[0].objects.find(o => o.shape === 'line');
    const pin3 = [pin2[0], pin2[1] + 80];
    t.check(near(line.from, pin3), `followed (${JSON.stringify(line.from)} vs ${pin3})`);
    await t.shot('followed');
  },
};
