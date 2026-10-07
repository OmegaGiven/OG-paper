// Upload a page from this device to a workspace: the Upload button on its
// row in Pages (it doesn't have to be the open page).
import { readFileSync } from 'fs';
import { join } from 'path';
import { startHub } from '../lib.mjs';

export default {
  name: 'upload-local',
  features: ['FILE-01', 'WS-05'],
  sizes: ['desktop', 'phone'],
  title: 'Upload a page to a workspace',
  async run(t) {
    const hub = await startHub();
    await t.open();
    const newPage = async name => {
      await t.pages();
      await t.tap('New page');
      await t.find('Name the new page');
      await t.type(name);
      await t.key('Enter');
      await t.wait(async () => (await t.state()).name === name, `the page ${name}`, 6000);
    };
    await t.caption('A page on this device...');
    await newPage('Sketch A');
    const x0 = t.phone ? t.w * 0.33 : t.w * 0.3;
    await t.scribble((x0 + t.w) / 2, t.h * 0.35, (t.w - x0) * 0.6);
    await t.scribble((x0 + t.w) / 2, t.h * 0.5, (t.w - x0) * 0.5);
    await newPage('Other page');

    await t.caption('...a server where you can make pages...');
    await t.connect(`127.0.0.1:${hub.port}`);
    await t.find('First page', { ms: 8000 });

    await t.caption('...Upload to… picks the server, and puts a copy there');
    await t.rowAction('Sketch A', 'Upload to…');
    await t.beat(300);
    const host = new RegExp(`127\\.0\\.0\\.1:${hub.port}`);
    // The menu's button (not the server's row in the tree behind it).
    const opts = (await t.nodes()).filter(n => n.w > 0 && n.role === 'Button' && host.test(n.label || n.value || ''));
    t.check(opts.length >= 1, 'the server is offered');
    await t.tap(opts[0]);
    const onServer = () => readFileSync(join(hub.dir, 'pages.txt'), 'utf8').includes('Sketch A');
    await t.wait(async () => onServer(), 'the page is made on the server', 10000);
    await t.wait(async () => /Merged: 2 new strokes/.test(hub.log()), 'its drawing reaches the server', 15000);
    t.check(true, 'Sketch A and its drawing are in the workspace');
    await t.shot('uploaded');
    hub.stop();
  },
};
