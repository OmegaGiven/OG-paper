// Text: letter spacing and line height in the panel; select a text and a
// line, Text on path runs the words along it; Off path puts them straight.
export default {
  name: 'text-style',
  features: ['DRAW-15'],
  sizes: ['desktop'],
  title: 'Text spacing and text on a path',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    await run([
      { add: 'text', text: 'Around the bend', x: -150, y: -120, size: 28 },
      { add: 'shape', kind: 'path', points: [[-200, 80], [0, -40], [200, 80]], smooth: true },
    ]);
    await t.caption('Select both, then Text on path');
    await t.tap('Slot 6');
    await t.drag([[t.w / 2 - 260, t.h / 2 - 160], [t.w / 2, t.h / 2], [t.w / 2 + 240, t.h / 2 + 120]]);
    await t.wait(async () => (await t.state()).selected === 2, 'both selected');
    if (!(await t.has('Text on path'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
      if (show) await t.tap(show);
    }
    await t.tap('Text on path');
    await t.wait(async () => (await objects()).length === 1, 'one text left, on the path');
    let o = (await objects())[0];
    t.check(o.kind === 'text' && o.w > 300, `the words span the curve (${o.kind} ${o.w})`);
    await t.shot('on-path');

    await t.caption('Wider letter spacing');
    const s = await t.find('Letter spacing');
    await t.tap({ x: s.x + s.w * 0.6, y: s.y + s.h / 2 });
    await t.beat(500);
    await t.shot('spaced');
    await t.caption('Off path');
    await t.tap('Off path');
    await t.wait(async () => { const x = (await objects())[0]; return x && x.h < 80; }, 'straight again');
  },
};
