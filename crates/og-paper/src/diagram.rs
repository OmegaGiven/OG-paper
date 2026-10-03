// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Diagram mode (Settings > Diagram): lines and arrows stick to shapes,
//! texts, tables and pictures.
//!
//! Nothing extra is stored: in diagram mode, a line or arrow end that sits
//! on an object's outline is attached to it. Drawing a line or arrow snaps
//! its ends onto the outlines near where it starts and stops (aimed at the
//! object's centre), and moving, resizing or rotating objects carries the
//! ends attached to them along, live while dragging. Files and undo need
//! nothing new, and any line touching an outline — old or new — follows.

use crate::objects::{to_cam, ObjData, ObjRef, Op};
use crate::shapes::{Geom, ShapeKind};
use crate::App;

/// How near (px) an end must be to an outline to count as attached.
const STICK_PX: f64 = 6.0;
/// How near (px) a drawn end must be to an object to snap onto it.
const SNAP_PX: f64 = 14.0;

/// A line or arrow (its points are its path).
pub(crate) fn is_connector(d: &ObjData) -> bool {
    matches!(d, ObjData::Shape { style, .. } if matches!(style.kind, ShapeKind::Line | ShapeKind::Arrow))
}

/// Into the box's own frame: unrotated, centred, scaled by its half size
/// (so the box is [-1, 1]^2). None for a flat box.
fn to_unit(g: &Geom, p: [f64; 2]) -> Option<[f64; 2]> {
    let (s, c) = g.rot.sin_cos();
    let d = [p[0] - g.center[0], p[1] - g.center[1]];
    let l = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c];
    let (hx, hy) = (g.half[0].abs(), g.half[1].abs());
    (hx > 1e-300 && hy > 1e-300).then(|| [l[0] / hx, l[1] / hy])
}

fn from_unit(g: &Geom, u: [f64; 2]) -> [f64; 2] {
    let (s, c) = g.rot.sin_cos();
    let l = [u[0] * g.half[0].abs(), u[1] * g.half[1].abs()];
    [
        g.center[0] + l[0] * c - l[1] * s,
        g.center[1] + l[0] * s + l[1] * c,
    ]
}

#[derive(Clone, Copy, PartialEq)]
enum Outline {
    Box,
    Ellipse,
    Diamond,
}

fn outline_of(d: &ObjData) -> Option<(Outline, &Geom)> {
    match d {
        ObjData::Portal { style, geom, .. } if geom.pts.len() < 3 => match style.kind {
            ShapeKind::Ellipse => Some((Outline::Ellipse, geom)),
            ShapeKind::Diamond => Some((Outline::Diamond, geom)),
            _ => Some((Outline::Box, geom)),
        },
        ObjData::Portal { geom, .. } => Some((Outline::Box, geom)),
        ObjData::Shape { style, geom, .. } => match style.kind {
            ShapeKind::Line | ShapeKind::Arrow => None,
            ShapeKind::Ellipse => Some((Outline::Ellipse, geom)),
            ShapeKind::Diamond => Some((Outline::Diamond, geom)),
            _ => Some((Outline::Box, geom)),
        },
        ObjData::Text { geom, .. } | ObjData::Table { geom, .. } | ObjData::Image { geom, .. } => {
            Some((Outline::Box, geom))
        }
    }
}

/// The outline point in the unit frame in direction `u` from the centre.
fn edge_dir(kind: Outline, u: [f64; 2]) -> [f64; 2] {
    let n = match kind {
        Outline::Box => u[0].abs().max(u[1].abs()),
        Outline::Ellipse => u[0].hypot(u[1]),
        Outline::Diamond => u[0].abs() + u[1].abs(),
    };
    if n < 1e-12 {
        return [1.0, 0.0];
    }
    [u[0] / n, u[1] / n]
}

/// Distance (same units as `p`) from `p` to the outline, roughly: exact for
/// boxes seen straight on, close enough for ellipses and diamonds.
fn outline_dist(kind: Outline, g: &Geom, p: [f64; 2]) -> Option<f64> {
    let u = to_unit(g, p)?;
    let e = from_unit(g, edge_dir(kind, u));
    Some(crate::dist(e, p))
}

impl App {
    /// Objects (camera units) that ends can stick to, near the screen.
    fn stick_targets(&self) -> Vec<(u32, ObjData)> {
        let mut seen = std::collections::HashSet::new();
        let mut v = Vec::new();
        for inst in &self.draw.strokes {
            let Some(&g) = self.objs.of_stroke.get(&inst.stroke) else {
                continue;
            };
            if !seen.insert(g) || self.scene.strokes[inst.stroke as usize].deleted {
                continue;
            }
            let grp = &self.objs.groups[g as usize];
            v.push((g, to_cam(&grp.cell, &grp.data, &self.cam)));
        }
        v
    }

    /// Lines and arrows (not in `sel`) with an end on the outline of a
    /// selected object: the group, its data (camera units) and which ends.
    pub(crate) fn attached_to(&self, sel: &[ObjRef]) -> Vec<(u32, ObjData, [bool; 2])> {
        if !self.ui.diagram {
            return vec![];
        }
        let tol = STICK_PX * self.ppp() / self.cam.ppc();
        let all = self.stick_targets();
        let picked: Vec<&ObjData> = all
            .iter()
            .filter(|(g, d)| sel.contains(&ObjRef::Group(*g)) && !is_connector(d))
            .map(|(_, d)| d)
            .collect();
        if picked.is_empty() {
            return vec![];
        }
        let mut out = Vec::new();
        for (g, d) in &all {
            if sel.contains(&ObjRef::Group(*g)) || !is_connector(d) {
                continue;
            }
            let pts = &d.geom().pts;
            if pts.len() < 2 {
                continue;
            }
            let ends = [pts[0], pts[pts.len() - 1]].map(|p| {
                picked.iter().any(|t| {
                    outline_of(t)
                        .and_then(|(k, geom)| outline_dist(k, geom, p))
                        .is_some_and(|dd| dd <= tol)
                })
            });
            if ends[0] || ends[1] {
                out.push((*g, d.clone(), ends));
            }
        }
        out
    }

    /// A connector with its attached ends moved by `op` (camera units).
    pub(crate) fn follow(d: &ObjData, ends: [bool; 2], op: &Op) -> ObjData {
        if ends == [true, true] {
            return d.edited(op);
        }
        let mut d = d.clone();
        if let ObjData::Shape { geom, .. } = &mut d {
            let n = geom.pts.len();
            if ends[0] {
                geom.pts[0] = op.apply(geom.pts[0]);
            }
            if ends[1] {
                geom.pts[n - 1] = op.apply(geom.pts[n - 1]);
            }
            let (lo, hi) = geom
                .pts
                .iter()
                .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                });
            geom.center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
            geom.half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5];
        }
        d
    }

    /// While dragging: hide the attached connectors (the overlay draws them
    /// following). Returns their strokes, to show again.
    pub(crate) fn hide_attached(&mut self, att: &[(u32, ObjData, [bool; 2])]) -> Vec<u32> {
        let mut ids = Vec::new();
        for (g, _, _) in att {
            for &s in &self.objs.groups[*g as usize].strokes {
                if !self.scene.strokes[s as usize].deleted {
                    self.scene.strokes[s as usize].deleted = true;
                    ids.push(s);
                }
            }
        }
        ids
    }

    pub(crate) fn unhide(&mut self, ids: &[u32]) {
        for &s in ids {
            self.scene.strokes[s as usize].deleted = false;
        }
    }

    /// The overlay lines (points) of connectors following `op` (camera units).
    pub(crate) fn follow_preview(
        &self,
        att: &[(u32, ObjData, [bool; 2])],
        op: Option<&Op>,
        ov: &mut crate::ui::Overlay,
    ) {
        let ppp = self.uipp();
        let ppc = self.cam.ppc();
        for (_, d, ends) in att {
            let d = op.map_or(d.clone(), |op| Self::follow(d, *ends, op));
            let px = d.map(|p| self.cam_to_px(p), ppc);
            for pc in px.pieces() {
                let [r, g, b, a] = pc.color.to_le_bytes();
                let col = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
                let pts = pc
                    .pts
                    .iter()
                    .map(|q| egui::pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32))
                    .collect();
                let fill = pc.brush == ogpaper_core::Brush::Fill;
                ov.lines.push((pts, (pc.width / ppp) as f32, col, fill));
            }
        }
    }

    /// Snap the ends of a line or arrow being drawn (px) onto nearby
    /// outlines, each aimed at its object's centre.
    pub(crate) fn snap_ends(&self, a: [f64; 2], b: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        if !self.ui.diagram {
            return (a, b);
        }
        let reach = SNAP_PX * self.ppp() / self.cam.ppc();
        let (ac, bc) = (self.px_to_cam(a), self.px_to_cam(b));
        let all = self.stick_targets();
        // The topmost object at or near an end.
        let find = |p: [f64; 2]| -> Option<(Outline, Geom)> {
            all.iter()
                .rev()
                .filter(|(_, d)| !is_connector(d))
                .filter_map(|(_, d)| outline_of(d).map(|(k, g)| (k, g.clone())))
                .find(|(k, g)| {
                    to_unit(g, p).is_some_and(|u| {
                        let inside = match k {
                            Outline::Box => u[0].abs().max(u[1].abs()) <= 1.0,
                            Outline::Ellipse => u[0].hypot(u[1]) <= 1.0,
                            Outline::Diamond => u[0].abs() + u[1].abs() <= 1.0,
                        };
                        inside || outline_dist(*k, g, p).is_some_and(|d| d <= reach)
                    })
                })
        };
        let snap = |p: [f64; 2], toward: [f64; 2]| -> [f64; 2] {
            match find(p) {
                Some((k, g)) => {
                    // Where the line from the centre toward the other end leaves.
                    let u = to_unit(&g, toward).unwrap_or([1.0, 0.0]);
                    from_unit(&g, edge_dir(k, u))
                }
                None => p,
            }
        };
        let a2 = snap(ac, bc);
        let b2 = snap(bc, a2);
        let a2 = if a2 != ac { snap(ac, b2) } else { a2 };
        (self.cam_to_px(a2), self.cam_to_px(b2))
    }

    /// One end of a line (px) snapped onto a nearby outline, aimed from that
    /// object's centre toward `toward` (the next point along the line).
    pub(crate) fn snap_end(&self, p: [f64; 2], toward: [f64; 2]) -> [f64; 2] {
        if !self.ui.diagram {
            return p;
        }
        let reach = SNAP_PX * self.ppp() / self.cam.ppc();
        let (pc, tc) = (self.px_to_cam(p), self.px_to_cam(toward));
        let hit = self
            .stick_targets()
            .into_iter()
            .rev()
            .filter(|(_, d)| !is_connector(d))
            .filter_map(|(_, d)| outline_of(&d).map(|(k, g)| (k, g.clone())))
            .find(|(k, g)| {
                to_unit(g, pc).is_some_and(|u| {
                    let inside = match k {
                        Outline::Box => u[0].abs().max(u[1].abs()) <= 1.0,
                        Outline::Ellipse => u[0].hypot(u[1]) <= 1.0,
                        Outline::Diamond => u[0].abs() + u[1].abs() <= 1.0,
                    };
                    inside || outline_dist(*k, g, pc).is_some_and(|d| d <= reach)
                })
            });
        match hit {
            Some((k, g)) => {
                let u = to_unit(&g, tc).unwrap_or([1.0, 0.0]);
                self.cam_to_px(from_unit(&g, edge_dir(k, u)))
            }
            None => p,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines() {
        let g = Geom {
            center: [10.0, 0.0],
            half: [4.0, 2.0],
            rot: 0.0,
            pts: vec![],
        };
        assert!(outline_dist(Outline::Box, &g, [14.0, 1.0]).unwrap() < 1e-9);
        assert!((outline_dist(Outline::Box, &g, [10.0, 0.5]).unwrap() - 1.5).abs() < 1e-9);
        assert!(outline_dist(Outline::Ellipse, &g, [10.0, 2.0]).unwrap() < 1e-9);
        // Aimed at the centre from the left: the left edge's middle.
        let e = from_unit(&g, edge_dir(Outline::Box, to_unit(&g, [0.0, 0.0]).unwrap()));
        assert!((e[0] - 6.0).abs() < 1e-9 && e[1].abs() < 1e-9);
    }
}
