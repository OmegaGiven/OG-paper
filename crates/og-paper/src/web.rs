// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The bridge between the page's JavaScript (bookmarks, timeline, offline
//! copies, try-mode tour) and the app. The app is owned by the event loop, so
//! calls from JS queue commands that the next frame applies, and the app
//! publishes its status and snapshots here for JS to pick up.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Arc;

use wasm_bindgen::prelude::{wasm_bindgen, JsValue};
use winit::window::Window;

pub enum Cmd {
    /// Load an offline copy; on `None` or a bad file, the demo or a blank canvas.
    Load(Option<Vec<u8>>, bool),
    Demo,
    Blank,
    Home,
    /// What the settings fan offers.
    Menu(Vec<crate::ui::AppItem>),
    BookmarkAdd(String),
    BookmarkGo(usize),
    BookmarkRemove(usize),
    BookmarkRename(usize, String),
    /// Show the canvas as it was after timeline event `i`; `None` leaves.
    Timeline(Option<usize>),
    /// Show only the ink drawn between events `from` and `to` (inclusive)
    /// that is still there at `to`; `None` leaves.
    TimelineRange(Option<(usize, usize)>),
    /// Run the text search in `SEARCH`; results go to `SEARCH_OUT`.
    Search,
    /// Fly to search result group `g`.
    SearchGo(u32),
    /// Place a library sticker (its bytes) at a point (CSS px) or the middle.
    Sticker(Vec<u8>, Option<[f64; 2]>),
    /// Export: format, only the selection, paper background.
    Export(String, bool, bool),
    /// Make the moment shown in the timeline the current canvas (undoable).
    TimelineRestore,
    /// The page's text editor finished (Some: the text) or was cancelled.
    Text(Option<String>),
    /// A font of the user's was added: use it.
    FontAdded(crate::font::FontId, String),
    /// Copy the selection (and delete it, for cut).
    Copy(bool),
    /// Paste what this app copied.
    PasteOwn,
    /// A picture pasted, dropped or picked; where (CSS px), if known.
    Picture(crate::images::Asset, Option<[f64; 2]>),
    /// A page of a PDF being imported: the page, its index, the page
    /// count, where (CSS px), the file name.
    PdfPage(crate::images::Asset, usize, usize, Option<[f64; 2]>, String),
    /// Text pasted or dropped (a table if it looks like one).
    PasteText(String, Option<[f64; 2]>),
}

/// Counters the try-mode tour checks off.
#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub drawn: u32,
    pub erased: u32,
    pub undos: u32,
    /// Deepest zoom (log10) at which something was drawn.
    pub deep_draw: f64,
}

thread_local! {
    static CMDS: RefCell<VecDeque<Cmd>> = RefCell::default();
    static WINDOW: RefCell<Option<Arc<Window>>> = RefCell::default();
    static STATUS: RefCell<String> = RefCell::new("{\"ready\":false}".into());
    static SNAPSHOT: RefCell<Option<Vec<u8>>> = RefCell::default();
    static WANT_SNAPSHOT: Cell<bool> = const { Cell::new(false) };
    static DIRTY: Cell<bool> = const { Cell::new(false) };
    static STATS: Cell<Stats> = Cell::new(Stats { deep_draw: f64::NEG_INFINITY, ..Default::default() });
    static OUTBOX: RefCell<Vec<&'static str>> = RefCell::default();
    static TEXT_REQ: RefCell<String> = RefCell::default();
    static SEARCH: RefCell<String> = RefCell::default();
    static STICKER: RefCell<Option<Vec<u8>>> = RefCell::default();
    static EXPORT: RefCell<Option<Result<Vec<u8>, String>>> = RefCell::default();
    static SEARCH_OUT: RefCell<String> = RefCell::new("[]".into());
    static HAS_SELECTION: Cell<bool> = const { Cell::new(false) };
}

/// Whether something is selected (so the page knows a copy has something).
pub fn set_has_selection(on: bool) {
    HAS_SELECTION.with(|h| h.set(on));
}

/// Ask the page to show its text editor; `json` says where and with what.
pub fn text_request(json: String) {
    TEXT_REQ.with(|t| *t.borrow_mut() = json);
    emit("text");
}

pub fn set_window(w: Arc<Window>) {
    WINDOW.with(|s| *s.borrow_mut() = Some(w));
}

fn push(c: Cmd) {
    CMDS.with(|q| q.borrow_mut().push_back(c));
    WINDOW.with(|w| {
        if let Some(w) = &*w.borrow() {
            w.request_redraw();
        }
    });
}

pub fn take_cmds() -> Vec<Cmd> {
    CMDS.with(|q| q.borrow_mut().drain(..).collect())
}

/// The canvas changed: it needs saving.
pub fn touch() {
    DIRTY.with(|d| d.set(true));
}

pub fn stats(f: impl FnOnce(&mut Stats)) {
    STATS.with(|s| {
        let mut v = s.get();
        f(&mut v);
        s.set(v);
    });
}

pub fn get_stats() -> Stats {
    STATS.with(|s| s.get())
}

/// Ask the page to do something only it can (file dialogs, downloads).
pub fn emit(what: &'static str) {
    OUTBOX.with(|o| o.borrow_mut().push(what));
}

pub fn set_status(json: String) {
    STATUS.with(|s| *s.borrow_mut() = json);
}

/// True once per request when a snapshot should be taken this frame.
pub fn snapshot_wanted() -> bool {
    WANT_SNAPSHOT.with(|w| w.replace(false))
}

pub fn put_snapshot(bytes: Vec<u8>) {
    DIRTY.with(|d| d.set(false));
    SNAPSHOT.with(|s| *s.borrow_mut() = Some(bytes));
}

pub fn is_dirty() -> bool {
    DIRTY.with(|d| d.get())
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

// ---- exported to JavaScript ------------------------------------------------

/// Load an offline copy (bytes of a `.ogpt` file). With no bytes, or bytes
/// that do not decode, start the demo (`demo`) or a blank canvas.
#[wasm_bindgen]
pub fn og_load(bytes: Option<Vec<u8>>, demo: bool) {
    push(Cmd::Load(bytes, demo));
}

/// Replace the canvas with the try-mode demo.
#[wasm_bindgen]
pub fn og_demo() {
    push(Cmd::Demo);
}

/// Replace the canvas with a blank one.
#[wasm_bindgen]
pub fn og_blank() {
    push(Cmd::Blank);
}

/// Set the settings fan's items from a comma-separated list of: new, open,
/// save, home, picture, bookmarks, timeline, fullscreen, tour (unknown names
/// skipped).
#[wasm_bindgen]
pub fn og_set_menu(items: &str) {
    use crate::ui::AppItem;
    let v = items
        .split(',')
        .filter_map(|s| {
            Some(match s.trim() {
                "new" => AppItem::New,
                "open" => AppItem::Open,
                "save" => AppItem::Save,
                "home" => AppItem::Home,
                "grid" => AppItem::Grid,
                "search" => AppItem::Search,
                "export" => AppItem::Export,
                "paste" => AppItem::Paste,
                "library" => AppItem::Library,
                "diagram" => AppItem::Diagram,
                "layout" => AppItem::Layout,
                "picture" => AppItem::Picture,
                "bookmarks" => AppItem::Bookmarks,
                "timeline" => AppItem::Timeline,
                "fullscreen" => AppItem::FullScreen,
                "tour" => AppItem::Tour,
                _ => return None,
            })
        })
        .collect();
    push(Cmd::Menu(v));
}

#[wasm_bindgen]
pub fn og_home() {
    push(Cmd::Home);
}

/// Bookmark the current view.
#[wasm_bindgen]
pub fn og_bookmark_add(name: String) {
    push(Cmd::BookmarkAdd(name));
}

/// Fly (animated zoom and pan) to bookmark `i`.
#[wasm_bindgen]
pub fn og_bookmark_go(i: usize) {
    push(Cmd::BookmarkGo(i));
}

#[wasm_bindgen]
pub fn og_bookmark_remove(i: usize) {
    push(Cmd::BookmarkRemove(i));
}

#[wasm_bindgen]
pub fn og_bookmark_rename(i: usize, name: String) {
    push(Cmd::BookmarkRename(i, name));
}

/// Show the canvas as it was after timeline event `i` (negative: back to now).
#[wasm_bindgen]
pub fn og_timeline(i: i32) {
    push(Cmd::Timeline((i >= 0).then_some(i as usize)));
}

/// Show only what was drawn between events `from` and `to` (inclusive);
/// a negative `to` goes back to now.
#[wasm_bindgen]
pub fn og_timeline_range(from: i32, to: i32) {
    push(Cmd::TimelineRange(
        (to >= 0).then_some((from.max(0) as usize, to as usize)),
    ));
}

/// Text search results for `q` as JSON: `[{"g":group,"text":excerpt,"zoom":log10}]`.
#[wasm_bindgen]
pub fn og_search(q: String) {
    SEARCH.with(|s| *s.borrow_mut() = q);
    push(Cmd::Search);
}

/// The query set by `og_search`.
pub fn search_query() -> String {
    SEARCH.with(|s| s.borrow().clone())
}

/// Store search results for the page.
pub fn set_search_results(json: String) {
    SEARCH_OUT.with(|s| *s.borrow_mut() = json);
}

/// The latest search results (see `og_search`).
#[wasm_bindgen]
pub fn og_search_results() -> String {
    SEARCH_OUT.with(|s| s.borrow().clone())
}

/// Export as `fmt` (png, jpg, svg, pdf); fetch the bytes with
/// `og_export_take`. PNG/JPEG come back as SVG for the page to rasterise.
#[wasm_bindgen]
pub fn og_export(fmt: String, selection: bool, background: bool) {
    EXPORT.with(|e| *e.borrow_mut() = None);
    push(Cmd::Export(fmt, selection, background));
}

/// The export's bytes once ready; throws with the reason if it failed.
#[wasm_bindgen]
pub fn og_export_take() -> Result<Option<Vec<u8>>, JsValue> {
    match EXPORT.with(|e| e.borrow_mut().take()) {
        None => Ok(None),
        Some(Ok(b)) => Ok(Some(b)),
        Some(Err(e)) => Err(JsValue::from_str(&e)),
    }
}

pub fn set_export(r: Result<Vec<u8>, String>) {
    EXPORT.with(|e| *e.borrow_mut() = Some(r));
}

/// A sticker just made from the selection (see "sticker" requests).
pub fn set_sticker(b: Vec<u8>) {
    STICKER.with(|s| *s.borrow_mut() = Some(b));
}

/// Take the sticker made for an "Add to library".
#[wasm_bindgen]
pub fn og_sticker_take() -> Option<Vec<u8>> {
    STICKER.with(|s| s.borrow_mut().take())
}

/// A sticker's thumbnail as SVG, `px` CSS pixels square ("" if unreadable).
#[wasm_bindgen]
pub fn og_sticker_svg(bytes: Vec<u8>, px: f64) -> String {
    crate::library::Sticker::decode(&bytes)
        .map(|s| crate::export::sticker_svg(&s, px))
        .unwrap_or_default()
}

/// Place a copy of a sticker at (x, y) CSS px, or the middle of the screen.
#[wasm_bindgen]
pub fn og_sticker_place(bytes: Vec<u8>, x: Option<f64>, y: Option<f64>) {
    push(Cmd::Sticker(bytes, at(x, y)));
}

/// Fly to a search result.
#[wasm_bindgen]
pub fn og_search_go(g: u32) {
    push(Cmd::SearchGo(g));
}

/// Make the moment shown in the timeline the current canvas.
#[wasm_bindgen]
pub fn og_timeline_restore() {
    push(Cmd::TimelineRestore);
}

/// The app's state as JSON: ready, zoom, strokes, bookmarks, timeline and
/// tour counters.
#[wasm_bindgen]
pub fn og_status() -> String {
    STATUS.with(|s| s.borrow().clone())
}

/// What the app asked the page to do since the last call, as a JSON array
/// ("save": download an offline copy, "open": load one).
#[wasm_bindgen]
pub fn og_requests() -> String {
    let v: Vec<String> = OUTBOX
        .with(|o| std::mem::take(&mut *o.borrow_mut()))
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect();
    format!("[{}]", v.join(","))
}

/// Where and how to show the text editor the app asked for ("text"
/// request): JSON with x, y (CSS px, top-left), size (cap height, CSS px),
/// color and the starting text.
#[wasm_bindgen]
pub fn og_text_request() -> String {
    TEXT_REQ.with(|t| t.borrow().clone())
}

/// The text editor's result: the text, or nothing if cancelled.
#[wasm_bindgen]
pub fn og_text_done(text: Option<String>) {
    push(Cmd::Text(text));
}

/// Register a font (bytes of a .ttf / .otf file). `name`: the name to list
/// it under (bundled fonts); without it, the file's family name. `user`:
/// added by the user (listed under "Yours"). `select`: make it the text
/// font now. Returns the font's name, or "!" and the reason it could not be
/// added.
#[wasm_bindgen]
pub fn og_font_add(
    name: Option<String>,
    category: String,
    bytes: Vec<u8>,
    user: bool,
    select: bool,
) -> String {
    match crate::font::register(name.as_deref(), &category, bytes, user) {
        Ok((id, name)) => {
            if select {
                push(Cmd::FontAdded(id, name.clone()));
            } else {
                WINDOW.with(|w| {
                    if let Some(w) = &*w.borrow() {
                        w.request_redraw();
                    }
                });
            }
            name
        }
        Err(e) => format!("!{e}"),
    }
}

/// Whether something is selected.
#[wasm_bindgen]
pub fn og_has_selection() -> bool {
    HAS_SELECTION.with(|h| h.get())
}

/// Copy (or cut) the selection. Returns false when nothing is selected,
/// so the page leaves the clipboard alone.
#[wasm_bindgen]
pub fn og_copy(cut: bool) -> bool {
    let on = HAS_SELECTION.with(|h| h.get());
    if on {
        push(Cmd::Copy(cut));
    }
    on
}

/// Paste what was copied in this app.
#[wasm_bindgen]
pub fn og_paste_own() {
    push(Cmd::PasteOwn);
}

fn at(x: Option<f64>, y: Option<f64>) -> Option<[f64; 2]> {
    Some([x?, y?])
}

/// Put a picture (bytes of a PNG, JPEG, GIF or WebP file) on the canvas,
/// centred on (x, y) in CSS px, or the middle of the screen. Returns "" or
/// why the picture could not be used.
#[wasm_bindgen]
pub fn og_paste_image(bytes: Vec<u8>, x: Option<f64>, y: Option<f64>) -> String {
    match crate::images::prepare(bytes) {
        Ok(a) => {
            push(Cmd::Picture(a, at(x, y)));
            String::new()
        }
        Err(e) => e,
    }
}

/// One page of a PDF the page is importing (rendered with pdf.js), in
/// order: page 0 starts the import. Returns an error message, or "".
#[wasm_bindgen]
pub fn og_pdf_page(
    bytes: Vec<u8>,
    index: usize,
    total: usize,
    x: Option<f64>,
    y: Option<f64>,
    name: String,
) -> String {
    match crate::images::prepare(bytes) {
        Ok(a) => {
            push(Cmd::PdfPage(a, index, total, at(x, y), name));
            String::new()
        }
        Err(e) => e,
    }
}

/// Put pasted text on the canvas: a table if it is tab-separated or a
/// Markdown table, else a text.
#[wasm_bindgen]
pub fn og_paste_text(text: String, x: Option<f64>, y: Option<f64>) {
    push(Cmd::PasteText(text, at(x, y)));
}

/// Ask for a snapshot of the canvas; pick it up with `og_snapshot_take`.
/// Without `force`, only when something changed since the last one.
#[wasm_bindgen]
pub fn og_snapshot_request(force: bool) {
    if force || is_dirty() {
        WANT_SNAPSHOT.with(|w| w.set(true));
        WINDOW.with(|w| {
            if let Some(w) = &*w.borrow() {
                w.request_redraw();
            }
        });
    }
}

#[wasm_bindgen]
pub fn og_snapshot_take() -> Option<Vec<u8>> {
    SNAPSHOT.with(|s| s.borrow_mut().take())
}
