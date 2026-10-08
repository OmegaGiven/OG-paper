// OG Paper web shell: the page-side panels around the canvas — bookmarks,
// the timeline, offline copies (download / load a .ogpt file), browser
// autosave, full screen, and (try mode) a guided tour of the canvas. They are
// opened from the app's own settings fan (the gear, top right).
// Used by /app/ and /try/; the canvas itself is the Rust app in ./pkg/.

import init, {
  og_load, og_demo, og_blank, og_status, og_requests, og_set_menu, og_text_request, og_text_done, og_font_add,
  og_copy, og_paste_own, og_selection_text, og_wants_text, og_text_field, og_copied_take, og_field_at, og_paste_image, og_paste_text, og_pdf_page,
  og_home, og_bookmark_add, og_bookmark_go, og_view_public, og_shared_view_go, og_shared_view_copy, og_bookmark_remove, og_bookmark_rename, og_bookmark_to_bar,
  og_search, og_search_results, og_search_go, og_export, og_export_take, og_has_selection,
  og_sticker_take, og_sticker_svg, og_sticker_place, og_import, og_merge, og_changes_take, og_merge_quiet, og_set_folder, og_net_url, og_net_take, og_net_open, og_net_recv, og_net_closed, og_join, og_poke, og_rtc_host, og_rtc_closing, og_view_token, og_relay_share, og_dir_requests, og_page_arg, og_set_pages, og_plugin_install, og_pack_install, og_pack_sticker_take, og_run, og_set_name, og_add_server, og_audio_max, og_audio_take, og_audio_take_id, og_audio_recorded, og_audio_cancelled, og_plugin_bytes_take,
  og_timeline, og_timeline_range, og_timeline_restore, og_snapshot_request, og_snapshot_take,
} from './pkg/og_paper.js';

const ICONS = {
  play: '<path d="M8 5v14l11-7z"/>',
  pause: '<path d="M8 5v14M16 5v14"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  fold: '<path d="M6 15l6-6 6 6"/>',
  unfold: '<path d="M6 9l6 6 6-6"/>',
  home: '<path d="M4 11l8-7 8 7M6 10v10h12V10"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
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
.og-card { box-sizing: border-box; position: fixed; z-index: 21; top: calc(96px + env(safe-area-inset-top)); right: calc(12px + env(safe-area-inset-right));
  width: min(330px, calc(100vw - 24px)); max-height: calc(100dvh - 200px); overflow: auto; background: var(--face);
  border: 1px solid var(--edge); border-radius: 14px; box-shadow: 0 6px 24px rgba(0,0,0,.18); padding: 14px; }
.og-card[hidden], .og-tl[hidden], .og-loading[hidden] { display: none; }
.og-card *, .og-card *::before, .og-card *::after { box-sizing: border-box; }
.og-card h2 { cursor: grab; touch-action: none; user-select: none; -webkit-user-select: none; }
.og-card.dragging { transition: none; opacity: .96; } .og-card.dragging h2 { cursor: grabbing; }
.og-card h2 .og-hbtns { display: flex; gap: 2px; align-items: center; }
.og-chips { display: none; }
.og-card.og-collapsed { width: auto; max-width: calc(100vw - 24px); padding: 6px 6px 6px 10px; max-height: none; overflow: visible; }
.og-card.og-collapsed > :not(h2) { display: none; }
.og-card.og-collapsed h2 { margin: 0; gap: 6px; }
.og-card.og-collapsed h2 > span:first-child { font-size: 0; width: 10px; height: 22px; flex: none;
  background: radial-gradient(circle, var(--muted) 1.2px, transparent 1.6px) 0 0 / 5px 5px; opacity: .7; }
.og-card.og-collapsed .og-chips { display: flex; gap: 6px; overflow-x: auto; flex: 1; min-width: 0; scrollbar-width: none; touch-action: pan-x; padding: 2px 0; }
.og-chips::-webkit-scrollbar { display: none; }
.og-card h2 .og-chips button { width: auto; height: auto; flex: none; border: 1px solid var(--edge); background: #fff; color: var(--ink); border-radius: 999px;
  padding: 5px 10px; font: 600 13px/1 inherit; white-space: nowrap; display: flex; align-items: center; gap: 4px; }
.og-card h2 .og-chips button svg { width: 15px; height: 15px; }
.og-card h2 .og-chips button.add { color: var(--muted); }
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
.og-list .mini.pub[aria-pressed="true"] { color: var(--accent, #c8285a); font-weight: 600; }
.og-rec { position: fixed; left: 50%; bottom: calc(96px + env(safe-area-inset-bottom)); transform: translateX(-50%); z-index: 30;
  display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-radius: 999px; background: var(--face); border: 1px solid var(--edge);
  box-shadow: 0 4px 16px rgba(0,0,0,.18); font-variant-numeric: tabular-nums; }
.og-rec .dot { width: 12px; height: 12px; border-radius: 50%; background: #d62f40; animation: og-pulse 1s ease-in-out infinite; }
@keyframes og-pulse { 50% { opacity: .35; } }
@media (prefers-reduced-motion: reduce) { .og-rec .dot { animation: none; } }
.og-shared-head { margin: 10px 0 2px; font-size: 13px; color: var(--muted); font-weight: 600; }
.og-fmt { flex-wrap: wrap; gap: 6px; }
.og-lib { display: grid; grid-template-columns: repeat(auto-fill, minmax(92px, 1fr)); gap: 8px; margin-top: 8px; max-height: 60vh; overflow: auto; }
.og-sticker { position: relative; border: 1px solid var(--edge); border-radius: 10px; background: #fff; padding: 4px; cursor: pointer; display: flex; flex-direction: column; align-items: center; }
.og-sticker img { width: 100%; aspect-ratio: 1; object-fit: contain; pointer-events: none; }
.og-sticker span { font-size: 12px; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.og-sticker .mini { position: absolute; top: 2px; padding: 2px 5px; }
.og-sticker [data-act=rename] { left: 2px; }
.og-sticker [data-act=remove] { right: 2px; }
.og-opt { display: flex; align-items: center; gap: 8px; margin: 8px 0 0; font-size: 14px; }
.og-opt select { padding: 4px 6px; border-radius: 7px; border: 1px solid var(--edge); }
.og-found { color: var(--muted); font-size: 13px; margin: 6px 0 0; }
.og-found:empty { display: none; }
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
.og-tl .dual { position: relative; flex: 1 1 160px; height: 30px; }
.og-tl .dual .track { position: absolute; left: 11px; right: 11px; top: 13px; height: 4px; border-radius: 2px; background: #d9d6cc; }
.og-tl .dual .sel { position: absolute; top: 0; bottom: 0; border-radius: 2px; background: var(--accent); }
.og-tl .dual input { position: absolute; inset: 0; width: 100%; height: 30px; margin: 0; background: none; pointer-events: none;
  -webkit-appearance: none; appearance: none; }
.og-tl .dual input::-webkit-slider-runnable-track { background: none; height: 30px; }
.og-tl .dual input::-moz-range-track { background: none; }
.og-tl .dual input::-webkit-slider-thumb { -webkit-appearance: none; appearance: none; pointer-events: auto; width: 22px; height: 22px;
  margin-top: 4px; border-radius: 50%; background: #fff; border: 3px solid var(--accent); box-shadow: 0 1px 3px rgba(0,0,0,.3); cursor: ew-resize; }
.og-tl .dual input::-moz-range-thumb { pointer-events: auto; width: 16px; height: 16px; border-radius: 50%; background: #fff;
  border: 3px solid var(--accent); box-shadow: 0 1px 3px rgba(0,0,0,.3); cursor: ew-resize; }
.og-tl .dual input.lo::-webkit-slider-thumb { background: var(--accent); }
.og-tl .dual input.lo::-moz-range-thumb { background: var(--accent); }
.og-tl .when { flex: 1 1 100%; font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; order: -1; display: flex; justify-content: space-between; gap: 8px; }
.og-tl .when b { color: var(--ink); }
.og-tl .og-icon { width: 36px; height: 36px; box-shadow: none; }
.og-loading { position: fixed; inset: 0; z-index: 30; display: grid; place-items: center; background: #f5f3ec; color: #5d5d68;
  font: 15px system-ui, sans-serif; text-align: center; padding: 16px; }
.og-text { position: fixed; z-index: 22; min-width: 160px; min-height: 1.4em; padding: 2px 4px; margin: -3px 0 0 -5px;
  border: 1.5px dashed #466ee6; border-radius: 4px; background: rgba(255,255,255,.85); outline: none; resize: none; overflow: hidden;
  font-family: "Comic Sans MS", "Segoe Print", system-ui, sans-serif; line-height: 1.25; color: #1c1c24; }
.og-text-hint { position: fixed; z-index: 22; font: 12px system-ui, sans-serif; color: #5d5d68; background: #fcfbf8;
  border: 1px solid #d4d1c7; border-radius: 6px; padding: 2px 6px; }
.og-toast { position: fixed; z-index: 25; left: 50%; bottom: calc(80px + env(safe-area-inset-bottom)); transform: translateX(-50%);
  background: #1c1c24; color: #fff; padding: 8px 14px; border-radius: 10px; font: 14px system-ui, sans-serif; opacity: 0;
  transition: opacity .25s; pointer-events: none; max-width: calc(100vw - 32px); }
.og-toast.on { opacity: .92; }
@media (max-width: 520px) { .og-tl .dual { flex-basis: 60px; } .og-tl .restore, .og-tl .mark { padding: 6px 8px; } .og-tl { right: calc(84px + env(safe-area-inset-right)); bottom: calc(86px + env(safe-area-inset-bottom)); } }
`;

const TOUR = [
  { id: 'draw', text: 'Draw something', hint: 'Pick the pen (bottom right) and drag on the canvas.', done: s => s.drawn > 0 },
  { id: 'zoom', text: 'Zoom in 100×', hint: 'Mouse wheel, trackpad pinch, or two fingers.', done: s => s.zoom >= 2 },
  { id: 'dot', text: 'Find the world inside the dot', hint: 'Zoom into the yellow dot of the big “i”.', done: s => s.zoom >= 3 },
  { id: 'deep', text: 'Go a trillion times deeper', hint: 'Keep zooming through the dots — or Gear → Views.', done: s => s.zoom >= 12 },
  { id: 'deepdraw', text: 'Write a note at 10^6 or deeper', hint: 'Ink is exact at any depth.', done: s => s.deepDraw >= 6 },
  { id: 'erase', text: 'Erase or undo something', hint: 'Eraser tool, Ctrl+Z, or a two-finger tap.', done: s => s.erased > 0 || s.undos > 0 },
  { id: 'mark', text: 'Save a view and fly back to it', hint: 'Gear (top right) → Views: save a view, then tap it.', done: (s, f) => f.flewToOwn },
  { id: 'time', text: 'Scrub the timeline', hint: 'Gear → Timeline: press play to watch the canvas being drawn.', done: (s, f) => f.scrubbed },
  { id: 'street', text: 'Walk the endless street', hint: 'Gear → Views → Endless street, then keep zooming into the far end: its portal shows the street again.', done: s => s.passes >= 2 },
  { id: 'out', text: 'Zoom out past the home page', hint: 'There is more up there too.', done: s => s.zoom <= -1 },
  { id: 'bottom', text: 'Reach 10^45', hint: 'The bottom of the demo (it is not the bottom of the canvas).', done: s => s.zoom >= 44 },
];

// ---- browser storage (IndexedDB; failures just mean no autosave) ----------

function idb() {
  return new Promise((ok, fail) => {
    // v2 adds the fonts you loaded, v3 the library.
    const r = indexedDB.open('og-paper', 3);
    r.onupgradeneeded = () => {
      const db = r.result;
      if (!db.objectStoreNames.contains('canvases')) db.createObjectStore('canvases');
      if (!db.objectStoreNames.contains('fonts')) db.createObjectStore('fonts');
      if (!db.objectStoreNames.contains('library')) db.createObjectStore('library');
    };
    r.onsuccess = () => ok(r.result);
    r.onerror = () => fail(r.error);
  });
}
async function idbGet(key, store = 'canvases') {
  try {
    const db = await idb();
    return await new Promise((ok, fail) => {
      const q = db.transaction(store).objectStore(store).get(key);
      q.onsuccess = () => ok(q.result || null);
      q.onerror = () => fail(q.error);
    });
  } catch (e) { console.warn('storage', e); return null; }
}
async function idbPut(key, value, store = 'canvases') {
  try {
    const db = await idb();
    await new Promise((ok, fail) => {
      const t = db.transaction(store, 'readwrite');
      t.objectStore(store).put(value, key);
      t.oncomplete = ok;
      t.onerror = () => fail(t.error);
    });
    return true;
  } catch (e) { console.warn('storage', e); return false; }
}
async function idbDel(key, store) {
  try {
    const db = await idb();
    await new Promise((ok, fail) => {
      const t = db.transaction(store, 'readwrite');
      t.objectStore(store).delete(key);
      t.oncomplete = ok;
      t.onerror = () => fail(t.error);
    });
  } catch (e) { console.warn('storage', e); }
}
async function idbAll(store) {
  try {
    const db = await idb();
    return await new Promise((ok, fail) => {
      const q = db.transaction(store).objectStore(store).getAll();
      q.onsuccess = () => ok(q.result || []);
      q.onerror = () => fail(q.error);
    });
  } catch (e) { console.warn('storage', e); return []; }
}

// ---- fonts -------------------------------------------------------------------

/** Let the page use a font too (the text box shows what you type in it). */
async function pageFont(name, bytes) {
  try {
    const face = new FontFace(name, bytes);
    await face.load();
    document.fonts.add(face);
  } catch (e) { console.warn('font', name, e); }
}

/** The bundled fonts (in the background), then the ones you added. */
async function loadFonts() {
  try {
    const list = await (await fetch(new URL('./fonts/fonts.json', import.meta.url))).json();
    await Promise.all(list.map(async f => {
      const bytes = new Uint8Array(await (await fetch(new URL(`./fonts/${f.file}`, import.meta.url))).arrayBuffer());
      const r = og_font_add(f.name, f.category, bytes, false, false);
      if (r.startsWith('!')) console.warn(f.name, r);
      else pageFont(f.name, bytes);
    }));
  } catch (e) { console.warn('fonts', e); }
  for (const f of await idbAll('fonts')) {
    if (!f || !f.bytes) continue;
    const r = og_font_add(undefined, 'Yours', f.bytes, true, false);
    if (!r.startsWith('!')) pageFont(r, f.bytes);
  }
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

  // Cards move by their title bar (mouse, pen or finger) and remember
  // where they were left, kept on screen.
  const posKey = c => 'og-card-pos:' + c.dataset.name;
  // Positions are in screen px; a zoomed panel layer (demos) scales its own.
  const uiZoom = () => parseFloat(getComputedStyle(root).zoom) || 1;
  const setPos = (c, x, y) => { const z = uiZoom(); c.style.left = x / z + 'px'; c.style.top = y / z + 'px'; };
  const keepOnScreen = c => {
    if (!c.style.left) return;
    const r = c.getBoundingClientRect();
    const x = Math.min(Math.max(8, r.left), Math.max(8, innerWidth - r.width - 8));
    const y = Math.min(Math.max(8, r.top), Math.max(8, innerHeight - r.height - 8));
    setPos(c, x, y);
  };
  const placeCard = c => {
    let p = null;
    try { p = JSON.parse(localStorage.getItem(posKey(c)) || 'null'); } catch (e) {}
    if (p && Number.isFinite(p.x) && Number.isFinite(p.y)) {
      setPos(c, p.x, p.y); c.style.right = 'auto';
      requestAnimationFrame(() => keepOnScreen(c));
    }
  };
  const draggable = c => {
    const h = c.querySelector('h2');
    let drag = null;
    h.addEventListener('pointerdown', e => {
      if (e.button || e.target.closest('button, input, .og-chips')) return;
      const r = c.getBoundingClientRect();
      drag = { id: e.pointerId, dx: e.clientX - r.left, dy: e.clientY - r.top };
      h.setPointerCapture(e.pointerId);
      c.classList.add('dragging');
      e.preventDefault();
    });
    h.addEventListener('pointermove', e => {
      if (!drag || e.pointerId !== drag.id) return;
      c.style.right = 'auto';
      setPos(c, e.clientX - drag.dx, e.clientY - drag.dy);
      keepOnScreen(c);
    });
    const end = e => {
      if (!drag || e.pointerId !== drag.id) return;
      drag = null;
      c.classList.remove('dragging');
      const r = c.getBoundingClientRect();
      try { localStorage.setItem(posKey(c), JSON.stringify({ x: r.left, y: r.top })); } catch (e) {}
    };
    h.addEventListener('pointerup', end);
    h.addEventListener('pointercancel', end);
    // Double-tap the title bar: back to where it starts.
    h.addEventListener('dblclick', e => {
      if (e.target.closest('button, .og-chips')) return;
      c.style.left = c.style.top = c.style.right = '';
      try { localStorage.removeItem(posKey(c)); } catch (e) {}
    });
  };
  addEventListener('resize', () => document.querySelectorAll('.og-card').forEach(keepOnScreen));

  const card = title => {
    const c = el('section', { class: 'og-card', hidden: '', 'data-name': title });
    c.append(el('h2', {}, `<span>${title}</span><span class="og-hbtns"><button class="close" aria-label="Close" title="Close">${svg('close')}</button></span>`));
    c.querySelector('h2 .close').onclick = () => { if (c === cards.bookmarks) bmBar = false; show(null); if (c === cards.bookmarks) syncCards(); };
    draggable(c);
    placeCard(c);
    root.append(c);
    return c;
  };

  // ---- cards ----
  const cards = {};
  let open = null;
  // Bookmarks folded into a bar stays up while other cards come and go.
  let bmCollapsed = false, bmBar = false;
  try { bmCollapsed = localStorage.getItem('og-bm-collapsed') === '1'; } catch (e) {}
  function syncCards() {
    for (const [n, c] of Object.entries(cards)) {
      c.hidden = !(n === open || (n === 'bookmarks' && bmCollapsed && bmBar));
    }
  }
  function show(name) {
    open = open === name ? null : name;
    if (open === 'bookmarks') bmBar = true;
    syncCards();
  }
  // Esc closes an open card (the app closes its own menus).
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && open && !(e.target instanceof HTMLTextAreaElement)) show(null);
  });

  if (isTry) {
    cards.tour = card('Try the endless canvas');
    cards.tour.append(
      el('p', {}, 'Everything here is the real app. Check these off in any order — your canvas autosaves in this browser. The gear (top right) holds saved views, the timeline, offline copies and this tour.'),
      el('div', { class: 'og-progress' }, '<div></div>'),
      el('ul', { class: 'og-list og-tour' }),
      el('button', { class: 'og-btn wide', 'data-act': 'demo' }, 'Reset the demo'),
      el('p', { class: 'og-saved' }, ''));
    cards.tour.querySelector('[data-act=demo]').onclick = () => {
      if (confirm('Reset the demo? Your drawings on it are replaced.')) { openTimeline(false); og_demo(); show(null); }
    };
  }
  cards.bookmarks = card('Views');
  cards.bookmarks.append(
    el('p', {}, 'Save the current view, then tap it to fly back — across any zoom depth.'),
    el('form', { class: 'og-row' }, '<input name="name" placeholder="Name this view" maxlength="60" autocomplete="off"><button class="og-btn primary">Save view</button>'),
    el('ul', { class: 'og-list og-home' }, '<li><button class="go" data-home>Home<small>where the canvas starts</small></button></li>'),
    el('ul', { class: 'og-list og-marks' }),
    el('h3', { class: 'og-shared-head', hidden: '' }, 'Public views on this page'),
    el('ul', { class: 'og-list og-shared', hidden: '' }));
  // Folded: a bar of chips that scrolls sideways (Home first, then each
  // bookmark by its initials), with + to save the view here.
  {
    const h = cards.bookmarks.querySelector('h2');
    const chips = el('div', { class: 'og-chips' });
    h.insertBefore(chips, h.querySelector('.og-hbtns'));
    const fold = el('button', { class: 'fold', 'aria-label': 'Fold into a bar', title: 'Fold into a bar' }, svg('fold'));
    h.querySelector('.og-hbtns').prepend(fold);
    const setFold = on => {
      bmCollapsed = on;
      cards.bookmarks.classList.toggle('og-collapsed', on);
      fold.innerHTML = svg(on ? 'unfold' : 'fold');
      fold.title = fold.ariaLabel = on ? 'Open the full list of views' : 'Fold into a bar';
      try { localStorage.setItem('og-bm-collapsed', on ? '1' : '0'); } catch (e) {}
      if (on) { bmBar = true; if (open === 'bookmarks') open = null; }
      else if (bmBar) open = 'bookmarks';
      syncCards();
      requestAnimationFrame(() => keepOnScreen(cards.bookmarks));
    };
    fold.onclick = () => setFold(!bmCollapsed);
    cards.bookmarks.classList.toggle('og-collapsed', bmCollapsed);
    fold.innerHTML = svg(bmCollapsed ? 'unfold' : 'fold');
    chips.onclick = e => {
      const b = e.target.closest('button');
      if (!b) return;
      if (b.dataset.home != null) og_home();
      else if (b.classList.contains('add')) {
        const name = prompt('Name this view', '');
        if (name != null) og_bookmark_add(name);
      } else og_bookmark_go(+b.dataset.i);
    };
    cards.bookmarks.renderChips = marks => {
      const abbr = n => {
        const w = n.trim().split(/\s+/).filter(Boolean);
        if (!w.length) return '?';
        if (w.length === 1) return w[0].slice(0, 4);
        return w.slice(0, 3).map(x => [...x][0]).join('').toUpperCase();
      };
      chips.innerHTML = `<button data-home title="Home: where the canvas starts">${svg('home')}Home</button>`
        + marks.map((b, i) => `<button data-i="${i}" title="${esc(b.name)}${b.at != null ? ` (as it was on ${when(b.at)})` : ''}">${b.at != null ? '◷ ' : ''}${esc(abbr(b.name))}</button>`).join('')
        + `<button class="add" title="Save this view">${svg('plus')}</button>`;
    };
    cards.bookmarks.renderChips([]);
  }
  // Home is always there, above your own bookmarks.
  cards.bookmarks.querySelector('[data-home]').onclick = () => {
    og_home();
    if (matchMedia('(max-width: 700px)').matches) show(null);
  };

  // ---- search ----
  cards.search = card('Search text');
  cards.search.append(
    el('form', { class: 'og-row' }, '<input name="q" type="search" placeholder="Find text anywhere on the canvas" autocomplete="off" enterkeyhint="search">'),
    el('p', { class: 'og-found' }, ''),
    el('ul', { class: 'og-list' }));
  const sForm = cards.search.querySelector('form');
  const sList = cards.search.querySelector('.og-list');
  const sFound = cards.search.querySelector('.og-found');
  let sHits = [];
  const runSearch = () => {
    og_search(sForm.q.value);
    // The app runs the search on its next frame.
    setTimeout(() => {
      sHits = JSON.parse(og_search_results() || '[]');
      const q = sForm.q.value.trim();
      sFound.textContent = !q ? '' : sHits.length ? `${sHits.length} found — tap one to go there` : 'No text matches';
      sList.replaceChildren(...sHits.map((h, i) => {
        const li = el('li');
        const b = el('button', { class: 'go', 'data-i': i });
        b.textContent = h.text;
        b.append(el('small', {}, `zoom 10^${h.zoom}`));
        li.append(b);
        return li;
      }));
    }, 50);
  };
  sForm.q.oninput = runSearch;
  sForm.onsubmit = e => {
    e.preventDefault();
    if (sHits[0]) goHit(0);
  };
  const goHit = i => {
    og_search_go(sHits[i].g);
    if (matchMedia('(max-width: 700px)').matches) show(null);
  };
  sList.onclick = e => {
    const b = e.target.closest('button');
    if (b) goHit(+b.dataset.i);
  };

  // ---- export ----
  cards.export = card('Export');
  cards.export.append(
    el('p', {}, 'Save what is on screen — or just the selection — as a picture or a document.'),
    el('div', { class: 'og-row og-fmt' },
      '<button class="og-btn primary" data-f="png">PNG</button><button class="og-btn" data-f="jpg">JPEG</button><button class="og-btn" data-f="svg">SVG</button><button class="og-btn" data-f="pdf">PDF</button>'),
    el('label', { class: 'og-opt' }, '<input type="checkbox" name="sel"> Only the selection'),
    el('label', { class: 'og-opt' }, '<input type="checkbox" name="bg" checked> Paper background (off = transparent PNG/SVG)'),
    el('label', { class: 'og-opt' }, 'Picture size <select name="scale"><option value="1">1×</option><option value="2" selected>2×</option><option value="4">4×</option></select>'),
    el('p', {}, 'Or keep the whole canvas, to open, merge or share later:'),
    el('div', { class: 'og-row' }, '<button class="og-btn" data-act="copy">OG Paper copy (.ogpt)</button>'));
  cards.export.querySelector('[data-act=copy]').onclick = () => { download(); show(null); };
  const xSel = cards.export.querySelector('[name=sel]');
  const xBg = cards.export.querySelector('[name=bg]');
  const xScale = cards.export.querySelector('[name=scale]');
  async function exportAs(f) {
    og_export(f, xSel.checked, xBg.checked);
    let bytes = null;
    try {
      for (let i = 0; i < 120 && !bytes; i++) {
        bytes = og_export_take();
        if (!bytes) await new Promise(r => requestAnimationFrame(r));
      }
    } catch (e) { return say(`Export failed: ${e}`); }
    if (!bytes) return say('Export failed — try again');
    let blob;
    if (f === 'png' || f === 'jpg') {
      // The app hands over an SVG; the browser rasterises it.
      const url = URL.createObjectURL(new Blob([bytes], { type: 'image/svg+xml' }));
      try {
        const img = new Image();
        img.src = url;
        await img.decode();
        const k = +xScale.value;
        const c = document.createElement('canvas');
        c.width = Math.max(1, Math.round(img.width * k));
        c.height = Math.max(1, Math.round(img.height * k));
        if (c.width * c.height > 16000 * 16000) return say('Too big for a picture; try a smaller size');
        c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
        blob = await new Promise(r => c.toBlob(r, f === 'png' ? 'image/png' : 'image/jpeg', 0.92));
      } finally { URL.revokeObjectURL(url); }
      if (!blob) return say('Export failed — the picture may be too big');
    } else {
      blob = new Blob([bytes], { type: f === 'svg' ? 'image/svg+xml' : 'application/pdf' });
    }
    const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-');
    const a = el('a', { download: `og-paper-${xSel.checked ? 'selection' : 'view'}-${stamp}.${f}` });
    a.href = URL.createObjectURL(blob);
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 10000);
    say(`Exported ${f.toUpperCase()}`);
  }
  cards.export.querySelector('.og-fmt').onclick = e => {
    const b = e.target.closest('button');
    if (b) exportAs(b.dataset.f);
  };
  const openExport = () => {
    const has = og_has_selection();
    xSel.disabled = !has;
    xSel.checked = has;
    if (open !== 'export') show('export');
  };

  // ---- library (sticker book) ----
  cards.library = card('Library');
  cards.library.append(
    el('p', { class: 'og-found' }, ''),
    el('div', { class: 'og-lib' }));
  const libGrid = cards.library.querySelector('.og-lib');
  const libNote = cards.library.querySelector('.og-found');
  const STICKER = 'application/x-og-sticker';
  let lib = [];
  const thumbs = new Map();
  async function libLoad() {
    lib = (await idbAll('library')).sort((a, b) => b.t - a.t);
    libNote.textContent = lib.length
      ? 'Tap to place a copy, or drag one onto the canvas.'
      : 'Empty. Select something, then press “Add to library” in the selection panel.';
    libGrid.replaceChildren(...lib.map(it => {
      let url = thumbs.get(it.id);
      if (!url) {
        url = URL.createObjectURL(new Blob([og_sticker_svg(it.bytes, 160)], { type: 'image/svg+xml' }));
        thumbs.set(it.id, url);
      }
      const d = el('div', { class: 'og-sticker', draggable: 'true', 'data-id': it.id, title: it.name });
      d.append(el('img', { src: url, alt: it.name, draggable: 'false' }),
        el('span', {}, ''),
        el('button', { class: 'mini', 'data-act': 'rename', 'aria-label': 'Rename', title: 'Rename' }, '✎'),
        el('button', { class: 'mini', 'data-act': 'remove', 'aria-label': 'Remove', title: 'Remove' }, '×'));
      d.querySelector('span').textContent = it.name;
      return d;
    }));
  }
  libGrid.onclick = async e => {
    const d = e.target.closest('.og-sticker');
    if (!d) return;
    const it = lib.find(x => x.id === d.dataset.id);
    if (!it) return;
    const act = e.target.closest('button')?.dataset.act;
    if (act === 'remove') {
      if (!confirm(`Remove “${it.name}” from the library?`)) return;
      await idbDel(it.id, 'library');
      URL.revokeObjectURL(thumbs.get(it.id) || '');
      thumbs.delete(it.id);
      return libLoad();
    }
    if (act === 'rename') {
      const name = prompt('Name', it.name);
      if (name) { it.name = name; await idbPut(it.id, it, 'library'); libLoad(); }
      return;
    }
    og_sticker_place(it.bytes);
    if (matchMedia('(max-width: 700px)').matches) show(null);
  };
  libGrid.addEventListener('dragstart', e => {
    const d = e.target.closest('.og-sticker');
    if (d) { e.dataTransfer.setData(STICKER, d.dataset.id); e.dataTransfer.effectAllowed = 'copy'; }
  });
  async function stickerSave() {
    const bytes = og_sticker_take();
    if (!bytes) return;
    const name = prompt('Name it for the library', `Sticker ${lib.length + 1}`);
    if (name === null) return;
    const id = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 7)}`;
    if (await idbPut(id, { id, name: name || 'Sticker', bytes, t: Date.now() }, 'library')) {
      say('Added to the library');
      await libLoad();
    } else say('Could not save it — is site storage blocked?');
  }

  // ---- the settings fan's items (drawn by the app) ----
  const standalone = matchMedia('(display-mode: fullscreen), (display-mode: standalone)').matches;
  const canFs = document.documentElement.requestFullscreen && !standalone;
  const syncMenu = () => {
    const items = ['pages', 'new', 'open', 'import', 'merge', 'changes', 'folder', 'live', 'connect', 'export', 'paste', 'library', 'picture', 'search', 'bookmarks', 'timeline', 'home', 'layout', 'plugins', 'diagram', 'hotkeys'];
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
    <div class="when"><span class="at">—</span><span class="count"></span></div>
    <button class="og-icon play" title="Play" aria-label="Play">${svg('play')}</button>
    <div class="dual" title="Drag the left handle to start later, the right one to end earlier">
      <div class="track"><div class="sel"></div></div>
      <input class="lo" type="range" min="0" max="0" value="0" aria-label="Window start">
      <input class="hi" type="range" min="0" max="0" value="0" aria-label="Window end">
    </div>
    <button class="og-btn mark" title="Save this moment as a view: fly back to it, as it was then, any time">Save view</button>
    <button class="og-btn restore" title="Make the moment at the right handle the current canvas (the left handle only narrows the view)">Restore</button>
    <button class="og-icon close" title="Back to now" aria-label="Close timeline">${svg('close')}</button>`;
  root.append(tl);
  // Two handles: lo = first change shown, hi = the moment shown. Only ink
  // drawn between them (and still there at hi) is on screen.
  const lo = tl.querySelector('input.lo');
  const range = tl.querySelector('input.hi');
  const sel = tl.querySelector('.sel');
  const paintSel = () => {
    const max = Math.max(1, +range.max);
    sel.style.left = `${(+lo.value / max) * 100}%`;
    sel.style.right = `${100 - (+range.value / max) * 100}%`;
  };
  const showRange = () => { paintSel(); og_timeline_range(+lo.value, +range.value); };
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
      else { lo.max = range.max = s.timeline.n - 1; lo.value = 0; range.value = s.timeline.n - 1; showRange(); }
    } else og_timeline(-1);
  }
  tl.querySelector('.close').onclick = () => openTimeline(false);
  tl.querySelector('.mark').onclick = () => {
    stopPlay();
    const name = prompt('Name this moment', '');
    if (name !== null) og_bookmark_add(name);
  };
  tl.querySelector('.restore').onclick = () => { stopPlay(); og_timeline_restore(); tlOpen = false; tl.hidden = true; };
  range.oninput = () => {
    stopPlay();
    if (+range.value < +lo.value) range.value = lo.value;
    showRange();
    setFlag('scrubbed');
  };
  lo.oninput = () => {
    stopPlay();
    if (+lo.value > +range.value) lo.value = range.value;
    showRange();
    setFlag('scrubbed');
  };
  // Whichever handle is nearer the pointer gets the drag (they can overlap).
  tl.querySelector('.dual').addEventListener('pointerdown', e => {
    const r = e.currentTarget.getBoundingClientRect();
    const v = ((e.clientX - r.left) / r.width) * Math.max(1, +range.max);
    const nearLo = Math.abs(v - +lo.value) < Math.abs(v - +range.value) || (+lo.value === +range.value && v < +lo.value);
    lo.style.zIndex = nearLo ? 2 : 1;
    range.style.zIndex = nearLo ? 1 : 2;
  }, true);
  playBtn.onclick = () => {
    if (playing) return stopPlay();
    const n = +range.max + 1;
    // Play the window: the right handle runs from the left one to the end.
    if (+range.value >= n - 1) range.value = lo.value;
    // About 8 seconds for the whole history, never slower than one change per tick.
    const step = Math.max(1, Math.round(n / 260));
    playBtn.innerHTML = svg('pause');
    playBtn.title = 'Pause';
    setFlag('scrubbed');
    playing = setInterval(() => {
      const v = Math.min(n - 1, +range.value + step);
      range.value = v;
      showRange();
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
  const markList = cards.bookmarks.querySelector('.og-marks');
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
      const name = prompt('Rename view', cur);
      if (name) og_bookmark_rename(i, name);
    } else if (b.dataset.act === 'remove') og_bookmark_remove(i);
    else if (b.dataset.act === 'bar') og_bookmark_to_bar(i);
    else if (b.dataset.act === 'public') og_view_public(i, b.getAttribute('aria-pressed') !== 'true');
  };
  // Others' public views: fly there, or keep a copy.
  const sharedList = cards.bookmarks.querySelector('.og-shared');
  const sharedHead = cards.bookmarks.querySelector('.og-shared-head');
  sharedList.onclick = e => {
    const b = e.target.closest('button');
    if (!b) return;
    const i = +b.closest('li').dataset.i;
    if (b.classList.contains('go')) {
      og_shared_view_go(i);
      if (matchMedia('(max-width: 700px)').matches) show(null);
    } else if (b.dataset.act === 'copy') og_shared_view_copy(i);
  };
  let sharedKey = '';
  let marksKey = '';

  // ---- files ----
  const picker = el('input', { type: 'file', accept: '.ogpt,application/octet-stream', hidden: '' });
  root.append(picker);
  const savedNote = cards.tour?.querySelector('.og-saved') || el('p');
  async function download() {
    const bytes = await snapshot(true);
    if (!bytes) return say('Could not take a copy — try again');
    await idbPut(isTry ? 'try' : 'page:' + status().canvas, bytes);
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
  // ---- live connection: the app speaks its protocol, the page carries it ----
  let ws = null;
  function netClose() {
    if (ws) { const w = ws; ws = null; w.onclose = null; try { w.close(); } catch (e) {} }
  }
  function netConnect(url) {
    netClose();
    let w;
    try { w = new WebSocket(url); } catch (e) { og_net_closed(0, String(e.message || e)); setTimeout(og_poke, 3200); return; }
    w.binaryType = 'arraybuffer';
    ws = w;
    w.onopen = () => og_net_open(0);
    w.onmessage = e => og_net_recv(0, new Uint8Array(e.data));
    w.onclose = e => {
      if (ws !== w) return;
      ws = null;
      og_net_closed(0, e.reason || (location.protocol === 'https:' && url.startsWith('ws:') ? 'an https page needs a wss:// address' : 'connection closed'));
      setTimeout(og_poke, 3200);
    };
  }
  // Pages: open a page kept in this browser (saving the open one first).
  async function pageOpen(c) {
    await autosave();
    const bytes = await idbGet('page:' + c);
    if (!bytes) return false;
    og_load(bytes, false);
    // Its name lives in the list.
    const name = pages.find(p => p.c === c)?.name;
    if (name) og_set_name(name);
    lsSet('og-page', c);
    return true;
  }
  // Pages: short connections to servers' directories (ids from 1000).
  const dirWs = {};
  function dirOpen(c, url) {
    try { dirWs[c]?.close(); } catch (e) {}
    let w;
    try { w = new WebSocket(url); } catch (e) { og_net_closed(c, String(e.message || e)); return; }
    w.binaryType = 'arraybuffer';
    dirWs[c] = w;
    w.onopen = () => og_net_open(c);
    w.onmessage = e => og_net_recv(c, new Uint8Array(e.data));
    w.onclose = e => { if (dirWs[c] === w) delete dirWs[c]; og_net_closed(c, e.reason || (location.protocol === 'https:' && url.startsWith('ws:') ? 'an https page needs a wss:// address' : 'closed')); };
  }
  // Frames come tagged with their connection: 0 is the WebSocket (or, for
  // a guest, its WebRTC channel); others are hosted guests' channels.
  function netFlush() {
    for (let b = og_net_take(); b; b = og_net_take()) {
      const conn = b[0] | (b[1] << 8) | (b[2] << 16) | (b[3] << 24);
      const frame = b.subarray(4);
      if (conn >= 1000) { if (dirWs[conn]?.readyState === 1) dirWs[conn].send(frame); }
      else if (conn === 0 && ws && ws.readyState === 1) ws.send(frame);
      else if (rtc.chans[conn] && rtc.chans[conn].readyState === 'open') rtc.chans[conn].send(frame);
    }
  }
  // ---- no-server live (WebRTC): one-time invites, set up by link ----
  const rtc = { pcs: {}, chans: {}, next: 1 };
  const ICE = [{ urls: 'stun:stun.l.google.com:19302' }];
  const pack = o => btoa(unescape(encodeURIComponent(JSON.stringify(o)))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const unpack = t => JSON.parse(decodeURIComponent(escape(atob(t.trim().replace(/-/g, '+').replace(/_/g, '/')))));
  const iceDone = pc => new Promise(r => {
    if (pc.iceGatheringState === 'complete') return r();
    pc.addEventListener('icegatheringstatechange', () => pc.iceGatheringState === 'complete' && r());
    setTimeout(r, 4000);
  });
  function wireChannel(ch, id) {
    ch.binaryType = 'arraybuffer';
    rtc.chans[id] = ch;
    ch.onopen = () => og_net_open(id);
    ch.onmessage = e => og_net_recv(id, new Uint8Array(e.data));
    ch.onclose = () => { delete rtc.chans[id]; og_net_closed(id, 'closed'); };
  }
  function rtcStop() {
    for (const pc of Object.values(rtc.pcs)) try { pc.close(); } catch (e) {}
    rtc.pcs = {}; rtc.chans = {};
  }
  // An edit key: 'e' + 32 random bytes, base64url (see seal.rs).
  function randKey() {
    const b = crypto.getRandomValues(new Uint8Array(32));
    return 'e' + btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }
  let rtcKeys = null;
  async function rtcInvite(view) {
    if (!rtcKeys) { const edit = randKey(); rtcKeys = { edit, view: og_view_token(edit) }; og_rtc_host(edit); }
    const id = rtc.next++;
    const pc = new RTCPeerConnection({ iceServers: ICE });
    rtc.pcs[id] = pc;
    wireChannel(pc.createDataChannel('og-paper', { ordered: true }), id);
    await pc.setLocalDescription(await pc.createOffer());
    await iceDone(pc);
    return { id, link: location.origin + location.pathname + '#rtc=' + pack({ sdp: pc.localDescription.sdp, k: view ? rtcKeys.view : rtcKeys.edit }) };
  }
  async function rtcAnswer(id, code) {
    const o = unpack(code);
    await rtc.pcs[id].setRemoteDescription({ type: 'answer', sdp: o.sdp });
  }
  async function rtcJoin(blob) {
    const o = unpack(blob);
    rtcStop();
    const pc = new RTCPeerConnection({ iceServers: ICE });
    rtc.pcs[0] = pc;
    pc.ondatachannel = e => wireChannel(e.channel, 0);
    await pc.setRemoteDescription({ type: 'offer', sdp: o.sdp });
    await pc.setLocalDescription(await pc.createAnswer());
    await iceDone(pc);
    og_join('rtc?k=' + o.k);
    return pack({ sdp: pc.localDescription.sdp });
  }
  cards.rtc = card('Draw together, no server');
  cards.rtc.append(
    el('p', {}, 'Invite someone with a one-time link. They open it and send you back a reply code; paste it here. Keep this tab open while you draw together.'),
    el('div', { class: 'og-row' }, '<button class="og-btn primary" data-act="edit">New invite (can draw)</button><button class="og-btn" data-act="view">New invite (view only)</button>'),
    el('ul', { class: 'og-list og-invites' }));
  const invites = cards.rtc.querySelector('.og-invites');
  for (const b of cards.rtc.querySelectorAll('[data-act]')) b.onclick = async () => {
    const view = b.dataset.act === 'view';
    const inv = await rtcInvite(view);
    const li = el('li', {}, `<div><b>Invite ${inv.id}${view ? ' (view only)' : ''}</b></div>
      <div class="og-row"><input readonly value="${esc(inv.link)}"><button class="og-btn" data-c>Copy</button></div>
      <div class="og-row"><input placeholder="Paste their reply code" data-r><button class="og-btn primary" data-go>Connect</button></div>`);
    li.querySelector('[data-c]').onclick = () => { navigator.clipboard?.writeText(inv.link); say('Invite copied: send it to them'); };
    li.querySelector('[data-go]').onclick = async () => {
      try { await rtcAnswer(inv.id, li.querySelector('[data-r]').value); say('Connecting…'); li.remove(); }
      catch (e) { say('That reply code did not work: ask them to copy it again'); }
    };
    invites.prepend(li);
  };
  cards.rtcReply = card('Joining: send this back');
  cards.rtcReply.append(
    el('p', {}, 'Send this reply code to the person who invited you. You are connected as soon as they paste it.'),
    el('div', { class: 'og-row' }, '<input readonly data-code><button class="og-btn primary" data-c>Copy</button>'));
  cards.rtcReply.querySelector('[data-c]').onclick = () => {
    navigator.clipboard?.writeText(cards.rtcReply.querySelector('[data-code]').value);
    say('Reply code copied');
  };
  let rtcJoined = false;
  setInterval(async () => {
    if (rtcJoined || !location.hash.startsWith('#rtc=') || !status().ready) return;
    rtcJoined = true;
    const st = status();
    if (st.strokes > 0 && !confirm('Joining a shared canvas replaces the canvas in this browser. Download a copy of this one first if you want to keep it.\n\nJoin now?')) return;
    try {
      const code = await rtcJoin(location.hash.slice(5));
      cards.rtcReply.querySelector('[data-code]').value = code;
      show('rtcReply');
    } catch (e) { say('That invite did not work: ask for a new one'); }
  }, 500);
  // A share link opened in the browser: join (asking first if it would
  // replace a different canvas that has drawing in it).
  let joinDone = false;
  function joinFromHash() {
    if (joinDone || !location.hash.startsWith('#join=')) return;
    const st = status();
    if (!st.ready) return;
    joinDone = true;
    const key = 'og-joined:' + location.hash.split('&k=')[0];
    let known = null;
    try { known = localStorage.getItem(key); } catch (e) {}
    // A relay link names its canvas: already this one, nothing to ask.
    const sameCanvas = known === st.canvas || location.hash.includes('&c=' + st.canvas);
    if (st.strokes > 0 && !sameCanvas &&
        !confirm('Joining a shared canvas replaces the canvas in this browser. Download a copy of this one first if you want to keep it.\n\nJoin now?')) return;
    og_join(location.href);
    const remember = setInterval(() => {
      const s2 = status();
      if (s2.net && (s2.net.startsWith('Live') || s2.net.startsWith('Synced'))) {
        try { localStorage.setItem(key, s2.canvas); } catch (e) {}
        clearInterval(remember);
      }
    }, 1000);
  }
  setInterval(joinFromHash, 500);
  // ---- sync folder: each device keeps its own copy in a shared folder ----
  // (File System Access API: Chromium browsers. The folder handle is kept
  // in IndexedDB per canvas, so syncing resumes after a reload once the
  // browser grants access again.)
  const folder = { root: null, dir: null, canvas: null, seen: {}, lastHash: '', busy: false };
  const fsOk = 'showDirectoryPicker' in window;
  async function hashOf(bytes) {
    const d = await crypto.subtle.digest('SHA-256', bytes);
    return Array.from(new Uint8Array(d).slice(0, 12)).map(b => b.toString(16).padStart(2, '0')).join('');
  }
  async function folderUse(root, canvas) {
    folder.root = root;
    folder.canvas = canvas;
    folder.dir = await root.getDirectoryHandle(`og-paper-${canvas}`, { create: true });
    folder.seen = {};
    folder.lastHash = '';
    og_set_folder(true);
  }
  async function folderToggle() {
    const st = status();
    if (folder.dir) {
      folder.root = folder.dir = null;
      await idbDel(`folder.${st.canvas}`, 'canvases');
      og_set_folder(false);
      return say('Stopped syncing through the folder');
    }
    if (!fsOk) return say('This browser cannot sync a folder: use Save copy and Merge copy, or a Chromium browser');
    try {
      // The folder used before for this canvas, once the browser allows it again.
      const kept = await idbGet(`folder.${st.canvas}`);
      if (kept && (await kept.requestPermission({ mode: 'readwrite' })) === 'granted') {
        await folderUse(kept, st.canvas);
        return say(`Syncing through “${kept.name}” again`);
      }
      const root = await showDirectoryPicker({ id: 'og-paper-sync', mode: 'readwrite' });
      await folderUse(root, st.canvas);
      await idbPut(`folder.${st.canvas}`, root);
      say(`Syncing through “${root.name}”: other devices pick the same folder`);
    } catch (e) {
      if (e.name !== 'AbortError') say('Could not use that folder');
    }
  }
  async function folderTick() {
    if (!folder.dir || folder.busy) return;
    const st = status();
    if (!st.ready) return;
    if (st.canvas !== folder.canvas) { folder.root = folder.dir = null; og_set_folder(false); return; }
    folder.busy = true;
    try {
      const own = `${st.peer}.ogpt`;
      for await (const [name, h] of folder.dir.entries()) {
        if (h.kind !== 'file' || !name.endsWith('.ogpt') || name === own) continue;
        const f = await h.getFile();
        if ((folder.seen[name] || 0) >= f.lastModified) continue;
        og_merge_quiet(new Uint8Array(await f.arrayBuffer()));
        folder.seen[name] = f.lastModified;
      }
      const bytes = await snapshot(true);
      if (bytes) {
        const hsh = await hashOf(bytes);
        if (hsh !== folder.lastHash) {
          const w = await (await folder.dir.getFileHandle(own, { create: true })).createWritable();
          await w.write(bytes);
          await w.close();
          folder.lastHash = hsh;
        }
      }
    } catch (e) {
      console.warn('sync folder', e);
    } finally {
      folder.busy = false;
    }
  }
  setInterval(folderTick, 3000);
  // After a reload: resume at once if access is still granted, else say how.
  let folderChecked = '';
  setInterval(async () => {
    const st = status();
    if (!fsOk || !st.ready || folder.dir || folderChecked === st.canvas) return;
    folderChecked = st.canvas;
    const kept = await idbGet(`folder.${st.canvas}`);
    if (!kept) return;
    if ((await kept.queryPermission({ mode: 'readwrite' })) === 'granted') await folderUse(kept, st.canvas);
    else say('Tap Sync folder to resume syncing this canvas');
  }, 1500);
  // ---- plugins and packs: installed from files, kept in browser storage ----
  const pluginPicker = el('input', { type: 'file', accept: '.wasm,.ogpack,application/wasm', hidden: '' });
  root.append(pluginPicker);
  pluginPicker.onchange = async () => {
    const f = pluginPicker.files[0];
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    if (/\.ogpack$/i.test(f.name)) { og_pack_install(bytes); return; }
    // Kept once the app has read it and says its name ("plugin-store").
    pendingPlugin = bytes;
    og_plugin_install(bytes, false);
  };
  let pendingPlugin = null;
  // The plugins kept from before (once the app has started).
  async function loadPlugins() {
    for (const name of lsGet('og-plugins') || []) {
      const b = await idbGet('plugin:' + name);
      if (b) og_plugin_install(b, true);
    }
  }
  // ---- import: another saved copy, placed into this canvas ----
  const importPicker = Object.assign(document.createElement('input'), { type: 'file', accept: '.ogpt', hidden: true, id: 'import-picker' });
  document.body.append(importPicker);
  importPicker.onchange = async () => {
    const f = importPicker.files[0];
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    if (importPicker.dataset.mode === 'merge') og_merge(bytes); else og_import(bytes);
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
    area.style.fontFamily = req.single
      ? 'ui-monospace, monospace'
      : `"${(req.font || '').replace(/"/g, '')}", system-ui, sans-serif`;
    area.style.left = `${req.x}px`;
    area.style.top = `${req.y}px`;
    area.style.color = req.color || '#1c1c24';
    const hint = el('div', { class: 'og-text-hint' }, req.table
      ? 'Tab between cells · Enter for a new row · Ctrl+Enter or tap away to finish · Esc to cancel'
      : 'Enter for a new line · Ctrl+Enter or tap away to finish · Esc to cancel');
    // UI > Helper text off: no hint.
    if (status().hints === false) hint.hidden = true;
    hint.style.left = `${req.x}px`;
    hint.style.top = `${Math.max(4, req.y - 26)}px`;
    // Fit the text: shrink first, then measure (scrollWidth never reports
    // less than the current width, so measuring at the old width only grows).
    const grow = () => {
      const max = Math.max(160, innerWidth - req.x - 12);
      area.style.width = '160px';
      area.style.height = 'auto';
      // Lines don't wrap until the box reaches the edge of the screen.
      area.style.whiteSpace = 'pre';
      const want = area.scrollWidth + 12;
      if (want > max) area.style.whiteSpace = 'pre-wrap';
      area.style.width = `${Math.min(max, Math.max(160, want))}px`;
      area.style.height = `${area.scrollHeight + 2}px`;
    };
    area.addEventListener('input', grow);
    area.addEventListener('keydown', e => {
      if (e.key === 'Escape') { e.preventDefault(); closeText(false); }
      else if (e.key === 'Tab' && req.table) {
        // Tables are edited as tab-separated cells.
        e.preventDefault();
        area.setRangeText('\t', area.selectionStart, area.selectionEnd, 'end');
        grow();
      }
      else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); closeText(true); }
      e.stopPropagation();
    });
    area.addEventListener('blur', () => setTimeout(() => closeText(true), 0));
    document.body.append(hint, area);
    textBox = { area, hint };
    grow();
    area.focus();
  }

  // ---- your own fonts ----
  const fontPicker = el('input', { type: 'file', accept: '.ttf,.otf,font/ttf,font/otf', hidden: '' });
  root.append(fontPicker);
  function pickFont() {
    fontPicker.value = '';
    fontPicker.click();
  }
  fontPicker.onchange = async () => {
    const f = fontPicker.files[0];
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    const r = og_font_add(undefined, 'Yours', bytes, true, true);
    if (r.startsWith('!')) { say(`Could not add ${f.name}: ${r.slice(1)}`); return; }
    pageFont(r, bytes);
    await idbPut(r, { name: r, bytes }, 'fonts');
  };

  // ---- clipboard, drag and drop, pictures ----
  // Copying in the app puts this marker on the clipboard: pasting it pastes
  // the app's own copy (shapes, ink and text keep everything).
  const CLIP_MARK = 'OG Paper selection (paste it into OG Paper)';
  // In the HTML a copy puts on the clipboard: pasting it here is this app's.
  const OWN = /<meta name="og-paper-clip"/;
  const onCanvas = e => {
    const t = e.target;
    return !(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t?.isContentEditable);
  };
  // Let Ctrl+C / X / V reach the browser (the canvas would swallow them),
  // so the copy and paste events below fire.
  window.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && ['c', 'x', 'v'].includes(e.key.toLowerCase()) && onCanvas(e)) e.stopPropagation();
  }, true);
  // A text box in the app (Share live, a name, ...) has the keyboard: copy,
  // cut and paste are its, not the canvas's.
  const intoField = (kind, e) => {
    if (!onCanvas(e) || !og_wants_text()) return false;
    e.preventDefault();
    og_text_field(kind, kind === 'paste' ? (e.clipboardData?.getData('text/plain') || '') : '');
    if (kind !== 'paste') {
      // The app hands the copied text over on its next frame.
      let tries = 0;
      const take = () => {
        const t = og_copied_take();
        if (t != null) navigator.clipboard?.writeText(t).catch(() => {});
        else if (++tries < 20) setTimeout(take, 25);
      };
      setTimeout(take, 25);
    }
    return true;
  };
  // Phones: the app's text boxes are drawn on the canvas, which can't
  // bring up a keyboard. A tap on one focuses this hidden input instead
  // (inside the tap, as iOS requires) and what is typed goes to the box.
  // A space stays in it so a backspace on an empty box still shows up.
  const kbd = el('input', {
    type: 'text', autocomplete: 'off', autocorrect: 'off', autocapitalize: 'off', spellcheck: 'false',
    enterkeyhint: 'done', 'aria-hidden': 'true', tabindex: '-1',
    style: 'position:fixed;left:0;bottom:0;width:1px;height:1px;opacity:0;border:0;padding:0;font-size:16px;pointer-events:none;',
  });
  root.append(kbd);
  const KBD_REST = ' ';
  let kbdOn = false, kbdSeen = false, kbdAt = 0;
  const kbdReset = () => { kbd.value = KBD_REST; kbd.setSelectionRange(1, 1); };
  document.addEventListener('touchend', e => {
    const t = e.changedTouches[0];
    if (!t || !onCanvas(e) || !og_field_at(t.clientX, t.clientY)) return;
    kbdReset();
    kbd.focus({ preventScroll: true });
    kbdOn = true; kbdSeen = false; kbdAt = performance.now();
  }, true);
  kbd.addEventListener('input', () => {
    const v = kbd.value;
    if (v.length < KBD_REST.length) og_text_field('Backspace', '');
    else if (v.startsWith(KBD_REST) && v.length > KBD_REST.length) og_text_field('text', v.slice(KBD_REST.length));
    else if (v !== KBD_REST) og_text_field('text', v);
    kbdReset();
  });
  kbd.addEventListener('paste', e => {
    e.preventDefault();
    og_text_field('paste', e.clipboardData?.getData('text/plain') || '');
    kbdReset();
  });
  kbd.addEventListener('keydown', e => {
    if (e.key === 'Enter') { e.preventDefault(); og_text_field('Enter', ''); }
  });
  // Let go of the keyboard when the app's text box loses focus (it takes
  // focus a frame after the tap, hence the grace).
  (function kbdWatch() {
    if (kbdOn && document.activeElement === kbd) {
      const wants = og_wants_text();
      if (wants) kbdSeen = true;
      if (!wants && (kbdSeen || performance.now() - kbdAt > 1000)) {
        kbd.blur();
        kbdOn = false;
      }
    }
    requestAnimationFrame(kbdWatch);
  })();
  for (const kind of ['copy', 'cut']) {
    document.addEventListener(kind, e => {
      if (intoField(kind, e)) return;
      if (!onCanvas(e)) return;
      const text = og_selection_text();
      if (!og_copy(kind === 'cut')) return;
      // Other apps get the selected texts; pasting here (the marker in the
      // HTML) brings back the objects themselves.
      e.clipboardData.setData('text/plain', text.trim() ? text : CLIP_MARK);
      const esc = t => t.replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
      e.clipboardData.setData('text/html', `<meta name="og-paper-clip" content="1"><pre>${esc(text)}</pre>`);
      e.preventDefault();
    });
  }
  const NATIVE = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
  // Anything the browser can show (SVG, BMP, AVIF, ...) as PNG bytes.
  async function asPng(blob) {
    const url = URL.createObjectURL(blob);
    try {
      const img = new Image();
      img.src = url;
      await img.decode();
      // Vector pictures are drawn at twice their size so they stay sharp.
      const k = blob.type === 'image/svg+xml' ? 2 : 1;
      const w = Math.max(1, Math.round((img.naturalWidth || 300) * k));
      const h = Math.max(1, Math.round((img.naturalHeight || 150) * k));
      const c = document.createElement('canvas');
      c.width = w; c.height = h;
      c.getContext('2d').drawImage(img, 0, 0, w, h);
      const out = await new Promise(r => c.toBlob(r, 'image/png'));
      return new Uint8Array(await out.arrayBuffer());
    } finally {
      URL.revokeObjectURL(url);
    }
  }
  async function addPicture(blob, at) {
    try {
      const bytes = NATIVE.includes(blob.type) ? new Uint8Array(await blob.arrayBuffer()) : await asPng(blob);
      const err = og_paste_image(bytes, at?.[0], at?.[1]);
      if (err) say(`Could not add the picture: ${err}`);
    } catch (e) {
      say(`Could not add the picture: ${e.message || e}`);
    }
  }
  const isPdf = f => f.type === 'application/pdf' || /\.pdf$/i.test(f.name || '');
  // PDFs are drawn by pdf.js, fetched the first time one is imported.
  const PDFJS = 'https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.10.38/';
  let pdfjs = null;
  async function addPdf(f, at) {
    const name = f.name || 'PDF';
    say(`Opening ${name}…`);
    try {
      if (!pdfjs) {
        pdfjs = await import(PDFJS + 'pdf.min.mjs');
        pdfjs.GlobalWorkerOptions.workerSrc = PDFJS + 'pdf.worker.min.mjs';
      }
      const doc = await pdfjs.getDocument({ data: new Uint8Array(await f.arrayBuffer()) }).promise;
      const total = Math.min(doc.numPages, 200);
      if (doc.numPages > total) say(`${name} has ${doc.numPages} pages; importing the first ${total}`);
      for (let i = 0; i < total; i++) {
        const page = await doc.getPage(i + 1);
        const v = page.getViewport({ scale: 1 });
        const k = Math.min(2200 / Math.max(v.width, v.height), 4);
        const vp = page.getViewport({ scale: k });
        const c = document.createElement('canvas');
        c.width = Math.ceil(vp.width);
        c.height = Math.ceil(vp.height);
        const g = c.getContext('2d');
        g.fillStyle = '#fff';
        g.fillRect(0, 0, c.width, c.height);
        await page.render({ canvasContext: g, viewport: vp }).promise;
        const png = await new Promise(r => c.toBlob(r, 'image/png'));
        const err = og_pdf_page(new Uint8Array(await png.arrayBuffer()), i, total, at?.[0], at?.[1], name);
        if (err) { say(`Could not import page ${i + 1}: ${err}`); return; }
        page.cleanup();
      }
      doc.destroy();
    } catch (e) {
      say(e?.name === 'PasswordException' ? `${name} is password-protected` : `Could not import ${name}: ${e?.message || e}`);
    }
  }
  // ---- audio clips (see crates/og-paper/src/audio.rs) ----
  // Recording: the microphone through MediaRecorder at a speech bitrate
  // (Opus where the browser has it, else MP4/AAC on Safari), with a bar to
  // stop or cancel; the clip goes back to the canvas when it stops.
  let recBar = null;
  async function audioRecord(maxMs) {
    if (recBar) return;
    let stream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } });
    } catch (e) {
      og_audio_cancelled(e?.name === 'NotAllowedError' ? 'The browser did not allow the microphone' : `No microphone: ${e?.message || e}`);
      return;
    }
    const types = ['audio/webm;codecs=opus', 'audio/ogg;codecs=opus', 'audio/mp4', 'audio/webm'];
    const mimeType = types.find(t => window.MediaRecorder?.isTypeSupported?.(t));
    let rec;
    try {
      rec = new MediaRecorder(stream, { ...(mimeType ? { mimeType } : {}), audioBitsPerSecond: 16000 });
    } catch (e) {
      stream.getTracks().forEach(t => t.stop());
      og_audio_cancelled(`Could not record here: ${e?.message || e}`);
      return;
    }
    const parts = [];
    let cancelled = false;
    const started = performance.now();
    recBar = el('div', { class: 'og-rec', role: 'status' },
      '<span class="dot"></span><span class="t">0:00</span><button class="og-btn stop">Stop</button><button class="og-btn cancel">Cancel</button>');
    root.append(recBar);
    const t = recBar.querySelector('.t');
    const tick = setInterval(() => {
      const s = Math.floor((performance.now() - started) / 1000);
      t.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
    }, 250);
    const limit = setTimeout(() => rec.state !== 'inactive' && rec.stop(), maxMs);
    rec.ondataavailable = e => e.data.size && parts.push(e.data);
    rec.onstop = async () => {
      clearInterval(tick); clearTimeout(limit);
      stream.getTracks().forEach(tr => tr.stop());
      recBar.remove(); recBar = null;
      if (cancelled) { og_audio_cancelled('Recording cancelled'); return; }
      const dur = Math.round(performance.now() - started);
      const blob = new Blob(parts, { type: rec.mimeType || mimeType || 'audio/webm' });
      og_audio_recorded(new Uint8Array(await blob.arrayBuffer()), Math.min(dur, maxMs));
    };
    recBar.querySelector('.stop').onclick = () => rec.stop();
    recBar.querySelector('.cancel').onclick = () => { cancelled = true; rec.stop(); };
    rec.start(250);
  }
  // Playing: one clip at a time; the same clip again stops it.
  let player = null, playingId = '';
  function audioPlay() {
    const id = og_audio_take_id();
    const bytes = og_audio_take();
    if (player) {
      player.pause(); URL.revokeObjectURL(player.src);
      const same = playingId === id;
      player = null; playingId = '';
      if (same) return;
    }
    if (!bytes) return;
    const b = bytes;
    const type = b[0] === 0x1a ? 'audio/webm' : (b[0] === 0x4f ? 'audio/ogg' : (b[4] === 0x66 ? 'audio/mp4' : (b[0] === 0x52 ? 'audio/wav' : 'audio/mpeg')));
    player = new Audio(URL.createObjectURL(new Blob([b], { type })));
    playingId = id;
    window.ogAudioPlaying = id;
    player.onended = () => { if (player) URL.revokeObjectURL(player.src); player = null; playingId = ''; window.ogAudioPlaying = ''; };
    player.play().catch(e => { say(`This browser could not play the clip (${type}): ${e?.message || e}`); player = null; playingId = ''; });
  }

  // An HTML table (from a spreadsheet or a web page) as tab-separated text.
  function htmlTable(html) {
    if (!/<table/i.test(html)) return null;
    const t = new DOMParser().parseFromString(html, 'text/html').querySelector('table');
    if (!t) return null;
    const rows = [...t.rows].map(r => [...r.cells].map(c => c.innerText.replace(/[\t\n]+/g, ' ').trim()).join('\t'));
    return rows.length ? rows.join('\n') : null;
  }
  async function pasteFrom(dt, at) {
    const sid = dt.getData?.(STICKER);
    if (sid) {
      const it = lib.find(x => x.id === sid);
      if (it) og_sticker_place(it.bytes, at?.[0], at?.[1]);
      return;
    }
    const text = dt.getData('text/plain');
    if (text === CLIP_MARK || OWN.test(dt.getData('text/html') || '')) { og_paste_own(); return; }
    // Spreadsheets also put a picture of the cells on the clipboard: the table wins.
    const table = htmlTable(dt.getData('text/html') || '');
    if (table) { og_paste_text(table, at?.[0], at?.[1]); return; }
    const files = [...dt.files];
    const pdfs = files.filter(isPdf);
    for (const [i, f] of pdfs.entries()) await addPdf(f, at && [at[0] + i * 24, at[1] + i * 24]);
    if (pdfs.length) return;
    const pics = files.filter(f => f.type.startsWith('image/'));
    if (pics.length) {
      for (const [i, f] of pics.entries()) await addPicture(f, at && [at[0] + i * 24, at[1] + i * 24]);
      return;
    }
    const copy = files.find(f => f.name.toLowerCase().endsWith('.ogpt'));
    if (copy) { og_load(new Uint8Array(await copy.arrayBuffer()), false); return; }
    const txt = files.find(f => /\.(txt|tsv|md)$/i.test(f.name));
    if (txt) { og_paste_text(await txt.text(), at?.[0], at?.[1]); return; }
    if (/^\s*<svg[\s>]/i.test(text)) { await addPicture(new Blob([text], { type: 'image/svg+xml' }), at); return; }
    if (text.trim()) og_paste_text(text, at?.[0], at?.[1]);
  }
  // The Paste button (phones and tablets have no Ctrl+V): read the
  // clipboard directly. Browsers ask permission or show a Paste prompt.
  // iPhone / iPad Safari only lets a page read the clipboard inside a tap
  // on a page element: Paste there goes through a "Tap to paste" button
  // (stickers, photos and text copied on iOS all come in this way).
  const isIOS = /iP(hone|ad|od)/.test(navigator.userAgent) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
  cards.pasteNow = card('Paste');
  cards.pasteNow.append(
    el('p', {}, 'Tap below to paste what you copied (a sticker, photo, picture or text).'),
    el('button', { class: 'og-btn primary wide', 'data-act': 'paste' }, 'Tap to paste'));
  cards.pasteNow.querySelector('[data-act=paste]').onclick = () => { show(null); pasteButton(true); };
  function pasteRequest() {
    if (isIOS) show('pasteNow');
    else pasteButton(false);
  }
  async function pasteButton(fromTap) {
    const mid = [innerWidth / 2, innerHeight / 2];
    if (!navigator.clipboard?.read && !navigator.clipboard?.readText) return say('This browser does not let pages read the clipboard — use Ctrl+V or Insert picture');
    try {
      if (navigator.clipboard.read) {
        const items = await navigator.clipboard.read();
        const types = items.flatMap(i => i.types);
        const get = async t => { for (const i of items) if (i.types.includes(t)) return i.getType(t); return null; };
        const textBlob = await get('text/plain');
        const text = textBlob ? await textBlob.text() : '';
        const html = await get('text/html');
        const htmlText = html ? await html.text() : '';
        if (text === CLIP_MARK || OWN.test(htmlText)) { og_paste_own(); return; }
        const table = htmlText && htmlTable(htmlText);
        if (table) { og_paste_text(table, mid[0], mid[1]); return; }
        const pic = types.find(t => t.startsWith('image/'));
        if (pic) { await addPicture(await get(pic), mid); return; }
        if (/^\s*<svg[\s>]/i.test(text)) { await addPicture(new Blob([text], { type: 'image/svg+xml' }), mid); return; }
        if (text.trim()) { og_paste_text(text, mid[0], mid[1]); return; }
        og_paste_own();
      } else {
        const text = await navigator.clipboard.readText();
        if (text === CLIP_MARK || !text.trim()) og_paste_own();
        else og_paste_text(text, mid[0], mid[1]);
      }
    } catch (e) {
      // Refused outside a tap: offer the button that pastes inside one.
      if (e?.name === 'NotAllowedError' && !fromTap) { show('pasteNow'); return; }
      // Permission refused or nothing readable: the app's own copy, if any.
      og_paste_own();
      if (e?.name === 'NotAllowedError') say('Clipboard access was blocked — allow it for this site, or use Ctrl+V');
    }
  }
  let pointer = null;
  document.addEventListener('pointermove', e => { pointer = [e.clientX, e.clientY]; });
  document.addEventListener('paste', e => {
    if (intoField('paste', e)) return;
    if (!onCanvas(e)) return;
    e.preventDefault();
    pasteFrom(e.clipboardData, pointer);
  });
  document.addEventListener('dragover', e => { if (onCanvas(e)) e.preventDefault(); });
  document.addEventListener('drop', e => {
    if (!onCanvas(e)) return;
    e.preventDefault();
    pasteFrom(e.dataTransfer, [e.clientX, e.clientY]);
  });
  const picPicker = el('input', { type: 'file', accept: 'image/*,application/pdf,.pdf', multiple: '', hidden: '' });
  root.append(picPicker);
  picPicker.onchange = async () => {
    for (const f of picPicker.files) await (isPdf(f) ? addPdf(f, null) : addPicture(f, null));
    picPicker.value = '';
  };

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
  loadFonts();
  // Pages kept in this browser: one copy per canvas ('page:<canvas>'),
  // listed in localStorage 'og-pages'; 'og-page' is the one last open.
  let pages = isTry ? [] : (lsGet('og-pages') || []);
  const pushPages = () => { if (!isTry) og_set_pages(JSON.stringify(pages)); };
  let saved = null;
  if (isTry) saved = await idbGet('try');
  else {
    const cur = lsGet('og-page');
    if (cur) saved = await idbGet('page:' + cur);
    if (!saved) saved = await idbGet('app'); // the single slot of before
  }
  og_load(saved, isTry);
  if (!isTry && saved) {
    const name = pages.find(p => p.c === lsGet('og-page'))?.name;
    if (name) og_set_name(name);
  }
  pushPages();
  loadPlugins();
  // Opened from a server's page: add that server to Pages.
  if (!isTry && location.hash.startsWith('#server=')) {
    // #server=<link>[&acct=<user:token>] (opened from the server's page
    // while signed in there: the app signs in with that account).
    const [rawLink, rawAcct] = location.hash.slice(8).split('&acct=');
    const link = decodeURIComponent(rawLink);
    history.replaceState(null, '', location.pathname + location.search);
    og_add_server(link, rawAcct ? decodeURIComponent(rawAcct) : '');
  }
  // Automation from the page (devtools, extensions): the same commands as
  // plugins and the server API (docs/PLUGINS.md).
  window.ogPaper = { run: cmds => og_run(JSON.stringify(cmds)) };
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
    const st = status();
    if (bytes && st.canvas && await idbPut(isTry ? 'try' : 'page:' + st.canvas, bytes)) {
      lastSave = Date.now();
      if (!isTry) {
        lsSet('og-page', st.canvas);
        const p = pages.find(p => p.c === st.canvas);
        if (p) { p.name = st.name; p.t = lastSave; }
        else pages.unshift({ c: st.canvas, name: st.name, t: lastSave });
        pages.sort((a, b) => b.t - a.t);
        lsSet('og-pages', pages);
        pushPages();
      }
      savedNote.textContent = `Saved in this browser at ${new Date(lastSave).toLocaleTimeString()}.`;
    }
    saving = false;
  }
  setInterval(() => { if (status().dirty) autosave(); }, 2000);
  document.addEventListener('visibilitychange', () => { if (document.hidden && status().dirty) autosave(); });

  function tick() {
    const s = status();
    if (s.ready) loading.hidden = true;
    // Dark mode: the page and the cards over the canvas flip with it.
    if (s.ready && document.documentElement.classList.contains('dark') !== !!s.dark)
      document.documentElement.classList.toggle('dark', !!s.dark);
    for (const r of JSON.parse(og_requests())) {
      if (r === 'save') download();
      else if (r === 'open') openCopy();
      else if (r === 'import') { importPicker.dataset.mode = 'import'; importPicker.value = ''; importPicker.click(); }
      else if (r === 'changes') {
        const b = og_changes_take();
        if (b) {
          const stamp = new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-');
          const a = el('a', { download: `og-paper-changes-${stamp}.ogpt` });
          a.href = URL.createObjectURL(new Blob([b], { type: 'application/octet-stream' }));
          document.body.append(a);
          a.click();
          a.remove();
          setTimeout(() => URL.revokeObjectURL(a.href), 10000);
        }
      }
      else if (r === 'folder') folderToggle();
      else if (r === 'net-connect') netConnect(og_net_url());
      else if (r === 'net-close') { netClose(); rtcStop(); }
      else if (r === 'rtc') show('rtc');
      else if (r === 'relay-new') og_relay_share(randKey());
      else if (r === 'rtc-stop') { rtcStop(); rtcKeys = null; }
      else if (r === 'rtc-close' || r === 'dir-close') for (const c of og_rtc_closing()) {
        try { rtc.chans[c]?.close(); rtc.pcs[c]?.close(); dirWs[c]?.close(); } catch (e) {}
        delete dirWs[c];
      }
      else if (r === 'dir-connect') for (const [c, url] of JSON.parse(og_dir_requests())) dirOpen(c, url);
      else if (r === 'open-page') pageOpen(og_page_arg());
      else if (r === 'plugin-pick') { pluginPicker.value = ''; pluginPicker.click(); }
      else if (r === 'audio-record') audioRecord(og_audio_max());
      else if (r === 'audio-play') audioPlay();
      else if (r === 'plugin-store' && (pendingPlugin || (pendingPlugin = og_plugin_bytes_take()))) {
        const name = og_page_arg();
        idbPut('plugin:' + name, pendingPlugin);
        pendingPlugin = null;
        const list = lsGet('og-plugins') || [];
        if (!list.includes(name)) { list.push(name); lsSet('og-plugins', list); }
      }
      else if (r === 'plugin-remove') {
        const name = og_page_arg();
        const list = (lsGet('og-plugins') || []).filter(n => n !== name);
        lsSet('og-plugins', list);
        idbDel('plugin:' + name, 'canvases');
      }
      else if (r === 'pack-export') {
        const b = og_changes_take();
        if (b) {
          const a = el('a', { download: 'My toolbars.ogpack' });
          a.href = URL.createObjectURL(new Blob([b], { type: 'text/plain' }));
          document.body.append(a);
          a.click();
          a.remove();
          setTimeout(() => URL.revokeObjectURL(a.href), 10000);
        }
      }
      else if (r === 'pack-stickers') (async () => {
        for (let b = og_pack_sticker_take(); b; b = og_pack_sticker_take()) {
          const tab = b.indexOf(9);
          const name = new TextDecoder().decode(b.subarray(0, tab));
          const bytes = b.slice(tab + 1);
          const id = 'pack-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 8);
          await idbPut(id, { id, name: name || 'Sticker', bytes, t: Date.now() }, 'library');
        }
        libLoad();
      })();
      else if (r === 'new-page') (async () => {
        const name = og_page_arg();
        await autosave();
        og_blank();
        if (name) og_set_name(name);
      })();
      else if (r === 'rename-page') {
        const [c, name] = og_page_arg().split('\t');
        const p = pages.find(p => p.c === c);
        if (p && name) { p.name = name; lsSet('og-pages', pages); pushPages(); }
      }
      else if (r === 'delete-page') {
        const c = og_page_arg();
        pages = pages.filter(p => p.c !== c);
        lsSet('og-pages', pages);
        idbDel('page:' + c, 'canvases');
        pushPages();
      }
      else if (r === 'open-shared') {
        const [c, link] = og_page_arg().split(' ');
        (async () => {
          // A copy here opens (and reconnects by its remembered link);
          // otherwise joining starts one.
          if (!(await pageOpen(c))) og_join(link);
        })();
      }
      else if (r === 'merge') { importPicker.dataset.mode = 'merge'; importPicker.value = ''; importPicker.click(); }
      else if (r === 'new') newCanvas();
      else if (r === 'bookmarks') show('bookmarks');
      else if (r === 'export') openExport();
      else if (r === 'library') { if (open !== 'library') { libLoad(); show('library'); } else show(null); }
      else if (r === 'sticker') stickerSave();
      else if (r === 'paste') pasteRequest();
      else if (r === 'search') { if (open !== 'search') show('search'); sForm.q.focus(); sForm.q.select(); }
      else if (r === 'timeline') openTimeline(!tlOpen);
      else if (r === 'fullscreen') fullScreen();
      else if (r === 'tour' && cards.tour) show('tour');
      else if (r === 'text') openTextEditor();
      else if (r === 'font') pickFont();
      else if (r === 'picture') picPicker.click();
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
          ? s.bookmarks.map((b, i) => `<li data-i="${i}"><button class="go">${fmt(b.name)}<small>${b.at != null ? `as it was on ${when(b.at)} · ` : ''}zoom ${zoomText(b.zoom)}</small></button>
              <button class="mini pub" data-act="public" aria-pressed="${!!b.public}" title="${b.public ? 'Public: everyone on this page can fly to it. Tap to make it private.' : 'Private. Tap to share it with everyone on this page.'}">${b.public ? '◉ Public' : '○ Public'}</button><button class="mini" data-act="bar" title="Put it in the quick toolbar">+ bar</button><button class="mini" data-act="rename" title="Rename">Rename</button><button class="mini" data-act="remove" title="Delete" aria-label="Delete">✕</button></li>`).join('')
          : '<li class="og-empty">No saved views yet.</li>';
        cards.bookmarks.renderChips(s.bookmarks);
      }
      const sk = JSON.stringify(s.shared || []);
      if (sk !== sharedKey) {
        sharedKey = sk;
        const shared = s.shared || [];
        sharedHead.hidden = sharedList.hidden = shared.length === 0;
        sharedList.innerHTML = shared.map((v, i) => `<li data-i="${i}"><button class="go">${fmt(v.name)}<small>by ${fmt(v.author)} · ${v.at != null ? `as it was on ${when(v.at)} · ` : ''}zoom ${zoomText(v.zoom)}</small></button>
            <button class="mini" data-act="copy" title="Keep a copy in your views">Save a copy</button></li>`).join('');
      }

      // Timeline (it opens itself when a bookmarked moment is shown).
      const t = s.timeline;
      if (!tlOpen && t?.on) { tlOpen = true; tl.hidden = false; }
      if (tlOpen && t) {
        lo.max = range.max = Math.max(0, t.n - 1);
        if (!playing && document.activeElement !== range && document.activeElement !== lo) {
          range.value = t.i;
          lo.value = t.from;
        }
        paintSel();
        tl.querySelector('.at').innerHTML = t.from > 0
          ? `Only what was drawn from <b>${when(t.tFrom)}</b> to <b>${when(t.t)}</b>`
          : `Showing the canvas at <b>${when(t.t)}</b>`;
        tl.querySelector('.count').textContent = t.from > 0
          ? `changes ${t.from + 1}–${t.i + 1} of ${t.n}`
          : `change ${t.i + 1} of ${t.n}`;
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
    netFlush();
    requestAnimationFrame(tick);
  }
  tick();
  return { say };
}
