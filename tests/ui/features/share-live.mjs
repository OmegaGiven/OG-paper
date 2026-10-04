// Paste a page's link into Share live's box (the phone keyboard comes up
// for it) and join; drawing goes to the page server.
import { startHub } from '../lib.mjs';

export default {
  name: 'share-live',
  features: ['LIVE-01', 'UI-13', 'UI-14'],
  sizes: ['desktop', 'phone'],
  title: 'Join a shared page by its link',
  async run(t) {
    const hub = await startHub();
    const link = await hub.pageLink('First page');
    await t.open();
    await t.caption('Menu › Share live');
    await t.tap('Menu');
    await t.tap('Share live');
    const box = await t.find(/^Paste a link/);
    await t.tap(box);
    if (t.phone) {
      const focus = await t.b.eval(`document.activeElement?.getAttribute('enterkeyhint')`);
      t.check(focus === 'done', 'the phone keyboard comes up for the box');
    }
    await t.caption('Paste the link: it goes in the box, not on the page');
    const before = (await t.state()).strokes;
    await t.paste(link);
    const filled = await t.find(/^Paste a link/);
    t.check(filled.value === link, 'the link is in the box');
    t.check((await t.state()).strokes === before, 'nothing was pasted onto the page');
    await t.shot('pasted');
    await t.caption('Join');
    await t.tap('Join');
    await t.find('Live with First page', { ms: 8000 });
    t.check(true, 'joined the page on the server');
    await t.shot('joined');
    hub.stop();
  },
};
