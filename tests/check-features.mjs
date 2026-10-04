// Check FEATURES.md against the tests:
// - every `ui:<name>` names a file in tests/ui/features/ that lists the
//   feature's ID, and every UI test's IDs are in FEATURES.md;
// - every `rust:<name>` is a test `cargo test --workspace` knows;
// - features with no test (only `todo`) are listed.
//
//   node tests/check-features.mjs            fail on broken references
//   node tests/check-features.mjs --strict   also fail on features with no test
//   node tests/check-features.mjs --no-rust  skip asking cargo for its tests

import { readFileSync, readdirSync } from 'fs';
import { execFileSync } from 'child_process';
import { join, dirname } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const strict = process.argv.includes('--strict');
const noRust = process.argv.includes('--no-rust');

const features = [];
for (const line of readFileSync(join(ROOT, 'FEATURES.md'), 'utf8').split('\n')) {
  const cells = line.split('|').slice(1, -1).map(c => c.trim());
  if (cells.length < 5 || !/^[A-Z]+-\d+$/.test(cells[0])) continue;
  const tests = cells[4].split(',').map(s => s.trim()).filter(Boolean);
  features.push({ id: cells[0], name: cells[1], where: cells[2], tests });
}
const ids = new Set(features.map(f => f.id));
const problems = [];
const dup = features.map(f => f.id).filter((id, i, a) => a.indexOf(id) !== i);
for (const d of dup) problems.push(`${d} is listed twice`);

const uiDir = join(ROOT, 'tests/ui/features');
const ui = {};
for (const f of readdirSync(uiDir).filter(f => f.endsWith('.mjs'))) {
  const t = (await import(pathToFileURL(join(uiDir, f)))).default;
  ui[t.name] = t;
  if (t.file !== undefined) {}
  for (const id of t.features || []) if (!ids.has(id)) problems.push(`tests/ui/features/${f} names ${id}, which is not in FEATURES.md`);
}

let rust = null;
if (!noRust) {
  const out = execFileSync('cargo', ['test', '--workspace', '-q', '--', '--list'], { cwd: ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  rust = new Set(out.split('\n').filter(l => l.endsWith(': test')).map(l => l.slice(0, -6)));
}

const untested = [];
for (const f of features) {
  const real = f.tests.filter(t => t !== 'todo');
  if (!real.length) untested.push(f);
  for (const t of real) {
    const [kind, name] = t.split(/:(.*)/s);
    if (kind === 'ui') {
      if (!ui[name]) problems.push(`${f.id}: no UI test named ${name} (tests/ui/features/${name}.mjs)`);
      else if (!(ui[name].features || []).includes(f.id)) problems.push(`${f.id}: ui:${name} doesn't list ${f.id} in its features`);
    } else if (kind === 'rust') {
      if (rust && !rust.has(name)) problems.push(`${f.id}: no Rust test named ${name}`);
    } else problems.push(`${f.id}: "${t}" is not ui:<name>, rust:<name> or todo`);
  }
}

const covered = features.length - untested.length;
console.log(`${features.length} features; ${covered} with tests, ${untested.length} without`);
console.log(`UI tests: ${Object.keys(ui).length} (each is also a demo)`);
if (untested.length) {
  console.log('\nWithout a test yet:');
  for (const f of untested) console.log(`  ${f.id.padEnd(8)} ${f.name}`);
}
const partly = features.filter(f => f.tests.includes('todo') && f.tests.length > 1);
if (partly.length) console.log(`\n${partly.length} more have some tests but are marked todo for the rest (${partly.map(f => f.id).join(', ')})`);
if (problems.length) {
  console.log('\nProblems:');
  for (const p of problems) console.log(`  ✗ ${p}`);
}
process.exit(problems.length || (strict && untested.length) ? 1 : 0);
