// Others on a live page show as a strip of badges (right side in
// landscape, across the top in portrait). Tap one to fly to their view;
// right click (press and hold) to follow them, and the view rides along
// as they move until you stop.
import { startHub, launch, T } from '../lib.mjs';

export default {
  name: 'people-strip',
  features: ['LIVE-08'],
  sizes: ['desktop', 'phone'],
  title: 'See who is here, go to them, follow them',
  async run(t) {
    const hub = await startHub();
    const link = await hub.pageLink('First page');
    const join = async (u, name) => {
      await u.open('/app/', { prefs: `hints=off\nname=${name}\n` });
      await u.tap('Menu');
      await u.tap('Share live');
      await u.tap(await u.find(/^Paste a link/));
      await u.paste(link);
      await u.tap('Join');
      await u.find('Live with First page', { ms: 8000 });
      await u.tap('Menu');
    };
    // Someone else, in their own browser.
    const b2 = await launch('desktop');
    const sam = new T(b2, { name: 'people-strip-sam', demo: false, out: t.out });
    try {
      await join(sam, 'Sam Rivera');
      await t.caption('Join the page: Sam is already there');
      await join(t, 'Omega');
      const badge = await t.find('Person Sam Rivera', { ms: 8000 });
      t.check(true, 'Sam has a badge');
      t.check(t.w > t.h ? badge.x > t.w * 0.8 : badge.y < t.h * 0.2,
        t.w > t.h ? 'down the right side' : 'across the top');
      await t.shot('strip');

      await t.caption('Sam zooms in somewhere; tap the badge to go there');
      await sam.zoom(3);
      const zSam = async () => (await sam.state()).zoom;
      await t.tap('Person Sam Rivera');
      await t.wait(async () => { const s = await t.state(); return !s.flying && Math.abs(s.zoom - await zSam()) < 0.1; }, 'flies to Sam\'s view', 10000);
      t.check(true, 'went to Sam\'s view');

      await t.caption('Right click (press and hold): Follow');
      const b = await t.find('Person Sam Rivera');
      const [x, y] = [b.x + b.w / 2, b.y + b.h / 2];
      if (t.phone) {
        await t.touch('touchStart', [{ x, y }]);
        await new Promise(r => setTimeout(r, 1200));
        await t.touch('touchEnd', []);
      } else {
        await t.b.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'right', buttons: 2, clickCount: 1 });
        await t.b.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'right', buttons: 0, clickCount: 1 });
      }
      await t.tap('Follow');
      await sam.zoom(-2);
      await t.wait(async () => Math.abs((await t.state()).zoom - await zSam()) < 0.1, 'the view rides along with Sam', 10000);
      t.check(true, 'following Sam');
      await t.shot('following');
    } finally {
      await b2.close();
      hub.stop();
    }
  },
};
