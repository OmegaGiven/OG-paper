// Bookmark a moment on the timeline, carry on drawing, and fly back to the
// canvas as it was then.
export default {
  name: 'timeline-bookmark',
  features: ['NAV-07', 'NAV-04'],
  sizes: ['desktop', 'phone'],
  title: 'Bookmark a moment in time',
  async run(t) {
    await t.open();
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    const cx = (x0 + t.w) / 2, span = (t.w - x0) * 0.6;
    await t.caption('Draw something...');
    await t.scribble(cx, t.h * 0.25, span);
    await t.beat(1200);
    await t.scribble(cx, t.h * 0.4, span);
    await t.wait(async () => (await t.state()).timeline?.n >= 2, 'two changes on the timeline');

    await t.caption('Menu › Timeline: go back to the first line');
    await t.tap('Menu');
    await t.tap('Timeline');
    await t.el('.og-tl .mark');
    await t.b.eval(`(() => { const r = document.querySelector('.og-tl input.hi'); r.value = 0; r.dispatchEvent(new Event('input')); })()`);
    await t.wait(async () => (await t.state()).timeline?.i === 0, 'showing the first change');
    await t.caption('Bookmark this moment');
    t.b.promptText = 'First line';
    await t.tap(await t.el('.og-tl .mark'));
    await t.wait(async () => (await t.state()).bookmarks.some(b => b.name === 'First line' && b.at != null), 'a bookmark with its moment');
    t.check(true, 'the bookmark keeps the moment');

    await t.caption('Back to now, and keep drawing');
    await t.tap(await t.el('.og-tl .close'));
    await t.wait(async () => !(await t.state()).timeline?.on, 'back to now');
    await t.scribble(cx, t.h * 0.55, span);

    await t.caption('Later: Bookmarks › First line');
    await t.tap('Menu');
    await t.tap('Bookmarks');
    const card = '.og-card[data-name=Bookmarks]';
    const item = await t.el(`${card} .og-marks li:first-child .go`);
    t.check(/as it was on/.test(item.label), 'the list says it is a moment');
    await t.tap(item);
    await t.wait(async () => { const s = await t.state(); return s.timeline?.on && s.timeline.i === 0; }, 'the canvas as it was then', 8000);
    await t.el('.og-tl .mark');
    t.check(true, 'back at that moment, with the timeline open');
    await t.shot('moment');
  },
};
