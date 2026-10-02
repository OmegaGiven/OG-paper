// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Shapes, text and the select tool: drawing shapes, typing text, and
//! selecting, moving, resizing, rotating, restyling, reordering, duplicating
//! and deleting what is on the canvas.
//!
//! Screen positions here are physical pixels from the top-left (like
//! pointer events); the UI overlay is in points (pixels / ppp).

use egui::{pos2, Pos2};
use ogpaper_core::{hit, Change, Point, Scene, Style};
use web_time::{Duration, Instant};

use crate::objects::{self, home, to_cam, Group, ObjData, ObjRef, Op, TextStyle};
use crate::shapes::{self, Geom};
use crate::ui::{self, Action, SelKind, SelStyle, Tool};
use crate::{uid, App, Gesture};

/// What a drag on the selection does.
#[derive(Clone, Copy, Debug)]
pub enum DragKind {
    Move,
    /// Corner index (0 top-left, clockwise).
    Scale(usize),
    Rotate,
}

pub struct SelDrag {
    pub kind: DragKind,
    pub start: [f64; 2],
    pub cur: [f64; 2],
    /// Selection box at the start (px), and its rotation.
    pub box0: [[f64; 2]; 4],
    pub rot0: f64,
    /// Original points of every selected stroke (restored before committing).
    pub orig: Vec<(u32, Vec<Point>)>,
    /// Diagram mode: connectors with ends on the selection (drawn following
    /// it), and their strokes, hidden meanwhile.
    pub att: Vec<(u32, ObjData, [bool; 2])>,
    pub hidden: Vec<u32>,
}

/// Text being typed.
pub struct TextEdit {
    /// Top-left of the text, camera units.
    pub at: [f64; 2],
    /// Rotation (kept when editing an existing text).
    pub rot: f64,
    /// Cap height, camera units.
    pub size: f64,
    pub style: TextStyle,
    pub text: String,
    /// The text group being edited, if any.
    pub group: Option<u32>,
    /// Editing a table's cells (as tab-separated text).
    pub table: bool,
}

/// Editing state kept by the app.
#[derive(Default)]
pub struct EditState {
    pub selection: Vec<ObjRef>,
    pub sel_drag: Option<SelDrag>,
    /// Shape being dragged out: start and current (px).
    pub shape_drag: Option<([f64; 2], [f64; 2])>,
    pub marquee: Option<([f64; 2], [f64; 2])>,
    /// Lasso loop being drawn (px).
    pub lasso: Option<Vec<[f64; 2]>>,
    /// A picture being cropped.
    pub crop: Option<crate::crop::CropEdit>,
    pub text: Option<TextEdit>,
    pub clipboard: Vec<ObjRef>,
    pub last_tap: Option<(Instant, [f64; 2])>,
    /// The selection style last handed to the panel.
    pub sel_seen: Option<SelStyle>,
    pub sel_seen_for: Vec<ObjRef>,
    pub seed: u32,
}

const HANDLE_PT: f64 = 12.0;
/// Share of an object's points that must be inside a lasso loop.
const LASSO_SHARE: f64 = 0.6;

/// Whether `p` is inside the closed polygon `poly` (even-odd rule).
pub(crate) fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn rot2(p: [f64; 2], a: f64) -> [f64; 2] {
    let (s, c) = a.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
}

fn inside_quad(p: [f64; 2], q: &[[f64; 2]; 4]) -> bool {
    let poly: Vec<[f32; 2]> = q.iter().map(|v| [v[0] as f32, v[1] as f32]).collect();
    hit::inside([p[0] as f32, p[1] as f32], &poly)
}

impl App {
    // ---- coordinates -----------------------------------------------------

    pub(crate) fn px_to_cam(&self, p: [f64; 2]) -> [f64; 2] {
        self.cam.screen_to_cam(self.centred(p))
    }

    pub(crate) fn cam_to_px(&self, q: [f64; 2]) -> [f64; 2] {
        let s = self.size();
        let ppc = self.cam.ppc();
        [
            (q[0] - self.cam.off[0]) * ppc + s[0] * 0.5,
            (q[1] - self.cam.off[1]) * ppc + s[1] * 0.5,
        ]
    }

    /// A screen-space edit as a camera-space one.
    fn op_to_cam(&self, op: &Op) -> Op {
        let ppc = self.cam.ppc();
        match *op {
            Op::Move(d) => Op::Move([d[0] / ppc, d[1] / ppc]),
            Op::Scale { pivot, sx, sy, rot } => Op::Scale {
                pivot: self.px_to_cam(pivot),
                sx,
                sy,
                rot,
            },
            Op::Rotate { pivot, angle } => Op::Rotate {
                pivot: self.px_to_cam(pivot),
                angle,
            },
        }
    }

    /// A stroke's points on screen (px).
    fn stroke_px(&self, id: u32) -> Vec<[f64; 2]> {
        let cell = self.scene.stroke_cell(id);
        let (o, side) = objects::frame(cell, &self.cam);
        self.scene
            .stroke_points(id)
            .iter()
            .map(|p| self.cam_to_px([o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side]))
            .collect()
    }

    fn stroke_width_px(&self, id: u32) -> f64 {
        let side = self.scene.stroke_cell(id).side_in(&self.cam.cell);
        self.scene.strokes[id as usize].width as f64 * side * self.cam.ppc()
    }

    // ---- records ---------------------------------------------------------

    /// Bookkeeping after strokes were added and removed by one edit.
    /// Record added strokes as part of the last undo step, if that step
    /// only added strokes; else as a new one.
    pub(crate) fn record_added_merged(&mut self, added: Vec<u32>) {
        if !self.history.extend_added(&added) {
            return self.record_edit(vec![], added);
        }
        for &id in &added {
            self.timeline.record(id, true);
            self.persist_new(id);
        }
        self.persist_groups();
        if let Some(g) = self.gpu.as_mut() {
            g.sync(&self.scene);
        }
    }

    pub(crate) fn record_edit(&mut self, removed: Vec<u32>, added: Vec<u32>) {
        for &id in &removed {
            self.scene.delete(id);
            self.timeline.record(id, false);
            self.persist_deleted(id);
        }
        for &id in &added {
            self.timeline.record(id, true);
            self.persist_new(id);
        }
        let change = match (removed.is_empty(), added.is_empty()) {
            (true, true) => return,
            (true, false) => Change::Added(added),
            (false, true) => Change::Deleted(removed),
            (false, false) => Change::Replace { removed, added },
        };
        self.history.record(change);
        self.persist_groups();
        if let Some(g) = self.gpu.as_mut() {
            g.sync(&self.scene);
        }
        self.redraw();
    }

    fn next_seed(&mut self) -> u32 {
        self.edit.seed = self.edit.seed.wrapping_add(0x9E37_79B9) ^ (uid::new() as u32);
        self.edit.seed
    }

    /// Store new object data (camera units) as a group at draw orders `z`
    /// (or on top). Returns the group and its strokes.
    pub(crate) fn add_group(
        &mut self,
        data_cam: &ObjData,
        z: Option<(f64, f64)>,
    ) -> (u32, Vec<u32>) {
        let (cell, data) = home(data_cam, &self.cam);
        let n = data.pieces().len().max(1) as f64;
        let z = z.unwrap_or((self.scene.z_top + 1.0, self.scene.z_top + n));
        let ids = objects::emit(&mut self.scene, &cell, &data, z);
        let g = self.objs.add(Group {
            cell,
            data,
            strokes: ids.clone(),
        });
        (g, ids)
    }

    pub(crate) fn z_range(&self, ids: &[u32]) -> (f64, f64) {
        ids.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &i| {
            let z = self.scene.strokes[i as usize].z;
            (lo.min(z), hi.max(z))
        })
    }

    // ---- shapes ----------------------------------------------------------

    pub(crate) fn shape_begin(&mut self, p: [f64; 2]) {
        self.edit.shape_drag = Some((p, p));
    }

    pub(crate) fn shape_move(&mut self, p: [f64; 2]) {
        if let Some(d) = self.edit.shape_drag.as_mut() {
            d.1 = p;
        }
        self.redraw();
    }

    /// The shape being dragged, in screen px.
    fn shape_geom_px(&self, a: [f64; 2], b: [f64; 2]) -> Geom {
        let st = &self.ui.shape;
        let shift = self.mods.shift_key();
        if st.kind.is_linear() {
            let mut b = b;
            if shift {
                // Snap to 15 degree steps.
                let d = [b[0] - a[0], b[1] - a[1]];
                let ang = (d[1].atan2(d[0]) / (std::f64::consts::PI / 12.0)).round()
                    * (std::f64::consts::PI / 12.0);
                let l = d[0].hypot(d[1]);
                b = [a[0] + l * ang.cos(), a[1] + l * ang.sin()];
            }
            Geom {
                pts: vec![a, b],
                ..Default::default()
            }
        } else {
            shapes::box_from_drag(a, b, shift, self.mods.alt_key())
        }
    }

    /// Diagram mode: a line or arrow's ends snapped onto outlines.
    fn snap_line(&self, a: [f64; 2], b: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        match self.ui.shape.kind {
            shapes::ShapeKind::Line | shapes::ShapeKind::Arrow => self.snap_ends(a, b),
            _ => (a, b),
        }
    }

    pub(crate) fn shape_end(&mut self, cancel: bool) {
        let Some((a, b)) = self.edit.shape_drag.take() else {
            return;
        };
        let min = 3.0 * self.ppp();
        if cancel || (b[0] - a[0]).abs().max((b[1] - a[1]).abs()) < min {
            self.redraw();
            return;
        }
        let (a, b) = self.snap_line(a, b);
        let g = self.shape_geom_px(a, b);
        let ppc = self.cam.ppc();
        let seed = self.next_seed();
        let to_cam = |p: [f64; 2]| self.px_to_cam(p);
        let data = ObjData::Shape {
            style: self.ui.shape,
            geom: Geom {
                center: to_cam(g.center),
                half: [g.half[0] / ppc, g.half[1] / ppc],
                rot: 0.0,
                pts: g.pts.iter().map(|&p| to_cam(p)).collect(),
            },
            width: self.ui.shape_width as f64 * self.ppp() / ppc,
            seed,
        };
        let (_, ids) = self.add_group(&data, None);
        self.record_edit(vec![], ids);
    }

    // ---- text ------------------------------------------------------------

    /// Start typing a new text at screen point `p`.
    pub(crate) fn text_begin(&mut self, p: [f64; 2]) {
        if self.edit.text.is_some() {
            // Tapping away finishes the text. On the web the page's editor
            // sends its text when it loses focus, so just wait for it.
            #[cfg(not(target_arch = "wasm32"))]
            self.text_commit();
            return;
        }
        // Tapping an existing text (or table) edits it instead of starting a
        // new one on top.
        if let Some(g) = self.text_at(p) {
            self.text_edit_group(g);
            return;
        }
        let ppc = self.cam.ppc();
        self.edit.text = Some(TextEdit {
            at: self.px_to_cam(p),
            rot: 0.0,
            size: self.ui.text_size as f64 * self.ppp() / ppc,
            style: self.ui.text,
            text: String::new(),
            group: None,
            table: false,
        });
        self.text_open_editor();
    }

    /// Edit an existing text group (or a table, as tab-separated text).
    pub(crate) fn text_edit_group(&mut self, g: u32) {
        let grp = &self.objs.groups[g as usize];
        let (text, style, geom, size, table) = match to_cam(&grp.cell, &grp.data, &self.cam) {
            ObjData::Text {
                text,
                style,
                geom,
                size,
                ..
            } => (text, style, geom, size, false),
            ObjData::Table {
                cells,
                style,
                geom,
                size,
                ..
            } => (objects::to_tsv(&cells), style, geom, size, true),
            _ => return,
        };
        let tl = {
            let d = rot2([-geom.half[0], -geom.half[1]], geom.rot);
            [geom.center[0] + d[0], geom.center[1] + d[1]]
        };
        // The text panel shows this text's settings, so new texts continue
        // in the same style.
        self.ui.text = style;
        self.ui.text_size = (size * self.cam.ppc() / self.ppp()) as f32;
        self.edit.text = Some(TextEdit {
            at: tl,
            rot: geom.rot,
            size,
            style,
            text,
            group: Some(g),
            table,
        });
        self.text_open_editor();
    }

    fn text_open_editor(&mut self) {
        let Some(t) = self.edit.text.as_ref() else {
            return;
        };
        let px = self.cam_to_px(t.at);
        let ppp = self.ppp();
        #[cfg(target_arch = "wasm32")]
        {
            let [r, g, b, _] = t.style.color.to_le_bytes();
            crate::web::text_request(format!(
                "{{\"x\":{:.1},\"y\":{:.1},\"size\":{:.1},\"color\":\"#{:02x}{:02x}{:02x}\",\"font\":{},\"single\":{},\"table\":{},\"text\":{}}}",
                px[0] / ppp,
                px[1] / ppp,
                t.size * self.cam.ppc() / ppp,
                r,
                g,
                b,
                crate::web::json_str(&crate::font::name_of(t.style.font)),
                crate::font::is_single_line(t.style.font),
                t.table,
                crate::web::json_str(&t.text)
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ui.text_edit = Some((
                pos2((px[0] / ppp) as f32, (px[1] / ppp) as f32),
                t.text.clone(),
            ));
        }
        self.redraw();
    }

    /// Finish typing: store the text (or remove an emptied one).
    pub(crate) fn text_commit(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(t), Some((_, s))) = (self.edit.text.as_mut(), self.ui.text_edit.as_ref()) {
            t.text = s.clone();
        }
        self.ui.text_edit = None;
        let Some(t) = self.edit.text.take() else {
            return;
        };
        let text = t.text.trim_end().to_string();
        let mut removed = Vec::new();
        let mut z = None;
        if let Some(g) = t.group {
            let ids = self.objs.groups[g as usize].strokes.clone();
            z = Some(self.z_range(&ids));
            removed = ids;
        }
        if text.trim().is_empty() {
            self.record_edit(removed, vec![]);
            return;
        }
        let seed = self.next_seed();
        let data = if t.table {
            let cells = objects::parse_tsv(&text);
            let b = objects::table_layout(&cells, &t.style, t.size).size();
            let half = [b[0] * 0.5, b[1] * 0.5];
            let d = rot2(half, t.rot);
            ObjData::Table {
                cells,
                style: t.style,
                geom: Geom {
                    center: [t.at[0] + d[0], t.at[1] + d[1]],
                    half,
                    rot: t.rot,
                    pts: vec![],
                },
                size: t.size,
                seed,
            }
        } else {
            let half = {
                let b = objects::text_box(&text, &t.style, t.size);
                [b[0] * 0.5, b[1] * 0.5]
            };
            let d = rot2(half, t.rot);
            ObjData::Text {
                text,
                style: t.style,
                geom: Geom {
                    center: [t.at[0] + d[0], t.at[1] + d[1]],
                    half,
                    rot: t.rot,
                    pts: vec![],
                },
                size: t.size,
                seed,
            }
        };
        let (g, ids) = self.add_group(&data, z);
        if t.group.is_some() {
            self.edit.selection = vec![ObjRef::Group(g)];
        }
        self.record_edit(removed, ids);
    }

    pub(crate) fn text_cancel(&mut self) {
        self.edit.text = None;
        self.ui.text_edit = None;
        self.redraw();
    }

    // ---- selection -------------------------------------------------------

    /// The object under screen point `p`, topmost first.
    /// The topmost text or table whose box contains screen point `p` (px),
    /// with a little slack, so a tap between letters still finds it.
    pub(crate) fn text_at(&self, p: [f64; 2]) -> Option<u32> {
        let q = self.px_to_cam(p);
        let slack = crate::PICK_PT * self.ppp() / self.cam.ppc();
        let mut best: Option<(f64, u32)> = None;
        for (g, grp) in self.objs.groups.iter().enumerate() {
            if !matches!(grp.data, ObjData::Text { .. } | ObjData::Table { .. }) {
                continue;
            }
            if !self.objs.alive(&self.scene, &ObjRef::Group(g as u32)) {
                continue;
            }
            let geom = to_cam(&grp.cell, &grp.data, &self.cam).geom().clone();
            let d = rot2([q[0] - geom.center[0], q[1] - geom.center[1]], -geom.rot);
            if d[0].abs() > geom.half[0] + slack || d[1].abs() > geom.half[1] + slack {
                continue;
            }
            // Topmost: the highest z among its strokes.
            let z = grp
                .strokes
                .iter()
                .map(|&id| self.scene.strokes[id as usize].z as f64)
                .fold(f64::MIN, f64::max);
            if best.is_none_or(|(bz, _)| z >= bz) {
                best = Some((z, g as u32));
            }
        }
        best.map(|(_, g)| g)
    }

    fn obj_at(&self, p: [f64; 2]) -> Option<ObjRef> {
        let ids = hit::strokes_near(
            &self.scene,
            &self.draw,
            [p[0] as f32, p[1] as f32],
            (crate::PICK_PT * self.ppp()) as f32,
        );
        let top = ids.into_iter().max_by(|&a, &b| {
            let (za, zb) = (
                self.scene.strokes[a as usize].z,
                self.scene.strokes[b as usize].z,
            );
            za.total_cmp(&zb).then(a.cmp(&b))
        })?;
        Some(self.objs.obj_of(top))
    }

    fn sel_alive(&mut self) {
        let objs = &self.objs;
        let scene = &self.scene;
        self.edit.selection.retain(|r| objs.alive(scene, r));
    }

    /// The selection's box on screen (px): corners clockwise from the
    /// top-left, and its rotation.
    pub(crate) fn sel_box(&self) -> Option<([[f64; 2]; 4], f64)> {
        let sel = &self.edit.selection;
        if sel.is_empty() {
            return None;
        }
        let rot = match sel.as_slice() {
            [ObjRef::Group(g)] => {
                let d = self.objs.groups[*g as usize].data.geom();
                if d.pts.is_empty() {
                    d.rot
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut pad: f64 = 0.0;
        for r in sel {
            for &id in self.objs.strokes(r) {
                pad = pad.max(self.stroke_width_px(id) * 0.5);
                for p in self.stroke_px(id) {
                    let q = rot2(p, -rot);
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        let pad = pad + 4.0 * self.ppp();
        let c = [
            [lo[0] - pad, lo[1] - pad],
            [hi[0] + pad, lo[1] - pad],
            [hi[0] + pad, hi[1] + pad],
            [lo[0] - pad, hi[1] + pad],
        ];
        Some((c.map(|q| rot2(q, rot)), rot))
    }

    pub(crate) fn select_begin(&mut self, p: [f64; 2]) {
        if self.edit.crop.is_some() && self.crop_begin(p) {
            return;
        }
        self.sel_alive();
        let ppp = self.ppp();
        // Double tap on a text: edit it.
        let double = self.edit.last_tap.is_some_and(|(t, q)| {
            t.elapsed() < Duration::from_millis(400) && crate::dist(q, p) < 12.0 * ppp
        });
        self.edit.last_tap = Some((Instant::now(), p));
        if double {
            if let Some(g) = self.text_at(p) {
                self.text_edit_group(g);
                return;
            }
        }
        // Handles of the current selection.
        if let Some((corners, rot)) = self.sel_box() {
            let pts: [Pos2; 4] = corners.map(|q| pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32));
            let (_, knob) = ui::rotate_knob(&pts, self.ui.touch_ui);
            let knob = [knob.x as f64 * ppp, knob.y as f64 * ppp];
            let reach = HANDLE_PT * ppp * if self.ui.touch_ui { 1.5 } else { 1.0 };
            let kind = if crate::dist(knob, p) < reach {
                Some(DragKind::Rotate)
            } else if let Some(i) = corners.iter().position(|&q| crate::dist(q, p) < reach) {
                Some(DragKind::Scale(i))
            } else if inside_quad(p, &corners) && !self.mods.shift_key() {
                Some(DragKind::Move)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.drag_start(kind, p, corners, rot);
                return;
            }
        }
        if self.ui.tool == Tool::Lasso {
            if !self.mods.shift_key() {
                self.edit.selection.clear();
            }
            self.edit.lasso = Some(vec![p]);
            self.redraw();
            return;
        }
        match self.obj_at(p) {
            Some(o) if self.mods.shift_key() || self.mods.control_key() => {
                if let Some(i) = self.edit.selection.iter().position(|r| *r == o) {
                    self.edit.selection.remove(i);
                } else {
                    self.edit.selection.push(o);
                }
                self.gesture = Gesture::None;
            }
            Some(o) => {
                self.edit.selection = vec![o];
                if let Some((corners, rot)) = self.sel_box() {
                    self.drag_start(DragKind::Move, p, corners, rot);
                }
            }
            None => {
                if !self.mods.shift_key() {
                    self.edit.selection.clear();
                }
                self.edit.marquee = Some((p, p));
            }
        }
        self.redraw();
    }

    fn drag_start(&mut self, kind: DragKind, p: [f64; 2], box0: [[f64; 2]; 4], rot0: f64) {
        let mut orig = Vec::new();
        for r in &self.edit.selection {
            for &id in self.objs.strokes(r) {
                orig.push((id, self.scene.stroke_points(id).to_vec()));
            }
        }
        let att = self.attached_to(&self.edit.selection);
        let hidden = self.hide_attached(&att);
        self.edit.sel_drag = Some(SelDrag {
            att,
            hidden,
            kind,
            start: p,
            cur: p,
            box0,
            rot0,
            orig,
        });
    }

    /// The edit a drag describes so far (screen px).
    fn drag_op(&self, d: &SelDrag) -> Option<Op> {
        let (s, c) = (d.start, d.cur);
        let center = [
            (d.box0[0][0] + d.box0[2][0]) * 0.5,
            (d.box0[0][1] + d.box0[2][1]) * 0.5,
        ];
        match d.kind {
            DragKind::Move => {
                let v = [c[0] - s[0], c[1] - s[1]];
                (v[0] != 0.0 || v[1] != 0.0).then_some(Op::Move(v))
            }
            DragKind::Rotate => {
                let a0 = (s[1] - center[1]).atan2(s[0] - center[0]);
                let a1 = (c[1] - center[1]).atan2(c[0] - center[0]);
                let mut angle = a1 - a0;
                if self.mods.shift_key() {
                    let step = std::f64::consts::PI / 12.0;
                    angle = ((d.rot0 + angle) / step).round() * step - d.rot0;
                }
                (angle != 0.0).then_some(Op::Rotate {
                    pivot: center,
                    angle,
                })
            }
            DragKind::Scale(i) => {
                let pivot = if self.mods.alt_key() {
                    center
                } else {
                    d.box0[(i + 2) % 4]
                };
                let u0 = rot2([s[0] - pivot[0], s[1] - pivot[1]], -d.rot0);
                let u1 = rot2([c[0] - pivot[0], c[1] - pivot[1]], -d.rot0);
                let k = |a: f64, b: f64| if a.abs() < 1e-6 { 1.0 } else { b / a };
                let (mut sx, mut sy) = (k(u0[0], u1[0]), k(u0[1], u1[1]));
                // Keep proportions with Shift, and always for text, tables
                // and pictures.
                let text = matches!(self.edit.selection.as_slice(), [ObjRef::Group(g)]
                    if !matches!(self.objs.groups[*g as usize].data, ObjData::Shape { .. }));
                if self.mods.shift_key() || text {
                    let m = sx.abs().max(sy.abs());
                    sx = m * sx.signum();
                    sy = m * sy.signum();
                }
                let (sx, sy) = (sx.clamp(-1e4, 1e4), sy.clamp(-1e4, 1e4));
                (sx != 1.0 || sy != 1.0).then_some(Op::Scale {
                    pivot,
                    sx: if sx.abs() < 1e-3 { 1e-3 } else { sx },
                    sy: if sy.abs() < 1e-3 { 1e-3 } else { sy },
                    rot: d.rot0,
                })
            }
        }
    }

    /// Show the drag live: move the selected strokes' points in place.
    fn drag_preview(&mut self) {
        let Some(d) = self.edit.sel_drag.as_ref() else {
            return;
        };
        let op = self.drag_op(d).map(|o| self.op_to_cam(&o));
        let mut ids = Vec::with_capacity(d.orig.len());
        let mut writes = Vec::with_capacity(d.orig.len());
        for (id, pts) in &d.orig {
            let cell = self.scene.stroke_cell(*id);
            let (o, side) = objects::frame(cell, &self.cam);
            let new: Vec<Point> = pts
                .iter()
                .map(|p| match &op {
                    Some(op) => {
                        let q = op.apply_local([p[0] as f64, p[1] as f64], o, side);
                        [q[0] as f32, q[1] as f32, p[2], p[3]]
                    }
                    None => *p,
                })
                .collect();
            ids.push(*id);
            writes.push((*id, new));
        }
        for (id, pts) in writes {
            let s = &self.scene.strokes[id as usize];
            let a = s.start as usize;
            self.scene.points[a..a + pts.len()].copy_from_slice(&pts);
        }
        if let Some(g) = self.gpu.as_mut() {
            g.update_points(&self.scene, &ids);
        }
        self.redraw();
    }

    pub(crate) fn select_move(&mut self, p: [f64; 2]) {
        if self.edit.crop.is_some() {
            return self.crop_move(p);
        }
        if let Some(d) = self.edit.sel_drag.as_mut() {
            d.cur = p;
            self.drag_preview();
        } else if let Some(m) = self.edit.marquee.as_mut() {
            m.1 = p;
            self.redraw();
        } else if let Some(l) = self.edit.lasso.as_mut() {
            if l.last().is_none_or(|&q| crate::dist(q, p) > 2.0) {
                l.push(p);
                self.redraw();
            }
        }
    }

    pub(crate) fn select_end(&mut self, cancel: bool) {
        if self.edit.crop.is_some() {
            return self.crop_end();
        }
        if let Some(d) = self.edit.sel_drag.take() {
            self.unhide(&d.hidden);
            let op = if cancel { None } else { self.drag_op(&d) };
            // Put the original points back; the edit makes new strokes.
            let ids: Vec<u32> = d.orig.iter().map(|o| o.0).collect();
            for (id, pts) in &d.orig {
                let a = self.scene.strokes[*id as usize].start as usize;
                self.scene.points[a..a + pts.len()].copy_from_slice(pts);
            }
            if let Some(g) = self.gpu.as_mut() {
                g.update_points(&self.scene, &ids);
            }
            if let Some(op) = op {
                let op = self.op_to_cam(&op);
                self.apply_op_with(&op, d.att);
            }
        }
        if let Some((a, b)) = self.edit.marquee.take() {
            if !cancel {
                self.marquee_select(a, b);
            }
        }
        if let Some(poly) = self.edit.lasso.take() {
            if !cancel {
                self.lasso_select(&poly);
            }
        }
        self.redraw();
    }

    fn marquee_select(&mut self, a: [f64; 2], b: [f64; 2]) {
        let (lo, hi) = (
            [a[0].min(b[0]), a[1].min(b[1])],
            [a[0].max(b[0]), a[1].max(b[1])],
        );
        if hi[0] - lo[0] < 3.0 && hi[1] - lo[1] < 3.0 {
            return;
        }
        let mut seen: Vec<(ObjRef, bool)> = Vec::new();
        for inst in &self.draw.strokes {
            let id = inst.stroke;
            if self.scene.strokes[id as usize].deleted {
                continue;
            }
            let r = self.objs.obj_of(id);
            let inside = self.scene.stroke_points(id).iter().all(|p| {
                let x = inst.ox as f64 + p[0] as f64 * inst.scale as f64;
                let y = inst.oy as f64 + p[1] as f64 * inst.scale as f64;
                x >= lo[0] && x <= hi[0] && y >= lo[1] && y <= hi[1]
            });
            match seen.iter_mut().find(|s| s.0 == r) {
                Some(s) => s.1 &= inside,
                None => seen.push((r, inside)),
            }
        }
        for (r, inside) in seen {
            if inside && !self.edit.selection.contains(&r) {
                self.edit.selection.push(r);
            }
        }
    }

    /// Select what is mostly inside the loop `poly` (px): an object counts
    /// when at least `LASSO_SHARE` of its points are inside, so a loose loop
    /// still catches strokes that poke out a little. A tap selects what is
    /// under it.
    fn lasso_select(&mut self, poly: &[[f64; 2]]) {
        let len: f64 = poly.windows(2).map(|w| crate::dist(w[0], w[1])).sum();
        if poly.len() < 3 || len < 12.0 * self.ppp() {
            if let Some(o) = self.obj_at(poly[0]) {
                if !self.edit.selection.contains(&o) {
                    self.edit.selection.push(o);
                }
            }
            return;
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for q in poly {
            lo = [lo[0].min(q[0]), lo[1].min(q[1])];
            hi = [hi[0].max(q[0]), hi[1].max(q[1])];
        }
        // (object, points inside, points)
        let mut seen: Vec<(ObjRef, usize, usize)> = Vec::new();
        for inst in &self.draw.strokes {
            let id = inst.stroke;
            if self.scene.strokes[id as usize].deleted {
                continue;
            }
            let r = self.objs.obj_of(id);
            let pts = self.scene.stroke_points(id);
            let inside = pts
                .iter()
                .filter(|p| {
                    let q = [
                        inst.ox as f64 + p[0] as f64 * inst.scale as f64,
                        inst.oy as f64 + p[1] as f64 * inst.scale as f64,
                    ];
                    q[0] >= lo[0]
                        && q[0] <= hi[0]
                        && q[1] >= lo[1]
                        && q[1] <= hi[1]
                        && in_poly(q, poly)
                })
                .count();
            match seen.iter_mut().find(|s| s.0 == r) {
                Some(s) => {
                    s.1 += inside;
                    s.2 += pts.len();
                }
                None => seen.push((r, inside, pts.len())),
            }
        }
        for (r, inside, n) in seen {
            if n > 0 && inside as f64 >= LASSO_SHARE * n as f64 && !self.edit.selection.contains(&r)
            {
                self.edit.selection.push(r);
            }
        }
    }

    // ---- edits on the selection --------------------------------------------

    /// Replace each selected object by an edited copy. `edit` returns the new
    /// strokes for one object (adding them to the scene); the old ones are
    /// deleted, all in one undo step.
    pub(crate) fn replace_selection(
        &mut self,
        mut edit: impl FnMut(&mut App, ObjRef) -> Option<ObjRef>,
    ) {
        self.sel_alive();
        let sel = self.edit.selection.clone();
        let mut removed = Vec::new();
        let mut added = Vec::new();
        let mut new_sel = Vec::new();
        for r in sel {
            let old: Vec<u32> = self.objs.strokes(&r).to_vec();
            if let Some(n) = edit(self, r) {
                added.extend_from_slice(self.objs.strokes(&n));
                removed.extend(old);
                new_sel.push(n);
            } else {
                new_sel.push(r);
            }
        }
        self.edit.selection = new_sel;
        self.record_edit(removed, added);
    }

    /// A copy of a freehand stroke with its points edited, at draw order `z`.
    fn ink_copy(
        &mut self,
        id: u32,
        op: Option<&Op>,
        style: Option<Style>,
        z: Option<f64>,
    ) -> ObjRef {
        let cell = self.scene.stroke_cell(id).clone();
        let (o, side) = objects::frame(&cell, &self.cam);
        let pts = self.scene.stroke_points(id).to_vec();
        let mut st = style.unwrap_or_else(|| self.scene.stroke_style(id));
        let k = op.map_or(1.0, |o| o.scale_factor());
        let cam_pts: Vec<[f64; 2]> = pts
            .iter()
            .map(|p| {
                let q = [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side];
                op.map_or(q, |op| op.apply(q))
            })
            .collect();
        let width_cam = st.width as f64 * side * k;
        let (anchor, local, nside) =
            Scene::anchor_for_min(&self.cam.cell, &cam_pts, width_cam.max(1e-12));
        let new_pts: Vec<Point> = local
            .iter()
            .zip(&pts)
            .map(|(l, p)| [l[0], l[1], p[2], 0.0])
            .collect();
        st.width = (width_cam / nside) as f32;
        let z = z.unwrap_or(self.scene.strokes[id as usize].z);
        ObjRef::Ink(
            self.scene
                .add_stroke_at(&anchor, &new_pts, st, uid::new(), z),
        )
    }

    /// A copy of a group with its data edited (camera units), at draw orders `z`.
    fn group_copy(
        &mut self,
        g: u32,
        f: impl FnOnce(ObjData) -> ObjData,
        z: Option<(f64, f64)>,
    ) -> ObjRef {
        let grp = &self.objs.groups[g as usize];
        let data = f(to_cam(&grp.cell, &grp.data, &self.cam));
        let z = z.or_else(|| Some(self.z_range(&self.objs.groups[g as usize].strokes.clone())));
        let (ng, _) = self.add_group(&data, z);
        ObjRef::Group(ng)
    }

    pub(crate) fn apply_op(&mut self, op: &Op) {
        let att = self.attached_to(&self.edit.selection);
        self.apply_op_with(op, att);
    }

    /// Apply `op` to the selection; in diagram mode the connectors in `att`
    /// follow it, in the same undo step.
    fn apply_op_with(&mut self, op: &Op, att: Vec<(u32, ObjData, [bool; 2])>) {
        let op = *op;
        self.sel_alive();
        let n = self.edit.selection.len();
        self.edit
            .selection
            .extend(att.iter().map(|(g, _, _)| ObjRef::Group(*g)));
        let mut k: usize = 0;
        self.replace_selection(|app, r| {
            let i = k;
            k += 1;
            if let Some((g, d, ends)) = i.checked_sub(n).and_then(|j| att.get(j)) {
                let nd = App::follow(d, *ends, &op);
                return Some(app.group_copy(*g, |_| nd, None));
            }
            Some(match r {
                ObjRef::Ink(id) => app.ink_copy(id, Some(&op), None, None),
                ObjRef::Group(g) => app.group_copy(g, |d| d.edited(&op), None),
            })
        });
        self.edit.selection.truncate(n);
    }

    /// Strokes of the selection in draw order, with the objects they belong to.
    pub(crate) fn sel_sorted_pub(&self) -> Vec<ObjRef> {
        self.sel_sorted()
    }

    fn sel_sorted(&self) -> Vec<ObjRef> {
        let mut v = self.edit.selection.clone();
        v.sort_by(|a, b| {
            let za = self.z_range(self.objs.strokes(a)).0;
            let zb = self.z_range(self.objs.strokes(b)).0;
            za.total_cmp(&zb)
        });
        v
    }

    fn reorder(&mut self, front: bool) {
        self.sel_alive();
        let order = self.sel_sorted();
        let total: usize = order.iter().map(|r| self.objs.strokes(r).len()).sum();
        let mut z = if front {
            self.scene.z_top + 1.0
        } else {
            self.scene.z_bottom - total as f64 - 1.0
        };
        let mut removed = Vec::new();
        let mut added = Vec::new();
        let mut new_sel = Vec::new();
        for r in order {
            let old = self.objs.strokes(&r).to_vec();
            let n = old.len() as f64;
            let nr = match r {
                ObjRef::Ink(id) => self.ink_copy(id, None, None, Some(z)),
                ObjRef::Group(g) => self.group_copy(g, |d| d, Some((z, z + (n - 1.0).max(0.0)))),
            };
            z += n + 1.0;
            added.extend_from_slice(self.objs.strokes(&nr));
            removed.extend(old);
            new_sel.push(nr);
        }
        self.edit.selection = new_sel;
        self.record_edit(removed, added);
    }

    fn duplicate(&mut self, refs: &[ObjRef], offset: [f64; 2]) {
        let op = Op::Move(offset);
        let mut added = Vec::new();
        let mut new_sel = Vec::new();
        for r in refs.iter().copied() {
            let n = self.objs.strokes(&r).len() as f64;
            let z = self.scene.z_top + 1.0;
            let nr = match r {
                ObjRef::Ink(id) => self.ink_copy(id, Some(&op), None, Some(z)),
                ObjRef::Group(g) => self.group_copy(g, |d| d.edited(&op), Some((z, z + n))),
            };
            added.extend_from_slice(self.objs.strokes(&nr));
            new_sel.push(nr);
        }
        self.edit.selection = new_sel;
        self.record_edit(vec![], added);
    }

    fn delete_selection(&mut self) {
        self.sel_alive();
        let mut removed = Vec::new();
        for r in std::mem::take(&mut self.edit.selection) {
            removed.extend_from_slice(self.objs.strokes(&r));
        }
        self.record_edit(removed, vec![]);
    }

    fn flip(&mut self, horizontal: bool) {
        let Some((c, rot)) = self.sel_box() else {
            return;
        };
        let pivot = [(c[0][0] + c[2][0]) * 0.5, (c[0][1] + c[2][1]) * 0.5];
        let op = Op::Scale {
            pivot,
            sx: if horizontal { -1.0 } else { 1.0 },
            sy: if horizontal { 1.0 } else { -1.0 },
            rot,
        };
        let op = self.op_to_cam(&op);
        self.apply_op(&op);
    }

    /// Selection actions from the panel and the keyboard.
    pub(crate) fn sel_action(&mut self, a: Action) {
        let off = 16.0 * self.ppp() / self.cam.ppc();
        match a {
            Action::Duplicate => {
                let sel = self.edit.selection.clone();
                self.duplicate(&sel, [off, off]);
            }
            Action::Delete => self.delete_selection(),
            Action::ToFront => self.reorder(true),
            Action::ToBack => self.reorder(false),
            Action::FlipH => self.flip(true),
            Action::FlipV => self.flip(false),
            Action::EditText => {
                if let [ObjRef::Group(g)] = self.edit.selection.as_slice() {
                    let g = *g;
                    self.text_edit_group(g);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn copy_selection(&mut self) {
        self.sel_alive();
        self.edit.clipboard = self.edit.selection.clone();
    }

    /// Paste copies centred on the pointer.
    pub(crate) fn paste(&mut self) {
        let clip = self.edit.clipboard.clone();
        if clip.is_empty() {
            return;
        }
        // Centre of what was copied, in camera units.
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for r in &clip {
            for &id in self.objs.strokes(r) {
                for p in self.stroke_px(id) {
                    let q = self.px_to_cam(p);
                    for k in 0..2 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
            }
        }
        let at = self.px_to_cam(self.cursor);
        let off = [at[0] - (lo[0] + hi[0]) * 0.5, at[1] - (lo[1] + hi[1]) * 0.5];
        self.duplicate(&clip, off);
        if !self.ui.tool.selects() {
            self.ui.tool = Tool::Select;
        }
    }

    pub(crate) fn select_all_visible(&mut self) {
        let mut sel: Vec<ObjRef> = Vec::new();
        for inst in &self.draw.strokes {
            if self.scene.strokes[inst.stroke as usize].deleted {
                continue;
            }
            let r = self.objs.obj_of(inst.stroke);
            if !sel.contains(&r) {
                sel.push(r);
            }
        }
        self.edit.selection = sel;
        if !self.ui.tool.selects() {
            self.ui.tool = Tool::Select;
        }
        self.redraw();
    }

    pub(crate) fn nudge(&mut self, dx: f64, dy: f64) {
        if self.edit.selection.is_empty() {
            return;
        }
        let k = self.ppp() / self.cam.ppc();
        self.apply_op(&Op::Move([dx * k, dy * k]));
    }

    // ---- pasting and inserting ---------------------------------------------

    /// Screen point (px) for something pasted: `at`, or the middle of the screen.
    pub(crate) fn drop_point(&self, at: Option<[f64; 2]>) -> [f64; 2] {
        let s = self.size();
        at.unwrap_or([s[0] * 0.5, s[1] * 0.5])
    }

    /// Add a new object centred on screen point `at` (px) and select it.
    fn insert(&mut self, data: ObjData) {
        let (g, ids) = self.add_group(&data, None);
        self.record_edit(vec![], ids);
        self.edit.selection = vec![ObjRef::Group(g)];
        if !self.ui.tool.selects() {
            self.ui.tool = Tool::Select;
        }
    }

    /// Put a picture on the canvas at its own size (at most 60% of the screen).
    pub(crate) fn insert_image(&mut self, asset: crate::images::Asset, at: Option<[f64; 2]>) {
        let at = self.drop_point(at);
        let ppp = self.ppp();
        let s = self.size();
        let (mut w, mut h) = (asset.w as f64 * ppp, asset.h as f64 * ppp);
        let k = (s[0] * 0.6 / w).min(s[1] * 0.6 / h).min(1.0);
        w *= k;
        h *= k;
        let id = crate::images::id_of(&asset.bytes);
        self.objs.images.entry(id).or_insert(asset);
        let ppc = self.cam.ppc();
        self.insert(ObjData::Image {
            id,
            geom: Geom {
                center: self.px_to_cam(at),
                half: [w * 0.5 / ppc, h * 0.5 / ppc],
                rot: 0.0,
                pts: vec![],
            },
            opacity: 255,
            crop: objects::FULL_CROP,
        });
        self.say("Picture added — drag to move, corners to resize");
    }

    /// Pasted text: a table if it looks like one (spreadsheet cells or a
    /// Markdown table), else a text, in the text tool's style.
    pub(crate) fn paste_text(&mut self, text: &str, at: Option<[f64; 2]>) {
        let at = self.px_to_cam(self.drop_point(at));
        let size = self.ui.text_size as f64 * self.ppp() / self.cam.ppc();
        let style = self.ui.text;
        let seed = self.next_seed();
        if let Some(cells) = objects::table_from_text(text) {
            let b = objects::table_layout(&cells, &style, size).size();
            self.insert(ObjData::Table {
                cells,
                style,
                geom: Geom {
                    center: at,
                    half: [b[0] * 0.5, b[1] * 0.5],
                    rot: 0.0,
                    pts: vec![],
                },
                size,
                seed,
            });
            return;
        }
        let text: String = text.trim().chars().take(5000).collect();
        if text.is_empty() {
            return;
        }
        let b = objects::text_box(&text, &style, size);
        self.insert(ObjData::Text {
            text,
            style,
            geom: Geom {
                center: at,
                half: [b[0] * 0.5, b[1] * 0.5],
                rot: 0.0,
                pts: vec![],
            },
            size,
            seed,
        });
    }

    // ---- panel <-> selection -----------------------------------------------

    /// Describe the selection's style for the panel; when the panel changed
    /// it (and the pointer is up), restyle the selection.
    pub(crate) fn sync_sel_panel(&mut self, pointer_down: bool) {
        self.sel_alive();
        if !self.ui.tool.selects() {
            self.crop_cancel();
            self.edit.selection.clear();
        }
        self.ui.cropping = self.edit.crop.is_some();
        let sel = self.edit.selection.clone();
        if sel != self.edit.sel_seen_for {
            let s = self.describe(&sel);
            self.ui.sel = s;
            self.edit.sel_seen = Some(s);
            self.edit.sel_seen_for = sel;
            return;
        }
        let Some(seen) = self.edit.sel_seen else {
            return;
        };
        if self.ui.sel == seen || pointer_down {
            return;
        }
        let now = self.ui.sel;
        self.restyle(&seen, &now);
        let after = self.edit.selection.clone();
        let s = self.describe(&after);
        self.ui.sel = s;
        self.edit.sel_seen = Some(s);
        self.edit.sel_seen_for = after;
    }

    fn describe(&self, sel: &[ObjRef]) -> SelStyle {
        let mut s = SelStyle {
            count: sel.len(),
            ..Default::default()
        };
        let mut kinds = sel.iter().map(|r| match r {
            ObjRef::Ink(_) => SelKind::Ink,
            ObjRef::Group(g) => match self.objs.groups[*g as usize].data {
                ObjData::Shape { .. } => SelKind::Shapes,
                ObjData::Text { .. } | ObjData::Table { .. } => SelKind::Text,
                ObjData::Image { .. } => SelKind::Images,
            },
        });
        let first = kinds.next().unwrap_or(SelKind::None);
        s.kind = if kinds.all(|k| k == first) {
            first
        } else {
            SelKind::Mixed
        };
        match sel.first() {
            Some(ObjRef::Ink(id)) => {
                let st = &self.scene.strokes[*id as usize];
                let [r, g, b, a] = st.color.to_le_bytes();
                s.ink = Some(ui::InkSettings {
                    color: egui::Color32::from_rgb(r, g, b),
                    width: (self.stroke_width_px(*id) / self.ppp()) as f32,
                    pressure: st.brush == ogpaper_core::Brush::Pen,
                    dash: st.dash,
                    opacity: a,
                    advanced: false,
                    params: Default::default(),
                });
            }
            Some(ObjRef::Group(g)) => match &self.objs.groups[*g as usize].data {
                ObjData::Shape { style, .. } => s.shape = *style,
                ObjData::Text { style, .. } | ObjData::Table { style, .. } => s.text = *style,
                ObjData::Image { opacity, .. } => s.opacity = *opacity,
            },
            None => {}
        }
        s
    }

    /// Apply the fields the panel changed (`before` -> `after`) to every
    /// selected object, keeping their other settings.
    fn restyle(&mut self, before: &SelStyle, after: &SelStyle) {
        let (b, a) = (*before, *after);
        let ppc_px = self.ppp() / self.cam.ppc();
        self.replace_selection(|app, r| match r {
            ObjRef::Ink(id) => {
                let (Some(bi), Some(ai)) = (b.ink, a.ink) else {
                    return None;
                };
                let mut st = app.scene.stroke_style(id);
                let [r0, g0, b0, a0] = st.color.to_le_bytes();
                let (mut rgb, mut alpha) = ([r0, g0, b0], a0);
                if ai.color != bi.color {
                    let [r, g, b, _] = ai.color.to_array();
                    rgb = [r, g, b];
                }
                if ai.opacity != bi.opacity {
                    alpha = ai.opacity;
                }
                st.color = u32::from_le_bytes([rgb[0], rgb[1], rgb[2], alpha]);
                if ai.dash != bi.dash {
                    st.dash = ai.dash;
                }
                if ai.width != bi.width {
                    let side = app.scene.stroke_cell(id).side_in(&app.cam.cell);
                    st.width = (ai.width as f64 * ppc_px / side) as f32;
                }
                Some(app.ink_copy(id, None, Some(st), None))
            }
            ObjRef::Group(g) => {
                let data = app.objs.groups[g as usize].data.clone();
                let changed = match data {
                    ObjData::Shape {
                        mut style,
                        geom,
                        width,
                        seed,
                    } => {
                        merge_shape(&mut style, &b.shape, &a.shape);
                        ObjData::Shape {
                            style,
                            geom,
                            width,
                            seed,
                        }
                    }
                    ObjData::Text {
                        text,
                        mut style,
                        geom,
                        size,
                        seed,
                    } => {
                        merge_text(&mut style, &b.text, &a.text);
                        // The box follows the new font.
                        let bx = objects::text_box(&text, &style, size);
                        let half = [bx[0] * 0.5, bx[1] * 0.5];
                        let tl = rot2([-geom.half[0], -geom.half[1]], geom.rot);
                        let tl = [geom.center[0] + tl[0], geom.center[1] + tl[1]];
                        let d = rot2(half, geom.rot);
                        ObjData::Text {
                            text,
                            style,
                            geom: Geom {
                                center: [tl[0] + d[0], tl[1] + d[1]],
                                half,
                                ..geom
                            },
                            size,
                            seed,
                        }
                    }
                    ObjData::Table {
                        cells,
                        mut style,
                        geom,
                        size,
                        seed,
                    } => {
                        merge_text(&mut style, &b.text, &a.text);
                        let bx = objects::table_layout(&cells, &style, size).size();
                        let half = [bx[0] * 0.5, bx[1] * 0.5];
                        let tl = rot2([-geom.half[0], -geom.half[1]], geom.rot);
                        let tl = [geom.center[0] + tl[0], geom.center[1] + tl[1]];
                        let d = rot2(half, geom.rot);
                        ObjData::Table {
                            cells,
                            style,
                            geom: Geom {
                                center: [tl[0] + d[0], tl[1] + d[1]],
                                half,
                                ..geom
                            },
                            size,
                            seed,
                        }
                    }
                    ObjData::Image {
                        id,
                        geom,
                        opacity,
                        crop,
                    } => ObjData::Image {
                        id,
                        geom,
                        crop,
                        opacity: if a.opacity != b.opacity {
                            a.opacity
                        } else {
                            opacity
                        },
                    },
                };
                let grp = &app.objs.groups[g as usize];
                if changed == grp.data {
                    return None;
                }
                let cell = grp.cell.clone();
                let z = app.z_range(&grp.strokes.clone());
                let ids = objects::emit(&mut app.scene, &cell, &changed, z);
                let ng = app.objs.add(Group {
                    cell,
                    data: changed,
                    strokes: ids,
                });
                Some(ObjRef::Group(ng))
            }
        });
    }

    // ---- overlay -------------------------------------------------------------

    /// What the UI draws over the canvas this frame.
    pub(crate) fn build_overlay(&mut self) {
        let ppp = self.ppp();
        let to_pt = |p: [f64; 2]| pos2((p[0] / ppp) as f32, (p[1] / ppp) as f32);
        let mut ov = ui::Overlay::default();
        if let Some((a, b)) = self.edit.shape_drag {
            let (a, b) = self.snap_line(a, b);
            let g = self.shape_geom_px(a, b);
            let w = self.ui.shape_width as f64 * ppp;
            for pc in shapes::pieces(&self.ui.shape, &g, w, 7) {
                let [r, gg, bb, al] = pc.color.to_le_bytes();
                let col = egui::Color32::from_rgba_unmultiplied(r, gg, bb, al);
                let pts = pc.pts.iter().map(|&p| to_pt(p)).collect();
                let fill = pc.brush == ogpaper_core::Brush::Fill;
                ov.lines.push((pts, (pc.width / ppp) as f32, col, fill));
            }
        }
        if let Some((a, b)) = self.edit.marquee {
            ov.marquee = Some(egui::Rect::from_two_pos(to_pt(a), to_pt(b)));
        }
        if let Some(l) = &self.edit.lasso {
            ov.lasso = Some(l.iter().map(|&q| to_pt(q)).collect());
        }
        if let Some(d) = self.edit.sel_drag.as_ref().filter(|d| !d.att.is_empty()) {
            let op = self.drag_op(d).map(|o| self.op_to_cam(&o));
            self.follow_preview(&d.att, op.as_ref(), &mut ov);
        }
        if self.edit.crop.is_some() {
            self.crop_overlay(&mut ov);
        } else if self.ui.tool.selects() && self.edit.text.is_none() {
            let boxed = match self.edit.sel_drag.as_ref() {
                Some(d) => {
                    let op = self.drag_op(d);
                    Some(d.box0.map(|q| op.map_or(q, |o| o.apply(q))))
                }
                None => self.sel_box().map(|b| b.0),
            };
            if let Some(c) = boxed {
                let handles = self.edit.sel_drag.is_none();
                ov.sel_box = Some((c.map(to_pt), handles));
            }
        }
        // A searched-for text, outlined for a moment.
        if let Some(c) = self.flash_corners() {
            let mut pts: Vec<Pos2> = c.iter().map(|&q| to_pt(q)).collect();
            pts.push(pts[0]);
            ov.lines
                .push((pts, 2.5, egui::Color32::from_rgb(200, 40, 90), false));
        }
        self.ui.overlay = ov;
    }
}

/// Copy the text settings that changed from `b` to `a` into `st`.
fn merge_text(st: &mut TextStyle, b: &TextStyle, a: &TextStyle) {
    if a.font != b.font {
        st.font = a.font;
    }
    if a.align != b.align {
        st.align = a.align;
    }
    if a.color != b.color {
        st.color = a.color;
    }
    if a.opacity != b.opacity {
        st.opacity = a.opacity;
    }
}

/// Copy the fields that changed from `b` to `a` into `st`.
fn merge_shape(st: &mut shapes::ShapeStyle, b: &shapes::ShapeStyle, a: &shapes::ShapeStyle) {
    if a.kind != b.kind && a.kind.is_linear() == st.kind.is_linear() {
        st.kind = a.kind;
    }
    macro_rules! take {
        ($($f:ident),*) => { $( if a.$f != b.$f { st.$f = a.$f; } )* };
    }
    take!(stroke, fill, fill_style, dash, sloppiness, round, sides, start, end, arrow, opacity);
}

#[cfg(test)]
mod tests {
    #[test]
    fn point_in_loop() {
        // A U shape: the notch is outside.
        let u = [
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, 3.0],
            [2.0, 3.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 3.0],
            [0.0, 3.0],
        ];
        assert!(super::in_poly([0.5, 2.0], &u));
        assert!(super::in_poly([2.5, 2.0], &u));
        assert!(!super::in_poly([1.5, 2.0], &u));
        assert!(!super::in_poly([4.0, 1.0], &u));
    }
}
