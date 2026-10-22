// A tool tapped up in the inventory, to put in a slot, is let go with Esc
// or a tap on nothing (in the panel, or on the canvas): the inventory
// stays open, and the next Esc closes it as usual.
export default {
  name: 'cancel-held',
  features: ['UI-21'],
  sizes: ['desktop', 'phone'],
  title: 'Let go of a tool in hand',
  async run(t) {
    await t.open();
    const inHand = async () => (await t.state()).inHand;
    const open = () => t.has('Inventory 1');
    await t.tap('Toolbar 1');
    const pick = async () => {
      await t.tap('Basic Eraser');
      await t.wait(inHand, 'the eraser is in hand');
    };

    if (!t.phone) {
      await t.caption('Esc lets go of it; the inventory stays');
      await pick();
      await t.key('Escape');
      await t.wait(async () => !(await inHand()), 'Esc lets go of it');
      t.check(await open(), 'the inventory stays open');
    }

    await t.caption('A tap on nothing in the panel lets go of it');
    await pick();
    const inv = await t.find('Inventory 1');
    // Just above the grid, beside the title: nothing there.
    await t.tap({ x: inv.x + inv.w * 2.5, y: inv.y - 6 });
    await t.wait(async () => !(await inHand()), 'a tap on nothing lets go of it');
    t.check(await open(), 'the inventory stays open');

    await t.caption('So does a tap on the canvas');
    await pick();
    await t.tap({ x: t.w * 0.9, y: t.h * 0.2 });
    await t.wait(async () => !(await inHand()), 'a canvas tap lets go of it');
    t.check(await open(), 'the inventory stays open');
    await t.shot('let-go');

    if (!t.phone) {
      await t.key('Escape');
      await t.wait(async () => !(await open()), 'then Esc closes the inventory');
      t.check(true, 'the next Esc closes the inventory');
    }
  },
};
