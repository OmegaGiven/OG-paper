// The inventory ends in a row of basic tools (every tool at its defaults)
// to assign from, even with nothing saved, on every device. It folds away
// to its title and remembers that. Drag one onto a slot, or (phones) tap
// it to pick up a copy and tap a slot to put it there. The toolbar list
// shows the one hold slot on every toolbar's row.
export default {
  name: 'basic-tools',
  features: ['UI-04'],
  sizes: ['phone', 'desktop'],
  title: 'Basic tools in the inventory',
  async run(t) {
    await t.open();
    await t.caption('Open the toolbars and inventory');
    await t.tap('Toolbar 1');
    await t.find('Basic Eraser');
    t.check(true, 'the basic tools row is there');
    const holds = (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Hold slot')).length;
    t.check(holds >= 1, 'the hold slot shows in the toolbar list');
    await t.shot('basics');

    await t.caption('Fold the basic tools away, and back');
    await t.tap(/^⏷ Basic tools/);
    t.check(!(await t.maybe('Basic Eraser')), 'folded: the tools are hidden');
    await t.shot('folded');
    await t.tap(/^⏵ Basic tools/);
    await t.find('Basic Eraser');
    t.check(true, 'unfolded again');

    // The main toolbar's slot 1: on a phone the toolbar list's rightmost
    // column; on a desktop the quick bar along the bottom.
    const slot1 = async () => (await t.nodes())
      .filter(n => n.w > 0 && [n.label, n.value].includes('Slot 1'))
      .sort((a, b) => (t.phone ? b.x - a.x : b.y - a.y))[0];
    if (t.phone) {
      await t.caption('Tap the eraser, then slot 1 of the main toolbar');
      await t.tap('Basic Eraser');
      await t.tap(await slot1());
    } else {
      await t.caption('Drag the eraser onto slot 1 of the main toolbar');
      const from = await t.find('Basic Eraser');
      const to = await slot1();
      const a = [from.x + from.w / 2, from.y + from.h / 2];
      const b = [to.x + to.w / 2, to.y + to.h / 2];
      await t.drag([a, [a[0] + 8, a[1] + 8], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], b], 900);
      await t.find('Basic Eraser');
      t.check(true, 'the basic eraser stays in its row');
    }
    await t.shot('placed');
    await t.caption('Close, and slot 1 is the eraser');
    await t.tap('×');
    await t.tap('Slot 1');
    await t.wait(async () => (await t.state()).tool === 'Eraser', 'slot 1 holds the eraser');
    t.check(true, 'assigned from the basic tools');
  },
};
