// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The pen (Shapes › Pen (curves)): tap to put down corner points, press
//! and drag to put down a smooth point and pull its curve out. Tap the
//! first point to close the shape; tap the last one again, press Enter or
//! Escape, or pick another tool to finish it open. The result is a shape
//! (`ShapeKind::Path`) styled like any other.

use crate::objects::ObjData;
use crate::shapes::{self, Geom, ShapeKind};
use crate::App;

/// How near (points) a tap must be to a point to close or finish on it.
const NEAR_PT: f64 = 10.0;

/// A point of the path being drawn (camera units): where it is, and its
/// curve's handle out of it (the handle in mirrors it).
#[derive(Clone, Copy, Debug)]
pub struct PenNode {
    pub at: [f64; 2],
    pub out: [f64; 2],
}

/// The path being drawn.
#[derive(Clone, Debug, Default)]
pub struct PenDraft {
    pub nodes: Vec<PenNode>,
    /// A press is pulling the last point's handle.
    pub pulling: bool,
}

fn mirror(at: [f64; 2], h: [f64; 2]) -> [f64; 2] {
    [2.0 * at[0] - h[0], 2.0 * at[1] - h[1]]
}

/// Path points (see `shapes::subpaths`) for `nodes`, closed or not.
pub fn path_pts(nodes: &[PenNode], closed: bool) -> Vec<[f64; 2]> {
    let Some(first) = nodes.first() else {
        return vec![];
    };
    let mut pts = vec![first.at];
    let n = nodes.len();
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (a, b) = (nodes[i], nodes[(i + 1) % n]);
        pts.extend([a.out, mirror(b.at, b.out), b.at]);
    }
    pts
}

impl App {
    pub(crate) fn pen_active(&self) -> bool {
        self.ui.tool == crate::ui::Tool::Shapes && self.ui.shape.kind == ShapeKind::Path
    }

    /// A press at `p` (screen px).
    pub(crate) fn pen_begin(&mut self, p: [f64; 2]) {
        let near = NEAR_PT * self.ppp();
        let close_to = |app: &App, q: [f64; 2]| {
            let s = app.cam_to_px(q);
            (s[0] - p[0]).hypot(s[1] - p[1]) < near
        };
        if let Some(d) = &self.edit.pen {
            let n = d.nodes.len();
            if n >= 2 && close_to(self, d.nodes[0].at) {
                self.pen_finish(true);
                return;
            }
            if n >= 2 && close_to(self, d.nodes[n - 1].at) {
                self.pen_finish(false);
                return;
            }
        }
        let at = self.px_to_cam(p);
        let d = self.edit.pen.get_or_insert_with(PenDraft::default);
        d.nodes.push(PenNode { at, out: at });
        d.pulling = true;
        self.redraw();
    }

    pub(crate) fn pen_move(&mut self, p: [f64; 2]) {
        let q = self.px_to_cam(p);
        if let Some(d) = self.edit.pen.as_mut().filter(|d| d.pulling) {
            if let Some(last) = d.nodes.last_mut() {
                last.out = q;
            }
        }
        self.redraw();
    }

    pub(crate) fn pen_end(&mut self, cancel: bool) {
        if cancel {
            self.edit.pen = None;
        } else {
            let (a, b) = (self.cam.ppc(), 3.0 * self.ppp());
            let Some(d) = self.edit.pen.as_mut() else {
                return;
            };
            d.pulling = false;
            // A short pull is a corner.
            if let Some(last) = d.nodes.last_mut() {
                if (last.out[0] - last.at[0]).hypot(last.out[1] - last.at[1]) * a < b {
                    last.out = last.at;
                }
            }
        }
        self.redraw();
    }

    /// Make the path drawn so far a shape (if it has two points or more).
    pub(crate) fn pen_finish(&mut self, closed: bool) {
        let Some(d) = self.edit.pen.take() else {
            return;
        };
        self.redraw();
        if d.nodes.len() < 2 {
            return;
        }
        let pts = path_pts(&d.nodes, closed && d.nodes.len() >= 3);
        let (center, half) = shapes::bounds_of(&pts);
        let mut style = self.ui.shape;
        style.kind = ShapeKind::Path;
        let data = ObjData::Shape {
            style,
            geom: Geom {
                center,
                half,
                rot: 0.0,
                pts,
            },
            width: self.ui.shape_width as f64 * self.ppp() / self.cam.ppc(),
            seed: self.next_seed_pub(),
        };
        let (_, ids) = self.add_group(&data, None);
        self.record_edit(vec![], ids);
    }

    /// The path being drawn, over the canvas: its curve, its points, and
    /// the handles of the last one. Finishes it if the pen was put down.
    pub(crate) fn pen_overlay(&mut self, ov: &mut crate::ui::Overlay) {
        if self.edit.pen.is_some() && !self.pen_active() {
            self.pen_finish(false);
        }
        let Some(d) = &self.edit.pen else {
            return;
        };
        let k = self.uipp();
        let ppc = self.cam.ppc();
        let to_pt = |q: [f64; 2]| {
            let s = self.cam_to_px(q);
            egui::pos2((s[0] / k) as f32, (s[1] / k) as f32)
        };
        let pts = path_pts(&d.nodes, false);
        let g = Geom {
            pts: pts.clone(),
            ..Default::default()
        };
        let mut style = self.ui.shape;
        style.kind = ShapeKind::Path;
        let w = self.ui.shape_width as f64 * self.ppp() / ppc;
        for pc in shapes::pieces(&style, &g, w, 7) {
            let [r, gg, bb, al] = pc.color.to_le_bytes();
            let col = egui::Color32::from_rgba_unmultiplied(r, gg, bb, al);
            let line = pc.pts.iter().map(|&q| to_pt(q)).collect();
            ov.lines
                .push((line, (pc.width * ppc / k) as f32, col, false));
        }
        let blue = egui::Color32::from_rgb(40, 110, 230);
        if let Some(last) = d.nodes.last() {
            if last.out != last.at {
                let line = vec![
                    to_pt(mirror(last.at, last.out)),
                    to_pt(last.at),
                    to_pt(last.out),
                ];
                ov.lines.push((line, 1.0, blue, false));
            }
        }
        let anchors = d.nodes.iter().map(|n| to_pt(n.at)).collect();
        ov.joints = Some((anchors, vec![]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_draws_corners_and_curves_and_closes() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.ui.tool = crate::ui::Tool::Shapes;
        app.ui.shape.kind = ShapeKind::Path;
        let c = [400.0, 300.0];
        // A corner, a pulled (smooth) point, a corner, then close.
        app.pen_begin(c);
        app.pen_end(false);
        app.pen_begin([c[0] + 100.0, c[1]]);
        app.pen_move([c[0] + 150.0, c[1] + 40.0]);
        app.pen_end(false);
        app.pen_begin([c[0] + 50.0, c[1] + 100.0]);
        app.pen_end(false);
        assert_eq!(app.edit.pen.as_ref().unwrap().nodes.len(), 3);
        app.pen_begin([c[0] + 2.0, c[1] + 2.0]);
        app.pen_end(false);
        assert!(app.edit.pen.is_none(), "tapping the first point closes it");
        let g = app.objs.groups.last().expect("a shape");
        let ObjData::Shape { style, geom, .. } = &g.data else {
            panic!("a shape")
        };
        assert_eq!(style.kind, ShapeKind::Path);
        // 3 points, closed: 3 segments of 3 points after the first.
        assert_eq!(geom.pts.len(), 10);
        let parts = shapes::subpaths(&geom.pts);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].1, "closed");
        // The pulled point curves: more than the 3 corners.
        assert!(parts[0].0.len() > 10);
        // Lists as a path.
        let r = app.run_commands(&serde_json::json!([{"get": "objects", "kinds": ["shape"]}]));
        assert_eq!(r[0]["objects"][0]["shape"], "path", "{r}");
    }

    #[test]
    fn paths_from_points_open_and_closed() {
        let open = shapes::path_through(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false, false);
        assert_eq!(open.len(), 7);
        let p = shapes::subpaths(&open);
        assert_eq!(p.len(), 1);
        assert!(!p[0].1);
        let closed = shapes::path_through(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], true, true);
        let p = shapes::subpaths(&closed);
        assert!(p[0].1);
        // Two squares, one inside the other: two closed parts (a hole).
        let two = shapes::path_of_polygons(&[
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            vec![[3.0, 3.0], [6.0, 3.0], [6.0, 6.0], [3.0, 6.0]],
        ]);
        let p = shapes::subpaths(&two);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p.iter().all(|q| q.1 && q.0.len() == 4));
    }
}
