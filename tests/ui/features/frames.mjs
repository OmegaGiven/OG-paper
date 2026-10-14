// Shapes › Frame: drag out a named white page (an artboard). Its name
// shows over it; selected, it can be renamed; Menu › Frames flies to it.
export default {
  name: 'frames',
  features: ['DRAW-14'],
  sizes: ['desktop', 'phone'],
  title: 'Frames (artboards)',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    const D = t.phone ? 2 : 1;
    const s = t.phone ? 0.6 : 1;
    const px = (x, y) => [t.w / 2 + x / D, t.h / 2 + y / D];
    const panel = async () => {
      if (!(await t.has('Hide settings'))) {
        const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI|Tool) /.test(n.label));
        if (show) await t.tap(show);
      }
    };
    await t.caption('Shapes › Frame, then drag one out');
    await t.tap('Slot 8');
    await panel();
    await t.tap('Frame');
    if (await t.has('Hide settings')) await t.tap('Hide settings');
    await t.drag([px(-220 * s, -150 * s), px(0, 0), px(160 * s, 150 * s)]);
    await t.wait(async () => (await objects()).some(o => o.kind === 'frame'), 'a frame');
    let f = (await objects()).find(o => o.kind === 'frame');
    t.check(f.name === 'Frame 1', `named Frame 1 (${f.name})`);
    await t.shot('frame');

    await t.caption('Rename it in the selection panel');
    await t.tap('Slot 6');
    const [fx, fy] = px(-218 * s, 0);
    await t.tap({ x: fx, y: fy });
    await t.wait(async () => (await t.state()).selected === 1, 'the frame selected');
    await panel();
    await t.tap('Frame name');
    await t.beat(400);
    await t.type('Cover');
    await t.key('Enter');
    await t.wait(async () => (await objects()).some(o => o.kind === 'frame' && o.name === 'Cover'), 'renamed Cover');

    await t.caption('Pan away, then Menu › Frames › Cover flies back');
    await run([{ say: 'away' }]);
    await t.wheel(t.w / 2, t.h / 2, 600, 10);
    await t.tap('Menu');
    await t.tap('Frames');
    await t.tap('Frame Cover');
    await t.beat(1500);
    f = (await objects()).find(o => o.kind === 'frame');
    await t.shot('flown');
    t.check(!!f, 'the frame is still there');
  },
};
