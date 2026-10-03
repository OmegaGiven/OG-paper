// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Import: another saved canvas brought into this one. It comes in at its
//! own place and scale, the view zooms out to show both canvases, and it can
//! be dragged to an offset before it is placed (one undo step).
//!
//! Until placed it stays a separate scene, drawn over the canvas as a
//! preview. Placing moves every stroke and object exactly: the offset is
//! kept in cells 2^-OFF_BITS the size of the camera cell the import started
//! in, so cells at that size or deeper move by a whole number of their own
//! cells (nothing deep blurs); coarser ones take the leftover fraction in
//! their own units, where it is tiny next to their size.

use std::collections::HashSet;

use egui::{pos2, Color32, Pos2};
use num_bigint::BigInt;
use ogpaper_core::{query, Brush, Camera, CellAddr, DrawList, Point, Scene, Style};

use crate::objects::{self, Group, Objects};
use crate::App;

/// How fine the offset is: 2^-OFF_BITS of the starting camera cell.
const OFF_BITS: i64 = 32;
/// Most points drawn in the preview (more just skips strokes).
const PREVIEW_PTS: usize = 400_000;

pub struct Import {
    pub scene: Scene,
    pub objs: Objects,
    /// The camera cell when the import started; the offset is in its units.
    base: CellAddr,
    pub off: [f64; 2],
    /// Its extent (lowest, highest corner) in `base` units, before the offset.
    bounds: Option<([f64; 2], [f64; 2])>,
    draw: DrawList,
}

/// The extent of a scene's live ink in `reference` cell units.
fn bounds(scene: &Scene, reference: &CellAddr) -> Option<([f64; 2], [f64; 2])> {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (id, s) in scene.strokes.iter().enumerate() {
        if s.deleted {
            continue;
        }
        let cell = scene.stroke_cell(id as u32);
        let o = cell.origin_in(reference);
        let side = cell.side_in(reference);
        let r = s.width as f64 * side * 0.5;
        for p in scene.stroke_points(id as u32) {
            let q = [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side];
            lo = [lo[0].min(q[0] - r), lo[1].min(q[1] - r)];
            hi = [hi[0].max(q[0] + r), hi[1].max(q[1] + r)];
        }
    }
    (lo[0] <= hi[0] && lo.iter().chain(&hi).all(|v| v.is_finite())).then_some((lo, hi))
}

/// The coarsest level any live stroke of `scene` is stored at.
fn coarsest(scene: &Scene) -> Option<i64> {
    (0..scene.strokes.len() as u32)
        .filter(|&i| !scene.strokes[i as usize].deleted)
        .map(|i| scene.stroke_cell(i).level)
        .min()
}

impl App {
    /// Start placing `scene` (and its objects): zoom out to show it with this
    /// canvas; it follows drags until placed or cancelled.
    pub(crate) fn import_begin(&mut self, scene: Scene, objs: Objects, name: &str) {
        let Some(top) = coarsest(&scene) else {
            self.say(format!("{name} has nothing to import"));
            return;
        };
        let base = self.cam.cell.clone();
        let b = bounds(&scene, &base);
        self.import = Some(Import {
            scene,
            objs,
            base,
            off: [0.0; 2],
            bounds: b,
            draw: DrawList::default(),
        });
        // Frame both canvases, measured from a cell as coarse as the
        // coarsest ink so the numbers stay small.
        let mut reference = self.cam.cell.clone();
        let floor = coarsest(&self.scene)
            .map_or(top, |l| l.min(top))
            .min(reference.level);
        while reference.level > floor {
            reference = reference.parent();
        }
        let imp = self.import.as_ref().expect("import");
        let mut all = bounds(&imp.scene, &reference);
        if let Some((lo, hi)) = bounds(&self.scene, &reference) {
            all = Some(match all {
                Some((a, b)) => (
                    [a[0].min(lo[0]), a[1].min(lo[1])],
                    [b[0].max(hi[0]), b[1].max(hi[1])],
                ),
                None => (lo, hi),
            });
        }
        if let Some((lo, hi)) = all {
            let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
            let mut cam = Camera::new(reference, c, self.cam.base_px);
            let ppc = cam.ppc();
            let [w, h] = self.size();
            let bw = ((hi[0] - lo[0]) * ppc).max(1e-300);
            let bh = ((hi[1] - lo[1]) * ppc).max(1e-300);
            cam.zoom_at((0.75 * w / bw).min(0.65 * h / bh), [0.0, 0.0]);
            self.fly = Some(cam);
            self.fly_last = web_time::Instant::now();
        }
        self.ui.close_menus();
        self.say(format!("Drag to place {name}, then tap Place"));
        self.redraw();
    }

    /// A drag while placing: move the import with the pointer (px).
    pub(crate) fn import_drag(&mut self, from: [f64; 2], to: [f64; 2]) {
        let ppc = self.cam.ppc();
        let Some(imp) = self.import.as_mut() else {
            return;
        };
        // Camera-cell units, then the starting cell's.
        let k = self.cam.cell.side_in(&imp.base);
        imp.off[0] += (to[0] - from[0]) / ppc * k;
        imp.off[1] += (to[1] - from[1]) / ppc * k;
        self.redraw();
    }

    /// Place the import where it is (`place`), or drop it.
    pub(crate) fn import_finish(&mut self, place: bool) {
        let Some(imp) = self.import.take() else {
            return;
        };
        // The banner goes; it must not keep the keyboard (shortcuts).
        self.egui_ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        self.redraw();
        if !place {
            self.say("Import cancelled");
            return;
        }
        let lq = imp.base.level + OFF_BITS;
        let k = 2f64.powi(OFF_BITS as i32);
        let dx = BigInt::from((imp.off[0] * k).round() as i128);
        let dy = BigInt::from((imp.off[1] * k).round() as i128);
        for (id, a) in imp.objs.images {
            self.objs.images.entry(id).or_insert(a);
        }
        // Above everything here, in the order it had.
        let z0 = self.scene.z_top + 1.0 - imp.scene.z_bottom;
        let grouped: HashSet<u32> = imp
            .objs
            .groups
            .iter()
            .flat_map(|g| g.strokes.iter().copied())
            .collect();
        let mut added = Vec::new();
        let src = &imp.scene;
        for (i, s) in src.strokes.iter().enumerate() {
            let id = i as u32;
            if s.deleted || grouped.contains(&id) {
                continue;
            }
            let (cell, f) = src.stroke_cell(id).shifted(&dx, &dy, lq);
            let st = src.stroke_style(id);
            let pts = src.stroke_points(id);
            let (anchor, pts, width) = if f == [0.0, 0.0] {
                (cell, pts.to_vec(), st.width)
            } else {
                let moved: Vec<[f64; 2]> = pts
                    .iter()
                    .map(|p| [p[0] as f64 + f[0], p[1] as f64 + f[1]])
                    .collect();
                let (a, local, side) =
                    Scene::anchor_for_min(&cell, &moved, (st.width as f64).max(1e-12));
                let pts: Vec<Point> = local
                    .iter()
                    .zip(pts)
                    .map(|(q, p)| [q[0], q[1], p[2], 0.0])
                    .collect();
                (a, pts, (st.width as f64 / side) as f32)
            };
            let style = Style { width, ..st };
            let nid = self
                .scene
                .add_stroke_at(&anchor, &pts, style, crate::uid::new(), z0 + s.z);
            added.push(nid);
        }
        for g in &imp.objs.groups {
            let live: Vec<u32> = g
                .strokes
                .iter()
                .copied()
                .filter(|&s| !src.strokes[s as usize].deleted)
                .collect();
            if live.is_empty() {
                continue;
            }
            let (cell, f) = g.cell.shifted(&dx, &dy, lq);
            let data = g.data.map(|p| [p[0] + f[0], p[1] + f[1]], 1.0);
            let z = live.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &s| {
                let z = src.strokes[s as usize].z + z0;
                (lo.min(z), hi.max(z))
            });
            let ids = objects::emit(&mut self.scene, &cell, &data, z);
            self.objs.add(Group {
                cell,
                data,
                strokes: ids.clone(),
            });
            added.extend(ids);
        }
        let n = added.len();
        self.record_edit(vec![], added);
        self.say(format!("Imported {n} strokes (undo removes them)"));
    }

    /// The import drawn where it would land, over the canvas.
    pub(crate) fn import_overlay(&mut self, ov: &mut crate::ui::Overlay) {
        let ppp = self.ppp();
        let [w, h] = self.size();
        let Some(imp) = self.import.as_mut() else {
            return;
        };
        let pt = |x: f64, y: f64| pos2((x / ppp) as f32, (y / ppp) as f32);
        // Its outline, in the camera's units, then on screen.
        let k = imp.base.side_in(&self.cam.cell);
        let o = imp.base.origin_in(&self.cam.cell);
        let to_cam = |q: [f64; 2]| {
            [
                o[0] + (q[0] + imp.off[0]) * k,
                o[1] + (q[1] + imp.off[1]) * k,
            ]
        };
        let accent = Color32::from_rgb(200, 40, 90);
        if let Some((lo, hi)) = imp.bounds {
            let ppc = self.cam.ppc();
            let px = |q: [f64; 2]| {
                let c = to_cam(q);
                pt(
                    (c[0] - self.cam.off[0]) * ppc + w * 0.5,
                    (c[1] - self.cam.off[1]) * ppc + h * 0.5,
                )
            };
            let corners: Vec<Pos2> = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]], lo]
                .iter()
                .map(|&q| px(q))
                .collect();
            if corners.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                ov.lines.push((corners, 2.0, accent, false));
            }
        }
        // The ink, through a camera moved the other way.
        let shift = [imp.off[0] * k, imp.off[1] * k];
        if shift[0].abs() > 1e15 || shift[1].abs() > 1e15 {
            return; // far off screen at this zoom
        }
        let mut cam = self.cam.clone();
        cam.off = [cam.off[0] - shift[0], cam.off[1] - shift[1]];
        cam.normalize();
        query(&imp.scene, &cam, w, h, crate::VIEW, &mut imp.draw);
        let mut budget = PREVIEW_PTS;
        for i in &imp.draw.strokes {
            let s = &imp.scene.strokes[i.stroke as usize];
            let p = imp.scene.stroke_points(i.stroke);
            if p.len() > budget {
                break;
            }
            budget -= p.len();
            let (ox, oy, sc) = (i.ox as f64, i.oy as f64, i.scale as f64);
            let mut pts: Vec<Pos2> = p
                .iter()
                .map(|q| pt(ox + q[0] as f64 * sc, oy + q[1] as f64 * sc))
                .collect();
            let [r, g, b, a] = s.color.to_le_bytes();
            let fill = s.brush == Brush::Fill;
            if fill && s.color == 0 {
                // A picture's corners: its outline.
                if let Some(&f) = pts.first() {
                    pts.push(f);
                }
                ov.lines.push((pts, 1.5, Color32::from_gray(140), false));
                continue;
            }
            let col = Color32::from_rgba_unmultiplied(r, g, b, a);
            let width = ((s.width as f64 * sc) / ppp).max(0.6) as f32;
            ov.lines.push((pts, width, col, fill));
        }
        for d in &imp.draw.dots {
            let (x, y, r) = (d.x as f64, d.y as f64, (d.size as f64 * 0.5).max(1.0));
            ov.lines.push((
                vec![
                    pt(x - r, y - r),
                    pt(x + r, y - r),
                    pt(x + r, y + r),
                    pt(x - r, y + r),
                ],
                0.0,
                Color32::from_black_alpha((d.alpha * 200.0) as u8),
                true,
            ));
        }
    }
}
