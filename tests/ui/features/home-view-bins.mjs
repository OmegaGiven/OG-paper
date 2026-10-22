// The inventory's Views starts with Home: put it in a slot and a tap flies
// back to the page's home view. A tool dragged onto any bin goes — the
// inventory's 🗑 or a toolbar's own 🗑 in the toolbar list (the toolbar
// stays).
export default {
  name: 'home-view-bins',
  features: ['UI-18', 'UI-19'],
  sizes: ['desktop'],
  title: 'Home in Views, every bin takes a tool',
  async run(t) {
    await t.open();
    const z0 = (await t.state()).zoom;
    await t.zoom(2);
    const dragTo = async (from, to) => {
      const f = await t.find(from);
      const d = typeof to === 'string' ? await t.find(to) : to;
      const a = [f.x + f.w / 2, f.y + f.h / 2];
      const b = [d.x + d.w / 2, d.y + d.h / 2];
      await t.drag([a, [a[0] + 8, a[1] + 8], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], b], 900);
    };

    await t.caption('Views starts with Home');
    await t.tap('Toolbar 1');
    const views = (await t.nodes()).filter(n => n.w > 0 && /^View /.test(n.label || ''));
    t.check(views.length >= 1 && views[0].label === 'View Home', `Home is the first view (${views.map(v => v.label)})`);
    await t.shot('home-view');
    await dragTo('View Home', 'Add a slot');
    await t.find('Extra 1');
    t.check(await t.has('View Home'), 'Home stays in Views');
    await t.tap('×');
    await t.tap('Extra 1');
    await t.wait(async () => { const s = await t.state(); return !s.flying && Math.abs(s.zoom - z0) < 0.05; }, 'it flies home', 8000);
    t.check(true, 'the Home slot flies to the home view');

    await t.caption("Drag a slot onto a toolbar's own 🗑");
    await t.tap('Toolbar 1');
    await dragTo('Extra 1', 'Delete toolbar 1');
    await t.wait(async () => !(await t.has('Extra 1')), 'the slot is thrown away');
    t.check(await t.has('Delete toolbar 1'), 'the toolbar stays');
    await dragTo('Basic Eraser', 'Delete toolbar 1');
    await t.beat(300);
    t.check(await t.has('Basic Eraser'), 'a basic tool dropped on a bin stays in the basics');
    t.check(await t.has('View Home'), 'and Home stays in Views');
    await t.shot('thrown');
  },
};
