// OG Paper web shell: where pages are kept. Signed out, pages live in this
// browser (IndexedDB, as always). On a site that hosts OG Paper (for
// example omegagiven.com/paper/) and signed in there, they live in the
// account: the page list shows on opening, every page opened is a live room
// that saves as you draw (and keeps a copy here for offline), and pages can
// be shared, copied and given away. See docs/HOSTED.md.
//
// A site turns this on with <meta name="og-paper-api" content="/paper/api">
// (and optionally og-paper-signin: where to send people to sign in, with
// {next} for the way back).

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
  remove(page) { forget(page.id); return this.call('DELETE', `/pages/${page.id}`); }
  leave(page, me) { forget(page.id); return this.call('DELETE', `/pages/${page.id}/members/${encodeURIComponent(me)}`); }
  share(page, who, role) { return this.call('POST', `/pages/${page.id}/members`, { who, role }); }
  setRole(page, uid, role) { return this.call('PATCH', `/pages/${page.id}/members/${encodeURIComponent(uid)}`, { role }); }
  unshare(page, uid) { return this.call('DELETE', `/pages/${page.id}/members/${encodeURIComponent(uid)}`); }
  link(page, role) { return this.call('POST', `/pages/${page.id}/links`, { role }); }
  unlink(page, id) { return this.call('DELETE', `/pages/${page.id}/links/${id}`); }
  transfer(page, who, leave) { return this.call('POST', `/pages/${page.id}/transfer`, { who, leave }); }
  report(page, reason) { return this.call('POST', `/pages/${page.id}/report`, { reason }); }
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
const pageOfCanvas = canvas => Object.entries(known()).find(([, v]) => v.canvas === canvas)?.[0] || null;

export function hostedConfig() {
  const meta = n => document.querySelector(`meta[name="${n}"]`)?.content || null;
  const api = meta('og-paper-api') || window.OG_PAPER_HOSTED?.api;
  if (!api) return null;
  return { api, signIn: meta('og-paper-signin') || window.OG_PAPER_HOSTED?.signIn || '/account/?next={next}' };
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
  const full = ui.card('Storage full');
  ui.cards.full = full;
  full.append(el('p', { class: 'msg' }), el('div', { class: 'og-row' },
    '<button class="og-btn primary" data-act="export">Export a copy</button><button class="og-btn" data-act="pages">Delete pages</button>'));
  full.querySelector('[data-act=export]').onclick = () => { ui.show(null); ui.exporter(); };
  full.querySelector('[data-act=pages]').onclick = () => openPanel();

  const currentPage = () => pageOfCanvas(ui.status().canvas);

  async function refresh() {
    try {
      state.me = await hp.me();
      state.list = state.me.user ? await hp.list() : null;
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
      <p>${fmtBytes(u.used)} of ${fmtBytes(u.quota)} used · ${u.pages} of ${u.maxPages} pages${pct >= 100 ? ' · <b>full</b>: export or delete pages to free space' : ''}</p>`;
  }

  function item(p, mine) {
    const live = p.id === currentPage();
    const meta = mine
      ? `${p.bytes ? fmtBytes(p.bytes) + ' · ' : ''}${ago(p.updated)}${p.blocked ? ' · taken down' : ''}`
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
      <div class="og-shared-head">Shared with me</div>
      <ul class="og-list" data-list="shared">${l.shared.map(p => item(p, false)).join('') || '<li class="og-empty">Nothing shared with you yet.</li>'}</ul>`;
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
        <ul class="og-list">${(info.links || []).map(k => `<li data-link="${k.id}"><span class="go">${k.role === 'editor' ? 'Can edit' : 'Can view'}<small>made ${ago(k.created)}</small></span><button class="mini" data-act="unlink">Turn off</button></li>`).join('') || '<li class="og-empty">No links.</li>'}</ul>
        <div class="og-row"><button class="og-btn" data-act="link-view">New view link</button><button class="og-btn" data-act="link-edit">New edit link</button></div>
        <div class="og-row" hidden data-newlink><input readonly><button class="og-btn primary" data-act="copylink">Copy</button></div>` : ''}` : ''}
      <div class="og-row" style="flex-wrap:wrap">
        <button class="og-btn" data-act="copy">Make a copy</button>
        ${info.role !== 'viewer' ? '<button class="og-btn" data-act="rename">Rename</button>' : ''}
        ${billing ? '<button class="og-btn" data-act="transfer">Give away</button><button class="og-btn" data-act="delete">Delete</button>' : '<button class="og-btn" data-act="leave">Leave</button>'}
        ${billing ? '' : '<button class="og-btn" data-act="report">Report</button>'}
      </div>`);
    d.append(wrap);
    const q = s => wrap.querySelector(s);
    const on = (s, f) => { const b = q(s); if (b) b.onclick = () => guard(f); };
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
      const who = prompt('Give this page to (username or email). It then counts toward their storage, not yours.');
      if (!who) return;
      const stay = confirm('Keep editing it as a co-owner?\n\nOK: stay on it as a co-owner. Cancel: leave it.');
      await hp.transfer(page, who, !stay);
      say(`Given to ${who}`);
      ui.show(null); refresh();
    });
    on('[data-act=delete]', async () => {
      if (!confirm(`Delete “${info.name}” for everyone it is shared with? This cannot be undone. Export a copy first to keep it.`)) return;
      await hp.remove(page);
      say('Deleted');
      ui.show(null); refresh();
    });
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
    ui.show('page');
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
    full.querySelector('.msg').innerHTML = esc(msg) + (u && u.quota ? `<br><small>${fmtBytes(u.used)} of ${fmtBytes(u.quota)} used</small>` : '');
    ui.show('full');
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
    /** The room's control messages (text frames on the live socket). */
    control(text) {
      let m;
      try { m = JSON.parse(text); } catch (e) { return; }
      if (m.t === 'welcome') { state.role = m.role; closedFor = null; }
      else if (m.t === 'peers') { state.peers = m.list; if (ui.isOpen() === 'account') render(); }
      else if (m.t === 'full') showFull(m.msg, m);
      else if (m.t === 'denied' || m.t === 'error') say(m.msg);
      else if (m.t === 'kicked') { closedFor = m.msg; say(m.msg); refresh(); }
    },
    /** Whether to dial a room again (not after being shown the door). */
    mayConnect(url) {
      if (!closedFor || !url.includes(cfg.api)) return true;
      return false;
    },
    get refusal() { return closedFor; },
  };
}
