// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Crop a picture in place. The picture keeps its whole file and a crop
//! window (fractions of the picture), so a crop can always be widened again.
//! While cropping, the whole picture shows with the cut-away parts dimmed;
//! drag the edges or corners, or drag inside to slide the window.

use egui::{pos2, Color32, Pos2};
use ogpaper_core::Point;

use crate::objects::{self, to_cam, Group, ObjData, ObjRef};
use crate::shapes::Geom;
use crate::App;

/// Smallest crop window, as a fraction of the picture.
const MIN_FRAC: f32 = 0.02;
const HANDLE_PT: f64 = 14.0;

/// Which edges a drag moves: left, top, right, bottom; all four = slide.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edges([bool; 4]);

pub struct CropEdit {
    group: u32,
    /// The stroke that places the picture, and its points before cropping.
    stroke: u32,
    orig: Vec<Point>,
    orig_crop: [f32; 4],
    pub crop: [f32; 4],
    /// Drag: edges, start (picture fractions), crop at the start.
    drag: Option<(Edges, [f64; 2], [f32; 4])>,
}

/// In the box's own frame, unrotated: the point at picture fraction (u, v)
/// of a box `g` that shows `crop`.
fn local(g: &Geom, crop: [f32; 4], u: f64, v: f64) -> [f64; 2] {
    let (c0, c1, c2, c3) = (
        crop[0] as f64,
        crop[1] as f64,
        crop[2] as f64,
        crop[3] as f64,
    );
    [
        -g.half[0] + (u - c0) / (c2 - c0).max(1e-9) * 2.0 * g.half[0],
        -g.half[1] + (v - c1) / (c3 - c1).max(1e-9) * 2.0 * g.half[1],
    ]
}

fn place(g: &Geom, l: [f64; 2]) -> [f64; 2] {
    let (s, c) = g.rot.sin_cos();
    [
        g.center[0] + l[0] * c - l[1] * s,
        g.center[1] + l[0] * s + l[1] * c,
    ]
}

/// The box of the whole picture, from a box showing `crop`.
pub fn full_geom(g: &Geom, crop: [f32; 4]) -> Geom {
    let c = crop.map(|v| v as f64);
    let (du, dv) = ((c[2] - c[0]).max(1e-6), (c[3] - c[1]).max(1e-6));
    Geom {
        center: place(g, local(g, crop, 0.5, 0.5)),
        half: [g.half[0] / du, g.half[1] / dv],
        rot: g.rot,
        pts: vec![],
    }
}

/// The box showing `crop` of the whole picture's box `full`.
pub fn cropped_geom(full: &Geom, crop: [f32; 4]) -> Geom {
    let mid = local(
        full,
        objects::FULL_CROP,
        (crop[0] as f64 + crop[2] as f64) * 0.5,
        (crop[1] as f64 + crop[3] as f64) * 0.5,
    );
    Geom {
        center: place(full, mid),
        half: [
            full.half[0] * (crop[2] as f64 - crop[0] as f64),
            full.half[1] * (crop[3] as f64 - crop[1] as f64),
        ],
        rot: full.rot,
        pts: vec![],
    }
}

impl App {
    /// The picture being cropped: its whole box in camera units.
    fn crop_full_cam(&self) -> Option<Geom> {
        let ce = self.edit.crop.as_ref()?;
        let grp = self.objs.groups.get(ce.group as usize)?;
        let d = to_cam(&grp.cell, &grp.data, &self.cam);
        Some(full_geom(d.geom(), ce.orig_crop))
    }

    /// Screen px of picture fraction (u, v).
    fn crop_px(&self, full: &Geom, u: f64, v: f64) -> [f64; 2] {
        self.cam_to_px(place(full, local(full, objects::FULL_CROP, u, v)))
    }

    /// Picture fraction under screen point `p`.
    fn crop_uv(&self, full: &Geom, p: [f64; 2]) -> [f64; 2] {
        let q = self.px_to_cam(p);
        let (s, c) = full.rot.sin_cos();
        let d = [q[0] - full.center[0], q[1] - full.center[1]];
        let l = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c];
        [
            (l[0] / full.half[0] + 1.0) * 0.5,
            (l[1] / full.half[1] + 1.0) * 0.5,
        ]
    }

    /// Start cropping the selected picture: show all of it.
    pub(crate) fn crop_start(&mut self) {
        let [ObjRef::Group(g)] = self.edit.selection.as_slice() else {
            return;
        };
        let g = *g;
        let grp = &self.objs.groups[g as usize];
        let ObjData::Image { geom, crop, .. } = &grp.data else {
            return;
        };
        let Some(&stroke) = grp.strokes.first() else {
            return;
        };
        let full = full_geom(geom, *crop);
        let corners = ObjData::Image {
            id: 0,
            geom: full,
            opacity: 0,
            crop: objects::FULL_CROP,
        }
        .extent();
        let orig = self.scene.stroke_points(stroke).to_vec();
        if orig.len() != 4 {
            return;
        }
        let a = self.scene.strokes[stroke as usize].start as usize;
        for (k, q) in corners.iter().enumerate() {
            self.scene.points[a + k][0] = q[0] as f32;
            self.scene.points[a + k][1] = q[1] as f32;
        }
        if let Some(e) = self.objs.image_of.get_mut(&stroke) {
            e.2 = objects::FULL_CROP;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.update_points(&self.scene, &[stroke]);
        }
        self.edit.crop = Some(CropEdit {
            group: g,
            stroke,
            orig,
            orig_crop: *crop,
            crop: *crop,
            drag: None,
        });
        self.say("Drag the edges or corners to crop; Enter or Done to finish");
        self.redraw();
    }

    /// Put the picture back as it was before cropping started.
    fn crop_restore(&mut self, ce: &CropEdit) {
        let a = self.scene.strokes[ce.stroke as usize].start as usize;
        self.scene.points[a..a + ce.orig.len()].copy_from_slice(&ce.orig);
        if let Some(e) = self.objs.image_of.get_mut(&ce.stroke) {
            e.2 = ce.orig_crop;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.update_points(&self.scene, &[ce.stroke]);
        }
    }

    pub(crate) fn crop_cancel(&mut self) {
        if let Some(ce) = self.edit.crop.take() {
            self.crop_restore(&ce);
            self.redraw();
        }
    }

    /// Finish: the picture with the new crop, as one undo step.
    pub(crate) fn crop_done(&mut self) {
        let Some(ce) = self.edit.crop.take() else {
            return;
        };
        self.crop_restore(&ce);
        if ce.crop == ce.orig_crop {
            return self.redraw();
        }
        let (g, crop) = (ce.group, ce.crop);
        self.edit.selection = vec![ObjRef::Group(g)];
        self.replace_selection(move |app, r| {
            let ObjRef::Group(gi) = r else { return None };
            let grp = &app.objs.groups[gi as usize];
            let ObjData::Image {
                id,
                geom,
                opacity,
                crop: old,
            } = &grp.data
            else {
                return None;
            };
            let full = full_geom(geom, *old);
            let data = ObjData::Image {
                id: *id,
                geom: cropped_geom(&full, crop),
                opacity: *opacity,
                crop,
            };
            let cell = grp.cell.clone();
            let z = app.z_range(&grp.strokes.clone());
            let ids = objects::emit(&mut app.scene, &cell, &data, z);
            Some(ObjRef::Group(app.objs.add(Group {
                cell,
                data,
                strokes: ids,
            })))
        });
        self.redraw();
    }

    /// A press while cropping: grab an edge, a corner or the window; a
    /// press off the picture finishes. True when the press was used.
    pub(crate) fn crop_begin(&mut self, p: [f64; 2]) -> bool {
        let Some(full) = self.crop_full_cam() else {
            return false;
        };
        let crop = self
            .edit
            .crop
            .as_ref()
            .map(|c| c.crop)
            .unwrap_or(objects::FULL_CROP);
        let reach = HANDLE_PT * self.ppp() * if self.ui.touch_ui { 1.5 } else { 1.0 };
        let [c0, c1, c2, c3] = crop.map(|v| v as f64);
        // Distance (px) from p to each edge's segment.
        let seg = |a: [f64; 2], b: [f64; 2]| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy).max(1e-9))
                .clamp(0.0, 1.0);
            crate::dist(p, [a[0] + t * dx, a[1] + t * dy])
        };
        let tl = self.crop_px(&full, c0, c1);
        let tr = self.crop_px(&full, c2, c1);
        let br = self.crop_px(&full, c2, c3);
        let bl = self.crop_px(&full, c0, c3);
        let near = [seg(tl, bl), seg(tl, tr), seg(tr, br), seg(bl, br)].map(|d| d < reach);
        let uv = self.crop_uv(&full, p);
        let edges = if near.iter().any(|&n| n) {
            Edges(near)
        } else if uv[0] > c0 && uv[0] < c2 && uv[1] > c1 && uv[1] < c3 {
            Edges([true; 4])
        } else if (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]) {
            // On the dimmed part: nothing to grab.
            return true;
        } else {
            self.crop_done();
            return true;
        };
        if let Some(ce) = self.edit.crop.as_mut() {
            ce.drag = Some((edges, uv, crop));
        }
        true
    }

    pub(crate) fn crop_move(&mut self, p: [f64; 2]) {
        let Some(full) = self.crop_full_cam() else {
            return;
        };
        let uv = self.crop_uv(&full, p);
        let Some(ce) = self.edit.crop.as_mut() else {
            return;
        };
        let Some((Edges(e), start, c)) = ce.drag else {
            return;
        };
        let d = [(uv[0] - start[0]) as f32, (uv[1] - start[1]) as f32];
        let mut n = c;
        if e == [true; 4] {
            // Slide the window, keeping it inside the picture.
            let dx = d[0].clamp(-c[0], 1.0 - c[2]);
            let dy = d[1].clamp(-c[1], 1.0 - c[3]);
            n = [c[0] + dx, c[1] + dy, c[2] + dx, c[3] + dy];
        } else {
            if e[0] {
                n[0] = (c[0] + d[0]).clamp(0.0, c[2] - MIN_FRAC);
            }
            if e[1] {
                n[1] = (c[1] + d[1]).clamp(0.0, c[3] - MIN_FRAC);
            }
            if e[2] {
                n[2] = (c[2] + d[0]).clamp(c[0] + MIN_FRAC, 1.0);
            }
            if e[3] {
                n[3] = (c[3] + d[1]).clamp(c[1] + MIN_FRAC, 1.0);
            }
        }
        ce.crop = n;
        self.redraw();
    }

    pub(crate) fn crop_end(&mut self) {
        if let Some(ce) = self.edit.crop.as_mut() {
            ce.drag = None;
        }
    }

    /// What the overlay draws while cropping: dimmed bands over the parts
    /// cut away, the window's frame and its handles (points).
    pub(crate) fn crop_overlay(&self, ov: &mut crate::ui::Overlay) {
        let Some(full) = self.crop_full_cam() else {
            return;
        };
        let Some(ce) = self.edit.crop.as_ref() else {
            return;
        };
        let ppp = self.ppp();
        let pt = |q: [f64; 2]| pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32);
        let at = |u: f64, v: f64| pt(self.crop_px(&full, u, v));
        let [c0, c1, c2, c3] = ce.crop.map(|v| v as f64);
        let quad = |a: f64, b: f64, c: f64, d: f64| -> Vec<Pos2> {
            vec![at(a, b), at(c, b), at(c, d), at(a, d)]
        };
        let dim = Color32::from_rgba_unmultiplied(245, 242, 235, 190);
        for (a, b, c, d) in [
            (0.0, 0.0, 1.0, c1),
            (0.0, c3, 1.0, 1.0),
            (0.0, c1, c0, c3),
            (c2, c1, 1.0, c3),
        ] {
            if c - a > 1e-6 && d - b > 1e-6 {
                ov.lines.push((quad(a, b, c, d), 0.0, dim, true));
            }
        }
        let blue = Color32::from_rgb(70, 110, 230);
        let mut frame = quad(c0, c1, c2, c3);
        frame.push(frame[0]);
        ov.lines.push((frame, 2.0, blue, false));
        // Thirds, to help framing.
        let grid = Color32::from_rgba_unmultiplied(255, 255, 255, 150);
        for k in [1.0 / 3.0, 2.0 / 3.0] {
            let x = c0 + (c2 - c0) * k;
            let y = c1 + (c3 - c1) * k;
            ov.lines
                .push((vec![at(x, c1), at(x, c3)], 1.0, grid, false));
            ov.lines
                .push((vec![at(c0, y), at(c2, y)], 1.0, grid, false));
        }
        // L-shaped corner handles and edge bars.
        let len = 0.12;
        let (w, h) = (c2 - c0, c3 - c1);
        let (lx, ly) = (w.min(h * 4.0) * len, h.min(w * 4.0) * len);
        for (x, y, sx, sy) in [
            (c0, c1, 1.0, 1.0),
            (c2, c1, -1.0, 1.0),
            (c2, c3, -1.0, -1.0),
            (c0, c3, 1.0, -1.0),
        ] {
            ov.lines.push((
                vec![at(x + sx * lx, y), at(x, y), at(x, y + sy * ly)],
                5.0,
                blue,
                false,
            ));
        }
        let (mx, my) = ((c0 + c2) * 0.5, (c1 + c3) * 0.5);
        ov.lines.push((
            vec![at(mx - lx * 0.5, c1), at(mx + lx * 0.5, c1)],
            5.0,
            blue,
            false,
        ));
        ov.lines.push((
            vec![at(mx - lx * 0.5, c3), at(mx + lx * 0.5, c3)],
            5.0,
            blue,
            false,
        ));
        ov.lines.push((
            vec![at(c0, my - ly * 0.5), at(c0, my + ly * 0.5)],
            5.0,
            blue,
            false,
        ));
        ov.lines.push((
            vec![at(c2, my - ly * 0.5), at(c2, my + ly * 0.5)],
            5.0,
            blue,
            false,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn crop_round_trips_through_the_full_box() {
        for rot in [0.0, 0.7] {
            for flip in [1.0, -1.0] {
                let g = Geom {
                    center: [3.0, -2.0],
                    half: [2.0 * flip, 1.0],
                    rot,
                    pts: vec![],
                };
                let crop = [0.1, 0.2, 0.6, 0.9];
                let full = full_geom(&g, crop);
                let back = cropped_geom(&full, crop);
                assert!(close(back.center, g.center), "{rot} {flip}");
                assert!(close(back.half, g.half));
                // The crop's corner is where the full box puts that fraction.
                let p = place(
                    &full,
                    local(&full, objects::FULL_CROP, crop[0] as f64, crop[1] as f64),
                );
                let q = place(&g, [-g.half[0], -g.half[1]]);
                assert!(close(p, q));
            }
        }
    }
}
