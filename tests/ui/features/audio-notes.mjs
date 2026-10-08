// The Audio notes plugin: install it from the app's own list, allow the
// microphone it asks for, record a note (the app records, with a bar to
// stop) and play it back from the canvas. Recording is refused until the
// microphone is allowed. (The test browser has a fake microphone and no
// sound output.)
export default {
  name: 'audio-notes',
  features: ['PLUG-04'],
  sizes: ['desktop', 'phone'],
  title: 'Audio notes',
  async run(t) {
    await t.open();
    await t.caption('Menu › Plugins › Install Audio notes');
    await t.tap('Menu');
    const m = await t.find('Search text');
    for (let i = 0; i < 4 && !(await t.has('Plugins').catch(() => false)); i++) await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 500);
    await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 800);
    await t.tap('Plugins');
    await t.tap('Install Audio notes');
    await t.find('Record a note (up to 30 s)');
    t.check(true, 'installed');

    await t.caption('Not allowed yet: no recording');
    await t.tap('Record a note (up to 30 s)');
    await t.beat(500);
    t.check(!(await t.b.eval(`!!document.querySelector('.og-rec')`)), 'no recording before the microphone is allowed');

    await t.caption('Allow the microphone, then record');
    await t.tap('Allow');
    await t.tap('Record a note (up to 30 s)');
    await t.wait(async () => t.b.eval(`!!document.querySelector('.og-rec')`), 'the recording bar shows', 8000);
    await t.shot('recording');
    await t.wait(async () => (await t.b.eval(`document.querySelector('.og-rec .t')?.textContent`)) === '0:02', 'two seconds recorded', 8000);
    await t.tap(await t.el('.og-rec .stop'));
    await t.wait(async () => (await t.state()).clips === 1, 'a clip is on the canvas', 8000);
    t.check(true, 'recorded a clip');
    await t.tap('×');

    await t.caption('Tap its play button');
    // The clip sits in the middle of the screen; its play button at its left.
    const w = Math.min(t.w, t.h) * 0.45;
    await t.tap({ x: t.w / 2 - w / 2 + w * 0.12, y: t.h / 2 });
    await t.wait(async () => !!(await t.b.eval('window.ogAudioPlaying')), 'it plays', 5000);
    t.check(true, 'played back');
    await t.shot('clip');
  },
};
