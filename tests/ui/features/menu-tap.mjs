// A tap on a menu item does what it says (it used to close the menu on
// phones: the tap reached the canvas first). Taps the way fingers do: a
// quick one (down and up in the same frame) and one that wobbles a little.
export default {
  name: 'menu-tap',
  features: ['UI-01'],
  sizes: ['phone'],
  title: 'Menu items work by tap',
  async run(t) {
    const finger = async (label, how) => {
      const n = await t.reveal(label);
      const x = n.x + n.w / 2, y = n.y + n.h / 2;
      await t.touch('touchStart', [{ x, y }]);
      if (how === 'wobble') {
        for (const [dx, dy] of [[1, 2], [3, 1], [2, 4]]) { await new Promise(r => setTimeout(r, 30)); await t.touch('touchMove', [{ x: x + dx, y: y + dy }]); }
        await new Promise(r => setTimeout(r, 60));
      }
      await t.touch('touchEnd', []);
      t.log(`${how} tap ${label}`);
    };
    for (const how of ['quick', 'wobble']) {
      await t.open();
      await t.tap('Menu');
      await finger('UI settings', how);
      await t.find('Dark mode', { ms: 3000 });
      t.check(true, `${how}: UI settings opened from the menu`);
      await finger('Dark mode', how);
      await t.wait(async () => (await t.state()).dark, `${how}: a tap on Dark mode turns it on`);
      t.check(await t.has('Dark mode'), `${how}: UI settings stays open after the tap`);
      await finger('Dark mode', how);
      await t.wait(async () => !(await t.state()).dark, `${how}: a second tap turns it off`);
      await t.tap('Menu');
      await finger('Search text', how);
      await t.el('.og-card[data-name="Search text"] input', 3000);
      t.check(true, `${how}: Search text opened its box`);
    }
    // The toolbar's own menu (with the inventory): its buttons work by tap too.
    await t.open();
    await t.tap('Toolbar 1');
    await finger('Save this view', 'quick');
    await finger('Save', 'quick');
    await t.wait(async () => (await t.state()).bookmarks.length === 1, 'Save this view in the toolbar menu saved a view');
    await t.shot('toolbar-menu');
  },
};
