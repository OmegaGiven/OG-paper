// OG Paper automation client (browser or Node 22+, no dependencies).
//
//   import { connect } from './ogpaper.mjs';
//   const og = await connect('ws://nas:8991', 'e…server key…');
//   const [page] = await og.pages();
//   await og.run(page.page, [{ add: 'text', x: 0, y: 0, text: 'Hello' }]);
//   og.watch(page.page, () => console.log('changed'));
//
// Commands and coordinates: see docs/PLUGINS.md.

export async function connect(url, key) {
  const ws = new WebSocket(url.replace(/\/$/, '') + '/api');
  const waiting = new Map();
  const watchers = new Map();
  let next = 1;
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = () => fail(new Error('could not connect to ' + url)); });
  ws.onmessage = async e => {
    const text = typeof e.data === 'string' ? e.data : await new Response(e.data).text();
    const m = JSON.parse(text);
    if (m.event === 'changed') { (watchers.get(m.page) || []).forEach(f => f(m.page)); return; }
    const w = waiting.get(m.id);
    if (w) { waiting.delete(m.id); m.ok ? w.ok(m) : w.fail(new Error(m.error)); }
  };
  const call = msg => new Promise((ok, fail) => {
    const id = next++;
    waiting.set(id, { ok, fail });
    ws.send(JSON.stringify({ ...msg, id }));
  });
  const auth = await call({ auth: key });
  return {
    canEdit: auth.can_edit,
    pages: async () => (await call({ cmd: 'pages' })).pages,
    newPage: async name => (await call({ cmd: 'new_page', name })).page,
    renamePage: (page, name) => call({ cmd: 'rename_page', page, name }),
    deletePage: page => call({ cmd: 'delete_page', page }),
    run: async (page, commands) => (await call({ cmd: 'run', page, commands })).results,
    texts: async page => (await call({ cmd: 'texts', page })).texts,
    watch: (page, fn) => {
      watchers.set(page, [...(watchers.get(page) || []), fn]);
      return call({ cmd: 'watch', page });
    },
    close: () => ws.close(),
  };
}
