// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Editable objects. A freehand stroke is its own object; a shape or a text
//! is a *group*: the settings it was made from plus the strokes generated
//! from them. Editing (move, resize, rotate, restyle, reorder) never changes
//! strokes in place: it deletes them and adds new ones in one undoable step,
//! so undo, the timeline, autosave and sync all keep working unchanged.

use std::collections::HashMap;

use ogpaper_core::{Brush, Camera, CellAddr, Dash, Scene, Style};

use crate::font::{self, Align, FontId, Mark};
use crate::shapes::{self, Geom, Piece, ShapeStyle};

/// How text looks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextStyle {
    /// A font in the registry (stored by name in files).
    pub font: FontId,
    pub align: Align,
    pub color: u32,
    /// 0..=255.
    pub opacity: u8,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: font::default_font(),
            align: Align::Left,
            color: shapes::rgba(28, 28, 36, 255),
            opacity: 255,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ObjData {
    /// `width` is the outline width in frame units.
    Shape {
        style: ShapeStyle,
        geom: Geom,
        width: f64,
        seed: u32,
    },
    /// `geom.center` is the text box's centre, `geom.half` its half size,
    /// `size` the cap height (frame units).
    Text {
        text: String,
        style: TextStyle,
        geom: Geom,
        size: f64,
        seed: u32,
    },
}

/// A shape or a text: its settings, in the units of `cell`, and its strokes.
#[derive(Clone, Debug)]
pub struct Group {
    pub cell: CellAddr,
    pub data: ObjData,
    pub strokes: Vec<u32>,
}

/// Something that can be selected.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ObjRef {
    Ink(u32),
    Group(u32),
}

#[derive(Default)]
pub struct Objects {
    pub groups: Vec<Group>,
    pub of_stroke: HashMap<u32, u32>,
}

impl Objects {
    pub fn obj_of(&self, stroke: u32) -> ObjRef {
        match self.of_stroke.get(&stroke) {
            Some(&g) => ObjRef::Group(g),
            None => ObjRef::Ink(stroke),
        }
    }

    pub fn strokes<'a>(&'a self, r: &'a ObjRef) -> &'a [u32] {
        match r {
            ObjRef::Ink(id) => std::slice::from_ref(id),
            ObjRef::Group(g) => &self.groups[*g as usize].strokes,
        }
    }

    pub fn add(&mut self, g: Group) -> u32 {
        let gi = self.groups.len() as u32;
        for &s in &g.strokes {
            self.of_stroke.insert(s, gi);
        }
        self.groups.push(g);
        gi
    }

    pub fn alive(&self, scene: &Scene, r: &ObjRef) -> bool {
        self.strokes(r)
            .first()
            .is_some_and(|&s| !scene.strokes[s as usize].deleted)
    }
}

/// Origin and side of `cell` in camera-cell units.
pub fn frame(cell: &CellAddr, cam: &Camera) -> ([f64; 2], f64) {
    (cell.origin_in(&cam.cell), cell.side_in(&cam.cell))
}

/// An edit to apply to points in camera-cell units.
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Move([f64; 2]),
    /// Scale by (sx, sy) about `pivot`, along axes turned by `rot`.
    Scale {
        pivot: [f64; 2],
        sx: f64,
        sy: f64,
        rot: f64,
    },
    Rotate {
        pivot: [f64; 2],
        angle: f64,
    },
}

impl Op {
    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        match *self {
            Op::Move(d) => [p[0] + d[0], p[1] + d[1]],
            Op::Scale { pivot, sx, sy, rot } => {
                let (s, c) = rot.sin_cos();
                let d = [p[0] - pivot[0], p[1] - pivot[1]];
                // Into the box's axes, scale, and back.
                let (u, v) = (d[0] * c + d[1] * s, -d[0] * s + d[1] * c);
                let (u, v) = (u * sx, v * sy);
                [pivot[0] + u * c - v * s, pivot[1] + u * s + v * c]
            }
            Op::Rotate { pivot, angle } => {
                let (s, c) = angle.sin_cos();
                let d = [p[0] - pivot[0], p[1] - pivot[1]];
                [
                    pivot[0] + d[0] * c - d[1] * s,
                    pivot[1] + d[0] * s + d[1] * c,
                ]
            }
        }
    }

    /// How much lengths grow (for stroke widths and text size).
    pub fn scale_factor(&self) -> f64 {
        match *self {
            Op::Scale { sx, sy, .. } => (sx * sy).abs().sqrt(),
            _ => 1.0,
        }
    }

    /// Apply to a point given in a cell's local units (origin `o`, side `side`
    /// in camera units).
    pub fn apply_local(&self, p: [f64; 2], o: [f64; 2], side: f64) -> [f64; 2] {
        let q = self.apply([o[0] + p[0] * side, o[1] + p[1] * side]);
        [(q[0] - o[0]) / side, (q[1] - o[1]) / side]
    }

    /// The geometry after the edit (camera units in, camera units out).
    fn geom(&self, g: &Geom) -> Geom {
        let mut out = g.clone();
        out.center = self.apply(g.center);
        out.pts = g.pts.iter().map(|&p| self.apply(p)).collect();
        match *self {
            Op::Move(_) => {}
            Op::Rotate { angle, .. } => out.rot += angle,
            Op::Scale { sx, sy, rot, .. } => {
                // Box shapes keep their own axes: scale along them when they
                // line up with the selection's, else uniformly.
                let d = (g.rot - rot).rem_euclid(std::f64::consts::FRAC_PI_2);
                let aligned = d < 1e-6 || (std::f64::consts::FRAC_PI_2 - d) < 1e-6;
                if aligned {
                    let quarter = ((g.rot - rot) / std::f64::consts::FRAC_PI_2).round() as i64;
                    let (ax, ay) = if quarter.rem_euclid(2) == 0 {
                        (sx, sy)
                    } else {
                        (sy, sx)
                    };
                    out.half = [g.half[0] * ax, g.half[1] * ay];
                } else {
                    let k = (sx * sy).abs().sqrt();
                    out.half = [g.half[0] * k, g.half[1] * k];
                }
            }
        }
        out
    }
}

fn geom_map(g: &Geom, f: impl Fn([f64; 2]) -> [f64; 2], k: f64) -> Geom {
    Geom {
        center: f(g.center),
        half: [g.half[0] * k, g.half[1] * k],
        rot: g.rot,
        pts: g.pts.iter().map(|&p| f(p)).collect(),
    }
}

impl ObjData {
    pub fn geom(&self) -> &Geom {
        match self {
            ObjData::Shape { geom, .. } | ObjData::Text { geom, .. } => geom,
        }
    }

    /// Convert lengths and positions between frames: `f` maps points, `k`
    /// scales lengths.
    pub fn map(&self, f: impl Fn([f64; 2]) -> [f64; 2], k: f64) -> ObjData {
        match self {
            ObjData::Shape {
                style,
                geom,
                width,
                seed,
            } => ObjData::Shape {
                style: *style,
                geom: geom_map(geom, &f, k),
                width: width * k,
                seed: *seed,
            },
            ObjData::Text {
                text,
                style,
                geom,
                size,
                seed,
            } => ObjData::Text {
                text: text.clone(),
                style: *style,
                geom: geom_map(geom, &f, k),
                size: size * k,
                seed: *seed,
            },
        }
    }

    /// The edit applied (camera units).
    pub fn edited(&self, op: &Op) -> ObjData {
        match self {
            ObjData::Shape {
                style,
                geom,
                width,
                seed,
            } => ObjData::Shape {
                style: *style,
                geom: op.geom(geom),
                width: *width,
                seed: *seed,
            },
            ObjData::Text {
                text,
                style,
                geom,
                size,
                seed,
            } => {
                let k = op.scale_factor();
                let mut g = op.geom(geom);
                g.half = [geom.half[0] * k, geom.half[1] * k];
                ObjData::Text {
                    text: text.clone(),
                    style: *style,
                    geom: g,
                    size: size * k,
                    seed: *seed,
                }
            }
        }
    }

    /// The strokes to store, in frame units.
    pub fn pieces(&self) -> Vec<Piece> {
        match self {
            ObjData::Shape {
                style,
                geom,
                width,
                seed,
            } => shapes::pieces(style, geom, *width, *seed),
            ObjData::Text {
                text,
                style,
                geom,
                size,
                seed,
            } => text_pieces(text, style, geom, *size, *seed),
        }
    }

    /// The points that bound it (frame units), for choosing its cell.
    pub fn extent(&self) -> Vec<[f64; 2]> {
        let g = self.geom();
        if !g.pts.is_empty() {
            return g.pts.clone();
        }
        let (s, c) = g.rot.sin_cos();
        [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .iter()
            .map(|u| {
                let (x, y) = (u[0] * g.half[0], u[1] * g.half[1]);
                [g.center[0] + x * c - y * s, g.center[1] + x * s + y * c]
            })
            .collect()
    }
}

/// Text box size (frame units) for `text` at cap height `size`.
pub fn text_box(text: &str, style: &TextStyle, size: f64) -> [f64; 2] {
    let (_, b) = font::layout(text, style.font, style.align, 0);
    let u = size / 6.0;
    [b[0] * u, b[1] * u]
}

fn text_pieces(text: &str, style: &TextStyle, g: &Geom, size: f64, seed: u32) -> Vec<Piece> {
    let (marks, b) = font::layout(text, style.font, style.align, seed);
    let u = size / 6.0;
    let (s, c) = g.rot.sin_cos();
    let [r, gg, bb, a] = style.color.to_le_bytes();
    let color = u32::from_le_bytes([r, gg, bb, ((a as u32 * style.opacity as u32) / 255) as u8]);
    // Single-line fonts: pen width from the size (thinner for mono).
    let width = size * if style.font == 2 { 0.07 } else { 0.085 };
    let place = |p: &[f64; 2]| {
        let (x, y) = ((p[0] - b[0] * 0.5) * u, (p[1] - b[1] * 0.5) * u);
        [g.center[0] + x * c - y * s, g.center[1] + x * s + y * c]
    };
    marks
        .into_iter()
        .map(|m| match m {
            Mark::Line(pts) => Piece {
                pts: pts.iter().map(place).collect(),
                width,
                brush: Brush::Marker,
                dash: Dash::Solid,
                color,
                bridges: vec![],
            },
            Mark::Fill(pts, bridges) => Piece {
                pts: pts.iter().map(place).collect(),
                width: 0.0,
                brush: Brush::Fill,
                dash: Dash::Solid,
                color,
                bridges,
            },
        })
        .collect()
}

/// Re-express object data given in camera units in the cell that fits it.
pub fn home(data: &ObjData, cam: &Camera) -> (CellAddr, ObjData) {
    let ext = data.extent();
    let (cell, _, side) = Scene::anchor_for_min(&cam.cell, &ext, 1e-12);
    let o = cell.origin_in(&cam.cell);
    let d = data.map(|p| [(p[0] - o[0]) / side, (p[1] - o[1]) / side], 1.0 / side);
    (cell, d)
}

/// Object data in camera units.
pub fn to_cam(cell: &CellAddr, data: &ObjData, cam: &Camera) -> ObjData {
    let (o, side) = frame(cell, cam);
    data.map(|p| [o[0] + p[0] * side, o[1] + p[1] * side], side)
}

/// Store a group's pieces as strokes, at draw orders spread over `z`
/// (lowest, highest). Returns the new stroke ids.
pub fn emit(scene: &mut Scene, cell: &CellAddr, data: &ObjData, z: (f64, f64)) -> Vec<u32> {
    let pieces = data.pieces();
    let n = pieces.len().max(1);
    let mut ids = Vec::with_capacity(pieces.len());
    for (i, p) in pieces.into_iter().enumerate() {
        if p.pts.is_empty() {
            continue;
        }
        let min = if p.brush == Brush::Fill {
            1e-12
        } else {
            p.width.max(1e-12)
        };
        let (anchor, local, side) = Scene::anchor_for_min(cell, &p.pts, min);
        // Fill polygons mark their contour joins with pressure -1.
        let mut pts: Vec<[f32; 4]> = local.iter().map(|l| [l[0], l[1], 1.0, 0.0]).collect();
        for &j in &p.bridges {
            if let Some(q) = pts.get_mut(j as usize) {
                q[2] = -1.0;
            }
        }
        let style = Style {
            width: (p.width / side) as f32,
            color: p.color,
            brush: p.brush,
            dash: p.dash,
        };
        let zi = if n > 1 {
            z.0 + (z.1 - z.0) * i as f64 / (n - 1) as f64
        } else {
            z.0
        };
        ids.push(scene.add_stroke_at(&anchor, &pts, style, crate::uid::new(), zi));
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::ShapeKind;

    #[test]
    fn rotate_and_scale_ops() {
        let r = Op::Rotate {
            pivot: [1.0, 1.0],
            angle: std::f64::consts::FRAC_PI_2,
        };
        let p = r.apply([2.0, 1.0]);
        assert!((p[0] - 1.0).abs() < 1e-12 && (p[1] - 2.0).abs() < 1e-12);
        let s = Op::Scale {
            pivot: [0.0, 0.0],
            sx: 2.0,
            sy: 3.0,
            rot: 0.0,
        };
        assert_eq!(s.apply([1.0, 1.0]), [2.0, 3.0]);
    }

    #[test]
    fn emit_and_rehome_a_shape() {
        let cam = Camera::new(CellAddr::new(40, 5, 7), [0.5, 0.5], 800.0);
        let data = ObjData::Shape {
            style: ShapeStyle {
                kind: ShapeKind::Star,
                fill_style: shapes::FillStyle::Solid,
                ..Default::default()
            },
            geom: Geom {
                center: [0.5, 0.5],
                half: [0.1, 0.1],
                rot: 0.0,
                pts: vec![],
            },
            width: 0.004,
            seed: 3,
        };
        let (cell, local) = home(&data, &cam);
        assert!(
            cell.level >= 42,
            "a fifth of the camera cell anchors deeper"
        );
        let back = to_cam(&cell, &local, &cam);
        assert!((back.geom().center[0] - 0.5).abs() < 1e-9);
        let mut scene = Scene::new();
        let ids = emit(&mut scene, &cell, &local, (1.0, 2.0));
        assert!(ids.len() >= 2);
        assert!(ids
            .iter()
            .any(|&i| scene.strokes[i as usize].brush == Brush::Fill));
        let z: Vec<f64> = ids.iter().map(|&i| scene.strokes[i as usize].z).collect();
        assert!(z.windows(2).all(|w| w[0] <= w[1]) && z[0] == 1.0);
    }

    #[test]
    fn text_scales_with_resize() {
        let st = TextStyle::default();
        let half = text_box("Hello", &st, 0.06);
        let data = ObjData::Text {
            text: "Hello".into(),
            style: st,
            geom: Geom {
                center: [0.0, 0.0],
                half: [half[0] * 0.5, half[1] * 0.5],
                rot: 0.0,
                pts: vec![],
            },
            size: 0.06,
            seed: 1,
        };
        let big = data.edited(&Op::Scale {
            pivot: [0.0, 0.0],
            sx: 2.0,
            sy: 2.0,
            rot: 0.0,
        });
        match big {
            ObjData::Text { size, .. } => assert!((size - 0.12).abs() < 1e-12),
            _ => unreachable!(),
        }
        assert!(!data.pieces().is_empty());
    }
}
