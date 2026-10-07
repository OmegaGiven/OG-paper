// A page opened from a workspace without an account is view-only; signing
// in to that workspace in Pages lets you draw on it straight away (no need
// to leave the live session and join again).
import { startHub } from '../lib.mjs';

export default {
  name: 'sign-in-upgrade',
  features: ['SRV-07'],
  sizes: ['desktop'],
  title: 'Sign in to draw',
  async run(t) {
    const hub = await startHub();
    await t.open();
    await t.caption('A workspace added without an account...');
    await t.connect(`127.0.0.1:${hub.port}`, null);
    await t.find('First page', { ms: 8000 });
    await t.rowAction('First page', 'Open');
    await t.wait(async () => (await t.state()).viewOnly === true, 'the page is view-only', 10000);
    t.check(true, 'opened view-only');

    await t.caption('...sign in there in Pages...');
    await t.tap('Menu');
    await t.tap('Pages');
    await t.tap(await t.find(`127.0.0.1:${hub.port}`));
    await t.tap('Sign in');
    await t.tap(await t.find('User name'));
    await t.type('admin');
    await t.tap(await t.find('Password'));
    await t.type('password');
    // The one beside the password (the toolbar has a Sign in button too).
    const btns = (await t.nodes()).filter(n => n.w > 0 && [n.label, n.value].includes('Sign in')).sort((a, b) => b.y - a.y);
    await t.tap(btns[0]);
    await t.caption('...and the open page can be drawn on');
    await t.wait(async () => (await t.state()).viewOnly === false, 'drawing allowed after signing in', 12000);
    t.check(true, 'the page upgraded to drawing without rejoining');
    await t.shot('signed-in');
    hub.stop();
  },
};
