// Chat between the people on a page: who may say what, owners' settings,
// kept vs ephemeral history, moderation, limits and plain-text cleaning.

import test from 'node:test';
import assert from 'node:assert/strict';
import { site } from './fakes.js';
import { client, last, newPage } from './helpers.js';
import { cleanText, REACTIONS } from '../src/chat.js';

const said = c => c.notes.filter(n => n.t === 'chat').map(n => n.msg.text);
const errs = c => c.notes.filter(n => n.t === 'chat-error').map(n => n.msg);

async function shared(s, roles = { bob: 'editor', carol: 'viewer' }) {
  const page = await newPage(s, 1, 'Plans');
  for (const [who, role] of Object.entries(roles)) await s.call(1, 'POST', `/pages/${page}/members`, { who, role });
  return page;
}

test('everyone on the page chats live, with names from their accounts', async () => {
  const s = site();
  const page = await shared(s);
  const a = await client(s, 1, page), b = await client(s, 2, page), v = await client(s, 3, page);
  // Chat is on by default, ephemeral; the room says so on joining.
  assert.deepEqual({ on: last(a, 'welcome').chat.on, keep: last(a, 'welcome').chat.keep }, { on: true, keep: false });
  assert.equal(last(b, 'welcome').user, '2');
  await a.say('hi both', 'r1');
  await v.say('viewers can talk too');
  for (const c of [a, b, v]) assert.deepEqual(said(c), ['hi both', 'viewers can talk too']);
  const m = last(b, 'chat').msg;
  assert.equal(m.name, 'carol');
  assert.equal(m.user, '3');
  assert.ok(m.at > 0 && m.id > 0);
  // The sender's own echo carries its ref (to match what it showed as pending).
  assert.equal(a.notes.find(n => n.t === 'chat' && n.msg.text === 'hi both').ref, 'r1');
  assert.equal(b.notes.find(n => n.t === 'chat' && n.msg.text === 'hi both').ref, undefined);
});

test('who someone is comes from the session, not from the message', async () => {
  const s = site();
  const page = await shared(s);
  const b = await client(s, 2, page);
  const a = await client(s, 1, page);
  await b.control({ t: 'chat', text: 'I am alice', user: '1', name: 'alice' });
  const m = last(a, 'chat').msg;
  assert.equal(m.user, '2');
  assert.equal(m.name, 'bob');
  // Nor can anyone chat before joining the room (hello).
  const c = await client(s, 3, page, { connect: false });
  const ws = await s.dial(3, page)();
  await ws.send(JSON.stringify({ t: 'chat', text: 'sneaky' }));
  assert.ok(!said(a).includes('sneaky'));
  assert.equal(c.notes.length, 0);
});

test('people without access cannot open the chat: no socket, no history, no export', async () => {
  const s = site();
  const page = await shared(s);
  assert.equal((await s.dial(4, page)()).status, 404);
  assert.equal((await s.call(4, 'GET', `/pages/${page}/chat`)).status, 404);
  assert.equal((await s.call(null, 'GET', `/pages/${page}/chat`)).status, 401);
  // A removed participant is disconnected and loses the chat with the page.
  const a = await client(s, 1, page), b = await client(s, 2, page);
  await a.say('before');
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/members/2`)).status, 200);
  assert.ok(b.notes.some(n => n.t === 'kicked'));
  await a.say('after');
  assert.ok(!said(b).includes('after'));
  assert.equal((await s.call(2, 'GET', `/pages/${page}/chat`)).status, 404);
});

test('only owners turn chat on or off, or choose to keep it; off means refused', async () => {
  const s = site();
  const page = await shared(s, { bob: 'editor', carol: 'owner' });
  const a = await client(s, 1, page), b = await client(s, 2, page);
  // An editor cannot change it.
  assert.equal((await s.call(2, 'PATCH', `/pages/${page}/chat`, { on: false })).status, 403);
  assert.equal((await s.call(2, 'DELETE', `/pages/${page}/chat`)).status, 403);
  // The owner turns it off: everyone is told, and the room refuses messages.
  const r = await s.call(1, 'PATCH', `/pages/${page}/chat`, { on: false });
  assert.equal(r.status, 200);
  assert.equal(r.body.on, false);
  assert.equal(last(b, 'chat-settings').on, false);
  await b.say('anyone?');
  assert.deepEqual(errs(b), ['Chat is off on this page']);
  assert.ok(!said(a).includes('anyone?'));
  await b.control({ t: 'typing', on: true });
  assert.ok(!a.notes.some(n => n.t === 'typing'));
  // New joiners learn it is off and get no history.
  const b2 = await client(s, 2, page);
  assert.equal(last(b2, 'welcome').chat.on, false);
  assert.ok(!b2.notes.some(n => n.t === 'chat-history'));
  // Export is for owners only while it is off.
  assert.equal((await s.call(2, 'GET', `/pages/${page}/chat`)).status, 403);
  assert.equal((await s.call(1, 'GET', `/pages/${page}/chat`)).status, 200);
  // A co-owner turns it back on.
  assert.equal((await s.call(3, 'PATCH', `/pages/${page}/chat`, { on: true })).status, 200);
  await b.say('back');
  assert.ok(said(a).includes('back'));
  assert.equal((await s.call(1, 'PATCH', `/pages/${page}/chat`, {})).status, 400);
});

test('ephemeral chat goes once everyone has left; kept chat stays (the last 500)', async () => {
  const s = site({ config: { chatHistory: 5, chatBurst: 50, chatRate: 50 } });
  const page = await shared(s);
  let a = await client(s, 1, page), b = await client(s, 2, page);
  await a.say('one'); await b.say('two');
  // Someone joining while people are there sees what has been said.
  const v = await client(s, 3, page);
  assert.deepEqual(last(v, 'chat-history').list.map(m => m.text), ['one', 'two']);
  a.disconnect(); b.disconnect(); v.disconnect();
  a = await client(s, 1, page);
  assert.deepEqual(last(a, 'chat-history').list, []);
  // Kept: survives everyone leaving, and only the newest messages stay.
  await s.call(1, 'PATCH', `/pages/${page}/chat`, { keep: true });
  for (let i = 1; i <= 7; i++) await a.say('m' + i);
  a.disconnect();
  await s.room(page).alarm();
  b = await client(s, 2, page);
  assert.deepEqual(last(b, 'chat-history').list.map(m => m.text), ['m3', 'm4', 'm5', 'm6', 'm7']);
  assert.equal(s.room(page).chat.count(), 5);
  // Data export: settings and messages, with names and times.
  const x = await s.call(2, 'GET', `/pages/${page}/chat`);
  assert.equal(x.status, 200);
  assert.equal(x.body.page.name, 'Plans');
  assert.equal(x.body.settings.keep, true);
  assert.deepEqual(x.body.messages.map(m => [m.name, m.text]).slice(-1), [['alice', 'm7']]);
});

test('deleting the page deletes its chat', async () => {
  const s = site();
  const page = await shared(s);
  await s.call(1, 'PATCH', `/pages/${page}/chat`, { keep: true });
  const a = await client(s, 1, page);
  await a.say('secret plans');
  assert.equal(s.room(page).chat.count(), 1);
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}`)).status, 200);
  const tables = s.room(page).ctx.storage.db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'chat'").all();
  assert.deepEqual(tables, []);
  assert.equal((await s.call(1, 'GET', `/pages/${page}/chat`)).status, 404);
});

test('people delete their own messages; owners clear the chat or take any message down', async () => {
  const s = site();
  const page = await shared(s);
  const a = await client(s, 1, page), b = await client(s, 2, page), v = await client(s, 3, page);
  await b.say('oops');
  await v.say('carol here');
  const oops = last(a, 'chat').msg.id - 1;
  const carols = last(a, 'chat').msg.id;
  // Bob cannot delete carol's message.
  await b.control({ t: 'chat-del', id: carols });
  assert.deepEqual(errs(b), ['You can only delete your own messages']);
  await b.control({ t: 'chat-del', id: oops });
  assert.equal(last(v, 'chat-del').id, oops);
  // The owner may take down anyone's.
  await a.control({ t: 'chat-del', id: carols });
  assert.equal(last(b, 'chat-del').id, carols);
  assert.equal(s.room(page).chat.count(), 0);
  await v.say('again');
  assert.equal((await s.call(1, 'DELETE', `/pages/${page}/chat`)).body.cleared, 1);
  assert.ok(b.notes.some(n => n.t === 'chat-clear'));
});

test('reactions: a fixed set, one per person per emoji, toggled', async () => {
  const s = site();
  const page = await shared(s);
  const a = await client(s, 1, page), b = await client(s, 2, page);
  await a.say('ship it?');
  const id = last(a, 'chat').msg.id;
  await b.control({ t: 'chat-react', id, emoji: '👍' });
  await a.control({ t: 'chat-react', id, emoji: '👍' });
  assert.deepEqual(last(a, 'chat-react').reacts['👍'].map(x => x.name), ['bob', 'alice']);
  await b.control({ t: 'chat-react', id, emoji: '👍' });
  assert.deepEqual(last(a, 'chat-react').reacts['👍'].map(x => x.name), ['alice']);
  const n = a.notes.length;
  await b.control({ t: 'chat-react', id, emoji: '<img src=x onerror=alert(1)>' });
  assert.equal(a.notes.length, n, 'not a listed reaction: ignored');
  assert.equal(REACTIONS.length, 6);
});

test('typing notes go to the others only', async () => {
  const s = site();
  const page = await shared(s);
  const a = await client(s, 1, page), b = await client(s, 2, page);
  await b.control({ t: 'typing', on: true });
  assert.deepEqual(last(a, 'typing'), { t: 'typing', user: '2', name: 'bob', on: true });
  assert.ok(!b.notes.some(n => n.t === 'typing'));
});

test('limits: message length, per-account rate across sockets', async () => {
  const s = site({ config: { chatMaxChars: 20, chatBurst: 3, chatRate: 0.001 } });
  const page = await shared(s);
  const a = await client(s, 1, page), b1 = await client(s, 2, page), b2 = await client(s, 2, page);
  await b1.say('x'.repeat(21));
  assert.match(errs(b1)[0], /too long \(at most 20/);
  // Emoji count as one character each.
  await b1.say('👍'.repeat(20));
  // Two sockets of one account share its allowance.
  await b2.say('two'); await b1.say('three');
  await b2.say('four');
  assert.deepEqual(said(a), ['👍'.repeat(20), 'two', 'three']);
  assert.equal(errs(b2).at(-1), 'Slow down: too many messages at once');
  // Alice has her own.
  await a.say('mine');
  assert.ok(said(b1).includes('mine'));
});

test('messages are plain text: control and direction-override characters go', () => {
  assert.equal(cleanText('  hi‮gnp.exe\u0000\u0007 there \r\n\r\n\r\n\r\nbye ').text, 'hignp.exe there \n\nbye');
  assert.equal(cleanText('<b>bold</b> & <script>').text, '<b>bold</b> & <script>', 'kept as text: the page shows it, never runs it');
  assert.equal(cleanText('👨‍👩‍👧 family').text, '👨‍👩‍👧 family', 'joiners inside emoji stay');
  assert.deepEqual(cleanText('abcdef', 3), { text: 'abc', over: true });
});

test('a site can turn chat off everywhere (PAPER_CHAT=off)', async () => {
  const s = site({ config: { chat: false } });
  const page = await shared(s);
  const a = await client(s, 1, page), b = await client(s, 2, page);
  assert.equal(last(a, 'welcome').chat, null);
  await b.say('hello?');
  assert.deepEqual(errs(b), ['Chat is not available on this site']);
  assert.ok(!said(a).length);
  assert.equal((await s.call(1, 'PATCH', `/pages/${page}/chat`, { on: true })).status, 404);
  assert.equal((await s.call(1, 'GET', '/me')).body.chat, false);
});

test('anonymous link visitors read along but cannot chat', async () => {
  const s = site({ config: { linkAnonymousView: true } });
  const page = await shared(s);
  const l = await s.call(1, 'POST', `/pages/${page}/links`, { role: 'viewer' });
  const a = await client(s, 1, page);
  const { HostedClient } = await import('../client/oplog.js');
  const dial = async () => {
    const res = await s.api(new Request(`https://omegagiven.com/paper/api/l/${l.body.token}/relay`, { headers: { Origin: 'https://omegagiven.com', Upgrade: 'websocket' } }), s.env, {});
    return res.webSocket;
  };
  const anon = new HostedClient({ peer: 'anon', dial, now: () => Date.now() });
  assert.ok(await anon.connect());
  await a.say('hello visitor');
  assert.ok(said(anon).includes('hello visitor'));
  await anon.say('hi from nowhere');
  assert.deepEqual(errs(anon), ['Sign in to chat']);
  assert.ok(!said(a).includes('hi from nowhere'));
});

test('owners see the chat settings with the page; others do not', async () => {
  const s = site();
  const page = await shared(s);
  assert.deepEqual((({ on, keep, max, history }) => ({ on, keep, max, history }))((await s.call(1, 'GET', `/pages/${page}`)).body.chat), { on: true, keep: false, max: 1000, history: 500 });
  assert.equal((await s.call(2, 'GET', `/pages/${page}`)).body.chat, undefined);
});
