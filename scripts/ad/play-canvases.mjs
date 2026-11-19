// The store scenes (tests/ui/store-scenes.mjs) as .ogp canvases, for
// screenshots taken in the real Android app (packaging/android/play/README.md).
// Builds them on a page server (`og-paper --serve-dir DIR`) through its /api,
// with the same commands the web screenshots use.
//   OGP_SERVER_KEY=k og-paper --serve-dir DIR --port 8995 &
//   node scripts/ad/play-canvases.mjs ws://127.0.0.1:8995/api k W H
// W x H: the screen in points (dp). Pages land in DIR as <id>.ogp, named in
// DIR/pages.txt: diagrams, design, sketch.
import { diagram, design, sketch } from '../../tests/ui/store-scenes.mjs';

const [url, key, W, H] = process.argv.slice(2);
const ws = new WebSocket(url);
const queue = [];
ws.onmessage = e => queue.shift()?.(JSON.parse(e.data));
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
const ask = m => new Promise(r => { queue.push(r); ws.send(JSON.stringify(m)); });

const auth = await ask({ auth: key });
if (!auth.ok || !auth.can_edit) throw new Error(`auth: ${JSON.stringify(auth)}`);

for (const [name, scene] of [['diagrams', diagram], ['design', design], ['sketch', sketch]]) {
  const { page } = await ask({ cmd: 'new_page', name });
  let last = [];
  // The scenes drive a page through t.b.eval(`window.ogPaper.run(...)`) and
  // read 'window.ogPaper.results()'; captions (page HTML) are skipped here.
  const t = {
    w: Number(W), h: Number(H),
    beat: async () => {},
    b: {
      eval: async code => {
        const m = /^window\.ogPaper\.run\((.*)\)$/s.exec(code);
        if (m) {
          const r = await ask({ cmd: 'run', page, commands: JSON.parse(m[1]) });
          last = r.results;
          const bad = last.find(x => x && x.ok === false);
          if (bad) throw new Error(`${name}: ${bad.error}`);
          return undefined;
        }
        if (code === 'window.ogPaper.results()') return last;
        return undefined;
      },
    },
  };
  await scene(t);
  console.log(name, page);
}
ws.close();
