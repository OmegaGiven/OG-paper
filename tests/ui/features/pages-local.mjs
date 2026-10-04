// Make a named page on this device, then rename it.
export default {
  name: 'pages-local',
  features: ['FILE-01'],
  sizes: ['desktop', 'phone'],
  title: 'Pages on this device',
  async run(t) {
    await t.open();
    await t.caption('Menu › Pages lists the pages on this device');
    await t.tap('Menu');
    await t.tap('Pages');
    await t.caption('Make a new page and name it');
    await t.tap('+ New page');
    await t.find('Name the new page');
    await t.type('Sketches');
    await t.key('Enter');
    await t.wait(async () => (await t.state()).name === 'Sketches', 'the new page is named Sketches', 6000);
    t.check(true, 'made the page "Sketches"');
    await t.shot('made');

    await t.caption('Rename it');
    if (!(await t.has('Rename'))) { await t.tap('Menu'); await t.tap('Pages'); }
    await t.tap('Rename');
    for (let i = 0; i < 12; i++) await t.key('Backspace');
    await t.type('Ideas');
    await t.key('Enter');
    await t.wait(async () => (await t.state()).name === 'Ideas', 'the page is renamed Ideas', 6000);
    t.check(true, 'renamed to "Ideas"');
    await t.shot('renamed');
  },
};
