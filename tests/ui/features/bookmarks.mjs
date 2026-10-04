// Save views, fly to one, fold the card into a bar and move it; Home on the bar.
export default {
  name: 'bookmarks',
  features: ['NAV-04', 'NAV-05', 'NAV-03', 'UI-12'],
  sizes: ['desktop', 'phone'],
  title: 'Bookmarks: save a view, fly back',
  async run(t) {
    await t.open();
    await t.scribble(t.w * 0.55, t.h * 0.42, t.w * 0.4);
    await t.caption('Menu › Bookmarks');
    await t.tap('Menu');
    await t.tap('Bookmarks');
    const card = '.og-card[data-name=Bookmarks]';
    const box = await t.el(`${card} input`);
    const r = await t.el(card);
    t.check(r.x >= 0 && r.x + r.w <= t.w + 1, 'the card fits on the screen');

    await t.caption('Name the view and save it');
    await t.tap(box);
    await t.type('Start');
    await t.tap(await t.el(`${card} form button`));
    await t.wait(async () => (await t.state()).bookmarks.length === 1, 'the bookmark is saved');
    await t.zoom(2);
    await t.tap(box);
    await t.type('Up close');
    await t.tap(await t.el(`${card} form button`));
    await t.wait(async () => (await t.state()).bookmarks.length === 2, 'the second bookmark is saved');
    await t.shot('saved');

    await t.caption('Tap a bookmark to fly back to it');
    await t.tap(await t.el(`${card} .og-marks li:first-child .go`));
    const zStart = (await t.state()).bookmarks[0].zoom;
    await t.wait(async () => { const s = await t.state(); return !s.flying && Math.abs(s.zoom - zStart) < 0.05; }, 'it flies to "Start"', 8000);
    // Small screens close the card after flying: open it again.
    if (await t.b.eval(`document.querySelector('${card}').hidden`)) { await t.tap('Menu'); await t.tap('Bookmarks'); }

    await t.caption('Fold it into a bar: Home first, then each bookmark');
    await t.tap(await t.el(`${card} .fold`));
    const chips = await t.b.eval(`[...document.querySelectorAll('${card} .og-chips button')].map(b => b.textContent.trim())`);
    t.check(chips[0] === 'Home' && chips[1] === 'Star' && chips[2] === 'UC', `the bar shows ${chips.join(' · ')}`);
    await t.shot('bar');

    await t.caption('Drag the bar anywhere');
    const grip = await t.el(`${card} h2 > span:first-child`);
    await t.drag([[grip.x + grip.w / 2, grip.y + grip.h / 2], [t.w * 0.3, t.h * 0.75], [24, t.h * 0.8]], 700);
    const moved = await t.el(card);
    t.check(moved.y > t.h * 0.6 && moved.x >= 0, 'the bar moved and stays on screen');
    const saved = await t.b.eval(`localStorage.getItem('og-card-pos:Bookmarks')`);
    t.check(!!saved, 'its place is remembered');

    await t.caption('Zoom in, then ⌂ Home on the bar');
    await t.zoom(2);
    await t.tap(await t.el(`${card} .og-chips button[data-home]`));
    await t.wait(async () => { const s = await t.state(); return !s.flying && Math.abs(s.zoom) < 0.05; }, 'home again', 8000);
    t.check(true, 'Home on the bar flies home');
    await t.shot('home');
  },
};
