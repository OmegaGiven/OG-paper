// OG Paper web shell: where pages are kept. Signed out, pages live in this
// browser (IndexedDB, as always). On a site that hosts OG Paper (for
// example omegagiven.com/paper/) and signed in there, they live in the
// account: the page list shows on opening, every page opened is a live room
// that saves as you draw (and keeps a copy here for offline), and pages can
// be shared, copied and given away. See docs/HOSTED.md.
//
// A site turns this on with <meta name="og-paper-api" content="/paper/api">
// (and optionally og-paper-signin: where to send people to sign in, with
// {next} for the way back; og-paper-upgrade: where to buy more storage).

import { startChat } from './og-chat.js';

/**
 * Where pages are kept. Both kinds answer the same calls:
 *   kind                       'local' | 'hosted'
 *   list()                     { mine: [page], shared: [page] }
 *   create(name)               a new page, opened
 *   open(page)                 open it on the canvas
 *   rename(page, name), remove(page)
 *   usage()                    { used, quota } or null (no limit)
 * A page is { id, name, updated, role?, bytes?, owner? }.
 */
export class LocalProvider {
  constructor(shell) { this.kind = 'local'; this.shell = shell; }
  async list() { return { mine: this.shell.localPages().map(p => ({ id: p.c, name: p.name, updated: p.t, role: 'owner' })), shared: [] }; }
  async create(name) { return this.shell.newLocal(name); }
  async open(page) { return this.shell.openLocal(page.id); }
  async rename(page, name) { return this.shell.renameLocal(page.id, name); }
  async remove(page) { return this.shell.removeLocal(page.id); }
  async usage() { return null; }
}

export class HostedProvider {
  constructor(api, app) { this.kind = 'hosted'; this.api = api.replace(/\/$/, ''); this.app = app; }

  async call(method, path, body) {
    const r = await fetch(this.api + path, {
      method, credentials: 'same-origin',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let data = {};
    try { data = await r.json(); } catch (e) { /* empty */ }
    if (!r.ok) { const e = new Error(data.error || `Request failed (${r.status})`); e.status = r.status; e.data = data; throw e; }
    return data;
  }

  me() { return this.call('GET', '/me'); }
  async list() { return this.call('GET', '/pages'); }
  async usage() { return (await this.list()).usage; }
  detail(id) { return this.call('GET', `/pages/${id}`); }

  /** New keys for a page: the room never sees inside, so they are made here. */
  keys() {
    const b = crypto.getRandomValues(new Uint8Array(32));
    const edit = 'e' + btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    const canvas = [...crypto.getRandomValues(new Uint8Array(16))].map(x => x.toString(16).padStart(2, '0')).join('');
    return { canvas, edit, view: this.app.og_view_token(edit) };
  }

  async create(name) {
    const r = await this.call('POST', '/pages', { name, ...this.keys() });
    remember(r.page.id, r.open);
    this.app.og_join(r.open.link);
    return r.page;
  }
  /** The open canvas, into the account as a new page. */
  async keep(name) {
    const r = await this.call('POST', '/pages', { name, ...this.keys() });
    remember(r.page.id, r.open);
    this.app.og_join_upload(r.open.link);
    return r.page;
  }
  async open(page) {
    const r = await this.call('POST', `/pages/${page.id}/open`);
    remember(page.id, r);
    this.app.og_join(r.link);
    return r;
  }
  /** Copy a page into my account: open it, then join the copy with its content. */
  async copy(page, current, waitSynced) {
    const r = await this.call('POST', `/pages/${page.id}/copy`, this.keys());
    if (current !== page.id) { this.app.og_join(r.from.link); await waitSynced(); }
    remember(r.page.id, r.open);
    this.app.og_join_upload(r.open.link);
    return r.page;
  }
  rename(page, name) { return this.call('PATCH', `/pages/${page.id}`, { name }); }
  /** To the trash (restorable for 30 days); from the trash, `forever`. */
  remove(page, forever = false) { forget(page.id); return this.call('DELETE', `/pages/${page.id}${forever ? '?forever=1' : ''}`); }
  untrash(page) { return this.call('POST', `/pages/${page.id}/untrash`); }
  history(page) { return this.call('GET', `/pages/${page.id}/history`); }
  restore(page, since) { return this.call('POST', `/pages/${page.id}/restore`, { since }); }
  leave(page, me) { forget(page.id); return this.call('DELETE', `/pages/${page.id}/members/${encodeURIComponent(me)}`); }
  share(page, who, role) { return this.call('POST', `/pages/${page.id}/members`, { who, role }); }
  setRole(page, uid, role) { return this.call('PATCH', `/pages/${page.id}/members/${encodeURIComponent(uid)}`, { role }); }
  unshare(page, uid) { return this.call('DELETE', `/pages/${page.id}/members/${encodeURIComponent(uid)}`); }
  link(page, role) { return this.call('POST', `/pages/${page.id}/links`, { role }); }
  unlink(page, id) { return this.call('DELETE', `/pages/${page.id}/links/${id}`); }
  /** Offer the page to someone: it moves only once they accept. */
  transfer(page, who, leave) { return this.call('POST', `/pages/${page.id}/transfer`, { who, leave }); }
  withdraw(page) { return this.call('DELETE', `/pages/${page.id}/transfer`); }
  async acceptOffer(id) { const r = await this.call('POST', `/offers/${id}/accept`); remember(r.page.id, r.open); return r; }
  declineOffer(id) { return this.call('POST', `/offers/${id}/decline`); }
  report(page, reason) { return this.call('POST', `/pages/${page.id}/report`, { reason }); }
  chatSettings(page, set) { return this.call('PATCH', `/pages/${page.id}/chat`, set); }
  accept(token) { return this.call('POST', `/links/${token}`); }
  peek(token) { return this.call('GET', `/links/${token}`); }
}

// Pages opened from the account, so this device can reopen them offline
// (the link holds the page's key; it stays on this device, like any link).
const MAP = 'og-hosted-pages';
const known = () => { try { return JSON.parse(localStorage.getItem(MAP)) || {}; } catch (e) { return {}; } };
function remember(id, open) {
  const m = known();
  m[id] = { link: open.link, canvas: open.canvas, role: open.role, name: open.name };
  try { localStorage.setItem(MAP, JSON.stringify(m)); localStorage.setItem('og-hosted-last', id); } catch (e) {}
}
function forget(id) {
  const m = known();
  delete m[id];
  try { localStorage.setItem(MAP, JSON.stringify(m)); } catch (e) {}
}
/** Every account page this device keeps (signing out drops them all). */
function forgetAll() {
  const canvases = Object.values(known()).map(v => v.canvas).filter(Boolean);
  try { localStorage.removeItem(MAP); localStorage.removeItem('og-hosted-last'); } catch (e) {}
  return canvases;
}
const pageOfCanvas = canvas => Object.entries(known()).find(([, v]) => v.canvas === canvas)?.[0] || null;

export function hostedConfig() {
  const meta = n => document.querySelector(`meta[name="${n}"]`)?.content || null;
  const api = meta('og-paper-api') || window.OG_PAPER_HOSTED?.api;
  if (!api) return null;
  return {
    api,
    signIn: meta('og-paper-signin') || window.OG_PAPER_HOSTED?.signIn || '/account/?next={next}',
    // The site's page for buying more storage (the API's /me can name it too).
    upgrade: meta('og-paper-upgrade') || window.OG_PAPER_HOSTED?.upgrade || null,
  };
}

const fmtBytes = n => n >= 1048576 ? `${(n / 1048576).toFixed(n >= 10485760 ? 0 : 1)} MB` : n >= 1024 ? `${Math.round(n / 1024)} KB` : `${n} B`;
const ago = t => {
  const s = (Date.now() - t) / 1000;
  return s < 60 ? 'just now' : s < 3600 ? `${Math.round(s / 60)} min ago` : s < 86400 ? `${Math.round(s / 3600)} h ago` : new Date(t).toLocaleDateString();
};

/**
 * The account panel and the live room's side of things. `ui` comes from
 * og-web.js: card, cards, show, say, status, el, esc, autosave, exporter,
 * and the app's calls (og_join, og_join_upload, og_view_token).
 */
export function startHosted(ui, cfg) {
  const { el, esc, say } = ui;
  // Set when the room showed us the door (access taken away): no redialling
  // until another page is opened.
  let closedFor = null;
  const app = {
    ...ui.app,
    og_join: link => { closedFor = null; ui.app.og_join(link); },
    og_join_upload: link => { closedFor = null; ui.app.og_join_upload(link); },
  };
  const hp = new HostedProvider(cfg.api, app);
  const state = { me: null, list: null, peers: [], role: null, refused: false, open: null };
  const signInUrl = () => cfg.signIn.replace('{next}', encodeURIComponent(location.pathname + location.hash));

  const c = ui.card('My account');
  ui.cards.account = c;
  const body = el('div');
  c.append(body);
  // Storage warnings: nearly full, and full (changes no longer saved). Both
  // offer the site's paid storage when it has some (PAPER_UPGRADE_URL).
  const upgradeUrl = () => state.me?.upgrade || cfg.upgrade || null;
  const warnCard = (name, title) => {
    const w = ui.card(title);
    ui.cards[name] = w;
    w.append(el('p', { class: 'msg' }), el('div', { class: 'og-row', style: 'flex-wrap:wrap' },
      `<a class="og-btn primary" data-act="upgrade" target="_blank" rel="noopener" hidden>Get more storage</a>
       <button class="og-btn" data-act="export">Export a copy</button><button class="og-btn" data-act="pages">Delete pages</button>`));
    w.querySelector('[data-act=export]').onclick = () => { ui.show(null); ui.exporter(); };
    w.querySelector('[data-act=pages]').onclick = () => openPanel();
    return w;
  };
  const full = warnCard('full', 'Storage full');
  const nearly = warnCard('nearly', 'Storage nearly full');
  const fillCard = (w, html, offer) => {
    w.querySelector('.msg').innerHTML = html;
    const up = w.querySelector('[data-act=upgrade]');
    up.hidden = !(offer && upgradeUrl());
    if (!up.hidden) up.href = upgradeUrl();
  };

  // ---- is the open page safe in the account? ------------------------------
  // The room answers each change it keeps ("kept"), or says it dropped it.
  // Changes made while not connected count until the room has them.
  const cloud = { open: false, live: false, sent: 0, kept: 0, dropped: false, syncedN: null };
  const changesNow = () => ui.status().timeline?.n ?? 0;
  function cloudState() {
    const st = ui.status();
    if (!st.ready) return 'empty';
    const page = currentPage();
    if (!page) return st.strokes > 0 ? 'local' : 'empty';
    if (st.viewOnly) return 'saved';
    if (cloud.dropped) return 'unsaved';
    const n = changesNow();
    if (cloud.open && cloud.live) {
      if (cloud.sent <= cloud.kept) { cloud.syncedN = n; return 'saved'; }
      return 'saving';
    }
    return cloud.syncedN === n ? 'saved' : 'unsaved';
  }
  const chip = el('button', { class: 'og-cloud', hidden: '', type: 'button' });
  ui.root.append(chip);
  chip.onclick = () => openPanel();
  let chipKey = '';
  setInterval(() => {
    const c = cloudState();
    const signedIn = !!state.me?.user;
    const text = c === 'local' ? (signedIn ? 'Only in this browser: keep it in your account' : 'Only in this browser: sign in to save to the cloud')
      : c === 'unsaved' ? (cloud.dropped ? '⚠ Not saved to your account' : '⚠ Not saved to your account yet (offline)')
      : '';
    if (text === chipKey) return;
    chipKey = text;
    chip.hidden = !text;
    chip.textContent = text;
    chip.classList.toggle('warn', c === 'unsaved');
  }, 1000);

  // Usage: from the page list, and live from the room while drawing.
  const usage = { used: 0, quota: 0 };
  const WARN = [[1, 'full'], [0.95, 'almost'], [0.8, 'nearly']];
  function noteUsage(used, quota) {
    if (!quota) return;
    usage.used = used; usage.quota = quota;
    const f = used / quota;
    const key = 'og-quota-warned';
    let warned = {};
    try { warned = JSON.parse(localStorage.getItem(key)) || {}; } catch (e) {}
    // Back under 80% (pages deleted, more storage bought): warn again next time.
    if (f < 0.8) { if (Object.keys(warned).length) localStorage.removeItem(key); return; }
    const level = WARN.find(([at]) => f >= at)[1];
    if (level === 'full' || warned[level]) return;   // full: the room says so when it drops a change
    warned[level] = Date.now();
    try { localStorage.setItem(key, JSON.stringify(warned)); } catch (e) {}
    fillCard(nearly, `You have used <b>${fmtBytes(used)}</b> of your <b>${fmtBytes(quota)}</b>.
      When it is full, new edits stay on this device only and are <b>not saved to your account</b>.
      ${upgradeUrl() ? 'Get more storage to keep everything in the cloud, or export' : 'Export'} or delete pages to make room.`, true);
    ui.show('nearly');
  }

  const currentPage = () => pageOfCanvas(ui.status().canvas);

  // Chat with the people on the open page (when the room offers it).
  const chat = startChat(ui, {
    me: () => state.me?.user || null,
    page: () => (state.role ? currentPage() : null),
    call: (method, path, body) => hp.call(method, path, body),
    removePerson: uid => hp.unshare({ id: currentPage() }, uid),
  });

  // Whose pages this device holds. Signed out, or someone else signed in:
  // their links (with edit keys) and offline copies go (a shared computer
  // must not reopen them).
  const OWNER = 'og-hosted-user';
  async function checkOwner() {
    const now = state.me?.user?.id || null;
    let was = null;
    try { was = localStorage.getItem(OWNER); } catch (e) {}
    if (was !== now && (was || !now) && Object.keys(known()).length) {
      await ui.dropLocal?.(forgetAll());
      state.open = null;
      if (was) say(now ? 'Signed in as someone else: the other account\'s pages were removed from this device' : 'Signed out: your pages were removed from this device');
    }
    try { if (now) localStorage.setItem(OWNER, now); else localStorage.removeItem(OWNER); } catch (e) {}
  }

  async function refresh() {
    try {
      state.me = await hp.me();
      await checkOwner();
      state.list = state.me.user ? await hp.list() : null;
      if (state.list?.usage) noteUsage(state.list.usage.used, state.list.usage.quota);
    } catch (e) {
      state.me = state.me || { user: null };
      say(e.message);
    }
    render();
  }

  function usageBar(u) {
    if (!u) return '';
    const pct = Math.min(100, (100 * u.used) / Math.max(1, u.quota));
    return `<div class="og-progress" title="${pct.toFixed(0)}%"><div style="width:${pct}%;background:${pct > 90 ? 'var(--accent)' : 'var(--ok)'}"></div></div>
      <p>${fmtBytes(u.used)} of ${fmtBytes(u.quota)} used · ${u.pages} of ${u.maxPages} pages${pct >= 100 ? ' · <b>full</b>: new edits are not saved to your account. Export or delete pages to free space' : ''}</p>
      ${upgradeUrl() ? `<p><a href="${esc(upgradeUrl())}" target="_blank" rel="noopener">${pct >= 80 ? '<b>Get more storage</b>' : 'Need more room? Get more storage'}</a></p>` : ''}`;
  }

  function item(p, mine) {
    const live = p.id === currentPage();
    const meta = mine
      ? `${p.bytes ? fmtBytes(p.bytes) + ' · ' : ''}${ago(p.updated)}${p.blocked ? ' · taken down' : ''}${p.offer ? ` · offered to ${esc(p.offer.to.name || 'someone')}` : ''}`
      : `${esc(p.owner?.name || 'someone')} · ${p.role === 'viewer' ? 'can view' : p.role === 'owner' ? 'co-owner' : 'can edit'} · ${ago(p.updated)}`;
    return `<li data-id="${p.id}" data-mine="${mine ? 1 : 0}"><button class="go">${live ? '● ' : ''}${esc(p.name)}<small>${meta}</small></button>
      <button class="mini" data-act="share" title="${mine || p.role === 'owner' ? 'Share' : 'Details'}">${mine || p.role === 'owner' ? 'Share' : '⋯'}</button></li>`;
  }

  function render() {
    const me = state.me;
    if (!me || !me.user) {
      body.innerHTML = `<p>Sign in to keep your pages in your account: they open on any device, save as you draw, and you can share them and draw on them together.</p>
        <p>Signed out, pages stay in this browser only.</p>
        <div class="og-row"><a class="og-btn primary" href="${esc(signInUrl())}">Sign in</a></div>`;
      return;
    }
    const l = state.list || { mine: [], shared: [] };
    const cur = currentPage();
    const here = state.open && state.open.id === cur ? state.open : null;
    body.innerHTML = `
      <p>Signed in as <b>${esc(me.user.name || 'you')}</b></p>
      ${usageBar(l.usage || me)}
      ${here ? `<p>On this page: ${state.peers.map(p => esc(p.name || 'someone') + (p.role === 'viewer' ? ' (viewing)' : '')).join(', ') || 'just you'}</p>` : ''}
      <div class="og-row"><button class="og-btn primary" data-act="new">+ New page</button>
        ${cur ? '' : '<button class="og-btn" data-act="keep" title="Put the canvas that is open now into your account">Keep this one</button>'}</div>
      <div class="og-shared-head">My pages</div>
      <ul class="og-list" data-list="mine">${l.mine.map(p => item(p, true)).join('') || '<li class="og-empty">No pages yet.</li>'}</ul>
      ${(l.offers || []).length ? `<div class="og-shared-head">Offered to you</div>
      <ul class="og-list" data-list="offers">${l.offers.map(o => `<li data-offer="${esc(o.page.id)}"><span class="go">${esc(o.page.name)}<small>from ${esc(o.from.name || 'someone')}${o.page.bytes ? ' · ' + fmtBytes(o.page.bytes) : ''} · ${ago(o.created)}</small></span>
        <button class="mini" data-act="accept" title="Take it: it becomes yours and counts toward your storage">Accept</button><button class="mini" data-act="decline" title="No thanks">✕</button></li>`).join('')}</ul>` : ''}
      <div class="og-shared-head">Shared with me</div>
      <ul class="og-list" data-list="shared">${l.shared.map(p => item(p, false)).join('') || '<li class="og-empty">Nothing shared with you yet.</li>'}</ul>
      ${(l.trash || []).length ? `<div class="og-shared-head">Trash <small>(deleted pages, kept 30 days)</small></div>
      <ul class="og-list" data-list="trash">${l.trash.map(p => `<li data-trash="${esc(p.id)}"><span class="go">${esc(p.name)}<small>${p.bytes ? fmtBytes(p.bytes) + ' · ' : ''}deleted ${ago(p.deleted)} · gone ${new Date(p.purge).toLocaleDateString()}</small></span>
        <button class="mini" data-act="untrash" title="Put it back in My pages">Restore</button><button class="mini" data-act="forever" title="Delete it for good now">✕</button></li>`).join('')}</ul>` : ''}`;
    const act = (sel, f) => { const b = body.querySelector(sel); if (b) b.onclick = f; };
    act('[data-act=new]', async () => {
      const name = prompt('Name the new page', 'Untitled');
      if (name === null) return;
      await guard(async () => { await ui.autosave(); await hp.create(name || 'Untitled'); say('New page: it saves to your account as you draw'); ui.show(null); refresh(); });
    });
    act('[data-act=keep]', async () => {
      const name = prompt('Keep the open canvas in your account as', ui.status().name || 'Untitled');
      if (name === null) return;
      await guard(async () => { await hp.keep(name || 'Untitled'); refresh(); });
    });
    for (const li of body.querySelectorAll('li[data-id]')) {
      const page = [...l.mine, ...l.shared].find(p => p.id === li.dataset.id);
      li.querySelector('.go').onclick = () => guard(async () => {
        await ui.autosave();
        await hp.open(page);
        state.open = page;
        ui.show(null);
      });
      li.querySelector('[data-act=share]').onclick = () => details(page);
    }
    for (const li of body.querySelectorAll('li[data-offer]')) {
      const o = l.offers.find(x => x.page.id === li.dataset.offer);
      li.querySelector('[data-act=accept]').onclick = () => guard(async () => {
        if (!confirm(`Take “${o.page.name}” from ${o.from.name || 'them'}? It becomes yours and counts toward your storage${o.page.bytes ? ` (${fmtBytes(o.page.bytes)})` : ''}.`)) return;
        await hp.acceptOffer(o.page.id);
        say(`“${o.page.name}” is yours now`);
        refresh();
      });
      li.querySelector('[data-act=decline]').onclick = () => guard(async () => { await hp.declineOffer(o.page.id); say('Declined'); refresh(); });
    }
    for (const li of body.querySelectorAll('li[data-trash]')) {
      const p = l.trash.find(x => x.id === li.dataset.trash);
      li.querySelector('[data-act=untrash]').onclick = () => guard(async () => { await hp.untrash(p); say(`“${p.name}” is back in My pages`); refresh(); });
      li.querySelector('[data-act=forever]').onclick = () => guard(async () => {
        if (!confirm(`Delete “${p.name}” for good? This cannot be undone.`)) return;
        await hp.remove(p, true);
        say('Deleted for good');
        refresh();
      });
    }
  }

  // A page's sharing and settings.
  const d = ui.card('Page');
  ui.cards.page = d;
  async function details(page) {
    let info;
    try { info = await hp.detail(page.id); } catch (e) { return say(e.message); }
    const me = state.me.user;
    const owner = info.role === 'owner';
    const billing = info.owner.id === me.id;
    const roleSel = (r, attrs = '') => `<select ${attrs}>${['viewer', 'editor', 'owner'].map(x => `<option value="${x}"${x === r ? ' selected' : ''}>${x === 'viewer' ? 'Can view' : x === 'editor' ? 'Can edit' : 'Co-owner'}</option>`).join('')}</select>`;
    d.querySelector('h2 span').textContent = info.name;
    d.querySelectorAll(':scope > :not(h2)').forEach(n => n.remove());
    const wrap = el('div', {}, `
      <p>${billing ? 'Yours' : `Owned by ${esc(info.owner.name || 'someone')}`} · ${info.role === 'viewer' ? 'you can view' : info.role === 'editor' ? 'you can edit' : 'you are an owner'}${info.bytes ? ' · ' + fmtBytes(info.bytes) : ''}</p>
      ${owner ? `
        <div class="og-shared-head">People</div>
        <ul class="og-list og-people">${(info.members || []).map(m => `<li data-uid="${esc(m.id)}"><span class="go">${esc(m.name || m.id)}${m.billing ? ' <small>owner</small>' : m.viaLink ? ' <small>joined by link</small>' : ''}</span>
          ${m.billing ? '' : roleSel(m.role, 'data-role') + '<button class="mini" data-act="unshare" title="Remove">✕</button>'}</li>`).join('')}</ul>
        <div class="og-row"><input placeholder="Username or email" data-who>${roleSel('editor', 'data-newrole')}<button class="og-btn primary" data-act="add">Share</button></div>
        ${state.me.linkSharing ? `<div class="og-shared-head">Anyone with the link</div>
        <ul class="og-list">${(info.links || []).map(k => `<li data-link="${k.id}"><span class="go">${k.role === 'editor' ? 'Can edit' : 'Can view'}<small>made ${k.by?.name ? `by ${esc(k.by.name)} ` : ''}${ago(k.created)}</small></span><button class="mini" data-act="unlink">Turn off</button></li>`).join('') || '<li class="og-empty">No links.</li>'}</ul>
        <div class="og-row"><button class="og-btn" data-act="link-view">New view link</button><button class="og-btn" data-act="link-edit">New edit link</button></div>
        <div class="og-row" hidden data-newlink><input readonly><button class="og-btn primary" data-act="copylink">Copy</button></div>` : ''}` : ''}
      ${owner && info.chat ? `
        <div class="og-shared-head">Chat</div>
        <label class="og-check"><input type="checkbox" data-chat="on"${info.chat.on ? ' checked' : ''}> Chat for the people on this page</label>
        <label class="og-check"><input type="checkbox" data-chat="keep"${info.chat.keep ? ' checked' : ''}${info.chat.on ? '' : ' disabled'}> Keep chat history (the last ${info.chat.history} messages)</label>
        <p><small>${info.chat.keep ? 'Messages stay with the page until deleted.' : 'Messages go once everyone has left the page.'}</small></p>` : ''}
      ${billing && info.offer ? `<p>Offered to <b>${esc(info.offer.to.name || 'someone')}</b> ${ago(info.offer.created)}: it moves to them when they accept. <button class="mini" data-act="withdraw">Withdraw</button></p>` : ''}
      ${owner ? '<div data-history></div>' : ''}
      <div class="og-row" style="flex-wrap:wrap">
        <button class="og-btn" data-act="copy">Make a copy</button>
        ${info.role !== 'viewer' ? '<button class="og-btn" data-act="rename">Rename</button>' : ''}
        ${billing ? '<button class="og-btn" data-act="transfer">Give away</button><button class="og-btn" data-act="delete">Delete</button>' : '<button class="og-btn" data-act="leave">Leave</button>'}
        ${billing ? '' : '<button class="og-btn" data-act="report">Report</button>'}
      </div>`);
    d.append(wrap);
    const q = s => wrap.querySelector(s);
    const on = (s, f) => { const b = q(s); if (b) b.onclick = () => guard(f); };
    for (const box of wrap.querySelectorAll('[data-chat]')) {
      box.onchange = () => guard(async () => {
        const r = await hp.chatSettings(page, { [box.dataset.chat]: box.checked });
        say(r.on ? (r.keep ? 'Chat is on; its history is kept' : 'Chat is on; it goes once everyone leaves') : 'Chat is off on this page');
        details(page);
      });
    }
    on('[data-act=add]', async () => {
      const who = q('[data-who]').value.trim();
      if (!who) return;
      await hp.share(page, who, q('[data-newrole]').value);
      say(`Shared with ${who}`);
      details(page);
    });
    for (const li of wrap.querySelectorAll('[data-uid]')) {
      const sel = li.querySelector('[data-role]');
      if (sel) sel.onchange = () => guard(async () => { await hp.setRole(page, li.dataset.uid, sel.value); say('Changed'); });
      const x = li.querySelector('[data-act=unshare]');
      if (x) x.onclick = () => guard(async () => { await hp.unshare(page, li.dataset.uid); say('Removed: they lost access at once'); details(page); });
    }
    for (const li of wrap.querySelectorAll('[data-link]')) {
      li.querySelector('[data-act=unlink]').onclick = () => guard(async () => { await hp.unlink(page, li.dataset.link); say('Link turned off'); details(page); });
    }
    const newLink = async role => {
      const r = await hp.link(page, role);
      const url = `${location.origin}${location.pathname}#l=${r.token}`;
      const row = q('[data-newlink]');
      row.hidden = false;
      row.querySelector('input').value = url;
      navigator.clipboard?.writeText(url).then(() => say('Link copied: anyone signed in who opens it gets in'), () => {});
    };
    on('[data-act=link-view]', () => newLink('viewer'));
    on('[data-act=link-edit]', () => newLink('editor'));
    on('[data-act=copylink]', () => { navigator.clipboard?.writeText(q('[data-newlink] input').value); say('Link copied'); });
    on('[data-act=copy]', async () => {
      await ui.autosave();
      await hp.copy(page, currentPage(), () => waitSynced());
      say('Copying into your account…');
      ui.show(null);
      setTimeout(refresh, 3000);
    });
    on('[data-act=rename]', async () => {
      const name = prompt('Rename the page', info.name);
      if (!name) return;
      await hp.rename(page, name);
      if (page.id === currentPage()) ui.app.og_set_name(name);
      details(page); refresh();
    });
    on('[data-act=transfer]', async () => {
      const who = prompt('Give this page to (username or email). They are asked first; once they accept, it is theirs and counts toward their storage, not yours.');
      if (!who) return;
      const stay = confirm('Once they accept: keep editing it as a co-owner?\n\nOK: stay on it as a co-owner. Cancel: leave it.');
      const r = await hp.transfer(page, who, !stay);
      say(`Offered to ${r.offer?.to?.name || who}: it moves when they accept`);
      details(page); refresh();
    });
    on('[data-act=withdraw]', async () => { await hp.withdraw(page); say('Offer withdrawn'); details(page); refresh(); });
    on('[data-act=delete]', async () => {
      if (!confirm(`Delete “${info.name}”? It goes to your trash for 30 days (gone for everyone it is shared with); restore it from there.`)) return;
      await hp.remove(page);
      say('Moved to the trash: restore it from My account for 30 days');
      ui.show(null); refresh();
    });
    // Owners: what full copies replaced (kept 30 days), to bring back.
    const hist = q('[data-history]');
    if (hist) hp.history(page).then(h => {
      if (!h.replaced?.length || !hist.isConnected) return;
      hist.innerHTML = `<div class="og-shared-head">Earlier versions</div>
        <p><small>Someone saved a full copy of this page that left out earlier work. What it replaced is kept ${h.keepDays} days; bringing it back adds it to the page again (nothing is removed).</small></p>
        <ul class="og-list">${h.replaced.map(r => `<li data-since="${r.seq}"><span class="go">${r.frames} change${r.frames === 1 ? '' : 's'} · ${fmtBytes(r.bytes)}<small>replaced ${ago(r.at)}${r.size != null ? ` by a ${fmtBytes(r.size)} copy` : ''}</small></span><button class="mini" data-act="restore">Bring back</button></li>`).join('')}</ul>`;
      for (const li of hist.querySelectorAll('[data-since]')) {
        li.querySelector('[data-act=restore]').onclick = () => guard(async () => {
          const r = await hp.restore(page, Number(li.dataset.since));
          say(`Brought back ${r.restored} change${r.restored === 1 ? '' : 's'}`);
          details(page);
        });
      }
    }).catch(() => {});
    on('[data-act=leave]', async () => {
      if (!confirm(`Leave “${info.name}”? It goes from Shared with me. Make a copy first to keep one.`)) return;
      await hp.leave(page, me.id);
      ui.show(null); refresh();
    });
    on('[data-act=report]', async () => {
      const reason = prompt('What is wrong with this page? The site\'s admins will look at it.');
      if (!reason) return;
      await hp.report(page, reason);
      say('Reported. Thank you.');
    });
    // Shown again after a change: keep it open (show() toggles).
    if (ui.isOpen() !== 'page') ui.show('page');
  }

  async function guard(f) {
    try { await f(); } catch (e) {
      if (e.status === 401) { state.me = { user: null }; render(); }
      if (e.data?.full) showFull(e.message, e.data);
      else say(e.message || String(e));
    }
  }

  function waitSynced(ms = 20000) {
    return new Promise((ok, fail) => {
      const t0 = Date.now();
      const iv = setInterval(() => {
        const n = ui.status().net || '';
        if (n.startsWith('Synced') || n.startsWith('Back online')) { clearInterval(iv); ok(); }
        else if (Date.now() - t0 > ms) { clearInterval(iv); fail(new Error('Could not open that page to copy it: try again')); }
      }, 300);
    });
  }

  function showFull(msg, u) {
    // Mine: offer more storage. Someone else's page: making a copy into my
    // own account is the way to keep working.
    const mine = !u || u.owner !== false;
    fillCard(full, esc(msg) + (u && u.quota ? `<br><small>${fmtBytes(u.used)} of ${fmtBytes(u.quota)} used</small>` : '')
      + (mine && upgradeUrl() ? '<br>Get more storage and everything you draw is kept in the cloud again.' : ''), mine);
    if (ui.isOpen() !== 'full') ui.show('full');
  }

  function openPanel() {
    if (ui.isOpen() !== 'account') ui.show('account');
    refresh();
  }

  // Links: #l=<token> opens a shared page (once signed in).
  async function fromLink() {
    const m = location.hash.match(/^#l=([A-Za-z0-9_-]{20,64})/);
    if (!m) return;
    if (!state.me?.user) {
      let name = 'a page';
      try { name = `“${(await hp.peek(m[1])).name}”`; } catch (e) { return say(e.message); }
      body.innerHTML = `<p>Sign in to open ${esc(name)}, shared with you by link.</p><div class="og-row"><a class="og-btn primary" href="${esc(signInUrl())}">Sign in</a></div>`;
      return ui.show('account');
    }
    history.replaceState(null, '', location.pathname + location.search);
    await guard(async () => {
      const r = await hp.accept(m[1]);
      await ui.autosave();
      remember(r.page.id, r.open);
      state.open = r.page;
      ui.app.og_join(r.open.link);
      say(`Opened ${r.page.name}: it is under Shared with me now`);
    });
  }

  async function boot() {
    await refresh();
    if (!state.me?.user) {
      await fromLink();
      return;
    }
    // People see who they are on others' screens.
    try {
      const prefs = localStorage.getItem('og-prefs') || '';
      if (state.me.user.name && !/^name=/m.test(prefs)) localStorage.setItem('og-prefs', prefs + `name=${state.me.user.name}\n`);
    } catch (e) {}
    if (location.hash.startsWith('#l=')) return fromLink();
    // Signed in: carry on with the account page last open, else show the list.
    const last = localStorage.getItem('og-hosted-last');
    const l = state.list;
    const page = last && l && [...l.mine, ...l.shared].find(p => p.id === last);
    if (page) {
      await guard(async () => { await hp.open(page); state.open = page; });
    } else if (location.hash === '' || location.hash === '#') {
      ui.show('account');
    }
  }

  return {
    provider: hp,
    boot,
    open: openPanel,
    /** The most bytes one picture may take to be kept in the account (null: no limit here). */
    pictureLimit() {
      if (!state.me?.user || !state.me.maxOpBytes || !currentPage()) return null;
      // Room for what the app wraps around it (stamps, signature, sealing).
      return Math.max(64 * 1024, state.me.maxOpBytes - 32 * 1024);
    },
    /** The room's control messages (text frames on the live socket). */
    control(text) {
      let m;
      try { m = JSON.parse(text); } catch (e) { return; }
      if (chat.control(m) && m.t !== 'welcome') return;
      if (m.t === 'welcome') {
        // A fresh connection: the app now sends whatever the room lacks.
        state.role = m.role; closedFor = null;
        Object.assign(cloud, { live: true, sent: 0, kept: 0, dropped: false });
      }
      else if (m.t === 'kept') cloud.kept++;
      else if (m.t === 'usage') noteUsage(m.used, m.quota);
      else if (m.t === 'peers') { state.peers = m.list; if (ui.isOpen() === 'account') render(); }
      else if (m.t === 'full') { cloud.kept++; cloud.dropped = true; showFull(m.msg, m); }
      else if (m.t === 'denied' || m.t === 'error') { if (m.drop) { cloud.kept++; if (m.t === 'error') cloud.dropped = true; } say(m.msg); }
      else if (m.t === 'kicked') { closedFor = m.msg; say(m.msg); chat.reset(); refresh(); }
    },
    /** Whether to dial a room again (not after being shown the door). */
    mayConnect(url) {
      if (!closedFor || !url.includes(cfg.api)) return true;
      return false;
    },
    get refusal() { return closedFor; },
    /** The live socket opened or closed (url: where to). */
    conn(open, url = '') {
      cloud.open = open && url.includes(cfg.api);
      if (!open) { cloud.live = false; chat.disconnected(); }
      else if (!cloud.open) chat.reset();
    },
    /** A frame went out on the live socket. */
    sent(frame) {
      if (!cloud.open || frame.length < 4) return;
      const tag = String.fromCharCode(frame[0], frame[1], frame[2], frame[3]);
      if (tag === 'PUT ' || tag === 'CKPT') cloud.sent++;
    },
    /** The chat panel (tests: state, open). */
    chat,
    /** saved | saving | unsaved | local (not an account page) | empty */
    cloudState,
    /** Whether closing the tab now would leave work out of the account. */
    warnOnClose() { const c = cloudState(); return c === 'unsaved' || c === 'saving' || c === 'local'; },
  };
}
