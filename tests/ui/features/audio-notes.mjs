// The Audio notes plugin: install it from the app's own list and its tool,
// Voice note, joins the tool menu. Pick it, tap the canvas and the note is
// recorded right there (the app records, with a bar to stop), then played
// back from the canvas. Recording is refused until the microphone is
// allowed. (The test browser has a fake microphone and no sound output.)
export default {
  name: 'audio-notes',
  features: ['PLUG-04'],
  sizes: ['desktop', 'phone'],
  title: 'Voice notes',
  async run(t) {
    await t.open();
    // Something named Plugins outside the menu (a message at the top that
    // mentions it) goes first, so taps find the menu's item.
    const named = async () => (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Plugins')).length;
    const plugins = async () => {
      await t.wait(async () => (await named()) === 0, 'messages gone', 8000);
      await t.tap('Menu');
      const m = await t.find('Search text');
      // A desktop menu scrolls under the wheel; a phone's by dragging.
      if (!t.phone) {
        for (let i = 0; i < 4 && !(await t.has('Plugins').catch(() => false)); i++) await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 500);
        await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 800);
      }
      await t.tap('Plugins');
    };
    const recording = () => t.b.eval(`!!document.querySelector('.og-rec')`);
    // Where the note goes: right of the middle.
    const at = { x: t.w * 0.62, y: t.h * 0.4 };

    await t.caption('Menu › Plugins › Install Audio notes');
    await plugins();
    t.check(!(await t.maybe('Install Starter kit', 300)), 'only plugins worth having come with the app');
    await t.tap('Install Audio notes');
    await t.tap('×');

    await t.caption('Its Voice note tool is in the tool menu');
    await t.tap('Tools');
    await t.find('Voice note');
    await t.shot('fan');
    await t.tap('Voice note');
    await t.shot('armed');
    await t.tap(at);
    await t.beat(500);
    t.check(!(await recording()), 'no recording before the microphone is allowed');

    await t.caption('Allow the microphone, then tap where the note goes');
    await plugins();
    await t.tap('Allow');
    await t.tap('×');
    await t.tap('Tools');
    await t.tap('Voice note');
    await t.tap(at);
    await t.wait(recording, 'the recording bar shows', 8000);
    await t.shot('recording');
    await t.wait(async () => (await t.b.eval(`document.querySelector('.og-rec .t')?.textContent`)) === '0:02', 'two seconds recorded', 8000);
    await t.tap(await t.el('.og-rec .stop'));
    await t.wait(async () => (await t.state()).clips === 1, 'a clip is on the canvas', 8000);
    t.check(true, 'recorded a clip where it was tapped');

    await t.caption('Tap its play button');
    // The clip is centred where it was tapped; its play button at its left.
    const w = Math.min(t.w, t.h) * 0.3;
    await t.tap({ x: at.x - w / 2 + w * 0.12, y: at.y });
    await t.wait(async () => !!(await t.b.eval('window.ogAudioPlaying')), 'it plays', 5000);
    t.check(true, 'played back');
    await t.shot('clip');
  },
};
