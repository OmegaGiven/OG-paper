// Tools and settings follow your account: set them up in one browser,
// sign in to the server, and a fresh browser that signs in gets the same
// toolbars and settings. UI settings › Tools & settings switches between
// this device's own set and the account's.
import { startHub } from '../lib.mjs';

export default {
  name: 'settings-sync',
  features: ['WS-07'],
  sizes: ['desktop'],
  title: 'Your tools and settings, on every device',
  async run(t) {
    const hub = await startHub();
    const host = `127.0.0.1:${hub.port}`;
    const hotbar = () => t.b.eval(`localStorage.getItem('og-hotbar')`);
    const uiSettings = async () => {
      if (await t.has('Dark mode')) return;
      await t.tap('Menu');
      // The menu scrolls: bring its App section up.
      const m = await t.find('Search text');
      for (let i = 0; i < 4 && !(await t.has('UI settings').catch(() => false)); i++) await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 500);
      await t.wheel(m.x + m.w / 2, m.y + m.h / 2, 800);
      await t.tap('UI settings');
    };

    await t.caption('One browser: dark mode on, a tool kept in the inventory');
    await t.open();
    await uiSettings();
    await t.tap('Dark mode');
    await t.wait(async () => (await t.state()).dark, 'dark mode on');
    if (await t.has('Close')) await t.tap('Close');
    await t.tap('Toolbar 1');
    await t.tap('+ Save current');
    await t.beat(500);
    const mine = await hotbar();
    t.check(!!mine, 'toolbars saved in this browser');

    await t.caption('Sign in to the server: it keeps them for the account');
    await t.connect(host);
    await t.wait(async () => /profiles/.test(require_fs(hub.dir)), 'the server keeps the set', 10000);

    await t.caption('A new browser signs in: the same tools and settings');
    await t.open();
    t.check(!(await t.state()).dark, 'a fresh browser starts light');
    await t.connect(host);
    await t.wait(async () => (await t.state()).dark, 'dark mode came from the account', 10000);
    await t.wait(async () => (await hotbar()) === mine, 'the same toolbars and inventory', 6000);
    await t.shot('synced');

    await t.caption('UI settings › Tools & settings: this device, or the account');
    await uiSettings();
    await t.tap('Tools from This device');
    await t.wait(async () => !(await t.state()).dark, "this browser's own settings again");
    await t.tap(new RegExp(`^Tools from admin on ${host.replace(/\./g, '\\.')}$`));
    await t.wait(async () => (await t.state()).dark, "the account's again");
    await t.shot('switch');
    hub.stop();
  },
};

import { readdirSync, existsSync } from 'fs';
import { join } from 'path';
function require_fs(dir) {
  const d = join(dir, '.profiles');
  return existsSync(d) ? 'profiles ' + readdirSync(d).join(',') : '';
}
