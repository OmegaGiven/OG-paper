// Records "The Maker's Loop" promo: the endless zoom through the demo
// canvas's artwork, one full loop that starts and ends on the same frame.
//   node scripts/promo/makers-loop.mjs [desktop|phone] [--captions]
// Needs the web build (scripts/build-web.sh) and ffmpeg. Writes
// out/promo/makers-loop-<size>[-captions].mp4.
import { join } from 'path';
import { ROOT, ensureWeb, launch, T, record } from '../../tests/ui/lib.mjs';

const size = process.argv[2] || 'desktop';
const captions = process.argv.includes('--captions');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const STORY = [
  'A maker in his man cave builds a drawing app...',
  '...and sketches his life: his job, taken by AI.',
  'But he dreams of a game: Wizard Quest.',
  'Inside the spell: the god who makes spells for us.',
  'And in the god\'s eye...',
];
const HOLD = 1300; // ms on each scene
const ZOOM = 4200; // ms from one scene into the next

await ensureWeb();
const b = await launch(size, { demo: true });
const t = new T(b, { name: 'makers-loop', demo: true, out: join(ROOT, 'out') });
t.log = () => {};
await t.open('/try/', { prefs: 'hints=off\n' });
const pkg = `import('${new URL('/app/pkg/og_paper.js', 'http://x').pathname}')`;
const view = x => b.eval(`${pkg}.then(m => m.og_art_view(${x}))`);
// Just the picture: no tour card, no buttons, no cursor.
await b.eval(`document.querySelectorAll('.og-card').forEach(c => c.hidden = true); (() => { const s = document.createElement('style'); s.textContent = '.og-ui > *:not(#ogcap) { display: none !important } #ogcap { display: block !important }'; document.head.append(s); })()`);
await b.eval(`${pkg}.then(m => m.og_present(true))`);
await b.eval(`window.ogDemo?.move(-200, -200)`);
await view(0);
await sleep(2500);
const smooth = u => u * u * (3 - 2 * u);
const file = join(ROOT, 'out', 'promo', `makers-loop-${size}${captions ? '-captions' : ''}.mp4`);
await record(b, file, async () => {
  for (let i = 0; i < 5; i++) {
    if (captions) await b.eval(`window.ogDemo?.cap(${JSON.stringify(STORY[i])})`);
    await sleep(HOLD);
    const t0 = Date.now();
    for (;;) {
      const u = Math.min(1, (Date.now() - t0) / ZOOM);
      await view(i + smooth(u));
      if (u >= 1) break;
      await sleep(12);
    }
  }
  // Round again: the pupil is the man cave.
  await view(0);
  if (captions) await b.eval(`window.ogDemo?.cap(${JSON.stringify('...it all begins again. Made in OG Paper.')})`);
  await sleep(HOLD);
});
await b.close();
console.log(file);
process.exit(0);
