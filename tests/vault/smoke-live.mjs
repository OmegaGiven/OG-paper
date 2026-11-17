// Smoke check of the vault-served web app (no account needed): it loads, the account panel
// offers OG Studio and Vault, Vault has this vault's address, Continue reaches the vault's own
// sign-in page for OG Paper. Nothing is signed in or changed.
//   URL=https://og-host1.tail6c6721.ts.net:8610/paper/ PW=<dir> node tests/vault/smoke-live.mjs
const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const URL = process.env.URL || 'https://og-host1.tail6c6721.ts.net:8610/paper/';
const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });
const page = await (await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
const errors = [];
page.on('pageerror', e => errors.push(e.message));
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
await page.goto(URL);
await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
await page.evaluate(() => window.ogPaper.account());
await page.waitForSelector('section[data-name="My account"] [data-act=vault]');
if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/live-choice.png` });
await page.click('section[data-name="My account"] [data-act=vault]');
const addr = await page.inputValue('section[data-name="My account"] [data-addr]');
console.log('vault address filled in:', addr);
if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/live-vault-form.png` });
await Promise.all([page.waitForURL(/\/auth\/vault\/continue\//), page.click('section[data-name="My account"] [data-vault] button[type=submit]')]);
await page.waitForSelector('form#login');
const body = (await page.textContent('body')).replace(/\s+/g, ' ');
console.log('vault sign-in page:', body.slice(0, 160));
if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/live-vault-page.png` });
if (!/OG Paper/.test(body)) throw new Error('the vault page does not name OG Paper');
console.log(errors.length ? `console errors: ${errors.join(' | ')}` : 'no console errors');
await browser.close();
console.log('live smoke: ok');
