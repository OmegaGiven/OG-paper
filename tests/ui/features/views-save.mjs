// The inventory's Views has a "+": it asks for a name and saves the
// current view under it, ready to drag onto a slot.
export default {
  name: 'views-save',
  features: ['UI-18'],
  sizes: ['desktop', 'phone'],
  title: 'Save a view from the inventory',
  async run(t) {
    await t.open();
    await t.zoom(2);
    await t.tap('Toolbar 1');
    await t.caption('Views › +: name it, save it');
    await t.tap('Save this view');
    const box = await t.find('View name');
    t.check(box.value === 'View 1', `a name to start from (${box.value})`);
    // It comes up focused with its name selected: typing replaces it.
    await t.type('Lake');
    await t.shot('naming');
    await t.key('Enter');
    await t.wait(async () => (await t.state()).bookmarks.length === 1, 'the view is saved');
    t.check((await t.state()).bookmarks[0].name === 'Lake', 'under the name given');
    await t.find('View Lake');
    t.check(!(await t.has('View name')), 'the name box closes');
    await t.shot('saved');
  },
};
