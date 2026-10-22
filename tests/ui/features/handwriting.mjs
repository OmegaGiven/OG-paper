// The Write tool: write words, and a moment after you stop they become a
// text where the ink was (undo brings the ink back). On the web the
// browser's own handwriting recognizer reads it; here a stand-in recognizer
// answers "Hello", so the whole path runs. Without one, the tool says it
// can't read here and leaves the ink.
export default {
  name: 'handwriting',
  features: ['DRAW-18'],
  sizes: ['desktop', 'phone'],
  title: 'Write: handwriting becomes text',
  async run(t) {
    await t.open();
    const run = async cmds => {
      await t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
      await t.beat(250);
      return t.b.eval('window.ogPaper.results()');
    };
    const objects = async () => (await run([{ get: 'objects' }]))[0].objects;
    const scribble = async () => {
      const [x, y] = [t.w * 0.5, t.h * 0.35];
      const pts = [];
      for (let i = 0; i <= 30; i++) pts.push([x + i * 6, y + Math.sin(i / 2) * 18]);
      await t.drag(pts, 600);
    };
    await t.caption('Tools › Write');
    await t.tap('Tools');
    await t.tap('Write');
    await t.wait(async () => (await t.state()).tool === 'Write', 'the Write tool is in hand');

    await t.caption('No recognizer here: the ink stays');
    const s0 = (await t.state()).strokes;
    await scribble();
    await t.beat(2200);
    t.check((await t.state()).strokes === s0 + 1, 'the ink stays');
    t.check((await objects()).every(o => o.kind !== 'text'), 'no text made');

    await t.caption('With a recognizer: the writing becomes a text');
    await t.b.eval(`(async () => {
      window.HandwritingStroke = class { constructor() { this.p = []; } addPoint(p) { this.p.push(p); } };
      navigator.createHandwritingRecognizer = async () => ({
        startDrawing: () => ({ n: 0, addStroke() { this.n++; }, async getPrediction() { return this.n ? [{ text: 'Hello' }] : []; }, clear() {} }),
        finish() {},
      });
      const m = await import('${'/app/pkg/og_paper.js'}');
      m.og_handwrite_support(true);
    })()`);
    await scribble();
    await t.wait(async () => (await objects()).some(o => o.kind === 'text' && o.text === 'Hello'), 'the writing becomes "Hello"', 6000);
    await t.shot('text');
    await t.tap('Tools');
    await t.tap('Undo');
    await t.wait(async () => (await objects()).every(o => o.kind !== 'text'), 'undo brings the ink back');
    t.check((await t.state()).strokes === s0 + 2, `both scribbles are there (${(await t.state()).strokes})`);
  },
};
