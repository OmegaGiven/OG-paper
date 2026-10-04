// The menu (gear, top right) opens with its sections and items, Home among them.
export default {
  name: 'menu',
  features: ['UI-01', 'NAV-03'],
  sizes: ['desktop', 'phone'],
  title: 'Everything is in the menu',
  async run(t) {
    await t.open();
    await t.caption('The gear (top right) opens the menu');
    await t.tap('Menu');
    for (const item of ['New canvas', 'Pages', 'Export', 'Paste', 'Insert picture / PDF', 'Search text', 'Bookmarks', 'Home', 'Share live', 'UI', 'Plugins']) {
      await t.find(item, { ms: 2000 });
      t.check(true, `the menu has ${item}`);
    }
    await t.shot('menu');
    await t.caption('Tap the gear again to close it');
    await t.tap('Menu');
    await t.wait(async () => !(await t.has('Bookmarks')), 'the menu closes');
  },
};
