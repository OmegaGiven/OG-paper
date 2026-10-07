// Phones: the inventory ends in a fixed row of basic tools (every tool at
// its defaults) to assign from, even with nothing saved. A tap picks up a
// copy; a tap on a slot puts it there. The toolbar list shows the one hold
// slot on every toolbar's row.
export default {
  name: 'basic-tools',
  features: ['UI-04'],
  sizes: ['phone'],
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

    await t.caption('Tap the eraser, then slot 1 of the main toolbar');
    await t.tap('Basic Eraser');
    // The toolbar list shows every toolbar; the main one is the rightmost column.
    const slot1 = (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Slot 1')).sort((a, b) => b.x - a.x)[0];
    await t.tap(slot1);
    await t.shot('placed');
    await t.caption('Close, and slot 1 is the eraser');
    await t.tap('×');
    await t.tap('Slot 1');
    await t.wait(async () => (await t.state()).tool === 'Eraser', 'slot 1 holds the eraser');
    t.check(true, 'assigned from the basic tools');
  },
};
