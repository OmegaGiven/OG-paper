// Drag a page from This device onto a server or folder to upload it
// (only where you may), and a server page onto a folder to move it.
import { readFileSync } from 'fs';
import { join } from 'path';
import { startHub } from '../lib.mjs';

export default {
  name: 'drag-upload',
  features: ['WS-04', 'WS-05'],
  sizes: ['desktop'],
  title: 'Drag pages onto a server',
  async run(t) {
    const hub = await startHub();
    const host = `127.0.0.1:${hub.port}`;
    const centre = n => [n.x + Math.min(n.w, 160) / 2, n.y + n.h / 2];
    const dragRow = async (from, to) => {
      // Positions once the window has settled (it re-centres as rows come).
      await t.reveal(to);
      const a = centre(await t.reveal(from));
      const b = centre(await t.reveal(to));
      await t.drag([a, [a[0] + 10, a[1] + 8], [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], b, [b[0] + 2, b[1]]], 900);
    };
    const pagesOnServer = () => readFileSync(join(hub.dir, 'pages.txt'), 'utf8');
    await t.open();
    await t.pages();
    await t.tap('New page');
    await t.type('Sketch A');
    await t.key('Enter');
    await t.wait(async () => (await t.state()).name === 'Sketch A', 'the page', 6000);
    await t.scribble(t.w * 0.6, t.h * 0.3, t.w * 0.3);

    await t.caption('Not signed in: the server says no');
    await t.connect(host, '');
    await t.find('First page', { ms: 8000 });
    await dragRow('Sketch A', host);
    await t.beat(1500);
    t.check(!pagesOnServer().includes('Sketch A'), 'nothing uploaded without permission');

    await t.caption('Signed in as an admin: drop it on a folder');
    await t.rowAction(host, 'Sign in');
    await t.tap(await t.find('User name'));
    await t.type('admin');
    await t.tap(await t.find('Password'));
    await t.type('password');
    await t.tap('Sign in', { row: 'Password' });
    await t.find(/^Signed in as admin/, { ms: 8000 }).catch(() => {});
    await t.rowAction(host, 'New folder');
    await t.tap(await t.find('Name'));
    await t.type('Art');
    await t.tap('OK');
    await t.find('Art', { ms: 6000 });
    await dragRow('Sketch A', 'Art');
    const inArt = name => {
      const ids = pagesOnServer().split('\n').filter(l => l.endsWith('\t' + name)).map(l => l.split('\t')[0]);
      const f = JSON.parse(readFileSync(join(hub.dir, 'folders.json'), 'utf8')).pages || {};
      return ids.some(id => f[id] === 'Art');
    };
    await t.wait(async () => inArt('Sketch A'), 'Sketch A lands in Art', 10000);
    t.check(true, 'uploaded into the folder by dragging');

    await t.caption('Drag a server page into a folder to move it');
    await t.pages();
    await dragRow('First page', 'Art');
    await t.wait(async () => inArt('First page'), 'First page moves into Art', 10000);
    t.check(true, 'moved by dragging');
    await t.shot('done');
    hub.stop();
  },
};
