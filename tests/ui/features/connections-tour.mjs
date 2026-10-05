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
    const signIn = async u => {
      await u.tap('Sign in');
      await u.tap(await u.find('User name'));
      await u.type('admin');
      await u.tap(await u.find('Password'));
      await u.type('password');
      await u.tap('Sign in');
      await u.find(/^Signed in as admin/, { ms: 8000 });
    };
    const openPages = async u => {
      if (!(await u.maybe('Workspaces', 1200))) { await u.tap('Menu'); await u.tap('Pages'); }
    };

    await t.open();
    await t.caption('A workspace is a shared home for pages');
    await openPages(t);
    await t.tap(await t.find(/^Workspace link/));
    await t.paste(hub.view);
    await t.tap('Add');
    await t.find('First page', { ms: 8000 });
    t.check(true, 'the workspace lists its pages');

    await t.caption('Sign in with your account to draw');
    await signIn(t);

    await t.caption('Open a page: you keep a copy, it syncs');
    await t.tap('Open', { row: 'First page' });
    await t.find(/^Live with First page$/, { ms: 10000 }).catch(() => {});
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    await t.scribble((x0 + t.w) / 2, t.h * 0.3, (t.w - x0) * 0.6);
    const mine = (await t.state()).strokes;

    await t.caption('A friend opens the same page...');
    const b2 = await launch(t.size === 'phone' ? 'phone' : 'desktop');
    const friend = new T(b2, { name: `${t.name}-friend`, demo: false, out: t.out });
    friend.log = () => {};
    await friend.open('/app/', { hash: `server=${encodeURIComponent(hub.view)}` });
    await openPages(friend);
    await friend.find('First page', { ms: 8000 });
    await signIn(friend);
    await friend.tap('Open', { row: 'First page' });
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
    await t.find('Workspaces', { ms: 8000 });
    t.check(true, 'the server link opened the workspace');
    await t.shot('workspace');
    hub.stop();
  },
};
