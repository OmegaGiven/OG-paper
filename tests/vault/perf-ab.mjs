// Canvas frame time A/B in headless Chromium: the same canvas (2,000 strokes), panned and
// zoomed by mouse for a fixed time, in two builds of the web app served side by side.
//   A=http://127.0.0.1:8801/ B=http://127.0.0.1:8802/ PW=<dir with node_modules/playwright> node tests/vault/perf-ab.mjs
// Prints median and p95 frame intervals per build (rounds alternate, so load drift hits both).
const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const builds = { A: process.env.A, B: process.env.B };
const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });
const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };

async function round(url) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  await page.goto(url);
  await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
  const cmds = [];
  for (let i = 0; i < 2000; i++) {
    const x = (i % 50) * 30 - 750, y = Math.floor(i / 50) * 20 - 400;
    cmds.push({ add: 'stroke', points: [[x, y], [x + 10, y + 5], [x + 20, y - 3], [x + 25, y + 8]], width: 2, color: '#222222' });
  }
  await page.evaluate(c => window.ogPaper.run(c), cmds);
  await page.waitForTimeout(1500);
  await page.evaluate(() => {
    window.__ft = [];
    let last = performance.now();
    const f = t => { window.__ft.push(t - last); last = t; if (window.__ft.length < 100000) requestAnimationFrame(f); };
    requestAnimationFrame(f);
  });
  await page.mouse.move(640, 400);
  for (let k = 0; k < 6; k++) {
    await page.mouse.down({ button: 'middle' });
    for (let i = 0; i < 40; i++) await page.mouse.move(640 + (k % 2 ? -1 : 1) * i * 8, 400 + i * 3, { steps: 1 });
    await page.mouse.up({ button: 'middle' });
    for (let i = 0; i < 10; i++) await page.mouse.wheel(0, k % 2 ? 120 : -120);
  }
  const ft = (await page.evaluate(() => window.__ft)).slice(5);
  await ctx.close();
  return ft;
}

const all = { A: [], B: [] };
for (let r = 0; r < 3; r++) for (const k of r % 2 ? ['B', 'A'] : ['A', 'B']) all[k].push(...await round(builds[k]));
for (const k of ['A', 'B']) console.log(`${k}: ${all[k].length} frames, median ${pct(all[k], 0.5).toFixed(2)} ms, p95 ${pct(all[k], 0.95).toFixed(2)} ms`);
await browser.close();
