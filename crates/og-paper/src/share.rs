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

use egui::{pos2, Color32, Pos2};
use web_time::{Duration, Instant};

use ogpaper_core::sync::{Clock, Event, Hlc, Log, Merged, Replace, Version};
use ogpaper_core::{Camera, Scene};

use crate::objects::{Group, Objects};
use crate::timeline::{now_ms, uid_ms};
use crate::App;

/// The merge state of the open canvas.
pub struct Share {
    /// Names the canvas across its copies.
    pub canvas: u128,
    pub clock: Clock,
    pub log: Log,
    /// What the copy last merged in had: a changes file need only carry
    /// what is not in it (kept in the prefs per canvas).
    pub seen: Option<Version>,
}

/// What a copy carries for merging (from a file or a snapshot).
#[derive(Clone, Debug, Default)]
pub struct CopyLog {
    pub canvas: u128,
    pub events: Vec<Event>,
    pub replaces: Vec<Replace>,
    /// Only the changes since some sync, not the whole canvas.
    pub partial: bool,
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
            seen: None,
        }
    }

    fn seen_key(&self) -> String {
        format!("seen.{:032x}", self.canvas)
    }

    /// Remember what a merged copy had.
    pub fn remember_seen(&mut self, v: &Version) {
        let mut all = self.seen.clone().unwrap_or_default();
        for (p, h) in &v.0 {
            let e = all.0.entry(*p).or_insert(*h);
            *e = (*e).max(*h);
        }
        let mut prefs = crate::prefs::load();
        prefs.insert(self.seen_key(), all.to_text());
        crate::prefs::save(&prefs);
        self.seen = Some(all);
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
        s.seen = crate::prefs::load()
            .get(&s.seen_key())
            .map(|t| Version::parse(t));
        s
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn copy_log(&self) -> CopyLog {
        CopyLog {
            canvas: self.canvas,
            events: self.log.events().collect(),
            replaces: self.log.replacements().copied().collect(),
            partial: false,
        }
    }
}

/// How long a merge's changes stay highlighted.
const CHANGES_FOR: Duration = Duration::from_secs(10);

/// What the last merge changed, highlighted for a while.
pub struct Changes {
    pub shown: Vec<u32>,
    pub hidden: Vec<u32>,
    pub at: Instant,
}

impl Changes {
    pub fn live(&self) -> bool {
        self.at.elapsed() < CHANGES_FOR
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
        self.sync_touch();
    }

    /// Something changed that other copies should get.
    pub(crate) fn sync_touch(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.folder_touch();
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
        self.merge_with(scene, objs, copy, false);
    }

    /// A merge in the background (sync folder, a connection): no flight,
    /// and a message only when something changed.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn merge_quiet(
        &mut self,
        scene: Scene,
        objs: Objects,
        copy: Option<CopyLog>,
    ) -> Option<Version> {
        self.merge_with(scene, objs, copy, true)
    }

    /// Merge a copy; its version when it was of this canvas.
    fn merge_with(
        &mut self,
        scene: Scene,
        objs: Objects,
        copy: Option<CopyLog>,
        quiet: bool,
    ) -> Option<Version> {
        let mut theirs = Share::loaded(&scene, copy);
        theirs.log.policy = self.share.log.policy;
        if theirs.canvas != self.share.canvas {
            if !quiet {
                self.say("That file is a different canvas: use Import canvas to bring it in");
            }
            return None;
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
        self.share.remember_seen(theirs.log.version());
        // Every stroke shows or hides as the joined log says.
        let visible = self.share.log.visible();
        let known = self.share.log.state();
        let (mut shown, mut hidden) = (Vec::new(), Vec::new());
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
                shown.push(id);
            } else {
                self.scene.delete(id);
                hidden.push(id);
            }
            self.timeline.record(id, want);
        }
        // Show what changed: highlighted, and the view framed on it.
        let mut all = shown.clone();
        all.extend(&hidden);
        if !quiet {
            if let Some(cam) = self.frame_strokes(&all) {
                self.fly = Some(cam);
                self.fly_last = Instant::now();
            }
        }
        if !all.is_empty() || !added.is_empty() {
            self.sync_touch();
        }
        self.merge_changes = (!all.is_empty()).then(|| Changes {
            shown,
            hidden,
            at: Instant::now(),
        });
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
        if !quiet || !all.is_empty() {
            self.say(report.text());
        }
        self.redraw();
        Some(theirs.log.version().clone())
    }
}

impl App {
    /// A copy holding only what the copy last merged in lacks: the newer
    /// events, the strokes they name, their shapes and pictures. None when
    /// there was no merge yet (a full copy is then the way to share).
    pub(crate) fn changes_copy(&self) -> Option<(Vec<u8>, usize)> {
        let seen = self.share.seen.as_ref()?;
        Some(self.changes_since(seen, true))
    }

    /// A changes-only copy of what a copy at `seen` lacks, and how many
    /// changes it holds (0: nothing to send).
    /// Picture files go along only with `images` (live connections ask
    /// for the ones they lack instead).
    pub(crate) fn changes_since(&self, seen: &Version, images: bool) -> (Vec<u8>, usize) {
        let events = self.share.log.missing(seen);
        let replaces: Vec<Replace> = self
            .share
            .log
            .replacements()
            .filter(|r| !seen.has(&r.edit))
            .copied()
            .collect();
        let named: std::collections::HashSet<u128> = events.iter().map(|e| e.item).collect();
        // Strokes the changes name, in a scene of their own.
        let mut scene = Scene::new();
        let mut map = HashMap::new();
        for (i, s) in self.scene.strokes.iter().enumerate() {
            let i = i as u32;
            if !named.contains(&s.uid) {
                continue;
            }
            let id = scene.add_stroke_at(
                self.scene.stroke_cell(i),
                self.scene.stroke_points(i),
                self.scene.stroke_style(i),
                s.uid,
                s.z,
            );
            if s.deleted {
                scene.delete(id);
            }
            map.insert(i, id);
        }
        let mut objs = Objects::default();
        for g in &self.objs.groups {
            let strokes: Vec<u32> = g
                .strokes
                .iter()
                .filter_map(|s| map.get(s).copied())
                .collect();
            if strokes.len() == g.strokes.len() && !strokes.is_empty() {
                if let crate::objects::ObjData::Image { id, .. } = g.data {
                    if let Some(a) = self.objs.images.get(&id).filter(|_| images) {
                        objs.images.insert(id, a.clone());
                    }
                }
                objs.add(Group {
                    cell: g.cell.clone(),
                    data: g.data.clone(),
                    strokes,
                });
            }
        }
        let n = events.len() + replaces.len();
        let log = CopyLog {
            canvas: self.share.canvas,
            events,
            replaces,
            partial: true,
        };
        let bytes = crate::snapshot::encode(
            &scene,
            &self.cam,
            &crate::timeline::Timeline::default(),
            &[],
            &objs,
            &log,
        );
        (bytes, n)
    }

    /// A camera framing strokes `ids` (None if there are none).
    pub(crate) fn frame_strokes(&self, ids: &[u32]) -> Option<Camera> {
        let top = ids
            .iter()
            .map(|&i| self.scene.stroke_cell(i).level)
            .min()?
            .min(self.cam.cell.level);
        let mut reference = self.cam.cell.clone();
        while reference.level > top {
            reference = reference.parent();
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for &i in ids {
            let cell = self.scene.stroke_cell(i);
            let o = cell.origin_in(&reference);
            let side = cell.side_in(&reference);
            for p in self.scene.stroke_points(i) {
                let q = [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side];
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
        }
        if !(lo[0] <= hi[0]) || !lo.iter().chain(&hi).all(|v| v.is_finite()) {
            return None;
        }
        let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let mut cam = Camera::new(reference, c, self.cam.base_px);
        let ppc = cam.ppc();
        let [w, h] = self.size();
        let bw = ((hi[0] - lo[0]) * ppc).max(1e-300);
        let bh = ((hi[1] - lo[1]) * ppc).max(1e-300);
        // Not closer than the view already is for a single tiny change.
        let f = (0.7 * w / bw)
            .min(0.6 * h / bh)
            .min(self.cam.ppc() / ppc * 4.0);
        cam.zoom_at(f, [0.0, 0.0]);
        Some(cam)
    }

    /// The last merge's changes over the canvas: what came in glows, what
    /// went is a faint red ghost.
    pub(crate) fn changes_overlay(&self, ov: &mut crate::ui::Overlay) {
        let Some(c) = self.merge_changes.as_ref().filter(|c| c.live()) else {
            return;
        };
        let ppp = self.ppp();
        let fade = 1.0 - (c.at.elapsed().as_secs_f32() / CHANGES_FOR.as_secs_f32()).powi(3);
        let pts_of = |id: u32| -> Vec<Pos2> {
            let (o, side) = crate::objects::frame(self.scene.stroke_cell(id), &self.cam);
            self.scene
                .stroke_points(id)
                .iter()
                .map(|p| {
                    let q = self.cam_to_px([o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side]);
                    pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32)
                })
                .collect()
        };
        let width = |id: u32| {
            let s = &self.scene.strokes[id as usize];
            let side = self.scene.stroke_cell(id).side_in(&self.cam.cell);
            (s.width as f64 * side * self.cam.ppc() / ppp) as f32
        };
        let glow = Color32::from_rgba_unmultiplied(40, 160, 255, (110.0 * fade) as u8);
        let gone = Color32::from_rgba_unmultiplied(220, 50, 50, (120.0 * fade) as u8);
        for &id in c.shown.iter().take(5000) {
            ov.lines.push((pts_of(id), width(id) + 8.0, glow, false));
        }
        for &id in c.hidden.iter().take(5000) {
            ov.lines.push((pts_of(id), width(id).max(1.5), gone, false));
        }
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
