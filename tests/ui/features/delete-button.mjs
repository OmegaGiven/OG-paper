// A selection has a delete button (a red ✕) off its top right corner,
// clear of the resize handles and the rotate knob; tapping it deletes the
// selection (undo brings it back).
export default {
  name: 'delete-button',
  features: ['EDIT-08'],
  sizes: ['desktop', 'phone'],
  title: 'Delete what is selected',
  async run(t) {
    await t.open();
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    const cx = (x0 + t.w) / 2;
    await t.scribble(cx, t.h * 0.45, (t.w - x0) * 0.35);
    await t.wait(async () => (await t.state()).strokes >= 1, 'a stroke');
    await t.caption('Select it: the ✕ sits off the top right corner');
    await t.tap('Slot 6');
    await t.tap({ x: cx, y: t.h * 0.45 });
    await t.wait(async () => (await t.state()).delKnob, 'the delete button shows');
    const [fx, fy] = (await t.state()).delKnob;
    const knob = { x: fx * t.w, y: fy * t.h };
    t.check(knob.x > 0 && knob.x < t.w && knob.y > 0 && knob.y < t.h, 'on screen');
    await t.shot('selected');
    await t.caption('Tap ✕: deleted');
    const before = (await t.state()).strokes;
    await t.tap(knob);
    await t.wait(async () => (await t.state()).strokes < before, 'the stroke is deleted');
    t.check((await t.state()).selected === 0 && !(await t.state()).delKnob, 'nothing selected, no button');
    await t.caption('Undo brings it back');
    await t.tap('Tools');
    await t.tap('Undo');
    await t.wait(async () => (await t.state()).strokes === before, 'undone');
    t.check(true, 'undo restores it');
  },
};
