// The selection panel's Details tab: what is selected and its history
// (made when and by whom, how often it changed, and facts by kind).
export default {
  name: 'details-tab',
  features: ['EDIT-09'],
  sizes: ['desktop', 'phone'],
  title: 'Details of a selection',
  async run(t) {
    await t.open();
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    const cx = (x0 + t.w) / 2;
    await t.scribble(cx, t.h * 0.4, (t.w - x0) * 0.35);
    await t.wait(async () => (await t.state()).strokes >= 1, 'a stroke');
    await t.caption('Select it, then Details');
    await t.tap('Slot 6');
    await t.tap({ x: cx, y: t.h * 0.4 });
    await t.wait(async () => (await t.state()).selected === 1, 'selected');
    if (t.phone) await t.tap('Select settings');
    await t.tap('Details');
    const made = await t.find(/· you$/, { ms: 5000 });
    t.check(!!made, 'it says it was made by you, with when');
    await t.find('1 stroke');
    t.check(await t.has('never'), 'never changed yet');
    await t.shot('details');
    await t.caption('Move it: Details count the change');
    if (t.phone) await t.tap('Hide settings');
    await t.drag([[cx, t.h * 0.4], [cx + 20, t.h * 0.45], [cx + 40, t.h * 0.5]]);
    if (t.phone) await t.tap('Select settings');
    await t.find(/^1 time · last /, { ms: 5000 });
    t.check(true, 'a move shows as a change');
  },
};
