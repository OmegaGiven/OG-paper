// The tool panel window's corner grip: dragged anywhere (taller, shorter,
// wider, narrower, near the screen's edges), the grip stays under the
// pointer and the window fills exactly the size it was dragged to.
export default {
  name: 'panel-resize',
  features: ['UI-16'],
  sizes: ['desktop'],
  title: 'Resize the tool panel',
  async run(t) {
    await t.open();
    await t.find('Width');
    const grip = () => t.find('Resize settings');
    const title = () => t.find(/^(Brush|Shapes|Text|Highlighter)$/);
    const misses = [];
    // Drag the grip by (dx, dy); where it ends vs where the pointer let go.
    const resize = async (dx, dy, label) => {
      const g = await grip();
      const [x, y] = [g.x + g.w / 2, g.y + g.h / 2];
      const to = [x + dx, y + dy];
      await t.drag([[x, y], [x + dx / 3, y + dy / 3], [x + (2 * dx) / 3, y + (2 * dy) / 3], to], 600);
      await t.beat(250);
      const a = await grip();
      const off = [Math.round(a.x + a.w / 2 - to[0]), Math.round(a.y + a.h / 2 - to[1])];
      const tt = await title();
      console.log(`[resize] ${label}: grip off by ${off} · title at ${Math.round(tt.x)},${Math.round(tt.y)}`);
      // Clamped at the screen or the smallest size is fine; anything else should track.
      const clamped = to[0] > t.w - 20 || to[1] > t.h - 20 || to[1] < 120;
      if (!clamped && (Math.abs(off[0]) > 12 || Math.abs(off[1]) > 12)) misses.push(`${label}: ${off}`);
      await t.shot(label.replace(/\W+/g, '-'));
      return tt;
    };
    const t0 = await title();
    await resize(0, -150, 'shorter');
    await resize(0, -150, 'much-shorter');
    await resize(0, 200, 'taller');
    await resize(200, 0, 'wider');
    await resize(-120, -60, 'narrower-shorter');
    await resize(250, 120, 'wider-taller');
    const t1 = await title();
    t.check(Math.abs(t1.x - t0.x) < 4 && Math.abs(t1.y - t0.y) < 4, 'the top left stays put while resizing');
    // Move it near the top, then resize down past the screen.
    const tt = await title();
    await t.drag([[tt.x + tt.w / 2, tt.y + tt.h / 2], [tt.x + 200, tt.y - 60], [tt.x + 400, 30]]);
    await t.beat(300);
    await resize(0, 260, 'moved-taller');
    await resize(0, -320, 'moved-shorter');
    // Smallest size, then other tools' settings in it.
    await resize(-400, -500, 'smallest');
    for (const slot of ['Slot 8', 'Slot 9', 'Slot 3']) {
      await t.tap(slot);
      await t.beat(300);
      const g = await grip(), tt2 = await title();
      t.check(g.y > tt2.y + 100, `${slot}: the panel keeps its size (grip below the title)`);
      await t.shot(`small-${slot.replace(' ', '')}`);
    }
    await t.tap('Slot 1');
    // Near the right and bottom edges: growing past them keeps the window
    // where it is (the size stops at the edge).
    const g2 = await grip(), tt3 = await title();
    await t.drag([[tt3.x + tt3.w / 2, tt3.y + tt3.h / 2], [t.w * 0.6, t.h * 0.5], [t.w * 0.62, t.h * 0.55]]);
    await t.beat(300);
    const before = await title();
    await resize(600, 600, 'past-the-edges');
    const after = await title();
    t.check(Math.abs(after.x - before.x) < 4 && Math.abs(after.y - before.y) < 4, 'growing past the edges does not shove the window');
    const g3 = await grip();
    t.check(g3.x + g3.w <= t.w + 1 && g3.y + g3.h <= t.h + 1, 'the grip stays on screen');
    t.check(misses.length === 0, `the grip follows the pointer (${misses.join('; ') || 'all within 12px'})`);
  },
};
