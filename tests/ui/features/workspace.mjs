// Add a workspace, sign in, make a folder and upload the open page into it.
import { readFileSync } from 'fs';
import { join } from 'path';
import { startHub } from '../lib.mjs';

export default {
  name: 'workspace',
  features: ['WS-01', 'WS-02', 'WS-03', 'WS-04', 'WS-05'],
  sizes: ['desktop', 'phone'],
  title: 'Workspaces: sign in, organise, upload',
  async run(t) {
    const hub = await startHub();
    // "Open in the web app" on the server's page adds it like this.
    await t.open('/app/', { hash: `server=${encodeURIComponent(hub.view)}` });
    await t.scribble(t.w * 0.55, t.h * 0.35, t.w * 0.4);
    await t.caption('Menu › Pages › Workspaces');
    await t.tap('Menu');
    await t.tap('Pages');
    await t.find('First page');
    t.check(true, 'the workspace lists its pages');

    await t.caption('Sign in with your account there');
    await t.tap('Sign in');
    await t.tap(await t.find('User name'));
    await t.type('admin');
    await t.tap(await t.find('Password'));
    await t.type('password');
    await t.tap('Sign in');
    await t.find('Signed in as admin · Admin', { ms: 6000 });
    t.check(true, 'signed in as admin');
    await t.shot('signed-in');

    await t.caption('Make a folder');
    await t.tap('+');
    await t.tap('New folder');
    await t.tap(await t.find('Name'));
    await t.type('Art');
    await t.tap('OK');
    await t.find('Art', { ms: 6000 });
    t.check(true, 'the folder "Art" is there');

    await t.caption('Upload this page into it');
    await t.tap('Folder …');
    await t.tap('Upload this page');
    const inArt = () => {
      const f = JSON.parse(readFileSync(join(hub.dir, 'folders.json'), 'utf8'));
      return Object.values(f.pages || {}).includes('Art');
    };
    await t.wait(async () => inArt(), 'a page lands in Art on the server', 10000);
    await t.wait(async () => /Merged: \d+ new stroke/.test(hub.log()), 'the drawing reaches the server', 10000);
    t.check(true, 'the page and its drawing are in the workspace');
    await t.shot('uploaded');
    hub.stop();
  },
};
