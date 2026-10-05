// Connect to a server, sign in, make a folder and upload the open page into it.
import { readFileSync } from 'fs';
import { join } from 'path';
import { startHub } from '../lib.mjs';

export default {
  name: 'workspace',
  features: ['WS-01', 'WS-02', 'WS-03', 'WS-04', 'WS-05'],
  sizes: ['desktop', 'phone'],
  title: 'Servers: connect, organise, upload',
  async run(t) {
    const hub = await startHub();
    const host = `127.0.0.1:${hub.port}`;
    await t.open();
    await t.scribble(t.w * 0.55, t.h * 0.35, t.w * 0.4);
    await t.caption('Menu › Connect to server: its address and your account');
    await t.connect(host);
    await t.find('First page', { ms: 8000 });
    t.check(true, 'connected by its address: Pages shows its pages');
    await t.shot('connected');

    await t.caption('Select the server, then New folder');
    await t.rowAction(host, 'New folder');
    await t.tap(await t.find('Name'));
    await t.type('Art');
    await t.tap('OK');
    await t.find('Art', { ms: 6000 });
    t.check(true, 'the folder "Art" is there');

    await t.caption('Select the folder, then Upload open page');
    await t.rowAction('Art', 'Upload open page');
    const inArt = () => {
      const f = JSON.parse(readFileSync(join(hub.dir, 'folders.json'), 'utf8'));
      return Object.values(f.pages || {}).includes('Art');
    };
    await t.wait(async () => inArt(), 'a page lands in Art on the server', 10000);
    await t.wait(async () => /Merged: \d+ new stroke/.test(hub.log()), 'the drawing reaches the server', 10000);
    t.check(true, 'the page and its drawing are in the folder');
    await t.shot('uploaded');
    hub.stop();
  },
};
