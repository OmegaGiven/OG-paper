// Text › Text box: drag out a box (a border and fill if you like); what you
// type wraps to stay inside it and gets smaller when it needs to.
export default {
  name: 'text-box',
  features: ['DRAW-17'],
  sizes: ['desktop', 'phone'],
  title: 'Text boxes: words that wrap in a box',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    await t.caption('Text tool › Text box, with a border');
    await t.tap('Slot 9');
    if (!(await t.has('Text box'))) {
      const show = (await t.nodes()).find(n => n.w > 0 && / settings$/.test(n.label || '') && !/^(Hide|Resize|UI) /.test(n.label));
      if (show) await t.tap(show);
    }
    await t.tap('Text box');
    if (!(await t.has('Border'))) await t.tap('Size & align');
    await t.tap('Border');
    await t.shot('options');
    if (t.phone && (await t.has('Hide tool settings'))) await t.tap('Hide tool settings');
    await t.caption('Drag out the box, then type');
    const [x0, y0] = t.phone ? [t.w * 0.3, t.h * 0.3] : [t.w * 0.45, t.h * 0.3];
    const [x1, y1] = t.phone ? [t.w * 0.85, t.h * 0.45] : [t.w * 0.75, t.h * 0.5];
    await t.drag([[x0, y0], [x0 + 20, y0 + 20], [(x0 + x1) / 2, (y0 + y1) / 2], [x1, y1]]);
    await t.el('.og-text');
    await t.type('Words wrap to stay inside the box you drew, and get smaller when there are too many of them to fit.');
    await t.key('Enter', 2);
    await t.wait(async () => (await objects()).some(o => o.kind === 'textbox'), 'a text box is made');
    const o = (await objects()).find(o => o.kind === 'textbox');
    t.check(!!o.border, 'with a border');
    t.check(o.text.startsWith('Words wrap') && o.text.endsWith('fit.'), `the words kept as typed, unwrapped (${o.text})`);
    await t.shot('box');
  },
};
