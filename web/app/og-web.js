// OG Paper web shell: the page-side panels around the canvas — bookmarks,
// the timeline, offline copies (download / load a .ogpt file), browser
// autosave, full screen, and (try mode) a guided tour of the canvas. They are
// opened from the app's own settings fan (the gear, top right).
// Used by /app/ and /try/; the canvas itself is the Rust app in ./pkg/.

import init, {
  og_load, og_demo, og_blank, og_status, og_requests, og_set_menu, og_text_request, og_text_done,
  og_bookmark_add, og_bookmark_go, og_bookmark_remove, og_bookmark_rename,
  og_timeline, og_timeline_restore, og_snapshot_request, og_snapshot_take,
} from './pkg/og_paper.js';

const ICONS = {
  play: '<path d="M8 5v14l11-7z"/>',
  pause: '<path d="M8 5v14M16 5v14"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
};
const svg = name => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;

const CSS = `
.og-ui { --face:#fcfbf8; --ink:#1c1c24; --muted:#5d5d68; --edge:#d4d1c7; --accent:#c8285a; --ok:#1e965a;
  font: 14px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; color: var(--ink); }
.og-ui button { font: inherit; color: inherit; cursor: pointer; }
.og-icon { width: 42px; height: 42px; border-radius: 21px; border: 1px solid var(--edge); background: var(--face);
  box-shadow: 0 1px 3px rgba(0,0,0,.15); padding: 0; display: grid; place-items: center; color: #2d2d37; }
.og-icon svg { width: 22px; height: 22px; }
.og-icon[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
.og-card { position: fixed; z-index: 21; top: calc(96px + env(safe-area-inset-top)); right: calc(12px + env(safe-area-inset-right));
  width: min(330px, calc(100vw - 24px)); max-height: calc(100dvh - 200px); overflow: auto; background: var(--face);
  border: 1px solid var(--edge); border-radius: 14px; box-shadow: 0 6px 24px rgba(0,0,0,.18); padding: 14px; }
.og-card[hidden], .og-tl[hidden], .og-loading[hidden] { display: none; }
.og-card h2 { font-size: 15px; margin: 0 0 8px; display: flex; justify-content: space-between; align-items: center; }
.og-card h2 button { border: 0; background: none; padding: 2px; width: 26px; height: 26px; color: var(--muted); }
.og-card h2 button svg { width: 18px; height: 18px; }
.og-card p { margin: 6px 0; color: var(--muted); font-size: 13px; }
.og-row { display: flex; gap: 6px; margin: 8px 0; }
.og-row input { flex: 1; min-width: 0; font: inherit; padding: 8px 10px; border-radius: 9px; border: 1px solid var(--edge); background: #fff; color: var(--ink); }
.og-btn { padding: 8px 12px; border-radius: 9px; border: 1px solid var(--edge); background: #fff; font-weight: 600; }
.og-btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
.og-btn.wide { width: 100%; margin: 4px 0; text-align: left; }
.og-list { list-style: none; margin: 6px 0 0; padding: 0; }
.og-list li { display: flex; align-items: center; gap: 4px; border-top: 1px solid var(--edge); }
.og-list li:first-child { border-top: 0; }
.og-list .go { flex: 1; text-align: left; border: 0; background: none; padding: 9px 4px; min-width: 0; }
.og-list .go small { color: var(--muted); display: block; font-size: 12px; }
.og-list .mini { border: 0; background: none; color: var(--muted); padding: 6px; font-size: 13px; }
.og-empty { color: var(--muted); font-size: 13px; padding: 6px 0; }
.og-tour li { display: flex; gap: 8px; padding: 6px 0; border-top: 1px solid var(--edge); align-items: flex-start; }
.og-tour li:first-child { border-top: 0; }
.og-tour .tick { flex: none; width: 20px; height: 20px; border-radius: 10px; border: 2px solid var(--edge); margin-top: 1px; }
.og-tour li.done .tick { background: var(--ok); border-color: var(--ok); }
.og-tour li.done span { color: var(--muted); text-decoration: line-through; }
.og-tour small { display: block; color: var(--muted); }
.og-progress { height: 6px; border-radius: 3px; background: var(--edge); overflow: hidden; margin: 4px 0 10px; }
.og-progress div { height: 100%; background: var(--ok); width: 0; transition: width .3s; }
.og-tl { position: fixed; z-index: 20; left: calc(10px + env(safe-area-inset-left)); bottom: calc(10px + env(safe-area-inset-bottom));
  right: calc(210px + env(safe-area-inset-right)); background: var(--face); border: 1px solid var(--edge); border-radius: 14px;
  box-shadow: 0 6px 24px rgba(0,0,0,.18); padding: 10px 12px; display: flex; flex-wrap: wrap; gap: 6px 10px; align-items: center; }
.og-tl input[type=range] { flex: 1 1 160px; accent-color: var(--accent); }
.og-tl .when { flex: 1 1 100%; font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; order: -1; display: flex; justify-content: space-between; gap: 8px; }
.og-tl .when b { color: var(--ink); }
.og-tl .og-icon { width: 36px; height: 36px; box-shadow: none; }
.og-loading { position: fixed; inset: 0; z-index: 30; display: grid; place-items: center; background: #f5f3ec; color: #5d5d68;
  font: 15px system-ui, sans-serif; text-align: center; padding: 16px; }
.og-text { position: fixed; z-index: 22; min-width: 160px; min-height: 1.4em; padding: 2px 4px; margin: -3px 0 0 -5px;
  border: 1.5px dashed #466ee6; border-radius: 4px; background: rgba(255,255,255,.85); outline: none; resize: both;
  font-family: "Comic Sans MS", "Segoe Print", system-ui, sans-serif; line-height: 1.25; color: #1c1c24; }
.og-text-hint { position: fixed; z-index: 22; font: 12px system-ui, sans-serif; color: #5d5d68; background: #fcfbf8;
  border: 1px solid #d4d1c7; border-radius: 6px; padding: 2px 6px; }
.og-toast { position: fixed; z-index: 25; left: 50%; bottom: calc(80px + env(safe-area-inset-bottom)); transform: translateX(-50%);
  background: #1c1c24; color: #fff; padding: 8px 14px; border-radius: 10px; font: 14px system-ui, sans-serif; opacity: 0;
  transition: opacity .25s; pointer-events: none; max-width: calc(100vw - 32px); }
.og-toast.on { opacity: .92; }
@media (max-width: 520px) { .og-tl input[type=range] { flex-basis: 60px; } .og-tl .restore { padding: 6px 8px; } .og-tl { right: calc(84px + env(safe-area-inset-right)); bottom: calc(86px + env(safe-area-inset-bottom)); } }
`;

const TOUR = [
  { id: 'draw', text: 'Draw something', hint: 'Pick the pen (bottom right) and drag on the canvas.', done: s => s.drawn > 0 },
  { id: 'zoom', text: 'Zoom in 100×', hint: 'Mouse wheel, trackpad pinch, or two fingers.', done: s => s.zoom >= 2 },
  { id: 'dot', text: 'Find the world inside the dot', hint: 'Zoom into the yellow dot of the big “i”.', done: s => s.zoom >= 3 },
  { id: 'deep', text: 'Go a trillion times deeper', hint: 'Keep zooming through the dots — or Gear → Bookmarks.', done: s => s.zoom >= 12 },
  { id: 'deepdraw', text: 'Write a note at 10^6 or deeper', hint: 'Ink is exact at any depth.', done: s => s.deepDraw >= 6 },
  { id: 'erase', text: 'Erase or undo something', hint: 'Eraser tool, Ctrl+Z, or a two-finger tap.', done: s => s.erased > 0 || s.undos > 0 },
  { id: 'mark', text: 'Bookmark a view and fly back to it', hint: 'Gear (top right) → Bookmarks: save a view, then tap it.', done: (s, f) => f.flewToOwn },
  { id: 'time', text: 'Scrub the timeline', hint: 'Gear → Timeline: press play to watch the canvas being drawn.', done: (s, f) => f.scrubbed },
  { id: 'out', text: 'Zoom out past the home page', hint: 'There is more up there too.', done: s => s.zoom <= -1 },
  { id: 'bottom', text: 'Reach 10^45', hint: 'The bottom of the demo (it is not the bottom of the canvas).', done: s => s.zoom >= 44 },
];

// ---- browser storage (IndexedDB; failures just mean no autosave) ----------

function idb() {
  return new Promise((ok, fail) => {
    const r = indexedDB.open('og-paper', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('canvases');
    r.onsuccess = () => ok(r.result);
    r.onerror = () => fail(r.error);
  });
}
async function idbGet(key) {
  try {
    const db = await idb();
    return await new Promise((ok, fail) => {
      const q = db.transaction('canvases').objectStore('canvases').get(key);
      q.onsuccess = () => ok(q.result || null);
      q.onerror = () => fail(q.error);
    });
  } catch (e) { console.warn('storage', e); return null; }
}
async function idbPut(key, value) {
  try {
    const db = await idb();
    await new Promise((ok, fail) => {
      const t = db.transaction('canvases', 'readwrite');
      t.objectStore('canvases').put(value, key);
      t.oncomplete = ok;
      t.onerror = () => fail(t.error);
    });
    return true;
  } catch (e) { console.warn('storage', e); return false; }
}
const lsGet = k => { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private mode */ } };

// ---- helpers ---------------------------------------------------------------

const el = (tag, attrs = {}, html = '') => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  e.innerHTML = html;
  return e;
};
const esc = s => s.replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const zoomText = z => `10<sup>${(Math.abs(z) < 0.05 ? 0 : z).toFixed(1)}</sup>`;
/** Escape, then show "10^45" as a real superscript. */
const fmt = s => esc(s).replace(/\^(-?\d+)/g, '<sup>$1</sup>');
const when = t => t ? new Date(t).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'medium' }) : '—';

/** Take a fresh snapshot of the canvas (bytes of a .ogpt file). */
async function snapshot(force) {
  og_snapshot_request(force);
  for (let i = 0; i < 120; i++) {
    const s = og_snapshot_take();
    if (s) return s;
    await new Promise(r => requestAnimationFrame(r));
  }
  return null;
}

export async function start({ mode = 'app' } = {}) {
  const isTry = mode === 'try';
  const key = isTry ? 'try' : 'app';
  document.head.append(el('style', {}, CSS));

  const root = el('div', { class: 'og-ui' });
  const loading = el('div', { class: 'og-loading' }, 'Starting the canvas…');
  const toast = el('div', { class: 'og-toast', role: 'status' });
  root.append(loading, toast);
  document.body.append(root);

  let toastTimer;
  const say = msg => {
    toast.innerHTML = fmt(msg);
    toast.classList.add('on');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove('on'), 2600);
  };

  const card = title => {
    const c = el('section', { class: 'og-card', hidden: '' });
    c.append(el('h2', {}, `<span>${title}</span><button aria-label="Close">${svg('close')}</button>`));
    c.querySelector('h2 button').onclick = () => show(null);
    root.append(c);
    return c;
  };

  // ---- cards ----
  const cards = {};
  let open = null;
  function show(name) {
    open = open === name ? null : name;
    for (const [n, c] of Object.entries(cards)) c.hidden = n !== open;
  }

  if (isTry) {
    cards.tour = card('Try the endless canvas');
    cards.tour.append(
      el('p', {}, 'Everything here is the real app. Check these off in any order — your canvas autosaves in this browser. The gear (top right) holds bookmarks, the timeline, offline copies and this tour.'),
      el('div', { class: 'og-progress' }, '<div></div>'),
      el('ul', { class: 'og-list og-tour' }),
      el('button', { class: 'og-btn wide', 'data-act': 'demo' }, 'Reset the demo'),
      el('p', { class: 'og-saved' }, ''));
    cards.tour.querySelector('[data-act=demo]').onclick = () => {
      if (confirm('Reset the demo? Your drawings on it are replaced.')) { openTimeline(false); og_demo(); show(null); }
    };
  }
  cards.bookmarks = card('Bookmarks');
  cards.bookmarks.append(
    el('p', {}, 'Save the current view, then tap a bookmark to fly back to it — across any zoom depth.'),
    el('form', { class: 'og-row' }, '<input name="name" placeholder="Name this view" maxlength="60" autocomplete="off"><button class="og-btn primary">Save view</button>'),
    el('ul', { class: 'og-list' }));

  // ---- the settings fan's items (drawn by the app) ----
  const standalone = matchMedia('(display-mode: fullscreen), (display-mode: standalone)').matches;
  const canFs = document.documentElement.requestFullscreen && !standalone;
  const syncMenu = () => {
    const items = ['new', 'open', 'save', 'bookmarks', 'timeline', 'home'];
    if (canFs && !document.fullscreenElement) items.push('fullscreen');
    if (isTry) items.push('tour');
    og_set_menu(items.join(','));
  };
  document.addEventListener('fullscreenchange', syncMenu);
  async function fullScreen() {
    try { await document.documentElement.requestFullscreen({ navigationUI: 'hide' }); } catch (e) { say('Full screen is not available here'); }
  }

  // ---- timeline bar ----
  const tl = el('div', { class: 'og-tl', hidden: '' });
  tl.innerHTML = `
    <div class="when"><span>Showing the canvas at <b class="at">—</b></span><span class="count"></span></div>
    <button class="og-icon play" title="Play" aria-label="Play">${svg('play')}</button>
    <input type="range" min="0" max="0" value="0" aria-label="Time">
    <button class="og-btn restore" title="Make this moment the current canvas">Restore</button>
    <button class="og-icon close" title="Back to now" aria-label="Close timeline">${svg('close')}</button>`;
  root.append(tl);
  const range = tl.querySelector('input');
  const playBtn = tl.querySelector('.play');
  let tlOpen = false;
  let playing = null;
  const flags = lsGet(`og-tour-flags-${key}`) || {};
  const setFlag = f => { if (!flags[f]) { flags[f] = true; lsSet(`og-tour-flags-${key}`, flags); } };

  function stopPlay() {
    clearInterval(playing);
    playing = null;
    playBtn.innerHTML = svg('play');
    playBtn.title = 'Play';
  }
  function openTimeline(on) {
    tlOpen = on;
    tl.hidden = !on;
    stopPlay();
    if (on) {
      const s = status();
      if (!s.timeline || s.timeline.n === 0) { say('Nothing drawn yet — the timeline fills in as you draw'); tlOpen = false; tl.hidden = true; }
      else og_timeline(s.timeline.n - 1);
    } else og_timeline(-1);
  }
  tl.querySelector('.close').onclick = () => openTimeline(false);
  tl.querySelector('.restore').onclick = () => { stopPlay(); og_timeline_restore(); tlOpen = false; tl.hidden = true; };
  range.oninput = () => { stopPlay(); og_timeline(+range.value); setFlag('scrubbed'); };
  playBtn.onclick = () => {
    if (playing) return stopPlay();
    const n = +range.max + 1;
    if (+range.value >= n - 1) range.value = 0;
    // About 8 seconds for the whole history, never slower than one change per tick.
    const step = Math.max(1, Math.round(n / 260));
    playBtn.innerHTML = svg('pause');
    playBtn.title = 'Pause';
    setFlag('scrubbed');
    playing = setInterval(() => {
      const v = Math.min(n - 1, +range.value + step);
      range.value = v;
      og_timeline(v);
      if (v >= n - 1) stopPlay();
    }, 30);
  };

  // ---- bookmarks ----
  const form = cards.bookmarks.querySelector('form');
  form.onsubmit = e => {
    e.preventDefault();
    og_bookmark_add(form.name.value);
    form.name.value = '';
  };
  let ownMarksFrom = Infinity;
  const markList = cards.bookmarks.querySelector('.og-list');
  markList.onclick = e => {
    const b = e.target.closest('button');
    if (!b) return;
    const i = +b.closest('li').dataset.i;
    if (b.classList.contains('go')) {
      og_bookmark_go(i);
      if (i >= ownMarksFrom) setFlag('flewToOwn');
      if (matchMedia('(max-width: 700px)').matches) show(null);
    } else if (b.dataset.act === 'rename') {
      const cur = status().bookmarks[i]?.name || '';
      const name = prompt('Rename bookmark', cur);
      if (name) og_bookmark_rename(i, name);
    } else if (b.dataset.act === 'remove') og_bookmark_remove(i);
  };
  let marksKey = '';

  // ---- files ----
  const picker = el('input', { type: 'file', accept: '.ogpt,application/octet-stream', hidden: '' });
  root.append(picker);
  const savedNote = cards.tour?.querySelector('.og-saved') || el('p');
  async function download() {
    const bytes = await snapshot(true);
    if (!bytes) return say('Could not take a copy — try again');
    await idbPut(key, bytes);
    const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-');
    const a = el('a', { download: `og-paper-${stamp}.ogpt` });
    a.href = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 10000);
    say('Offline copy downloaded');
  }
  function openCopy() {
    picker.value = '';
    picker.click();
  }
  picker.onchange = async () => {
    const f = picker.files[0];
    if (!f) return;
    if (status().strokes > 0 && !confirm(`Replace the current canvas with “${f.name}”? Download a copy first if you want to keep it.`)) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    og_load(bytes, false);
    openTimeline(false);
    say(`Loaded ${f.name}`);
    show(null);
  };
  // ---- text: a real textarea, so phones show their keyboard ----
  let textBox = null;
  function closeText(commit) {
    if (!textBox) return;
    const { area, hint } = textBox;
    textBox = null;
    og_text_done(commit ? area.value : undefined);
    area.remove();
    hint.remove();
    // Shortcuts work again straight away.
    document.querySelector('canvas')?.focus();
  }
  function openTextEditor() {
    closeText(true);
    let req;
    try { req = JSON.parse(og_text_request()); } catch { return; }
    const area = el('textarea', { class: 'og-text', rows: '1', spellcheck: 'false', 'aria-label': 'Text' });
    area.value = req.text || '';
    // Cap height ~ 0.7 em.
    area.style.fontSize = `${Math.max(10, req.size / 0.7)}px`;
    area.style.left = `${req.x}px`;
    area.style.top = `${req.y}px`;
    area.style.color = req.color || '#1c1c24';
    const hint = el('div', { class: 'og-text-hint' }, 'Enter for a new line · Ctrl+Enter or tap away to finish · Esc to cancel');
    hint.style.left = `${req.x}px`;
    hint.style.top = `${Math.max(4, req.y - 26)}px`;
    const grow = () => { area.style.height = 'auto'; area.style.height = `${area.scrollHeight + 2}px`; area.style.width = `${Math.max(160, Math.min(innerWidth - req.x - 12, area.scrollWidth + 24))}px`; };
    area.addEventListener('input', grow);
    area.addEventListener('keydown', e => {
      if (e.key === 'Escape') { e.preventDefault(); closeText(false); }
      else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); closeText(true); }
      e.stopPropagation();
    });
    area.addEventListener('blur', () => setTimeout(() => closeText(true), 0));
    document.body.append(hint, area);
    textBox = { area, hint };
    grow();
    area.focus();
  }

  function newCanvas() {
    if (status().strokes > 0 && !confirm('Start a new, blank canvas? The current one is replaced (save a copy first to keep it).')) return;
    openTimeline(false);
    og_blank();
    show(null);
  }

  // ---- tour ----
  const tourList = cards.tour?.querySelector('.og-tour');
  if (tourList) {
    tourList.innerHTML = TOUR.map(t => `<li data-id="${t.id}"><div class="tick"></div><span>${fmt(t.text)}<small>${fmt(t.hint)}</small></span></li>`).join('');
  }
  const tourDone = new Set(lsGet(`og-tour-${key}`) || []);

  // ---- boot ----
  try {
    await init();
  } catch (e) {
    loading.textContent = 'This browser could not start the canvas (it needs WebGPU or WebGL2). ' + e;
    return;
  }
  document.addEventListener('pointerdown', e => {
    const c = document.querySelector('canvas');
    if (c && e.target === c) { c.tabIndex = 0; c.focus(); }
  });
  syncMenu();
  const saved = await idbGet(key);
  og_load(saved, isTry);
  if (isTry && !saved) {
    // First visit: open the tour once the canvas is up.
    setTimeout(() => { if (open === null) show('tour'); }, 600);
  }

  let last = {};
  function status() {
    try { last = JSON.parse(og_status()); } catch { /* keep last */ }
    return last;
  }

  let lastSave = 0;
  let saving = false;
  async function autosave() {
    if (saving) return;
    saving = true;
    const bytes = await snapshot(false);
    if (bytes && await idbPut(key, bytes)) {
      lastSave = Date.now();
      savedNote.textContent = `Saved in this browser at ${new Date(lastSave).toLocaleTimeString()}.`;
    }
    saving = false;
  }
  setInterval(() => { if (status().dirty) autosave(); }, 2000);
  document.addEventListener('visibilitychange', () => { if (document.hidden && status().dirty) autosave(); });

  function tick() {
    const s = status();
    if (s.ready) loading.hidden = true;
    for (const r of JSON.parse(og_requests())) {
      if (r === 'save') download();
      else if (r === 'open') openCopy();
      else if (r === 'new') newCanvas();
      else if (r === 'bookmarks') show('bookmarks');
      else if (r === 'timeline') openTimeline(!tlOpen);
      else if (r === 'fullscreen') fullScreen();
      else if (r === 'tour' && cards.tour) show('tour');
      else if (r === 'text') openTextEditor();
    }
    if (s.ready) {
      // Bookmarks list (only rebuilt when it changes).
      const mk = JSON.stringify(s.bookmarks);
      if (mk !== marksKey) {
        const before = marksKey ? JSON.parse(marksKey).length : null;
        marksKey = mk;
        // Bookmarks added in this session count as "your own" for the tour.
        if (before !== null && s.bookmarks.length > before) ownMarksFrom = Math.min(ownMarksFrom, before);
        markList.innerHTML = s.bookmarks.length
          ? s.bookmarks.map((b, i) => `<li data-i="${i}"><button class="go">${fmt(b.name)}<small>zoom ${zoomText(b.zoom)}</small></button>
              <button class="mini" data-act="rename" title="Rename">Rename</button><button class="mini" data-act="remove" title="Delete" aria-label="Delete">✕</button></li>`).join('')
          : '<li class="og-empty">No bookmarks yet.</li>';
      }

      // Timeline.
      const t = s.timeline;
      if (tlOpen && t) {
        range.max = Math.max(0, t.n - 1);
        if (!playing && document.activeElement !== range) range.value = t.i;
        tl.querySelector('.at').textContent = when(t.t);
        tl.querySelector('.count').textContent = `change ${t.i + 1} of ${t.n}`;
      }

      // Tour.
      if (tourList) {
        let changed = false;
        for (const item of TOUR) {
          if (!tourDone.has(item.id) && item.done(s, flags)) {
            tourDone.add(item.id);
            changed = true;
            say(`✓ ${item.text}`);
          }
        }
        if (changed) lsSet(`og-tour-${key}`, [...tourDone]);
        for (const li of tourList.children) li.classList.toggle('done', tourDone.has(li.dataset.id));
        cards.tour.querySelector('.og-progress div').style.width = `${(100 * tourDone.size) / TOUR.length}%`;
      }
    }
    requestAnimationFrame(tick);
  }
  tick();
  return { say };
}
