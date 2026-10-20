// App Store screenshots for iPhone (6.9") and iPad (13"): the scenes in
// store-scenes.mjs with a caption, saved to out/store/<size>/.
import { mkdirSync, writeFileSync } from 'fs';
import { caption, diagram, design, sketch } from '../store-scenes.mjs';

export default {
  name: 'store-ios',
  features: [],
  demoOnly: true,
  sizes: ['iphone-shot', 'ipad-shot'],
  title: 'App Store screenshots',
  async run(t) {
    const dir = `out/store/${t.size}`;
    mkdirSync(dir, { recursive: true });
    const save = async name => {
      await t.beat(700);
      const s = await t.b.send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(`${dir}/${name}.png`, Buffer.from(s.data, 'base64'));
      console.log('saved', `${dir}/${name}.png`);
    };
    const fresh = async () => {
      await t.open();
      if (await t.has('Hide settings')) await t.tap('Hide settings');
    };

    await fresh();
    await diagram(t);
    await caption(t, 'Diagrams that stay connected');
    await save('02-diagrams');

    await fresh();
    await design(t);
    await caption(t, 'Gradients, curves, words on a path');
    await save('03-design');

    await fresh();
    await sketch(t);
    await t.tap('Toolbar 1');
    await caption(t, 'Your tools, kept as you set them');
    await save('05-tools');
  },
};
