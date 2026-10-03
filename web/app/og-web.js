// OG Paper web shell: the page-side panels around the canvas — bookmarks,
// the timeline, offline copies (download / load a .ogpt file), browser
// autosave, full screen, and (try mode) a guided tour of the canvas. They are
// opened from the app's own settings fan (the gear, top right).
// Used by /app/ and /try/; the canvas itself is the Rust app in ./pkg/.

import init, {
  og_load, og_demo, og_blank, og_status, og_requests, og_set_menu, og_text_request, og_text_done, og_font_add,
  og_copy, og_paste_own, og_paste_image, og_paste_text, og_pdf_page,
  og_bookmark_add, og_bookmark_go, og_bookmark_remove, og_bookmark_rename, og_bookmark_to_bar,
  og_search, og_search_results, og_search_go, og_export, og_export_take, og_has_selection,
  og_sticker_take, og_sticker_svg, og_sticker_place,
  og_timeline, og_timeline_range, og_timeline_restore, og_snapshot_request, og_snapshot_take,
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
@media (max-width: 520px) { .og-tl .dual { flex-basis: 60px; } .og-tl .restore { padding: 6px 8px; } .og-tl { right: calc(84px + env(safe-area-inset-right)); bottom: calc(86px + env(safe-area-inset-bottom)); } }
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
    el('label', { class: 'og-opt' }, 'Picture size <select name="scale"><option value="1">1×</option><option value="2" selected>2×</option><option value="4">4×</option></select>'));
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
    const items = ['new', 'open', 'save', 'export', 'paste', 'library', 'picture', 'search', 'bookmarks', 'timeline', 'home', 'grid', 'diagram', 'layout', 'hotkeys'];
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
    else if (b.dataset.act === 'bar') og_bookmark_to_bar(i);
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
    area.style.fontFamily = req.single
      ? 'ui-monospace, monospace'
      : `"${(req.font || '').replace(/"/g, '')}", system-ui, sans-serif`;
    area.style.left = `${req.x}px`;
    area.style.top = `${req.y}px`;
    area.style.color = req.color || '#1c1c24';
    const hint = el('div', { class: 'og-text-hint' }, req.table
      ? 'Tab between cells · Enter for a new row · Ctrl+Enter or tap away to finish · Esc to cancel'
      : 'Enter for a new line · Ctrl+Enter or tap away to finish · Esc to cancel');
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
  const onCanvas = e => {
    const t = e.target;
    return !(t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t?.isContentEditable);
  };
  // Let Ctrl+C / X / V reach the browser (the canvas would swallow them),
  // so the copy and paste events below fire.
  window.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && ['c', 'x', 'v'].includes(e.key.toLowerCase()) && onCanvas(e)) e.stopPropagation();
  }, true);
  for (const kind of ['copy', 'cut']) {
    document.addEventListener(kind, e => {
      if (!onCanvas(e) || !og_copy(kind === 'cut')) return;
      e.clipboardData.setData('text/plain', CLIP_MARK);
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
    if (text === CLIP_MARK) { og_paste_own(); return; }
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
  async function pasteButton() {
    const mid = [innerWidth / 2, innerHeight / 2];
    if (!navigator.clipboard?.read && !navigator.clipboard?.readText) return say('This browser does not let pages read the clipboard — use Ctrl+V or Insert picture');
    try {
      if (navigator.clipboard.read) {
        const items = await navigator.clipboard.read();
        const types = items.flatMap(i => i.types);
        const get = async t => { for (const i of items) if (i.types.includes(t)) return i.getType(t); return null; };
        const textBlob = await get('text/plain');
        const text = textBlob ? await textBlob.text() : '';
        if (text === CLIP_MARK) { og_paste_own(); return; }
        const html = await get('text/html');
        const table = html && htmlTable(await html.text());
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
      // Permission refused or nothing readable: the app's own copy, if any.
      og_paste_own();
      if (e?.name === 'NotAllowedError') say('Clipboard access was blocked — allow it for this site, or use Ctrl+V');
    }
  }
  let pointer = null;
  document.addEventListener('pointermove', e => { pointer = [e.clientX, e.clientY]; });
  document.addEventListener('paste', e => {
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
      else if (r === 'export') openExport();
      else if (r === 'library') { if (open !== 'library') { libLoad(); show('library'); } else show(null); }
      else if (r === 'sticker') stickerSave();
      else if (r === 'paste') pasteButton();
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
          ? s.bookmarks.map((b, i) => `<li data-i="${i}"><button class="go">${fmt(b.name)}<small>zoom ${zoomText(b.zoom)}</small></button>
              <button class="mini" data-act="bar" title="Put it in the quick toolbar">+ bar</button><button class="mini" data-act="rename" title="Rename">Rename</button><button class="mini" data-act="remove" title="Delete" aria-label="Delete">✕</button></li>`).join('')
          : '<li class="og-empty">No bookmarks yet.</li>';
      }

      // Timeline.
      const t = s.timeline;
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
    requestAnimationFrame(tick);
  }
  tick();
  return { say };
}
