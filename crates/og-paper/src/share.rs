// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Copies of one canvas, kept mergeable (see `ogpaper_core::sync`).
//!
//! Every canvas has an id that its copies share, and every device a peer
//! id. Each time a stroke is shown or hidden (drawn, erased, undone,
//! edited) the change goes in the merge log under the stroke's uid with a
//! clock stamp; an edit also notes which stroke each new one replaced. Merge
//! copy takes another copy of the same canvas: strokes it has that this one
//! lacks come in as they are, both logs are joined, and every stroke then
//! shows or hides as the joined log says.

use std::collections::{BTreeMap, HashMap};

use ogpaper_core::sync::{Clock, Event, Hlc, Log, Merged, Replace};
use ogpaper_core::Scene;

use crate::objects::{Group, Objects};
use crate::timeline::{now_ms, uid_ms};
use crate::App;

/// The merge state of the open canvas.
pub struct Share {
    /// Names the canvas across its copies.
    pub canvas: u128,
    pub clock: Clock,
    pub log: Log,
}

/// What a copy carries for merging (from a file or a snapshot).
#[derive(Clone, Debug, Default)]
pub struct CopyLog {
    pub canvas: u128,
    pub events: Vec<Event>,
    pub replaces: Vec<Replace>,
}

/// This device's id, made once and kept in the prefs.
pub fn peer_id() -> u64 {
    let mut p = crate::prefs::load();
    if let Some(v) = p.get("peer").and_then(|v| u64::from_str_radix(v, 16).ok()) {
        if v != 0 {
            return v;
        }
    }
    let v = (crate::uid::new() as u64) | 1;
    p.insert("peer".into(), format!("{v:016x}"));
    crate::prefs::save(&p);
    v
}

/// A log for a canvas saved without one: each stroke shown when its id says
/// it was made (time 0 when it does not say), deleted ones hidden just
/// after, stamped by a "legacy" peer 0. Every device derives exactly the
/// same events for the same strokes, so derived logs merge cleanly.
pub fn derive_log(scene: &Scene) -> Vec<Event> {
    let mut out = Vec::with_capacity(scene.strokes.len());
    for s in &scene.strokes {
        let ms = uid_ms(s.uid).unwrap_or(0).max(0) as u64;
        out.push(Event {
            item: s.uid,
            alive: true,
            at: Hlc { ms, n: 0, peer: 0 },
        });
        if s.deleted {
            out.push(Event {
                item: s.uid,
                alive: false,
                at: Hlc { ms, n: 1, peer: 0 },
            });
        }
    }
    out
}

impl Share {
    /// A new canvas with an empty log.
    pub fn fresh() -> Self {
        Self {
            canvas: crate::uid::new(),
            clock: Clock::new(peer_id()),
            log: Log::new(),
        }
    }

    /// The state of a loaded copy: its id and log, or a new id and a log
    /// derived from its strokes when it has none.
    pub fn loaded(scene: &Scene, copy: Option<CopyLog>) -> Self {
        let mut s = Self::fresh();
        let copy = copy.filter(|c| c.canvas != 0);
        if let Some(c) = &copy {
            s.canvas = c.canvas;
        }
        let events = match copy {
            Some(c) if !c.events.is_empty() => {
                for r in &c.replaces {
                    s.log.add_replace(*r);
                }
                c.events
            }
            _ => derive_log(scene),
        };
        for e in events {
            s.clock.observe(e.at);
            s.log.add(e);
        }
        s
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn copy_log(&self) -> CopyLog {
        CopyLog {
            canvas: self.canvas,
            events: self.log.events().collect(),
            replaces: self.log.replacements().copied().collect(),
        }
    }
}

/// What a Merge copy did, for the message.
pub struct Report {
    pub new_strokes: usize,
    pub merged: Merged,
    pub conflicts: usize,
    pub me: u64,
}

impl Report {
    pub fn text(&self) -> String {
        let mine: usize = self.merged.events_from.get(&self.me).copied().unwrap_or(0);
        let theirs: usize = self.merged.events_from.values().sum::<usize>() - mine;
        if theirs == 0 && mine == 0 && self.new_strokes == 0 {
            return "Already up to date: nothing new in that copy".into();
        }
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut s = format!(
            "Merged: {}, {} from others",
            plural(self.new_strokes, "new stroke", "new strokes"),
            plural(theirs, "change", "changes")
        );
        if mine > 0 {
            s += &format!(", {mine} of yours back");
        }
        if self.conflicts > 0 {
            s += &format!(
                "; {} rival edits kept in the timeline (newest won)",
                self.conflicts
            );
        }
        s
    }
}

impl App {
    /// A stroke was shown or hidden by an edit here: timeline, merge log, file.
    pub(crate) fn note(&mut self, id: u32, alive: bool) {
        self.timeline.record(id, alive);
        let at = self.share.clock.now(now_ms().max(0) as u64);
        let e = Event {
            item: self.scene.strokes[id as usize].uid,
            alive,
            at,
        };
        self.share.log.add(e);
        self.persist_event(&e, None);
    }

    /// One edit's new strokes, each with the strokes of the thing it
    /// replaced (`old`; the first stands for them all).
    pub(crate) fn note_replaced(&mut self, pairs: &[(Vec<u32>, Vec<u32>)]) {
        if pairs.is_empty() {
            return;
        }
        let edit = self.share.clock.now(now_ms().max(0) as u64);
        for (old, new) in pairs {
            let Some(&lead) = old.first() else { continue };
            let replaces = self.scene.strokes[lead as usize].uid;
            for &n in new {
                let r = Replace {
                    item: self.scene.strokes[n as usize].uid,
                    replaces,
                    edit,
                };
                self.share.log.add_replace(r);
                self.persist_replace(&r);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_event(&mut self, e: &Event, _: Option<()>) {
        if let Some(f) = &self.file {
            let _ = f.put_event(e);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn persist_replace(&mut self, r: &Replace) {
        if let Some(f) = &self.file {
            let _ = f.put_replace(r);
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn persist_event(&mut self, _: &Event, _: Option<()>) {
        crate::web::touch();
    }
    #[cfg(target_arch = "wasm32")]
    fn persist_replace(&mut self, _: &Replace) {
        crate::web::touch();
    }

    /// Write the whole merge state to the open file (after a load or merge).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn persist_share(&mut self) {
        if let Some(f) = &self.file {
            let _ = f.set_canvas_id(self.share.canvas);
            for e in self.share.log.events() {
                let _ = f.put_event(&e);
            }
            for r in self.share.log.replacements() {
                let _ = f.put_replace(r);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn persist_share(&mut self) {
        crate::web::touch();
    }

    /// Merge another copy of this canvas into it.
    pub(crate) fn merge_copy(&mut self, scene: Scene, objs: Objects, copy: Option<CopyLog>) {
        let theirs = Share::loaded(&scene, copy);
        if theirs.canvas != self.share.canvas {
            self.say("That file is a different canvas: use Import canvas to bring it in");
            return;
        }
        // Strokes this copy lacks come in as they are (same cell, same uid).
        let mut by_uid: HashMap<u128, u32> = self
            .scene
            .strokes
            .iter()
            .enumerate()
            .map(|(i, s)| (s.uid, i as u32))
            .collect();
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut added = Vec::new();
        for (i, s) in scene.strokes.iter().enumerate() {
            let i = i as u32;
            if let Some(&local) = by_uid.get(&s.uid) {
                map.insert(i, local);
                continue;
            }
            let id = self.scene.add_stroke_at(
                scene.stroke_cell(i),
                scene.stroke_points(i),
                scene.stroke_style(i),
                s.uid,
                s.z,
            );
            // Hidden until the joined log says otherwise.
            self.scene.delete(id);
            by_uid.insert(s.uid, id);
            map.insert(i, id);
            added.push(id);
        }
        for g in objs.groups {
            let strokes: Vec<u32> = g
                .strokes
                .iter()
                .filter_map(|s| map.get(s).copied())
                .collect();
            let fresh = strokes.iter().all(|s| added.contains(s));
            if !strokes.is_empty() && fresh {
                self.objs.add(Group {
                    cell: g.cell,
                    data: g.data,
                    strokes,
                });
            }
        }
        for (id, a) in objs.images {
            self.objs.images.entry(id).or_insert(a);
        }
        let mut new_events: Vec<Event> = theirs.log.events().collect();
        new_events.retain(|e| !self.share.log.version().has(&e.at));
        for e in &new_events {
            self.share.clock.observe(e.at);
        }
        let replaces: Vec<Replace> = theirs.log.replacements().copied().collect();
        let merged = self.share.log.merge(&new_events, &replaces);
        // Every stroke shows or hides as the joined log says.
        let visible = self.share.log.visible();
        let known = self.share.log.state();
        for id in 0..self.scene.strokes.len() as u32 {
            let s = &self.scene.strokes[id as usize];
            if !known.contains_key(&s.uid) {
                continue;
            }
            let want = visible.contains(&s.uid);
            if want == !s.deleted {
                continue;
            }
            if want {
                self.scene.restore(id);
            } else {
                self.scene.delete(id);
            }
            self.timeline.record(id, want);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self.file {
            for &id in &added {
                let _ = f.put_stroke(&self.scene, id);
            }
            for id in 0..self.scene.strokes.len() as u32 {
                let _ = f.put_deleted(&self.scene, id);
            }
        }
        self.persist_share();
        self.persist_groups();
        if let Some(g) = self.gpu.as_mut() {
            g.sync(&self.scene);
        }
        let report = Report {
            new_strokes: added.len(),
            merged,
            conflicts: self.share.log.conflicts().len(),
            me: self.share.clock.peer(),
        };
        self.say(report.text());
        self.redraw();
    }
}

/// Events per peer, for tests and reports.
#[allow(dead_code)]
pub fn by_peer(events: &[Event]) -> BTreeMap<u64, usize> {
    let mut m = BTreeMap::new();
    for e in events {
        *m.entry(e.at.peer).or_default() += 1;
    }
    m
}
