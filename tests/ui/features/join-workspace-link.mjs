// Pasting a workspace (server) link into Share live › Join adds it as a
// workspace instead of sitting on "Connecting…".
import { startHub } from '../lib.mjs';

export default {
  name: 'join-workspace-link',
  features: ['LIVE-01', 'WS-01'],
  sizes: ['desktop', 'phone'],
  title: 'Join with a server link',
  async run(t) {
    const hub = await startHub();
    await t.open();
    await t.caption('Menu › Share live: paste the server\'s link and Join');
    await t.tap('Menu');
    await t.tap('Share live');
    await t.tap(await t.find(/^Paste a link/));
    await t.paste(hub.view);
    await t.tap('Join');
    await t.caption('It is a workspace: it opens under Pages');
    await t.find(`127.0.0.1:${hub.port}`, { ms: 8000 });
    await t.find('First page', { ms: 8000 });
    t.check(true, 'the server is added as a workspace, with its pages');
    t.check(!(await t.state()).net || !/Connecting/.test(JSON.stringify((await t.state()).net)), 'not left connecting');
    await t.shot('workspace');
    hub.stop();
  },
};
