// The ways to draw with other people: a workspace (a shared home for
// pages: add it, sign in, open a page), someone joining you live, and the
// Join box taking a page link or a server link.
import { startHub, launch, T } from '../lib.mjs';

export default {
  name: 'connections-tour',
  features: ['WS-01', 'WS-02', 'WS-03', 'LIVE-01', 'LIVE-05', 'LIVE-06'],
  sizes: ['desktop', 'phone'],
  title: 'OG Paper: drawing together',
  async run(t) {
    const hub = await startHub();
    await t.open();
    await t.caption('Menu › Connect to server: its address and your account');
    await t.connect(hub.view);
    await t.find('First page', { ms: 8000 });
    t.check(true, 'connected: Pages shows its pages');

    await t.caption('Open a page: you keep a copy, it syncs');
    await t.rowAction('First page', 'Open');
    await t.find(/^Live with First page$/, { ms: 10000 }).catch(() => {});
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    await t.scribble((x0 + t.w) / 2, t.h * 0.3, (t.w - x0) * 0.6);
    const mine = (await t.state()).strokes;

    await t.caption('A friend opens the same page...');
    const b2 = await launch(t.size === 'phone' ? 'phone' : 'desktop');
    const friend = new T(b2, { name: `${t.name}-friend`, demo: false, out: t.out });
    friend.log = () => {};
    await friend.open();
    await friend.connect(hub.view);
    await friend.find('First page', { ms: 8000 });
    await friend.rowAction('First page', 'Open');
    await friend.wait(async () => (await friend.state()).strokes >= mine, 'the friend gets my drawing', 10000);
    t.check(true, 'the friend sees what I drew');
    await t.caption('...and draws with you, live');
    await friend.drag(Array.from({ length: 14 }, (_, i) => [friend.w * (0.4 + 0.04 * i), friend.h * (0.55 + 0.06 * Math.sin(i / 2))]), 900);
    await t.wait(async () => (await t.state()).strokes > mine, 'their stroke arrives here', 10000);
    t.check(true, 'their stroke arrived live');
    await t.beat(1500);
    await t.shot('together');
    await b2.close();

    await t.caption('Got a link to one page? Share live › Join');
    const link = await hub.pageLink('First page');
    await t.tap('Menu');
    await t.tap('Share live');
    if (await t.maybe('Leave', 600)) await t.tap('Leave');
    await t.tap(await t.find(/^Paste a link/));
    await t.paste(link);
    await t.tap('Join');
    await t.find(/^Live with First page/, { ms: 10000 });
    t.check(true, 'joined by the page link');

    await t.caption('A server link in Join adds its workspace');
    await t.tap('Leave');
    await t.tap(await t.find(/^Paste a link/));
    await t.paste(hub.view);
    await t.tap('Join');
    await t.find('First page', { ms: 8000 });
    t.check(true, 'the server link opened its pages');
    await t.shot('workspace');
    hub.stop();
  },
};
