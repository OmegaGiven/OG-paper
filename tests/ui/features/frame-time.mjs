// UI settings › Show frame time: a readout of frames per second and how
// long each frame takes, for reporting slowness on a device.
export default {
  name: 'frame-time',
  features: ['UI-17'],
  sizes: ['desktop', 'phone'],
  title: 'Frame time readout',
  async run(t) {
    await t.open();
    await t.tap('Menu');
    // The menu scrolls: bring its App section up.
    const m = await t.find('Search text');
    for (let i = 0; i < 4 && !(await t.has('UI settings').catch(() => false)); i++) await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 500);
    await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 800);
    await t.tap('UI settings');
    await t.tap('Show frame time');
    const r = await t.find(/fps · .* ms a frame/, { ms: 5000 });
    t.check(/\d+ fps/.test(r.label || r.value), 'the readout shows frames per second');
    await t.shot('readout');
  },
};
