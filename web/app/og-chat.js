// OG Paper web shell: chat with the people on a hosted page (see
// docs/HOSTED.md "Chat"). Messages travel on the page's live socket as the
// room's text frames; the room says who sent them. Everything here is shown
// as text (textContent / text nodes), never as HTML: links are made into
// <a> elements only for http(s) addresses.
//
// It shows only when the room offers chat (welcome.chat) and the page's
// owners have it on: a round button with an unread badge once two or more
// people are on the page (or there is kept history), and a panel (a side
// panel on wide screens, a bottom sheet on phones) with who is here and
// typing, the messages, reactions, @mentions, and a jump to someone's view.
// Anyone can report a message to the site's moderators, and block a person
// (their messages are no longer shown; the room stops sending them too).

const CSS = `
.og-chat-btn { position: fixed; z-index: 20; right: calc(12px + env(safe-area-inset-right)); bottom: calc(150px + env(safe-area-inset-bottom));
  width: 48px; height: 48px; border-radius: 24px; border: 1px solid var(--edge); background: var(--face); color: #2d2d37;
  box-shadow: 0 2px 8px rgba(0,0,0,.2); display: grid; place-items: center; padding: 0; }
.og-chat-btn[hidden], .og-chat[hidden], .og-chat [hidden] { display: none !important; }
.og-chat-btn svg { width: 24px; height: 24px; }
.og-chat-btn .badge { position: absolute; top: -4px; right: -4px; min-width: 20px; height: 20px; border-radius: 10px; padding: 0 5px;
  background: #2d2d37; color: #fff; font: 700 12px/20px system-ui, sans-serif; text-align: center; box-sizing: border-box; }
.og-chat-btn .badge.at { background: var(--accent); }
/* Above the app's pen button (bottom right) and below its gear (top right). */
.og-chat { position: fixed; z-index: 20; right: calc(12px + env(safe-area-inset-right)); bottom: calc(76px + env(safe-area-inset-bottom));
  width: min(340px, calc(100vw - 24px)); height: min(520px, calc(100dvh - 170px)); display: flex; flex-direction: column;
  background: var(--face); border: 1px solid var(--edge); border-radius: 14px; box-shadow: 0 6px 24px rgba(0,0,0,.2); overflow: hidden; }
.og-chat * { box-sizing: border-box; }
.og-chat header { display: flex; align-items: center; gap: 6px; padding: 10px 10px 6px 14px; }
.og-chat header b { font-size: 15px; flex: 1; }
.og-chat header button { border: 0; background: none; width: 32px; height: 32px; border-radius: 8px; color: var(--muted); padding: 0; display: grid; place-items: center; }
.og-chat header button:hover { background: rgba(0,0,0,.06); }
.og-chat header svg { width: 18px; height: 18px; }
.og-chat .people { display: flex; gap: 6px; padding: 0 12px 8px; overflow-x: auto; scrollbar-width: none; border-bottom: 1px solid var(--edge); }
.og-chat .people::-webkit-scrollbar { display: none; }
.og-chat .person { flex: none; display: flex; align-items: center; gap: 5px; border: 1px solid var(--edge); background: #fff; border-radius: 999px;
  padding: 3px 9px 3px 3px; font: 600 12px/1 system-ui, sans-serif; color: var(--ink); max-width: 160px; }
.og-chat .person span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.og-chat .av { flex: none; width: 22px; height: 22px; border-radius: 11px; color: #fff; display: grid; place-items: center; font: 700 11px/1 system-ui, sans-serif; position: relative; }
.og-chat .person.typing .av::after { content: ''; position: absolute; right: -2px; bottom: -2px; width: 8px; height: 8px; border-radius: 4px; background: var(--ok); border: 2px solid #fff; }
.og-chat .log { flex: 1; overflow-y: auto; padding: 8px 12px; overscroll-behavior: contain; }
.og-chat .empty { color: var(--muted); font-size: 13px; text-align: center; margin: 24px 8px; }
.og-chat .msg { position: relative; padding: 4px 6px; margin: 0 -6px 2px; border-radius: 8px; }
.og-chat .msg:hover, .og-chat .msg.on { background: rgba(0,0,0,.04); }
.og-chat .msg.at { background: rgba(200,40,90,.08); }
.og-chat .msg .who { display: flex; align-items: baseline; gap: 6px; font-size: 13px; }
.og-chat .msg .who b { color: var(--ink); }
.og-chat .msg .who time { color: var(--muted); font-size: 11px; }
.og-chat .msg .who .jump { border: 0; background: none; color: var(--muted); font-size: 11px; padding: 0; text-decoration: underline; }
.og-chat .msg.same .who { display: none; }
.og-chat .msg .text { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; color: var(--ink); }
.og-chat .msg.pending .text { opacity: .55; }
.og-chat .msg .text a { color: #2856c8; }
.og-chat .msg .text .mention { color: var(--accent); font-weight: 600; }
.og-chat .msg .acts { position: absolute; right: 4px; top: -10px; display: none; gap: 2px; background: var(--face); border: 1px solid var(--edge); border-radius: 8px; padding: 2px; box-shadow: 0 1px 4px rgba(0,0,0,.12); }
.og-chat .msg:hover .acts, .og-chat .msg.on .acts { display: flex; }
.og-chat .acts button { border: 0; background: none; width: 28px; height: 26px; border-radius: 6px; padding: 0; font-size: 15px; }
.og-chat .acts button:hover { background: rgba(0,0,0,.07); }
.og-chat .reacts { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 3px; }
.og-chat .reacts button { border: 1px solid var(--edge); background: #fff; border-radius: 999px; padding: 1px 7px; font: 12px/18px system-ui, sans-serif; color: var(--ink); }
.og-chat .reacts button.mine { border-color: var(--accent); background: rgba(200,40,90,.08); }
.og-chat .typing { min-height: 18px; padding: 0 14px; color: var(--muted); font-size: 12px; font-style: italic; }
.og-chat form { display: flex; gap: 6px; padding: 6px 10px 10px; align-items: flex-end; position: relative; }
.og-chat textarea { flex: 1; min-width: 0; resize: none; font: 14px/1.35 system-ui, sans-serif; padding: 8px 10px; border-radius: 10px;
  border: 1px solid var(--edge); background: #fff; color: var(--ink); max-height: 120px; height: 38px; }
.og-chat form button { flex: none; height: 38px; padding: 0 14px; border-radius: 10px; border: 1px solid var(--accent); background: var(--accent); color: #fff; font-weight: 600; }
.og-chat form button:disabled { opacity: .5; }
.og-chat .count { position: absolute; right: 86px; bottom: 14px; font-size: 11px; color: var(--muted); pointer-events: none; }
.og-chat .pop { position: absolute; z-index: 2; background: var(--face); border: 1px solid var(--edge); border-radius: 10px; box-shadow: 0 4px 16px rgba(0,0,0,.18); padding: 4px; min-width: 170px; }
.og-chat .pop button, .og-chat .pop label { display: flex; align-items: center; gap: 8px; width: 100%; border: 0; background: none; text-align: left; padding: 8px 10px; border-radius: 7px; font-size: 13px; color: var(--ink); }
.og-chat .pop button:hover, .og-chat .pop label:hover, .og-chat .pop button.sel { background: rgba(0,0,0,.06); }
.og-chat .pop .note { font-size: 11px; color: var(--muted); padding: 0 10px 6px 34px; margin: -4px 0 0; }
.og-chat .pop .danger { color: var(--accent); }
.og-chat .pop.emoji { display: flex; min-width: 0; }
.og-chat .pop.emoji button { width: 34px; justify-content: center; padding: 6px 0; font-size: 18px; }
.og-chat .off { padding: 6px 14px; font-size: 12px; color: var(--muted); background: rgba(0,0,0,.03); }
@media (max-width: 520px) {
  .og-chat { left: 0; right: 0; bottom: 0; width: 100%; height: min(62dvh, 520px); border-radius: 16px 16px 0 0; border-bottom: 0;
    padding-bottom: env(safe-area-inset-bottom); }
  .og-chat-btn { bottom: calc(150px + env(safe-area-inset-bottom)); }
  .og-chat textarea { font-size: 16px; }
  .og-chat .msg .acts { display: none; }
  .og-chat .msg.on .acts { display: flex; }
}
.dark .og-chat .person, .dark .og-chat .reacts button, .dark .og-chat textarea { background: #2a2a33; }
`;

const ICON_CHAT = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z"/></svg>';
const ICON_MORE = '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>';
const ICON_DOWN = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>';

// http(s) addresses, and @mentions. Trailing punctuation is not part of a link.
const URL_RE = /\bhttps?:\/\/[^\s<>"'`]+/gi;
const TRAIL = /[.,;:!?)\]}'"]+$/;

/** A steady colour for an account. */
export function colorOf(id) {
  let h = 0;
  for (const c of String(id)) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return `hsl(${h % 360} 60% 42%)`;
}
const initial = name => ([...String(name || '?').trim()][0] || '?').toUpperCase();

/**
 * Put `text` into `node` as text, with http(s) links as <a> and @mentions of
 * `names` highlighted. Never parses HTML. Returns whether `me` is mentioned.
 */
export function renderText(node, text, names = [], me = '') {
  const doc = node.ownerDocument;
  let mentioned = false;
  // Longest names first, so "@Ann Lee" wins over "@Ann".
  const sorted = [...new Set(names.filter(Boolean))].sort((a, b) => b.length - a.length);
  const lower = text.toLowerCase();
  const spans = [];
  for (const m of text.matchAll(URL_RE)) {
    const url = m[0].replace(TRAIL, '');
    spans.push({ at: m.index, end: m.index + url.length, url });
  }
  for (let i = lower.indexOf('@'); i >= 0; i = lower.indexOf('@', i + 1)) {
    if (spans.some(s => i >= s.at && i < s.end)) continue;
    if (i > 0 && /[\p{L}\p{N}_.]/u.test(text[i - 1])) continue;   // mail@bob is not a mention
    const name = sorted.find(n => lower.startsWith(n.toLowerCase(), i + 1) && !/[\p{L}\p{N}_]/u.test(text[i + 1 + n.length] || ''));
    if (!name) continue;
    spans.push({ at: i, end: i + 1 + name.length, mention: name });
    if (me && name.toLowerCase() === me.toLowerCase()) mentioned = true;
  }
  spans.sort((a, b) => a.at - b.at);
  let at = 0;
  for (const s of spans) {
    if (s.at < at) continue;
    if (s.at > at) node.append(doc.createTextNode(text.slice(at, s.at)));
    const part = text.slice(s.at, s.end);
    if (s.url) {
      let ok = false;
      try { ok = ['http:', 'https:'].includes(new URL(s.url).protocol); } catch (e) {}
      if (ok) {
        const a = doc.createElement('a');
        a.href = s.url;
        a.target = '_blank';
        a.rel = 'noopener noreferrer nofollow ugc';
        a.textContent = part;
        node.append(a);
      } else node.append(doc.createTextNode(part));
    } else {
      const b = doc.createElement('span');
      b.className = 'mention';
      b.textContent = part;
      node.append(b);
    }
    at = s.end;
  }
  if (at < text.length) node.append(doc.createTextNode(text.slice(at)));
  return mentioned;
}

const hhmm = t => new Date(t).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });

/**
 * ui: from og-web.js (root, el, say, sendControl, app.og_go_to_peer).
 * host: what og-hosted.js knows: me() → {id, name}, page() → page id or null,
 * call(method, path, body) → the API, removePerson(uid) → Promise.
 */
export function startChat(ui, host) {
  const { el, say } = ui;
  document.head.append(el('style', {}, CSS));

  const st = {
    cfg: null,          // the room's chat settings (null: no chat here)
    role: null,
    me: null,           // my account id, as the room knows it
    peers: [],
    blocks: new Set(),  // accounts I blocked (the room says which)
    msgs: [],           // { id, user, name, text, at, reacts, pending?, ref? }
    typing: new Map(),  // user → { name, until }
    unread: 0,
    mentioned: false,
    open: false,
    live: false,
  };
  let refN = 0;

  const btn = el('button', { class: 'og-chat-btn', type: 'button', hidden: '', 'aria-label': 'Chat', title: 'Chat with the people on this page' }, ICON_CHAT + '<span class="badge" hidden></span>');
  const panel = el('section', { class: 'og-chat og-ui-chat', hidden: '', 'aria-label': 'Chat' }, `
    <header><b>Chat</b><button type="button" data-act="menu" aria-label="Chat options" title="Options">${ICON_MORE}</button>
      <button type="button" data-act="close" aria-label="Hide chat" title="Hide">${ICON_DOWN}</button></header>
    <div class="people" aria-label="On this page"></div>
    <div class="off" hidden></div>
    <div class="log" role="log" aria-live="polite"></div>
    <div class="typing"></div>
    <form><textarea rows="1" placeholder="Message everyone here…" aria-label="Message"></textarea><span class="count" hidden></span><button type="submit">Send</button></form>`);
  ui.root.append(btn, panel);
  const $ = s => panel.querySelector(s);
  const log = $('.log'), people = $('.people'), input = $('textarea'), form = $('form'), typingLine = $('.typing'), count = $('.count'), offNote = $('.off');
  const badge = btn.querySelector('.badge');

  const names = () => st.peers.map(p => p.name).filter(Boolean);
  const myName = () => host.me()?.name || st.peers.find(p => p.user === st.me)?.name || '';
  const isOwner = () => st.role === 'owner';
  const others = () => st.peers.filter(p => p.user !== st.me);

  // ---- showing it ----------------------------------------------------------
  function visible() {
    if (!st.cfg || !st.cfg.on || !host.page()) return false;
    return st.open || others().length > 0 || (st.cfg.keep && st.msgs.length > 0) || st.unread > 0;
  }
  function sync() {
    const v = visible();
    btn.hidden = !v || st.open;
    panel.hidden = !v || !st.open;
    badge.hidden = st.unread === 0;
    badge.textContent = st.unread > 99 ? '99+' : String(st.unread);
    badge.classList.toggle('at', st.mentioned);
    btn.setAttribute('aria-label', st.unread ? `Chat, ${st.unread} unread` : 'Chat');
    input.disabled = !st.live;
    form.querySelector('button').disabled = !st.live;
    input.placeholder = st.live ? (others().length ? 'Message everyone here…' : 'Nobody else is here right now') : 'Reconnecting…';
  }
  function setOpen(on) {
    st.open = on;
    // A card (Page, My account…) would sit under the panel: close it.
    if (on) { st.unread = 0; st.mentioned = false; if (ui.isOpen?.()) ui.show(null); }
    closePop();
    sync();
    if (on) { scrollEnd(true); if (matchMedia('(min-width: 521px)').matches) input.focus(); }
  }
  btn.onclick = () => setOpen(true);
  $('[data-act=close]').onclick = () => setOpen(false);

  // ---- people ----------------------------------------------------------------
  function renderPeople() {
    people.textContent = '';
    const now = Date.now();
    for (const p of st.peers) {
      const b = el('button', { type: 'button', class: 'person', title: p.user === st.me ? 'You' : `${p.name || 'Someone'}${p.role === 'viewer' ? ' (viewing)' : ''}` });
      const av = el('span', { class: 'av' });
      av.style.background = colorOf(p.user);
      av.textContent = initial(p.name);
      const n = el('span');
      n.textContent = p.user === st.me ? `${p.name || 'You'} (you)` : `${p.name || 'Someone'}${st.blocks.has(p.user) ? ' (blocked)' : ''}`;
      b.append(av, n);
      if ((st.typing.get(p.user)?.until || 0) > now) b.classList.add('typing');
      if (p.user !== st.me) b.onclick = e => personMenu(p, e.currentTarget);
      people.append(b);
    }
    const t = [...st.typing.entries()].filter(([u, v]) => v.until > now && u !== st.me).map(([, v]) => v.name || 'Someone');
    typingLine.textContent = t.length === 0 ? '' : t.length === 1 ? `${t[0]} is typing…` : t.length === 2 ? `${t[0]} and ${t[1]} are typing…` : 'Several people are typing…';
  }
  setInterval(() => { if (st.typing.size) { for (const [u, v] of st.typing) if (v.until < Date.now()) st.typing.delete(u); renderPeople(); } }, 1500);

  // ---- popovers (menus, emoji, people, mentions) ----------------------------
  let pop = null;
  function closePop() { pop?.remove(); pop = null; }
  function openPop(items, anchor, cls = '') {
    closePop();
    pop = el('div', { class: 'pop ' + cls, role: 'menu' });
    for (const it of items) {
      if (it.note) { const p = el('p', { class: 'note' }); p.textContent = it.note; pop.append(p); continue; }
      if (it.check != null) {
        const l = el('label', {}, `<input type="checkbox"${it.check ? ' checked' : ''}><span></span>`);
        l.querySelector('span').textContent = it.label;
        l.querySelector('input').onchange = e => { closePop(); it.run(e.target.checked); };
        pop.append(l);
        continue;
      }
      const b = el('button', { type: 'button', role: 'menuitem', class: it.danger ? 'danger' : '' });
      b.textContent = it.label;
      b.onclick = () => { closePop(); it.run(); };
      pop.append(b);
    }
    panel.append(pop);
    const pr = panel.getBoundingClientRect(), ar = anchor.getBoundingClientRect();
    const w = pop.offsetWidth, h = pop.offsetHeight;
    pop.style.left = `${Math.max(6, Math.min(pr.width - w - 6, ar.left - pr.left + ar.width - w))}px`;
    const below = ar.bottom - pr.top + 4;
    pop.style.top = `${below + h < pr.height ? below : Math.max(6, ar.top - pr.top - h - 4)}px`;
    return pop;
  }
  panel.addEventListener('pointerdown', e => { if (pop && !pop.contains(e.target) && !e.target.closest('[data-act=menu], .person, .acts, .reacts')) closePop(); });

  function personMenu(p, anchor) {
    const items = [
      { label: `Go to where ${p.name || 'they'} ${p.name ? 'is' : 'are'}`, run: () => ui.app.og_go_to_peer?.(p.name || '') },
      { label: `Mention @${p.name}`, run: () => insertMention(p.name) },
    ];
    if (canBlock(p.user)) {
      items.push(st.blocks.has(p.user)
        ? { label: 'Unblock', run: () => unblock(p) }
        : { label: 'Block…', danger: true, run: () => block(p) });
    }
    if (isOwner()) items.push({ label: 'Remove from this page…', danger: true, run: () => removePerson(p) });
    openPop(items, anchor);
  }
  // Visitors without an account can't be blocked (nor can they chat).
  const anon = () => String(st.me || '').startsWith('anon:');
  const canBlock = uid => uid && uid !== st.me && !anon() && !String(uid).startsWith('anon:');
  async function block(p) {
    if (!confirm(`Block ${p.name || 'them'}? You won't see their messages, and they can't share pages with you or offer you pages. They aren't told.`)) return;
    try { await host.call('POST', '/blocks', { user: p.user }); setBlocks([...st.blocks, p.user]); say(`${p.name || 'They'} is blocked`); }
    catch (e) { say(e.message || String(e)); }
  }
  async function unblock(p) {
    try { await host.call('DELETE', `/blocks/${encodeURIComponent(p.user)}`); st.blocks.delete(p.user); renderPeople(); say(`${p.name || 'They'} is unblocked`); }
    catch (e) { say(e.message || String(e)); }
  }
  function setBlocks(list) {
    st.blocks = new Set((list || []).map(String));
    st.msgs = st.msgs.filter(m => !st.blocks.has(m.user));
    renderPeople(); renderLog();
  }
  async function report(m) {
    const reason = prompt(`Report ${m.name || 'this person'}’s message to the site’s moderators? Say what is wrong (optional).`, '');
    if (reason == null) return;
    try { await host.call('POST', `/pages/${host.page()}/chat/report`, { id: m.id, reason }); say('Reported. Thank you: a moderator will look at it.'); }
    catch (e) { say(e.message || String(e)); }
  }
  async function removePerson(p) {
    if (!confirm(`Remove ${p.name || 'them'} from this page? They lose access at once (share it with them again to undo).`)) return;
    try { await host.removePerson(p.user); say(`${p.name || 'They'} can no longer open this page`); }
    catch (e) { say(e.message || String(e)); }
  }

  $('[data-act=menu]').onclick = e => {
    const items = [];
    if (isOwner()) {
      items.push(
        { label: 'Chat on this page', check: st.cfg.on, run: on => settings({ on }) },
        { label: 'Keep chat history', check: st.cfg.keep, run: keep => settings({ keep }) },
        { note: st.cfg.keep ? `The last ${st.cfg.history} messages stay with the page.` : 'Off: messages go once everyone has left the page.' },
        { label: 'Clear chat for everyone…', danger: true, run: clearAll },
      );
    }
    items.push({ label: 'Download chat', run: download });
    openPop(items, e.currentTarget);
  };
  async function settings(set) {
    try { await host.call('PATCH', `/pages/${host.page()}/chat`, set); say(set.on === false ? 'Chat is off on this page' : set.on ? 'Chat is on' : set.keep ? 'Chat history is kept with the page' : 'Chat goes once everyone leaves'); }
    catch (e) { say(e.message || String(e)); }
  }
  async function clearAll() {
    if (!confirm('Clear the chat for everyone on this page? This cannot be undone.')) return;
    try { await host.call('DELETE', `/pages/${host.page()}/chat`); } catch (e) { say(e.message || String(e)); }
  }
  async function download() {
    try {
      const x = await host.call('GET', `/pages/${host.page()}/chat`);
      const lines = [`OG Paper chat: ${x.page.name}`, `Exported ${new Date(x.exported).toLocaleString()}`, ''];
      for (const m of x.messages) {
        const r = Object.entries(m.reacts || {}).map(([e, w]) => `${e}${w.length}`).join(' ');
        lines.push(`[${new Date(m.at).toLocaleString()}] ${m.name || 'Someone'}: ${m.text}${r ? `  (${r})` : ''}`);
      }
      if (!x.messages.length) lines.push('(no messages)');
      const a = el('a', { download: `${(x.page.name || 'page').replace(/[^\p{L}\p{N} _-]+/gu, '').trim() || 'page'} chat.txt` });
      a.href = URL.createObjectURL(new Blob([lines.join('\n') + '\n'], { type: 'text/plain;charset=utf-8' }));
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 5000);
    } catch (e) { say(e.message || String(e)); }
  }

  // ---- messages ----------------------------------------------------------------
  const atEnd = () => log.scrollHeight - log.scrollTop - log.clientHeight < 40;
  const scrollEnd = force => { if (force || atEnd()) requestAnimationFrame(() => { log.scrollTop = log.scrollHeight; }); };

  function msgNode(m, prev) {
    const n = el('div', { class: 'msg', 'data-id': String(m.id || '') });
    if (m.pending) n.classList.add('pending');
    if (prev && prev.user === m.user && m.at - prev.at < 5 * 60_000) n.classList.add('same');
    const who = el('div', { class: 'who' });
    const b = el('b');
    b.textContent = m.user === st.me ? 'You' : (m.name || 'Someone');
    b.style.color = colorOf(m.user);
    const t = el('time', { datetime: new Date(m.at).toISOString(), title: new Date(m.at).toLocaleString() });
    t.textContent = hhmm(m.at);
    who.append(b, t);
    if (m.user !== st.me && st.peers.some(p => p.user === m.user)) {
      const j = el('button', { type: 'button', class: 'jump', title: `Go to where ${m.name || 'they'} ${m.name ? 'is' : 'are'} on the page` });
      j.textContent = 'go to them';
      j.onclick = () => ui.app.og_go_to_peer?.(m.name || '');
      who.append(j);
    }
    const text = el('div', { class: 'text' });
    if (renderText(text, m.text, names(), myName()) && m.user !== st.me) n.classList.add('at');
    n.append(who, text);
    const rs = Object.entries(m.reacts || {});
    if (rs.length) {
      const row = el('div', { class: 'reacts' });
      for (const [e, w] of rs) {
        const r = el('button', { type: 'button', title: w.map(x => x.name || 'Someone').join(', ') });
        r.textContent = `${e} ${w.length}`;
        if (w.some(x => x.user === st.me)) r.classList.add('mine');
        r.onclick = () => react(m.id, e);
        row.append(r);
      }
      n.append(row);
    }
    if (!m.pending && m.id) {
      const acts = el('div', { class: 'acts' });
      const add = el('button', { type: 'button', title: 'React', 'aria-label': 'React' });
      add.textContent = '☺';
      add.onclick = e => openPop((st.cfg.reactions || []).map(r => ({ label: r, run: () => react(m.id, r) })), e.currentTarget, 'emoji');
      acts.append(add);
      if (m.user !== st.me && !anon()) {
        const rep = el('button', { type: 'button', title: 'Report', 'aria-label': 'Report' });
        rep.textContent = '⚑';
        rep.onclick = () => report(m);
        acts.append(rep);
      }
      if (m.user === st.me || isOwner()) {
        const del = el('button', { type: 'button', title: m.user === st.me ? 'Delete' : 'Delete (owner)', 'aria-label': 'Delete' });
        del.textContent = '🗑';
        del.onclick = () => { if (m.user === st.me || confirm(`Delete ${m.name || 'their'}’s message for everyone?`)) send({ t: 'chat-del', id: m.id }); };
        acts.append(del);
      }
      n.append(acts);
      // Phones: tap a message for its actions.
      n.addEventListener('click', e => {
        if (e.target.closest('a, button')) return;
        const was = n.classList.contains('on');
        log.querySelectorAll('.msg.on').forEach(x => x.classList.remove('on'));
        if (!was) n.classList.add('on');
      });
    }
    return n;
  }
  function renderLog() {
    const keep = atEnd();
    log.textContent = '';
    if (!st.msgs.length) {
      const p = el('p', { class: 'empty' });
      p.textContent = st.cfg?.keep ? 'No messages yet. Say hi to the people drawing with you.' : 'No messages yet. Chat here goes once everyone has left the page.';
      log.append(p);
    }
    let prev = null;
    for (const m of st.msgs) { log.append(msgNode(m, prev)); prev = m; }
    if (keep) scrollEnd(true);
  }

  function send(obj) {
    if (!ui.sendControl(JSON.stringify(obj))) { say('Not connected: try again in a moment'); return false; }
    return true;
  }
  const react = (id, emoji) => send({ t: 'chat-react', id, emoji });

  // ---- composing ---------------------------------------------------------------
  let typingSent = 0;
  const stopTyping = () => { if (typingSent) { typingSent = 0; send({ t: 'typing', on: false }); } };
  form.onsubmit = e => {
    e.preventDefault();
    const text = input.value.trim();
    if (!text) return;
    const max = st.cfg?.max || 1000;
    if ([...text].length > max) return say(`That message is too long (at most ${max} characters)`);
    const ref = `r${++refN}-${Date.now()}`;
    if (!send({ t: 'chat', text, ref })) return;
    typingSent = 0;
    st.msgs.push({ id: 0, ref, user: st.me, name: myName(), text, at: Date.now(), reacts: {}, pending: true });
    input.value = '';
    grow();
    closePop();
    renderLog();
    scrollEnd(true);
  };
  const grow = () => {
    input.style.height = '38px';
    input.style.height = `${Math.min(120, input.scrollHeight + 2)}px`;
    const n = [...input.value].length, max = st.cfg?.max || 1000;
    count.hidden = n < max * 0.8;
    count.textContent = `${n}/${max}`;
  };
  input.addEventListener('input', () => {
    grow();
    if (input.value.trim() && Date.now() - typingSent > 2500) { typingSent = Date.now(); send({ t: 'typing', on: true }); }
    if (!input.value.trim()) stopTyping();
    mentionPop();
  });
  input.addEventListener('blur', () => setTimeout(stopTyping, 300));
  // The canvas must not take the keys typed here.
  for (const k of ['keydown', 'keyup', 'keypress']) input.addEventListener(k, e => {
    e.stopPropagation();
    if (k !== 'keydown') return;
    if (pop?.classList.contains('mentions')) {
      const bs = [...pop.querySelectorAll('button')];
      let i = bs.findIndex(b => b.classList.contains('sel'));
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); bs[i]?.classList.remove('sel'); i = (i + (e.key === 'ArrowDown' ? 1 : bs.length - 1)) % bs.length; bs[i].classList.add('sel'); return; }
      if ((e.key === 'Enter' || e.key === 'Tab') && bs[i]) { e.preventDefault(); bs[i].click(); return; }
      if (e.key === 'Escape') { e.preventDefault(); closePop(); return; }
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); form.requestSubmit(); }
    else if (e.key === 'Escape') { e.preventDefault(); setOpen(false); }
  });

  // @mentions: typing "@" offers the people on the page.
  function mentionPop() {
    const before = input.value.slice(0, input.selectionStart);
    const m = before.match(/(^|\s)@([^\s@]{0,30})$/);
    if (!m) { if (pop?.classList.contains('mentions')) closePop(); return; }
    const q = m[2].toLowerCase();
    const list = others().filter(p => p.name && p.name.toLowerCase().startsWith(q)).slice(0, 6);
    if (!list.length) { if (pop?.classList.contains('mentions')) closePop(); return; }
    openPop(list.map(p => ({ label: `@${p.name}`, run: () => insertMention(p.name, m[2].length + 1) })), input, 'mentions');
    pop.querySelector('button')?.classList.add('sel');
  }
  function insertMention(name, replace = 0) {
    if (!st.open) setOpen(true);
    const at = input.selectionStart ?? input.value.length;
    const start = at - replace;
    const pre = input.value.slice(0, start);
    const sep = replace || !pre || /\s$/.test(pre) ? '' : ' ';
    input.setRangeText(`${sep}@${name} `, start, at, 'end');
    input.focus();
    grow();
  }

  // ---- the room's notes -------------------------------------------------------
  function notify(m) {
    if (m.user === st.me) return;
    const holder = el('div');
    const at = renderText(holder, m.text, names(), myName());
    if (!st.open || document.hidden) {
      if (!st.open) { st.unread++; if (at) st.mentioned = true; }
      if (at) say(`${m.name || 'Someone'} mentioned you in the chat`);
    }
  }

  return {
    /** A text frame from the room (only chat ones are handled here). */
    control(m) {
      switch (m.t) {
        case 'welcome':
          st.cfg = m.chat || null;
          st.role = m.role;
          st.me = m.user != null ? String(m.user) : (host.me()?.id ?? null);
          st.live = true;
          // A new connection: history (if any) follows.
          st.msgs = [];
          st.typing.clear();
          if (!st.cfg?.on) st.open = false;
          renderLog(); sync();
          return true;
        case 'peers':
          st.peers = m.list || [];
          renderPeople(); sync();
          return false;
        case 'chat-history':
          st.msgs = (m.list || []).filter(x => !st.blocks.has(x.user));
          renderLog(); sync();
          return true;
        case 'blocks':
          setBlocks(m.list);
          return true;
        case 'chat': {
          if (st.blocks.has(m.msg?.user) && m.msg.user !== st.me) return true;
          const i = m.ref ? st.msgs.findIndex(x => x.pending && x.ref === m.ref) : -1;
          if (i >= 0) st.msgs[i] = m.msg; else { st.msgs.push(m.msg); notify(m.msg); }
          if (st.msgs.length > (st.cfg?.history || 500)) st.msgs.splice(0, st.msgs.length - (st.cfg?.history || 500));
          st.typing.delete(m.msg.user);
          renderLog(); renderPeople(); sync();
          return true;
        }
        case 'chat-del':
          st.msgs = st.msgs.filter(x => x.id !== m.id);
          renderLog();
          return true;
        case 'chat-react': {
          const x = st.msgs.find(y => y.id === m.id);
          if (x) { x.reacts = m.reacts || {}; renderLog(); }
          return true;
        }
        case 'chat-clear':
          st.msgs = []; st.unread = 0; st.mentioned = false;
          renderLog(); sync();
          return true;
        case 'chat-settings': {
          const was = st.cfg?.on;
          st.cfg = { ...(st.cfg || {}), ...m };
          delete st.cfg.t;
          if (!st.cfg.on) { st.open = false; st.unread = 0; if (was) say('The page’s owner turned chat off'); }
          else if (was === false) say('Chat is on for this page');
          renderLog(); sync();
          return true;
        }
        case 'typing':
          if (st.blocks.has(m.user)) return true;
          if (m.on) st.typing.set(m.user, { name: m.name, until: Date.now() + 6000 }); else st.typing.delete(m.user);
          renderPeople();
          return true;
        case 'chat-error': {
          if (m.ref) { st.msgs = st.msgs.filter(x => x.ref !== m.ref); renderLog(); }
          else if (/^Slow down/.test(m.msg || '')) { st.msgs = st.msgs.filter(x => !x.pending); renderLog(); }
          say(m.msg);
          return true;
        }
      }
      return false;
    },
    /** The live socket went away (or another page is opening). */
    disconnected() { st.live = false; st.typing.clear(); sync(); },
    /** Not on a hosted page any more. */
    reset() { Object.assign(st, { cfg: null, msgs: [], peers: [], blocks: new Set(), unread: 0, mentioned: false, open: false, live: false }); renderLog(); sync(); },
    /** For tests and the account panel: open the chat. */
    open: () => { if (visible() || st.cfg?.on) setOpen(true); },
    get state() { return { on: !!st.cfg?.on, keep: !!st.cfg?.keep, unread: st.unread, open: st.open, visible: visible(), messages: st.msgs.length, blocked: [...st.blocks] }; },
  };
}
