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

use wasm_bindgen::prelude::wasm_bindgen;
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
    /// Make the moment shown in the timeline the current canvas (undoable).
    TimelineRestore,
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
/// save, home, bookmarks, timeline, fullscreen, tour (unknown names skipped).
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
