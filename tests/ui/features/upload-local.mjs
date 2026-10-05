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
    const pages = async () => { if (!(await t.maybe('On this device', 900))) { await t.tap('Menu'); await t.tap('Pages'); } };
    const newPage = async name => {
      await pages();
      await t.tap('+ New page');
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

    await t.caption('...a workspace you can make pages in...');
    await pages();
    await t.tap(await t.find(/^Workspace link/));
    await t.paste(hub.view);
    await t.tap('Add');
    await t.find('First page', { ms: 8000 });
    await t.tap('Sign in');
    await t.tap(await t.find('User name'));
    await t.type('admin');
    await t.tap(await t.find('Password'));
    await t.type('password');
    await t.tap('Sign in');
    await t.find(/^Signed in as admin/, { ms: 8000 });

    await t.caption('...Upload puts a copy there');
    await t.tap('Upload', { row: 'Sketch A' });
    const onServer = () => readFileSync(join(hub.dir, 'pages.txt'), 'utf8').includes('Sketch A');
    await t.wait(async () => onServer(), 'the page is made on the server', 10000);
    await t.wait(async () => /Merged: 2 new strokes/.test(hub.log()), 'its drawing reaches the server', 15000);
    t.check(true, 'Sketch A and its drawing are in the workspace');
    await t.shot('uploaded');
    hub.stop();
  },
};
