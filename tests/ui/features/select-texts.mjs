// Dragging a box over texts on a filled background picks the texts, not
// the fill behind them (a fill is taken only when wholly inside). Copying
// several texts puts them on the clipboard as plain text for other apps
// (top to bottom), while pasting back into OG Paper still brings the
// objects themselves.
export default {
  name: 'select-texts',
  features: ['EDIT-10'],
  sizes: ['desktop'],
  title: 'Select and copy texts',
  async run(t) {
    await t.open();
    const run = cmds => t.b.eval(`window.ogPaper.run(${JSON.stringify(cmds)})`);
    // A big box (in home-view points around the middle), filled with the bucket.
    await run([{ add: 'shape', kind: 'rect', x: -220, y: -150, w: 440, h: 300, color: '#333333', width: 3 }]);
    await t.beat(300);
    await t.tap('Slot 4');
    await t.tap({ x: t.w / 2 + 150, y: t.h / 2 + 100 });
    await t.wait(async () => (await t.state()).strokes >= 2, 'the box is filled');
    await run([
      { add: 'text', text: 'First line', x: -120, y: -60, size: 24 },
      { add: 'text', text: 'Second line', x: -120, y: 0, size: 24 },
    ]);
    await t.beat(300);
    await t.caption('Drag a box over the two texts');
    await t.tap('Slot 6');
    await t.drag([[t.w / 2 - 140, t.h / 2 - 80], [t.w / 2 + 60, t.h / 2 - 10], [t.w / 2 + 80, t.h / 2 + 50]]);
    await t.wait(async () => (await t.state()).selected > 0, 'something selected');
    const n = (await t.state()).selected;
    t.check(n === 2, `just the two texts are selected (${n})`);
    await t.shot('selected');

    await t.caption('Copy: plain text for other apps');
    const got = await t.b.eval(`(() => {
      const dt = new DataTransfer();
      document.querySelector('canvas').dispatchEvent(new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true }));
      return { text: dt.getData('text/plain'), html: dt.getData('text/html') };
    })()`);
    t.check(got.text === 'First line\nSecond line', `the texts, top first (${JSON.stringify(got.text)})`);
    t.check(got.html.includes('og-paper-clip'), 'marked as this app\'s copy');
  },
};
