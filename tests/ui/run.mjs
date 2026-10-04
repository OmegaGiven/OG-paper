// Run the UI tests (tests/ui/features/*.mjs), or record them as demos.
//
//   node tests/ui/run.mjs                 every test, every size it names
//   node tests/ui/run.mjs bookmarks pages only tests whose name has these
//   node tests/ui/run.mjs --demo          record demos (out/demos/*.mp4)
//   node tests/ui/run.mjs --size=phone    only phone (or desktop) runs
//
// Each test file:
//   export default {
//     name: 'bookmarks-fold', features: ['BKM-03'], sizes: ['desktop', 'phone'],
//     title: 'Fold bookmarks into a bar',    // the demo's opening caption
//     async run(t) { ... }                   // see lib.mjs (class T)
//   };
//
// Writes out/ui-report.json and out/ui-report.md; exits 1 if a test fails.

import { readdirSync, writeFileSync, mkdirSync } from 'fs';
import { join } from 'path';
import { pathToFileURL } from 'url';
import { ROOT, ensureWeb, launch, T, record, Failure } from './lib.mjs';

const args = process.argv.slice(2);
const demo = args.includes('--demo');
const onlySize = args.find(a => a.startsWith('--size='))?.slice(7);
const filters = args.filter(a => !a.startsWith('--'));
const OUT = join(ROOT, 'out');

const dir = join(ROOT, 'tests/ui/features');
const tests = [];
for (const f of readdirSync(dir).filter(f => f.endsWith('.mjs')).sort()) {
  const t = (await import(pathToFileURL(join(dir, f)))).default;
  t.file = f;
  if (!filters.length || filters.some(x => t.name.includes(x))) tests.push(t);
}
if (!tests.length) { console.error('no tests match'); process.exit(2); }

await ensureWeb();
const results = [];
const bySize = {};
for (const t of tests) for (const size of t.sizes || ['desktop']) {
  if (onlySize && size !== onlySize) continue;
  (bySize[size] ||= []).push(t);
}

for (const [size, list] of Object.entries(bySize)) {
  const b = await launch(size, { demo });
  for (const test of list) {
    const t = new T(b, { name: test.name, demo, out: OUT });
    const t0 = Date.now();
    let ok = true, err = '', video = null;
    process.stdout.write(`${demo ? 'demo' : 'test'}  ${test.name} (${size}) … `);
    try {
      t.title = test.title;
      if (demo) {
        video = join(OUT, 'demos', `${test.name}-${size}.mp4`);
        await record(b, video, () => test.run(t));
      } else await test.run(t);
    } catch (e) {
      ok = false;
      err = e instanceof Failure ? e.message : (e.stack || String(e));
      try { await t.shot('failed'); } catch (_) {}
    }
    const secs = ((Date.now() - t0) / 1000).toFixed(1);
    console.log(ok ? `ok (${secs}s)` : `FAILED (${secs}s)\n      ${err.split('\n')[0]}`);
    if (!ok && process.env.UI_VERBOSE !== undefined) console.log(t.steps.map(s => `      ${s}`).join('\n'));
    if (b.errors.length) { console.log(`      page errors: ${b.errors.slice(0, 3).join(' | ')}`); b.errors.length = 0; }
    results.push({ name: test.name, features: test.features, size, ok, secs: +secs, error: err, steps: t.steps, shots: t.shots, video });
  }
  await b.close();
}

mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, 'ui-report.json'), JSON.stringify(results, null, 2));
const md = ['| Test | Features | Size | Result |', '|---|---|---|---|',
  ...results.map(r => `| ${r.name} | ${r.features.join(', ')} | ${r.size} | ${r.ok ? '✅' : '❌ ' + r.error.split('\n')[0].replace(/\|/g, '\\|')} |`)].join('\n');
writeFileSync(join(OUT, 'ui-report.md'), md + '\n');
const failed = results.filter(r => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed${demo ? `; videos in out/demos/` : ''}`);
process.exit(failed.length ? 1 : 0);
