// Chat in the real web app, two browsers (alice on a desktop, bob on a
// phone) against the example hosted site in workerd: the chat button shows
// only once two people are on the page, messages arrive live as text (HTML
// stays text, links are links), unread badge and @mention, typing, emoji
// reactions, deleting one's own message, the owner turning chat off and
// keeping history, the data export, and the owner removing someone.
//   BASE=http://127.0.0.1:8799 PW=/dir/with/node_modules node hosted/test/chat-e2e.mjs
// (hosted/test/run-browser-e2e.sh does all of it.) SHOTS=dir keeps screenshots.

import assert from 'node:assert/strict';

const { chromium } = await import(`${process.env.PW || '.'}/node_modules/playwright/index.mjs`);
const BASE = process.env.BASE || 'http://127.0.0.1:8799';
const wait = ms => new Promise(r => setTimeout(r, ms));
const shot = (p, name) => process.env.SHOTS ? p.screenshot({ path: `${process.env.SHOTS}/${name}.png` }) : null;
async function until(f, ms = 30000, what = '') {
  const t = Date.now();
  for (;;) { if (await f()) return; if (Date.now() - t > ms) throw new Error('timed out: ' + what); await wait(250); }
}

const browser = await chromium.launch({ args: ['--enable-unsafe-webgpu', '--use-angle=swiftshader', '--ignore-gpu-blocklist'] });

async function person(name, phone = false) {
  const ctx = await browser.newContext(phone ? { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true } : { viewport: { width: 1280, height: 800 } });
  await ctx.grantPermissions(['clipboard-read', 'clipboard-write']).catch(() => {});
  const page = await ctx.newPage();
  page.on('pageerror', e => console.log(`[${name}] page error:`, e.message));
  page.on('console', m => { if (m.type() === 'error') console.log(`[${name}]`, m.text()); });
  if (process.env.DEBUG) page.on('websocket', ws => { ws.on('framereceived', f => typeof f.payload === 'string' && console.log(`[${name} <]`, f.payload.slice(0, 160))); ws.on('framesent', f => typeof f.payload === 'string' && console.log(`[${name} >]`, f.payload.slice(0, 160))); });
  page.on('dialog', d => d.accept(d.type() === 'prompt' ? 'Chat page' : undefined));
  await page.goto(`${BASE}/dev/login?as=${name}`);
  await page.waitForFunction(() => window.ogPaper && document.querySelector('.og-loading')?.hidden, null, { timeout: 120000 });
  return page;
}
const api = (p, method, path, body) => p.evaluate(([method, path, body]) => fetch('/paper/api' + path, {
  method, headers: body ? { 'content-type': 'application/json' } : {}, body: body ? JSON.stringify(body) : undefined,
}).then(async r => ({ status: r.status, body: await r.json() })), [method, path, body]);
const chat = p => p.evaluate(() => window.ogPaper.chat());
const visible = (p, sel) => p.isVisible(sel);

// Alice makes a page and shares it with bob.
const alice = await person('alice');
await alice.waitForSelector('section[data-name="My account"]:not([hidden])', { timeout: 15000 });
await alice.click('section[data-name="My account"] [data-act=new]');
let id;
await until(async () => { id = (await api(alice, 'GET', '/pages')).body.mine.find(p => p.name === 'Chat page')?.id; return !!id; }, 30000, 'page made');
assert.equal((await api(alice, 'POST', `/pages/${id}/members`, { who: 'bob', role: 'editor' })).status, 200);
await until(async () => (await chat(alice)).on, 20000, 'alice: chat offered by the room');
// Alone on the page: no chat button.
await wait(500);
assert.equal(await visible(alice, '.og-chat-btn'), false, 'alone: no chat button');
console.log('alice: alone on her page, no chat button');

// Bob (on a phone) opens it: both get the button.
const bob = await person('bob', true);
await bob.waitForSelector(`[data-list=shared] li[data-id="${id}"] .go`, { timeout: 15000 });
await bob.click(`[data-list=shared] li[data-id="${id}"] .go`);
await alice.waitForSelector('.og-chat-btn:not([hidden])', { timeout: 30000 });
await bob.waitForSelector('.og-chat-btn:not([hidden])', { timeout: 30000 });
await shot(alice, 'chat-button-desktop');
await shot(bob, 'chat-button-phone');
console.log('both: chat button once two people are on the page');

// Alice says hello, with a mention, a link and some HTML.
await alice.click('.og-chat-btn');
await alice.waitForSelector('.og-chat:not([hidden])');
await alice.fill('.og-chat textarea', 'hi @');
await alice.waitForSelector('.og-chat .pop.mentions button');
assert.equal(await alice.textContent('.og-chat .pop.mentions button'), '@bob');
await alice.press('.og-chat textarea', 'Enter');
assert.equal(await alice.inputValue('.og-chat textarea'), 'hi @bob ');
await alice.type('.og-chat textarea', 'see https://example.com/plan <b>bold</b><img src=x onerror="window.pwned=1">');
await alice.press('.og-chat textarea', 'Enter');
await alice.waitForSelector('.og-chat .msg:not(.pending) .text a');
assert.equal(await alice.inputValue('.og-chat textarea'), '');
// Bob: unread badge marked as a mention.
await bob.waitForSelector('.og-chat-btn .badge:not([hidden])', { timeout: 15000 });
assert.equal(await bob.textContent('.og-chat-btn .badge'), '1');
assert.ok(await bob.evaluate(() => document.querySelector('.og-chat-btn .badge').classList.contains('at')), 'mention badge');
await shot(bob, 'chat-unread-phone');
await bob.click('.og-chat-btn');
await bob.waitForSelector('.og-chat:not([hidden]) .msg .text');
const shown = await bob.evaluate(() => {
  const t = document.querySelector('.og-chat .msg .text');
  return { text: t.textContent, links: [...t.querySelectorAll('a')].map(a => [a.href, a.rel, a.target]), tags: [...t.querySelectorAll('*')].map(e => e.tagName), mention: t.querySelector('.mention')?.textContent, at: t.closest('.msg').classList.contains('at') };
});
assert.equal(shown.text, 'hi @bob see https://example.com/plan <b>bold</b><img src=x onerror="window.pwned=1">');
assert.deepEqual(shown.links, [['https://example.com/plan', 'noopener noreferrer nofollow ugc', '_blank']]);
assert.deepEqual(shown.tags.sort(), ['A', 'SPAN']);
assert.equal(shown.mention, '@bob');
assert.ok(shown.at);
assert.equal(await bob.evaluate(() => window.pwned), undefined);
assert.equal((await chat(bob)).unread, 0);
console.log('bob: got it live as text (link, mention; HTML stayed text), badge cleared');

// Bob types: alice sees it. He answers; alice reacts; bob deletes his own.
await bob.type('.og-chat textarea', 'on my way');
await alice.waitForFunction(() => /bob is typing/.test(document.querySelector('.og-chat > .typing').textContent), null, { timeout: 10000 });
await shot(alice, 'chat-typing-desktop');
await bob.press('.og-chat textarea', 'Enter');
await alice.waitForFunction(() => [...document.querySelectorAll('.og-chat .msg .text')].some(t => t.textContent === 'on my way'));
const last = '.og-chat .msg:last-child';
await alice.hover(last);
await alice.click(`${last} .acts button[title=React]`);
await alice.click('.og-chat .pop.emoji button:has-text("👍")');
await bob.waitForFunction(() => /👍 1/.test(document.querySelector('.og-chat .msg:last-child .reacts')?.textContent || ''), null, { timeout: 10000 });
await bob.fill('.og-chat textarea', 'typo, ignore');
await bob.press('.og-chat textarea', 'Enter');
await bob.waitForFunction(() => document.querySelector('.og-chat .msg:last-child:not(.pending) .text')?.textContent === 'typo, ignore');
await shot(bob, 'chat-panel-phone');
await shot(alice, 'chat-panel-desktop');
await bob.click('.og-chat .msg:last-child .text');
await bob.click('.og-chat .msg:last-child .acts button[aria-label=Delete]');
await alice.waitForFunction(() => ![...document.querySelectorAll('.og-chat .msg .text')].some(t => t.textContent === 'typo, ignore'), null, { timeout: 10000 });
console.log('typing shows, reactions and deleting one\'s own message reach the other side');

// Bob (an editor) cannot change the settings; alice keeps the history.
assert.equal((await api(bob, 'PATCH', `/pages/${id}/chat`, { on: false })).status, 403);
await alice.click('.og-chat [data-act=menu]');
await shot(alice, 'chat-owner-menu-desktop');
await alice.click('.og-chat .pop label:has-text("Keep chat history") input');
await until(async () => (await chat(bob)).keep, 10000, 'bob told it is kept');
const x = await api(bob, 'GET', `/pages/${id}/chat`);
assert.equal(x.status, 200);
assert.deepEqual(x.body.messages.map(m => [m.name, m.text]), [['alice', shown.text], ['bob', 'on my way']]);
assert.deepEqual(x.body.messages[1].reacts['👍'].map(r => r.name), ['alice']);
console.log('owner: keep history; editor cannot change settings; export has names, text, reactions');

// Alice turns chat off: bob's chat goes away; on again from the Page card.
await alice.click('.og-chat [data-act=menu]');
await alice.click('.og-chat .pop label:has-text("Chat on this page") input');
await bob.waitForSelector('.og-chat', { state: 'hidden', timeout: 10000 });
assert.equal(await visible(bob, '.og-chat-btn'), false);
assert.equal(await visible(alice, '.og-chat-btn'), false);
await alice.evaluate(() => window.ogPaper.account());
await alice.click(`[data-list=mine] li[data-id="${id}"] [data-act=share]`);
await alice.waitForSelector('section[data-name="Page"] [data-chat=on]');
assert.equal(await alice.isChecked('section[data-name="Page"] [data-chat=on]'), false);
await shot(alice, 'chat-page-settings-desktop');
await alice.click('section[data-name="Page"] [data-chat=on]');
await bob.waitForSelector('.og-chat-btn:not([hidden])', { timeout: 10000 });
// Kept history came back with it.
assert.equal((await chat(bob)).messages, 2);
console.log('owner: chat off hides it for everyone; on again (Page card) brings the kept history back');

// The owner removes bob from the page, from the chat's people list.
// (The card is drawn again once the change is made: keep is enabled then.)
await alice.waitForSelector('section[data-name="Page"] [data-chat=keep]:not([disabled])');
await alice.click('section[data-name="Page"] h2 .close');
await alice.waitForSelector('section[data-name="Page"]', { state: 'hidden' });
await alice.click('.og-chat-btn');
await alice.click('.og-chat .people .person:has-text("bob")');
await shot(alice, 'chat-person-menu-desktop');
await alice.click('.og-chat .pop button:has-text("Remove from this page")');
await until(async () => (await api(bob, 'GET', `/pages/${id}/chat`)).status === 404, 15000, 'bob removed');
await bob.waitForSelector('.og-chat', { state: 'hidden', timeout: 10000 });
await until(async () => !(await visible(alice, '.og-chat-btn')) || (await chat(alice)).visible, 5000);
console.log('owner: removed bob from the page from the chat; he lost it at once');

await browser.close();
console.log('CHAT E2E OK');
process.exit(0);
