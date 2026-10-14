// The inventory's Views: the canvas's saved views, to put in any slot and
// fly there with a tap. The quick bar's "+": drop a tool (or a view) on it
// for another slot past the toolbar, picked by tapping (it has no key);
// drag it onto the bin to take it away again.
export default {
  name: 'quickbar-extras',
  features: ['UI-18', 'UI-19'],
  sizes: ['desktop', 'phone'],
  title: 'Views in the inventory, extra quick bar slots',
  async run(t) {
    await t.open();
    await t.scribble(t.w * 0.55, t.h * 0.42, t.w * 0.3);
    await t.caption('Save a view: Menu › Views');
    await t.tap('Menu');
    await t.tap('Views');
    const card = '.og-card[data-name=Views]';
    await t.tap(await t.el(`${card} input`));
    await t.type('Start');
    await t.tap(await t.el(`${card} form button`));
    await t.wait(async () => (await t.state()).bookmarks.length === 1, 'the view is saved');
    const z0 = (await t.state()).zoom;
    if (!(await t.b.eval(`document.querySelector('${card}').hidden`))) await t.tap('Menu');
    await t.zoom(2);

    // Put a slot's worth of thing on "+" (drag it; on a phone, tap it then "+").
    const onPlus = async name => {
      if (t.phone) {
        await t.tap(name);
        await t.tap('Add a slot');
        return;
      }
      const from = await t.find(name);
      const to = await t.find('Add a slot');
      const a = [from.x + from.w / 2, from.y + from.h / 2];
      const b = [to.x + to.w / 2, to.y + to.h / 2];
      await t.drag([a, [a[0] + 8, a[1] + 8], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], b], 900);
    };

    await t.caption('The inventory lists the saved views');
    await t.tap('Toolbar 1');
    await t.find('View Start');
    await t.shot('views');
    await t.caption('Drop the view, then the eraser, on "+" in the quick bar');
    await onPlus('View Start');
    await t.find('Extra 1');
    await onPlus('Basic Eraser');
    await t.find('Extra 2');
    t.check(true, 'two extra slots past the toolbar');
    await t.find('View Start');
    t.check(true, 'the view stays in the inventory');
    await t.shot('extras');
    await t.tap('×');

    await t.caption('Tap them: the eraser, then fly back to the view');
    await t.tap('Extra 2');
    await t.wait(async () => (await t.state()).tool === 'Eraser', 'the eraser is in hand');
    await t.tap('Extra 1');
    await t.wait(async () => { const s = await t.state(); return !s.flying && Math.abs(s.zoom - z0) < 0.05; }, 'it flies back to Start', 8000);
    t.check(true, 'an extra slot flies to its view');

    await t.caption('Drag an extra slot onto the bin to take it away');
    await t.tap('Toolbar 1');
    const x = await t.find('Extra 2');
    const bin = (await t.nodes()).find(n => n.w > 0 && n.label === '🗑');
    const a = [x.x + x.w / 2, x.y + x.h / 2];
    const b = [bin.x + bin.w / 2, bin.y + bin.h / 2];
    await t.drag([a, [a[0] + 8, a[1] - 8], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], b], 900);
    await t.wait(async () => !(await t.has('Extra 2')), 'the slot is gone');
    t.check(await t.has('Extra 1'), 'the other one stays');
    await t.shot('removed');
  },
};
