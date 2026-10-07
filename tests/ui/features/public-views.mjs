// A view made public reaches everyone on the page, with its author's name:
// they can fly to it or keep a copy. The page server keeps it for people
// who come later; made private again, it goes away for the others.
import { startHub, launch, T } from '../lib.mjs';

export default {
  name: 'public-views',
  features: ['NAV-10'],
  sizes: ['desktop'],
  title: 'Public views',
  async run(t) {
    const hub = await startHub();
    await t.open();
    await t.connect(hub.view);
    await t.rowAction('First page', 'Open');
    await t.wait(async () => /Live/i.test(JSON.stringify((await t.state()).net)), 'on the page', 10000).catch(() => {});

    await t.caption('Save a view, and make it public');
    await t.wheel(t.w / 2, t.h / 2, -1200);
    await t.beat(600);
    await t.tap('Menu');
    await t.tap('Views');
    const card = '.og-card[data-name=Views]';
    await t.tap(await t.el(`${card} input[name=name]`));
    await t.type('Tower');
    await t.tap(await t.el(`${card} form button`));
    await t.tap(await t.el(`${card} .og-marks li:first-child [data-act=public]`));
    await t.wait(async () => (await t.state()).bookmarks.some(b => b.name === 'Tower' && b.public), 'Tower is public');
    t.check(true, 'made public');
    await t.shot('mine');

    await t.caption('Someone else on the same page sees it');
    const b2 = await launch('desktop');
    const friend = new T(b2, { name: `${t.name}-friend`, demo: false, out: t.out });
    friend.log = () => {};
    await friend.open();
    await friend.connect(hub.view);
    await friend.rowAction('First page', 'Open');
    await friend.wait(async () => (await friend.state()).shared.some(v => v.name === 'Tower'), 'the friend gets Tower', 12000);
    t.check(true, 'the public view reached the other person');
    await friend.tap('Menu');
    await friend.tap('Views');
    const go = await friend.el(`${card} .og-shared li:first-child .go`);
    t.check(/by /.test(go.label), 'it shows who made it');
    const z0 = (await friend.state()).zoom;
    await friend.tap(go);
    await friend.wait(async () => Math.abs((await friend.state()).zoom - z0) > 0.3, 'the friend flies there', 8000);
    t.check(true, 'flew to it');
    await friend.tap(await friend.el(`${card} .og-shared li:first-child [data-act=copy]`));
    await friend.wait(async () => (await friend.state()).bookmarks.some(b => b.name === 'Tower'), 'a copy in their own views');
    t.check(true, 'kept a copy');
    await friend.shot('friend');

    await t.caption('Made private again, it goes away for them');
    await t.tap(await t.el(`${card} .og-marks li:first-child [data-act=public]`));
    await friend.wait(async () => !(await friend.state()).shared.some(v => v.name === 'Tower'), 'Tower leaves their public list', 10000);
    t.check(true, 'private again');
    await b2.close?.();
    hub.stop();
  },
};
