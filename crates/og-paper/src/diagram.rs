// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Diagram mode (Settings > Diagram): lines and arrows stick to shapes,
//! texts, tables and pictures, like draw.io's connectors.
//!
//! Nothing extra is stored: a line or arrow end that sits on an object's
//! outline is linked to it, and the link is read from where the end is:
//!
//! - **On a connection point** (the green dots shown while drawing a line:
//!   the middle and quarters of a box's sides, eight points round an
//!   ellipse, a diamond's corners and edge middles), it is *fixed*: it
//!   stays on that point however the object moves, turns or changes.
//! - **Anywhere else on the outline** it *floats*: it slides round the
//!   outline to face the other end (or, when that end floats too, the other
//!   object's centre).
//!
//! Every edit of linked objects (moving, resizing, turning, flipping,
//! restyling, editing a text) re-routes the lines linked to them in the
//! same undo step, so files, sync and undo need nothing new. Links are
//! found among every object near the edited ones, on screen or not, with a
//! tolerance that never exceeds a sliver of the object's own size, so
//! zooming out doesn't make lines that merely pass nearby follow.

use std::collections::HashMap;

use crate::objects::{to_cam, ObjData, ObjRef};
use crate::shapes::{Geom, ShapeKind};
use crate::App;

/// How near (px) an end must be to an outline to count as linked.
const STICK_PX: f64 = 6.0;
/// How near (px) a drawn end must be to an object to snap onto it.
const SNAP_PX: f64 = 14.0;
/// How near (px) a drawn end must be to a connection point to take it.
const PORT_PX: f64 = 12.0;
/// How near (unit frame) an end must be to a connection point to be fixed:
/// loose enough for coordinates rounded to a few digits (scripts and
/// assistants read them that way), far tighter than anyone draws.
const PORT_EPS: f64 = 2e-3;

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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Outline {
    Box,
    Ellipse,
    Diamond,
    /// A symbol (a group with pins, `geom.pts`): its box, joined at its pins.
    Pins,
}

fn outline_of(d: &ObjData) -> Option<(Outline, &Geom)> {
    match d {
        ObjData::Bundle { geom, .. } if !geom.pts.is_empty() => Some((Outline::Pins, geom)),
        ObjData::Bundle { .. } | ObjData::Frame { .. } | ObjData::LayerLook { .. } => None,
        ObjData::Portal { style, geom, .. } if geom.pts.len() < 3 => match style.kind {
            ShapeKind::Ellipse => Some((Outline::Ellipse, geom)),
            ShapeKind::Diamond => Some((Outline::Diamond, geom)),
            _ => Some((Outline::Box, geom)),
        },
        ObjData::Portal { geom, .. } => Some((Outline::Box, geom)),
        ObjData::Shape { style, geom, .. } => match style.kind {
            ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Pin => None,
            ShapeKind::Ellipse => Some((Outline::Ellipse, geom)),
            ShapeKind::Diamond => Some((Outline::Diamond, geom)),
            _ => Some((Outline::Box, geom)),
        },
        ObjData::Text { geom, .. }
        | ObjData::Table { geom, .. }
        | ObjData::Image { geom, .. }
        | ObjData::Audio { geom, .. } => Some((Outline::Box, geom)),
    }
}

/// The outline point in the unit frame in direction `u` from the centre.
fn edge_dir(kind: Outline, u: [f64; 2]) -> [f64; 2] {
    let n = match kind {
        Outline::Box | Outline::Pins => u[0].abs().max(u[1].abs()),
        Outline::Ellipse => u[0].hypot(u[1]),
        Outline::Diamond => u[0].abs() + u[1].abs(),
    };
    if n < 1e-12 {
        return [1.0, 0.0];
    }
    [u[0] / n, u[1] / n]
}

/// The connection points of an outline (`g` its frame), in its unit frame.
fn ports(kind: Outline, g: &Geom) -> Vec<[f64; 2]> {
    match kind {
        Outline::Pins => g.pts.iter().filter_map(|&p| to_unit(g, p)).collect(),
        // The middle and quarters of each side.
        Outline::Box => {
            let mut v = Vec::new();
            for t in [-0.5, 0.0, 0.5] {
                v.extend([[t, -1.0], [1.0, t], [-t, 1.0], [-1.0, -t]]);
            }
            v
        }
        Outline::Ellipse => (0..8)
            .map(|k| {
                let a = k as f64 * std::f64::consts::FRAC_PI_4;
                [a.cos(), a.sin()]
            })
            .collect(),
        // Corners and the middles of the edges.
        Outline::Diamond => vec![
            [0.0, -1.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [-1.0, 0.0],
            [0.5, -0.5],
            [0.5, 0.5],
            [-0.5, 0.5],
            [-0.5, -0.5],
        ],
    }
}

fn inside(kind: Outline, u: [f64; 2]) -> bool {
    match kind {
        Outline::Box | Outline::Pins => u[0].abs().max(u[1].abs()) <= 1.0,
        Outline::Ellipse => u[0].hypot(u[1]) <= 1.0,
        Outline::Diamond => u[0].abs() + u[1].abs() <= 1.0,
    }
}

/// Distance (same units as `p`) from `p` to the outline, roughly: exact for
/// boxes seen straight on, close enough for ellipses and diamonds.
fn outline_dist(kind: Outline, g: &Geom, p: [f64; 2]) -> Option<f64> {
    let u = to_unit(g, p)?;
    let e = from_unit(g, edge_dir(kind, u));
    let d = crate::dist(e, p);
    // A symbol's pins may be inside its box: near one is on it.
    Some(if kind == Outline::Pins {
        g.pts.iter().map(|&q| crate::dist(q, p)).fold(d, f64::min)
    } else {
        d
    })
}

/// The box around some points: low and high corners.
fn bounds(pts: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    pts.iter()
        .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        })
}

/// A line or arrow end linked to an object.
#[derive(Clone, Debug)]
pub(crate) struct Link {
    /// The object's group.
    pub target: u32,
    /// The object as it was when the link was found (camera units).
    pub data: ObjData,
    /// Where on it, in its unit frame.
    unit: [f64; 2],
    /// On a connection point (stays there) or floating (faces the other end).
    pub fixed: bool,
    /// On a symbol's pin.
    pub pin: bool,
}

/// A line or arrow with an end linked to an object being edited.
#[derive(Clone, Debug)]
pub(crate) struct Attached {
    pub conn: u32,
    /// Its data (camera units).
    pub data: ObjData,
    /// What its first and last points are linked to.
    pub links: [Option<Link>; 2],
}

impl App {
    /// Objects (camera units) on screen that drawn ends can snap to.
    fn stick_targets(&self) -> Vec<(u32, ObjData)> {
        let mut seen = std::collections::HashSet::new();
        let mut v = Vec::new();
        for inst in &self.draw.strokes {
            // A symbol's member: the symbol.
            let Some(&g) = self
                .objs
                .bundle_of
                .get(&inst.stroke)
                .filter(|&&b| {
                    matches!(&self.objs.groups[b as usize].data, ObjData::Bundle { geom, .. } if !geom.pts.is_empty())
                })
                .or_else(|| self.objs.of_stroke.get(&inst.stroke))
            else {
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

    /// Every object still on the page whose box meets `lo`..`hi` (camera
    /// units), on screen or not, bottom to top.
    fn groups_in(&self, lo: [f64; 2], hi: [f64; 2]) -> Vec<(u32, ObjData)> {
        let mut v: Vec<(f64, u32, ObjData)> = Vec::new();
        for (g, grp) in self.objs.groups.iter().enumerate() {
            let Some(&first) = grp.strokes.first() else {
                continue;
            };
            if self.scene.strokes[first as usize].deleted {
                continue;
            }
            let d = to_cam(&grp.cell, &grp.data, &self.cam);
            let (a, b) = bounds(&d.extent());
            let finite = a.iter().chain(b.iter()).all(|x| x.is_finite());
            if finite && a[0] <= hi[0] && b[0] >= lo[0] && a[1] <= hi[1] && b[1] >= lo[1] {
                v.push((self.scene.strokes[first as usize].z, g as u32, d));
            }
        }
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, g, d)| (g, d)).collect()
    }

    /// How near (camera units) an end must be to `d`'s outline to be
    /// linked: a few pixels, but never more than a sliver of its size.
    fn stick_tol(&self, g: &Geom) -> f64 {
        let small = g.half[0].abs().min(g.half[1].abs());
        let big = g.half[0].abs().max(g.half[1].abs());
        (STICK_PX * self.ppp() / self.cam.ppc())
            .min(0.05 * small.max(big * 0.02))
            .max(1e-9 * big)
    }

    /// What `p` (camera units) is linked to among `cands`: the nearest
    /// outline within tolerance.
    fn link_at(&self, p: [f64; 2], cands: &[(u32, ObjData)]) -> Option<Link> {
        cands
            .iter()
            .filter(|(_, d)| !is_connector(d))
            .filter_map(|(g, d)| {
                let (k, geom) = outline_of(d)?;
                let dd = outline_dist(k, geom, p)?;
                (dd <= self.stick_tol(geom)).then(|| {
                    let pin = (k == Outline::Pins)
                        .then(|| {
                            geom.pts
                                .iter()
                                .filter(|&&q| crate::dist(q, p) <= self.stick_tol(geom))
                                .min_by(|a, b| crate::dist(**a, p).total_cmp(&crate::dist(**b, p)))
                        })
                        .flatten();
                    let unit = match pin {
                        Some(&q) => to_unit(geom, q).unwrap_or([1.0, 0.0]),
                        None => edge_dir(k, to_unit(geom, p).unwrap_or([1.0, 0.0])),
                    };
                    let fixed = ports(k, geom)
                        .iter()
                        .any(|q| (q[0] - unit[0]).abs().max((q[1] - unit[1]).abs()) < PORT_EPS);
                    (
                        dd,
                        Link {
                            target: *g,
                            data: d.clone(),
                            unit,
                            fixed,
                            pin: pin.is_some(),
                        },
                    )
                })
            })
            // A symbol's pin first, then the nearest.
            .min_by(|a, b| {
                (!a.1.pin, a.0)
                    .partial_cmp(&(!b.1.pin, b.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, l)| l)
    }

    /// Lines and arrows (not in `sel`) with an end linked to a selected
    /// object, and what each of their ends is linked to.
    pub(crate) fn attached_to(&self, sel: &[ObjRef]) -> Vec<Attached> {
        if !self.ui.diagram {
            return vec![];
        }
        let targets: Vec<(u32, ObjData)> = sel
            .iter()
            .filter_map(|r| match *r {
                ObjRef::Group(g) => {
                    let grp = self.objs.groups.get(g as usize)?;
                    let d = to_cam(&grp.cell, &grp.data, &self.cam);
                    (!is_connector(&d)).then_some((g, d))
                }
                ObjRef::Ink(_) => None,
            })
            .collect();
        if targets.is_empty() {
            return vec![];
        }
        let all: Vec<[f64; 2]> = targets.iter().flat_map(|(_, d)| d.extent()).collect();
        let (mut lo, mut hi) = bounds(&all);
        let pad = targets
            .iter()
            .filter_map(|(_, d)| outline_of(d).map(|(_, g)| self.stick_tol(g)))
            .fold(0.0, f64::max);
        lo = [lo[0] - pad, lo[1] - pad];
        hi = [hi[0] + pad, hi[1] + pad];
        let mut out = Vec::new();
        for (g, d) in self.groups_in(lo, hi) {
            if !is_connector(&d) || sel.contains(&ObjRef::Group(g)) {
                continue;
            }
            let pts = &d.geom().pts;
            if pts.len() < 2 {
                continue;
            }
            let ends = [pts[0], pts[pts.len() - 1]];
            let on = ends.map(|p| self.link_at(p, &targets));
            if on.iter().all(Option::is_none) {
                continue;
            }
            // An end not on the selection may be linked to something else.
            let links = [0, 1].map(|i| {
                on[i].clone().or_else(|| {
                    let p = ends[i];
                    let r = 1e-9 * (1.0 + p[0].abs().max(p[1].abs()));
                    self.link_at(
                        p,
                        &self.groups_in([p[0] - r, p[1] - r], [p[0] + r, p[1] + r]),
                    )
                })
            });
            out.push(Attached {
                conn: g,
                data: d,
                links,
            });
        }
        out
    }

    /// The line `a` re-routed after the objects in `moved` (group -> new
    /// data, camera units) changed: fixed ends stay on their connection
    /// points, floating ends face the other end, and when both ends' objects
    /// changed its middle points come along.
    pub(crate) fn route(a: &Attached, moved: &HashMap<u32, ObjData>) -> ObjData {
        let mut d = a.data.clone();
        let ObjData::Shape { geom, .. } = &mut d else {
            return d;
        };
        let n = geom.pts.len();
        if n < 2 {
            return d;
        }
        let idx = [0, n - 1];
        // An end's object now: its outline and frame.
        let now = |l: &Link| -> Option<(Outline, Geom)> {
            let data = moved.get(&l.target).unwrap_or(&l.data);
            outline_of(data).map(|(k, g)| (k, g.clone()))
        };
        let was = |l: &Link| outline_of(&l.data).map(|(_, g)| g.clone());
        let changed = |i: usize| {
            a.links[i]
                .as_ref()
                .is_some_and(|l| moved.contains_key(&l.target))
        };
        // Both ends' objects changed: the middle points move with the first.
        if changed(0) && changed(1) && n > 2 {
            if let Some(l) = a.links[0].as_ref() {
                if let (Some(old), Some((_, new))) = (was(l), now(l)) {
                    for p in &mut geom.pts[1..n - 1] {
                        if let Some(u) = to_unit(&old, *p) {
                            *p = from_unit(&new, u);
                        }
                    }
                }
            }
        }
        for (link, &at) in a.links.iter().zip(&idx) {
            if let Some(l) = link.as_ref().filter(|l| l.fixed) {
                if let Some((k, g)) = now(l) {
                    // A symbol's pin: the same pin, wherever it went (the
                    // symbol may have turned).
                    let pin = (k == Outline::Pins)
                        .then(|| was(l))
                        .flatten()
                        .and_then(|old| {
                            ports(Outline::Pins, &old).iter().position(|q| {
                                (q[0] - l.unit[0]).abs().max((q[1] - l.unit[1]).abs()) < PORT_EPS
                            })
                        })
                        .and_then(|i| g.pts.get(i).copied());
                    geom.pts[at] = pin.unwrap_or_else(|| from_unit(&g, l.unit));
                }
            }
        }
        for i in 0..2 {
            let Some(l) = a.links[i].as_ref().filter(|l| !l.fixed) else {
                continue;
            };
            let Some((k, g)) = now(l) else { continue };
            let toward = if n > 2 {
                geom.pts[if i == 0 { 1 } else { n - 2 }]
            } else {
                match a.links[1 - i].as_ref() {
                    Some(o) if !o.fixed => now(o).map_or(geom.pts[idx[1 - i]], |(_, og)| og.center),
                    _ => geom.pts[idx[1 - i]],
                }
            };
            if let Some(u) = to_unit(&g, toward) {
                geom.pts[idx[i]] = from_unit(&g, edge_dir(k, u));
            }
        }
        let (lo, hi) = bounds(&geom.pts);
        geom.center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        geom.half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5];
        d
    }

    /// Re-route the lines in `att` after objects were replaced (`pairs`:
    /// old group -> new group). Returns each re-routed line's old and new
    /// strokes, for the caller's undo step.
    pub(crate) fn reroute(
        &mut self,
        att: &[Attached],
        pairs: &[(u32, u32)],
    ) -> Vec<(Vec<u32>, Vec<u32>)> {
        let moved: HashMap<u32, ObjData> = pairs
            .iter()
            .map(|&(old, new)| {
                let grp = &self.objs.groups[new as usize];
                (old, to_cam(&grp.cell, &grp.data, &self.cam))
            })
            .collect();
        let mut out = Vec::new();
        for a in att {
            let touched = a
                .links
                .iter()
                .flatten()
                .any(|l| moved.contains_key(&l.target));
            if !touched {
                continue;
            }
            let nd = Self::route(a, &moved);
            if nd == a.data {
                continue;
            }
            let old = self.objs.groups[a.conn as usize].strokes.clone();
            if old
                .first()
                .is_none_or(|&s| self.scene.strokes[s as usize].deleted)
            {
                continue;
            }
            let n = self.group_copy(a.conn, |_| nd, None);
            out.push((old, self.objs.strokes(&n).to_vec()));
        }
        out
    }

    /// While dragging: hide the attached connectors (the overlay draws them
    /// following). Returns their strokes, to show again.
    pub(crate) fn hide_attached(&mut self, att: &[Attached]) -> Vec<u32> {
        let mut ids = Vec::new();
        for a in att {
            for &s in &self.objs.groups[a.conn as usize].strokes {
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

    /// The overlay lines of connectors following the selection while `op`
    /// (camera units) is applied to it.
    pub(crate) fn follow_preview(
        &self,
        att: &[Attached],
        op: Option<&crate::objects::Op>,
        ov: &mut crate::ui::Overlay,
    ) {
        let ppp = self.uipp();
        let ppc = self.cam.ppc();
        let sel = &self.edit.selection;
        for a in att {
            let moved: HashMap<u32, ObjData> = match op {
                Some(op) => a
                    .links
                    .iter()
                    .flatten()
                    .filter(|l| sel.contains(&ObjRef::Group(l.target)))
                    .map(|l| (l.target, l.data.edited(op)))
                    .collect(),
                None => HashMap::new(),
            };
            let d = Self::route(a, &moved);
            let px = d.map(|p| self.cam_to_px(p), ppc);
            for pc in px.pieces() {
                let [r, g, b, al] = pc.color.to_le_bytes();
                let col = egui::Color32::from_rgba_unmultiplied(r, g, b, al);
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

    /// The object (camera units) at or near `p` (camera units) that a drawn
    /// end snaps to: the one whose outline it is nearest, within reach;
    /// else the innermost one it is inside (so things in a box can be
    /// linked, not only the box drawn around them). Ties go to the topmost.
    fn snap_target(&self, p: [f64; 2]) -> Option<(Outline, Geom)> {
        let reach = SNAP_PX * self.ppp() / self.cam.ppc();
        let mut near: Option<(f64, (Outline, Geom))> = None;
        let mut within: Option<(f64, (Outline, Geom))> = None;
        for (k, g) in self
            .stick_targets()
            .into_iter()
            .rev()
            .filter(|(_, d)| !is_connector(d))
            .filter_map(|(_, d)| outline_of(&d).map(|(k, g)| (k, g.clone())))
        {
            let Some(u) = to_unit(&g, p) else {
                continue;
            };
            if let Some(d) = outline_dist(k, &g, p).filter(|&d| d <= reach) {
                if near.as_ref().is_none_or(|(best, _)| d < *best) {
                    near = Some((d, (k, g)));
                }
            } else if inside(k, u) {
                let area = (g.half[0] * g.half[1]).abs();
                if within.as_ref().is_none_or(|(best, _)| area < *best) {
                    within = Some((area, (k, g)));
                }
            }
        }
        near.or(within).map(|(_, t)| t)
    }

    /// The connection point of `t` within reach of `p` (camera units).
    fn port_near(&self, t: &(Outline, Geom), p: [f64; 2]) -> Option<[f64; 2]> {
        let reach = PORT_PX * self.ppp() / self.cam.ppc();
        ports(t.0, &t.1)
            .into_iter()
            .map(|u| from_unit(&t.1, u))
            .map(|q| (crate::dist(q, p), q))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, q)| q)
    }

    /// Where an end at `p` lands on object `t`: a connection point within
    /// reach, else the outline facing `toward` (camera units).
    fn land(&self, t: &(Outline, Geom), p: [f64; 2], toward: [f64; 2]) -> [f64; 2] {
        self.port_near(t, p).unwrap_or_else(|| {
            let u = to_unit(&t.1, toward).unwrap_or([1.0, 0.0]);
            from_unit(&t.1, edge_dir(t.0, u))
        })
    }

    /// Snap the ends of a line or arrow being drawn (px) onto nearby
    /// objects: onto a connection point near the end, else onto the outline
    /// facing the other end (its object's centre when it floats too).
    pub(crate) fn snap_ends(&self, a: [f64; 2], b: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        if !self.ui.diagram {
            return (a, b);
        }
        let (ac, bc) = (self.px_to_cam(a), self.px_to_cam(b));
        let (ta, tb) = (self.snap_target(ac), self.snap_target(bc));
        let (pa, pb) = (
            ta.as_ref().and_then(|t| self.port_near(t, ac)),
            tb.as_ref().and_then(|t| self.port_near(t, bc)),
        );
        // What each end faces: the other end's point, or its object's centre.
        let aim = |t: &Option<(Outline, Geom)>, port: Option<[f64; 2]>, raw: [f64; 2]| {
            port.or_else(|| t.as_ref().map(|t| t.1.center))
                .unwrap_or(raw)
        };
        let a2 = match &ta {
            Some(t) => pa.unwrap_or_else(|| self.land(t, ac, aim(&tb, pb, bc))),
            None => ac,
        };
        let b2 = match &tb {
            Some(t) => pb.unwrap_or_else(|| self.land(t, bc, aim(&ta, pa, a2))),
            None => bc,
        };
        (self.cam_to_px(a2), self.cam_to_px(b2))
    }

    /// One end of a line (px) snapped onto a nearby object, facing `toward`
    /// (the next point along the line).
    pub(crate) fn snap_end(&self, p: [f64; 2], toward: [f64; 2]) -> [f64; 2] {
        if !self.ui.diagram {
            return p;
        }
        let (pc, tc) = (self.px_to_cam(p), self.px_to_cam(toward));
        match self.snap_target(pc) {
            Some(t) => self.cam_to_px(self.land(&t, pc, tc)),
            None => p,
        }
    }

    /// Diagram mode with a line or arrow in hand (or one's end being
    /// dragged): the connection points of the object under the pointer, and
    /// of the objects the ends being drawn are on, green; the one an end
    /// would take, bigger.
    pub(crate) fn port_overlay(&self, ov: &mut crate::ui::Overlay) {
        if !self.ui.diagram {
            return;
        }
        let drawing_line =
            self.ui.tool == crate::ui::Tool::Shapes && self.ui.shape.kind.is_linear();
        let mut at: Vec<[f64; 2]> = Vec::new();
        if let Some((a, b)) = self.edit.shape_drag.filter(|_| drawing_line) {
            at.extend([a, b]);
        } else if let Some(j) = &self.edit.joint {
            if let Some(&q) = j.pts.get(j.index) {
                at.push(self.cam_to_px(q));
            }
        } else if drawing_line {
            at.push(self.cursor);
        }
        let k = self.uipp();
        let ppp = self.ppp();
        let green = egui::Color32::from_rgb(40, 170, 80);
        let dot = |c: [f64; 2], r: f64, fill: egui::Color32, ov: &mut crate::ui::Overlay| {
            let pts: Vec<egui::Pos2> = (0..14)
                .map(|i| {
                    let a = i as f64 / 14.0 * std::f64::consts::TAU;
                    egui::pos2(
                        ((c[0] + r * a.cos()) / k) as f32,
                        ((c[1] + r * a.sin()) / k) as f32,
                    )
                })
                .collect();
            ov.lines.push((pts, 0.0, fill, true));
        };
        let mut shown = Vec::new();
        for p in at {
            let pc = self.px_to_cam(p);
            let Some(t) = self.snap_target(pc) else {
                continue;
            };
            if shown
                .iter()
                .any(|s: &Geom| s.center == t.1.center && s.half == t.1.half)
            {
                continue;
            }
            let hot = self.port_near(&t, pc);
            for u in ports(t.0, &t.1) {
                let q = from_unit(&t.1, u);
                let big = hot == Some(q);
                let qp = self.cam_to_px(q);
                dot(
                    qp,
                    if big { 7.0 } else { 5.0 } * ppp,
                    egui::Color32::WHITE,
                    ov,
                );
                dot(qp, if big { 5.5 } else { 3.5 } * ppp, green, ov);
            }
            shown.push(t.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// An app in diagram mode at the home view, 1200 x 800 px.
    fn app() -> App {
        let mut app = App::new(None);
        app.ui.diagram = true;
        app
    }

    fn run(app: &mut App, cmds: Value) -> Vec<Value> {
        let r = app.run_commands(&cmds).as_array().unwrap().clone();
        assert!(r.iter().all(|r| r["ok"] == true), "{r:?}");
        r
    }

    fn id(r: &Value) -> String {
        r.get("id")
            .or(r["ids"].get(0))
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }

    /// The arrow's ends (home-view points).
    fn ends(app: &mut App) -> ([f64; 2], [f64; 2]) {
        let list = app.run_commands(&json!([{"get": "objects", "kinds": ["shape"]}]))[0]["objects"]
            .clone();
        let a = list
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["shape"] == "arrow")
            .expect("an arrow")
            .clone();
        let p = |k: &str| [a[k][0].as_f64().unwrap(), a[k][1].as_f64().unwrap()];
        (p("from"), p("to"))
    }

    fn near(p: [f64; 2], q: [f64; 2]) -> bool {
        (p[0] - q[0]).abs() < 0.05 && (p[1] - q[1]).abs() < 0.05
    }

    /// Box A (-300..-200), box B (100..200), both -50..50 tall, and an arrow
    /// from A's right to B's left at height `y`.
    fn two_boxes(app: &mut App, y: f64) -> (String, String) {
        let r = run(
            app,
            json!([
                {"add": "shape", "kind": "rect", "x": -300, "y": -50, "w": 100, "h": 100},
                {"add": "shape", "kind": "rect", "x": 100, "y": -50, "w": 100, "h": 100},
                {"add": "shape", "kind": "arrow", "x": -200, "y": y, "w": 300, "h": 0},
            ]),
        );
        (id(&r[0]), id(&r[1]))
    }

    #[test]
    fn lines_on_connection_points_stay_on_them() {
        let mut app = app();
        let (a, b) = two_boxes(&mut app, 0.0);
        // Move A up: the arrow's start stays at A's right middle.
        let r = run(&mut app, json!([{"move": [a], "dx": 0, "dy": -200}]));
        let a = id(&r[0]);
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [-200.0, -200.0]) && near(e, [100.0, 0.0]),
            "{s:?} {e:?}"
        );
        // Resize A (from its top left): still its right middle.
        let r = run(&mut app, json!([{"set": a, "w": 200, "h": 50}]));
        let a = id(&r[0]);
        let (s, _) = ends(&mut app);
        assert!(near(s, [-100.0, -225.0]), "{s:?}");
        // Turn A a quarter: its right middle is now at the bottom.
        run(&mut app, json!([{"set": a, "rotate": 90}]));
        let (s, _) = ends(&mut app);
        assert!(near(s, [-200.0, -125.0]), "{s:?}");
        // B changes kind: its left middle is still where the arrow ends.
        run(&mut app, json!([{"set": b, "kind": "ellipse"}]));
        let (_, e) = ends(&mut app);
        assert!(near(e, [100.0, 0.0]), "{e:?}");
    }

    #[test]
    fn moving_both_ends_objects_moves_the_whole_line() {
        let mut app = app();
        let (a, b) = two_boxes(&mut app, 0.0);
        run(&mut app, json!([{"move": [a, b], "dx": 50, "dy": 30}]));
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [-150.0, 30.0]) && near(e, [150.0, 30.0]),
            "{s:?} {e:?}"
        );
        // One undo step takes both boxes and the arrow back.
        app.undo_redo(false);
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [-200.0, 0.0]) && near(e, [100.0, 0.0]),
            "{s:?} {e:?}"
        );
    }

    #[test]
    fn floating_ends_turn_to_face_each_other() {
        let mut app = app();
        // Off the connection points: both ends float.
        let (_, b) = two_boxes(&mut app, 20.0);
        // B goes below A: the arrow leaves A's bottom for B's top.
        run(&mut app, json!([{"move": [b], "dx": -400, "dy": 350}]));
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [-250.0, 50.0]) && near(e, [-250.0, 300.0]),
            "{s:?} {e:?}"
        );
    }

    #[test]
    fn a_text_growing_keeps_its_lines() {
        let mut app = app();
        let r = run(
            &mut app,
            json!([{"add": "text", "x": 0, "y": 0, "text": "Hi", "size": 20}]),
        );
        let (t, w, h) = (
            id(&r[0]),
            r[0]["w"].as_f64().unwrap(),
            r[0]["h"].as_f64().unwrap(),
        );
        // An arrow into the text's left middle, from the left.
        run(
            &mut app,
            json!([{"add": "shape", "kind": "arrow", "x": -200, "y": h / 2.0, "w": 200, "h": 0}]),
        );
        let _ = w;
        // Longer, bigger words: the box grows from its top left; the arrow
        // still ends on its left middle.
        let r = run(
            &mut app,
            json!([{"set": t, "text": "Hello there, world", "size": 40}]),
        );
        let h2 = app.run_commands(&json!([{"get": "objects", "kinds": ["text"]}]))[0]["objects"][0]
            ["h"]
            .as_f64()
            .unwrap();
        assert!(h2 > h * 1.5);
        let (_, e) = ends(&mut app);
        assert!(near(e, [0.0, h2 / 2.0]), "{e:?} (box {h2})");
        let _ = r;
    }

    #[test]
    fn lines_merely_near_a_small_thing_do_not_follow_it() {
        let mut app = app();
        let r = run(
            &mut app,
            json!([{"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 20, "h": 20},
                   {"add": "shape", "kind": "arrow", "x": -100, "y": 10, "w": 97, "h": 0}]),
        );
        // 3 points short of a 20-point box: not on it.
        run(&mut app, json!([{"move": [id(&r[0])], "dx": 0, "dy": 100}]));
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [-100.0, 10.0]) && near(e, [-3.0, 10.0]),
            "{s:?} {e:?}"
        );
    }

    #[test]
    fn far_from_the_view_lines_still_follow() {
        let mut app = app();
        // Far outside the home view (and nothing drawn on screen).
        run(
            &mut app,
            json!([
                {"add": "shape", "kind": "rect", "x": 50000, "y": 50000, "w": 100, "h": 100},
                {"add": "shape", "kind": "rect", "x": 50400, "y": 50000, "w": 100, "h": 100},
                {"add": "shape", "kind": "arrow", "x": 50100, "y": 50050, "w": 300, "h": 0},
            ]),
        );
        let list = app.run_commands(&json!([{"get": "objects"}]))[0]["objects"].clone();
        let a = list[0]["id"].as_str().unwrap().to_string();
        run(&mut app, json!([{"move": [a], "dx": 0, "dy": 500}]));
        let (s, e) = ends(&mut app);
        assert!(
            near(s, [50100.0, 50550.0]) && near(e, [50400.0, 50050.0]),
            "{s:?} {e:?}"
        );
    }

    #[test]
    fn drawn_ends_take_connection_points_or_float() {
        let mut app = app();
        run(
            &mut app,
            json!([{"add": "shape", "kind": "rect", "x": -300, "y": -50, "w": 100, "h": 100},
                   {"add": "shape", "kind": "ellipse", "x": 100, "y": -50, "w": 100, "h": 100}]),
        );
        // What's on screen, as a frame would see it.
        app.size_override = Some([1200.0, 800.0]);
        ogpaper_core::query(
            &app.scene,
            &app.cam,
            1200.0,
            800.0,
            crate::VIEW,
            &mut app.draw,
        );
        let px = |x: f64, y: f64| [x + 600.0, y + 400.0];
        // 5 px from A's right middle and inside B: the port, and B's
        // outline facing it.
        let (a, b) = app.snap_ends(px(-205.0, 4.0), px(150.0, 10.0));
        assert!(near(a, px(-200.0, 0.0)), "{a:?}");
        assert!(near(b, px(100.0, 0.0)), "{b:?}");
        // From empty space to near B's top-right diagonal point.
        let d = std::f64::consts::FRAC_1_SQRT_2 * 50.0;
        let (_, b) = app.snap_ends(px(0.0, -300.0), px(150.0 + d + 4.0, -d - 3.0));
        assert!(near(b, px(150.0 + d, -d)), "{b:?}");
        // Mid-side, between ports: floats, facing the other end.
        let (a, _) = app.snap_ends(px(-200.0, 30.0), px(-200.0 + 300.0, 30.0 + 300.0 * 0.0));
        assert!((a[0] - px(-200.0, 0.0)[0]).abs() < 0.05, "{a:?}");
    }

    #[test]
    fn ends_in_a_box_take_the_things_in_it() {
        let mut app = app();
        // Two boxes, then a big one drawn around them (on top of them).
        run(
            &mut app,
            json!([{"add": "shape", "kind": "rect", "x": -250, "y": -50, "w": 100, "h": 100},
                   {"add": "shape", "kind": "rect", "x": 150, "y": -50, "w": 100, "h": 100},
                   {"add": "shape", "kind": "rect", "x": -400, "y": -200, "w": 800, "h": 400}]),
        );
        app.size_override = Some([1200.0, 800.0]);
        ogpaper_core::query(
            &app.scene,
            &app.cam,
            1200.0,
            800.0,
            crate::VIEW,
            &mut app.draw,
        );
        let px = |x: f64, y: f64| [x + 600.0, y + 400.0];
        // From inside one inner box to inside the other: their sides.
        let (a, b) = app.snap_ends(px(-200.0, 0.0), px(200.0, 0.0));
        assert!(near(a, px(-150.0, 0.0)), "{a:?}");
        assert!(near(b, px(150.0, 0.0)), "{b:?}");
        // Inside the big box only: the big box, its side facing the other end.
        let (_, b) = app.snap_ends(px(-200.0, 0.0), px(0.0, 150.0));
        assert!(near(b, px(-400.0, 0.0)), "{b:?}");
    }

    #[test]
    fn ports_and_links() {
        let g = Geom {
            center: [0.0, 0.0],
            half: [2.0, 1.0],
            rot: 0.0,
            pts: vec![],
        };
        assert_eq!(ports(Outline::Box, &Geom::default()).len(), 12);
        for k in [Outline::Box, Outline::Ellipse, Outline::Diamond] {
            for u in ports(k, &Geom::default()) {
                let p = from_unit(&g, u);
                assert!(outline_dist(k, &g, p).unwrap() < 1e-9, "{k:?} {u:?}");
            }
        }
    }

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
