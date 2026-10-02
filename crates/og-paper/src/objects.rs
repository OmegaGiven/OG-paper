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
use crate::images::Asset;
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
    /// A picture: `geom` is its box; its file is in [`Objects::images`].
    Image { id: u64, geom: Geom, opacity: u8 },
    /// Rows of cells drawn as a grid; `size` is the cap height and `geom`
    /// the whole table's box.
    Table {
        cells: Vec<Vec<String>>,
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
    /// Picture files by id.
    pub images: HashMap<u64, Asset>,
    /// The stroke that places a picture (its corners): picture id and opacity.
    pub image_of: HashMap<u32, (u64, u8)>,
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
        if let ObjData::Image { id, opacity, .. } = g.data {
            for &s in &g.strokes {
                self.image_of.insert(s, (id, opacity));
            }
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
            ObjData::Shape { geom, .. }
            | ObjData::Text { geom, .. }
            | ObjData::Image { geom, .. }
            | ObjData::Table { geom, .. } => geom,
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
            ObjData::Image { id, geom, opacity } => ObjData::Image {
                id: *id,
                geom: geom_map(geom, &f, k),
                opacity: *opacity,
            },
            ObjData::Table {
                cells,
                style,
                geom,
                size,
                seed,
            } => ObjData::Table {
                cells: cells.clone(),
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
            ObjData::Image { id, geom, opacity } => ObjData::Image {
                id: *id,
                geom: op.geom(geom),
                opacity: *opacity,
            },
            ObjData::Table {
                cells,
                style,
                geom,
                size,
                seed,
            } => {
                let k = op.scale_factor();
                let mut g = op.geom(geom);
                g.half = [geom.half[0] * k, geom.half[1] * k];
                ObjData::Table {
                    cells: cells.clone(),
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
            // One invisible polygon on the picture's corners: it places the
            // picture (the renderer draws it there) and makes it selectable.
            ObjData::Image { .. } => vec![Piece {
                pts: self.extent(),
                width: 0.0,
                brush: Brush::Fill,
                dash: Dash::Solid,
                color: 0,
                bridges: vec![],
            }],
            ObjData::Table {
                cells,
                style,
                geom,
                size,
                seed,
            } => table_pieces(cells, style, geom, *size, *seed),
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

/// Column widths, row heights and padding of a table (frame units).
pub struct TableLayout {
    pub cols: Vec<f64>,
    pub rows: Vec<f64>,
    pub pad: f64,
}

impl TableLayout {
    pub fn size(&self) -> [f64; 2] {
        [self.cols.iter().sum(), self.rows.iter().sum()]
    }
}

pub fn table_layout(cells: &[Vec<String>], style: &TextStyle, size: f64) -> TableLayout {
    let pad = size * 0.8;
    let n = cells.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let line = text_box("Xg", style, size)[1];
    let mut cols = vec![size * 2.0; n];
    let mut rows = Vec::with_capacity(cells.len());
    for row in cells {
        let mut h = line;
        for (j, c) in row.iter().enumerate() {
            if c.trim().is_empty() {
                continue;
            }
            let b = text_box(c, style, size);
            cols[j] = cols[j].max(b[0]);
            h = h.max(b[1]);
        }
        rows.push(h + 2.0 * pad);
    }
    for c in &mut cols {
        *c += 2.0 * pad;
    }
    TableLayout { cols, rows, pad }
}

/// Tab-separated text (one row per line) as cells; trailing empty rows go.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = text
        .lines()
        .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
        .collect();
    while rows.last().is_some_and(|r| r.iter().all(|c| c.is_empty())) {
        rows.pop();
    }
    rows
}

/// Cells as tab-separated text (for editing a table).
pub fn to_tsv(cells: &[Vec<String>]) -> String {
    cells
        .iter()
        .map(|r| r.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pasted text as a table, if it looks like one: tab-separated (from a
/// spreadsheet) or a Markdown table. At most 200 rows and 40 columns.
pub fn table_from_text(text: &str) -> Option<Vec<Vec<String>>> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    let mut rows: Vec<Vec<String>> = if lines.iter().all(|l| l.trim_start().starts_with('|')) {
        lines
            .iter()
            .map(|l| {
                let t = l.trim();
                let t = t.strip_prefix('|').unwrap_or(t);
                let t = t.strip_suffix('|').unwrap_or(t);
                t.split('|')
                    .map(|c| c.trim().to_string())
                    .collect::<Vec<_>>()
            })
            // The |---|:--:| line under the header.
            .filter(|r: &Vec<String>| {
                !r.iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
            })
            .collect()
    } else if text.contains('\t') {
        parse_tsv(text)
    } else {
        return None;
    };
    rows.truncate(200);
    for r in &mut rows {
        r.truncate(40);
    }
    let cells: usize = rows.iter().map(|r| r.len()).sum();
    (cells >= 2).then_some(rows)
}

fn table_pieces(
    cells: &[Vec<String>],
    style: &TextStyle,
    g: &Geom,
    size: f64,
    seed: u32,
) -> Vec<Piece> {
    let lay = table_layout(cells, style, size);
    let [w, h] = lay.size();
    let (s, c) = g.rot.sin_cos();
    // Table-local (top-left origin) to frame units.
    let place = |x: f64, y: f64| {
        let (x, y) = (x - w * 0.5, y - h * 0.5);
        [g.center[0] + x * c - y * s, g.center[1] + x * s + y * c]
    };
    let [r, gg, bb, a] = style.color.to_le_bytes();
    let a = (a as u32 * style.opacity as u32) / 255;
    let strong = u32::from_le_bytes([r, gg, bb, a as u8]);
    let light = u32::from_le_bytes([r, gg, bb, (a * 2 / 5) as u8]);
    let line = |pts: Vec<[f64; 2]>, color: u32| Piece {
        pts,
        width: size * 0.07,
        brush: Brush::Marker,
        dash: Dash::Solid,
        color,
        bridges: vec![],
    };
    let mut out = Vec::new();
    // Inner lines first, so the frame and header rule draw over them.
    let mut x = 0.0;
    for cw in &lay.cols[..lay.cols.len() - 1] {
        x += cw;
        out.push(line(vec![place(x, 0.0), place(x, h)], light));
    }
    let mut y = 0.0;
    for (i, rh) in lay.rows[..lay.rows.len().saturating_sub(1)]
        .iter()
        .enumerate()
    {
        y += rh;
        let col = if i == 0 { strong } else { light };
        out.push(line(vec![place(0.0, y), place(w, y)], col));
    }
    out.push(line(
        vec![
            place(0.0, 0.0),
            place(w, 0.0),
            place(w, h),
            place(0.0, h),
            place(0.0, 0.0),
        ],
        strong,
    ));
    let mut y = 0.0;
    for (i, row) in cells.iter().enumerate() {
        let mut x = 0.0;
        for (j, cell) in row.iter().enumerate() {
            let cw = lay.cols[j];
            if !cell.trim().is_empty() {
                let b = text_box(cell, style, size);
                let x0 = match style.align {
                    Align::Left => x + lay.pad,
                    Align::Center => x + (cw - b[0]) * 0.5,
                    Align::Right => x + cw - lay.pad - b[0],
                };
                let cg = Geom {
                    center: place(x0 + b[0] * 0.5, y + lay.pad + b[1] * 0.5),
                    half: [b[0] * 0.5, b[1] * 0.5],
                    rot: g.rot,
                    pts: vec![],
                };
                let cseed = seed.wrapping_add((i * 131 + j) as u32);
                out.extend(text_pieces(cell, style, &cg, size, cseed));
            }
            x += cw;
        }
        y += lay.rows[i];
    }
    out
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
    fn pasted_tables_parse() {
        let t = table_from_text("a\tb\n1\t2\n\n").unwrap();
        assert_eq!(t, vec![vec!["a", "b"], vec!["1", "2"]]);
        let md = table_from_text("| x | y |\n|---|:-:|\n| 1 | 2 |").unwrap();
        assert_eq!(md, vec![vec!["x", "y"], vec!["1", "2"]]);
        assert!(table_from_text("just words").is_none());
        assert_eq!(parse_tsv(&to_tsv(&t)), t);
    }

    #[test]
    fn tables_and_pictures_make_pieces() {
        let st = TextStyle::default();
        let cells = vec![
            vec!["Name".to_string(), "Qty".into()],
            vec!["Pens".into(), "3".into()],
        ];
        let lay = table_layout(&cells, &st, 0.05);
        let [w, h] = lay.size();
        assert!(w > 0.0 && h > 0.0 && lay.rows.len() == 2 && lay.cols.len() == 2);
        let g = Geom {
            center: [0.5, 0.5],
            half: [w * 0.5, h * 0.5],
            rot: 0.0,
            pts: vec![],
        };
        let table = ObjData::Table {
            cells,
            style: st,
            geom: g.clone(),
            size: 0.05,
            seed: 1,
        };
        // 1 inner column line, 1 header rule, the frame, then the letters.
        assert!(table.pieces().len() > 3);
        let pic = ObjData::Image {
            id: 7,
            geom: g,
            opacity: 255,
        };
        let p = pic.pieces();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].pts.len(), 4);
        assert_eq!(p[0].color, 0);
        let mut objs = Objects::default();
        objs.add(Group {
            cell: CellAddr::new(0, 0, 0),
            data: pic,
            strokes: vec![3],
        });
        assert_eq!(objs.image_of.get(&3), Some(&(7, 255)));
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
