// On a phone, the toolbar list beside the quick bar keeps to the room the
// inventory leaves it: with more toolbars than fit it scrolls sideways,
// and the inventory stays on the screen. A press and hold on the quick
// bar's "+" opens its menu; a tap anywhere else closes it.
export default {
  name: 'toolbars-phone',
  features: ['UI-04', 'UI-19'],
  sizes: ['phone'],
  title: 'Many toolbars on a phone',
  async run(t) {
    await t.open();
    await t.tap('Toolbar 1');
    // The toolbar list's "+" (the leftmost one: the inventory has its own).
    const addBar = async () => {
      const plus = (await t.nodes()).filter(n => n.w > 0 && n.label === '+').sort((a, b) => a.x - b.x)[0];
      await t.tap(plus);
    };
    for (let i = 0; i < 4; i++) await addBar();
    await t.find('Toolbar 6');
    t.check(true, 'six toolbars');
    const inv = (await t.nodes()).filter(n => n.w > 0 && /^Inventory \d+$/.test(n.label || ''));
    const right = Math.max(...inv.map(n => n.x + n.w));
    t.check(inv.length > 0 && right <= t.w + 1, `the inventory stays on the screen (right edge ${Math.round(right)} of ${t.w})`);
    const cols = new Set(inv.map(n => Math.round(n.x))).size;
    t.check(cols >= 3, `three or more inventory columns (${cols})`);
    await t.shot('six-toolbars');
    await t.tap('×');

    await t.caption('Press and hold "+": its menu; tap elsewhere: it closes');
    const p = await t.find('Add a slot');
    const [px, py] = [p.x + p.w / 2, p.y + p.h / 2];
    await t.touch('touchStart', [{ x: px, y: py }]);
    await new Promise(r => setTimeout(r, 1200));
    await t.touch('touchEnd', []);
    await t.find('Add this view as a bookmark');
    await t.shot('menu');
    await t.tap({ x: t.w * 0.7, y: t.h * 0.3 });
    await t.wait(async () => !(await t.has('Add this view as a bookmark')), 'the menu closes');
    t.check(true, 'a tap away closes the menu');
  },
};
