// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Shapes as ink: a shape and its style become pieces (outline lines,
//! hachure / zigzag lines, filled polygons, arrowheads) that the app stores
//! as ordinary strokes, so they zoom, erase, undo and replay like ink.
//!
//! Geometry is in any square frame (the app uses a cell's local units). A box
//! shape is `center` + `half` size, rotated by `rot`; lines and arrows are a
//! list of points. Hand-drawn styles add seeded wobble, so a shape keeps the
//! same look every time it is regenerated (after a move, a resize, ...).

use std::f64::consts::{PI, TAU};

use ogpaper_core::{Brush, Dash};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShapeKind {
    #[default]
    Rect,
    Ellipse,
    Diamond,
    Triangle,
    Star,
    Polygon,
    Line,
    Arrow,
    /// Drawn with the pen: curves through points (see `subpaths`).
    Path,
    /// Shapes › Frame makes a frame (`ObjData::Frame`), not a shape.
    Frame,
    /// A symbol's pin (see `symbols`): where wires join it. Not drawn.
    Pin,
}

/// The kinds the Shapes panel offers (not pins).
pub const PICKABLE: usize = 10;

pub const SHAPES: [ShapeKind; 11] = [
    ShapeKind::Rect,
    ShapeKind::Ellipse,
    ShapeKind::Diamond,
    ShapeKind::Triangle,
    ShapeKind::Star,
    ShapeKind::Polygon,
    ShapeKind::Line,
    ShapeKind::Arrow,
    ShapeKind::Path,
    ShapeKind::Frame,
    ShapeKind::Pin,
];

impl ShapeKind {
    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::Star => "Star",
            ShapeKind::Polygon => "Polygon",
            ShapeKind::Line => "Line",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::Path => "Pen (curves)",
            ShapeKind::Frame => "Frame",
            ShapeKind::Pin => "Pin",
        }
    }

    /// Lines and arrows are drawn from point to point, not in a box.
    pub fn is_linear(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow)
    }

    pub fn from_u8(v: u8) -> Self {
        SHAPES.get(v as usize).copied().unwrap_or_default()
    }

    /// Whether a shape of this kind can become `other` (keeping its points
    /// or its box): lines with lines, boxes with boxes, pen paths alone.
    pub fn same_family(self, other: ShapeKind) -> bool {
        let fam = |k: ShapeKind| match k {
            ShapeKind::Line | ShapeKind::Arrow => 1,
            ShapeKind::Path => 2,
            _ => 0,
        };
        fam(self) == fam(other)
    }
}

/// Pen path points (see `subpaths`) through `anchors`: straight, or a
/// smooth curve through each; closed back to the first if `closed`.
pub fn path_through(anchors: &[[f64; 2]], smooth: bool, closed: bool) -> Vec<[f64; 2]> {
    let n = anchors.len();
    if n < 2 {
        return anchors.to_vec();
    }
    let at = |i: isize| -> [f64; 2] {
        if closed {
            anchors[i.rem_euclid(n as isize) as usize]
        } else {
            anchors[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let segs = if closed { n } else { n - 1 };
    let mut pts = vec![anchors[0]];
    for i in 0..segs as isize {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let (c1, c2) = if smooth {
            (
                [p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0],
                [p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0],
            )
        } else {
            (p1, p2)
        };
        pts.extend([c1, c2, p2]);
    }
    pts
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FillStyle {
    #[default]
    None,
    Hachure,
    CrossHatch,
    Zigzag,
    Solid,
    /// From the fill color to the end color, across (`ShapeStyle::grad`).
    Linear,
    /// From the fill color in the middle to the end color at the edge.
    Radial,
}

pub const FILLS: [FillStyle; 7] = [
    FillStyle::None,
    FillStyle::Hachure,
    FillStyle::CrossHatch,
    FillStyle::Zigzag,
    FillStyle::Solid,
    FillStyle::Linear,
    FillStyle::Radial,
];

impl FillStyle {
    pub fn name(self) -> &'static str {
        match self {
            FillStyle::None => "No fill",
            FillStyle::Hachure => "Hachure",
            FillStyle::CrossHatch => "Cross-hatch",
            FillStyle::Zigzag => "Zigzag",
            FillStyle::Solid => "Solid",
            FillStyle::Linear => "Gradient",
            FillStyle::Radial => "Radial gradient",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        FILLS.get(v as usize).copied().unwrap_or_default()
    }

    pub fn gradient(self) -> bool {
        matches!(self, FillStyle::Linear | FillStyle::Radial)
    }
}

/// How hand-drawn lines look (Excalidraw's "sloppiness").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sloppiness {
    /// Clean, exact lines.
    #[default]
    Architect,
    /// Lively: each line drawn twice with a little wobble.
    Artist,
    /// Cartoon: big wobble and overshoot.
    Cartoonist,
}

pub const SLOPPINESS: [Sloppiness; 3] = [
    Sloppiness::Architect,
    Sloppiness::Artist,
    Sloppiness::Cartoonist,
];

impl Sloppiness {
    pub fn name(self) -> &'static str {
        match self {
            Sloppiness::Architect => "Architect",
            Sloppiness::Artist => "Artist",
            Sloppiness::Cartoonist => "Cartoonist",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        SLOPPINESS.get(v as usize).copied().unwrap_or_default()
    }
    /// Wobble as a fraction of the shape's size, and passes per line.
    fn amount(self) -> (f64, usize) {
        match self {
            Sloppiness::Architect => (0.0, 1),
            Sloppiness::Artist => (0.012, 2),
            Sloppiness::Cartoonist => (0.035, 2),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Head {
    #[default]
    None,
    Arrow,
    Bar,
    Dot,
    Circle,
    Triangle,
    TriangleOutline,
    Diamond,
    DiamondOutline,
}

pub const HEADS: [Head; 9] = [
    Head::None,
    Head::Arrow,
    Head::Bar,
    Head::Dot,
    Head::Circle,
    Head::Triangle,
    Head::TriangleOutline,
    Head::Diamond,
    Head::DiamondOutline,
];

impl Head {
    pub fn name(self) -> &'static str {
        match self {
            Head::None => "None",
            Head::Arrow => "Arrow",
            Head::Bar => "Bar",
            Head::Dot => "Dot",
            Head::Circle => "Circle",
            Head::Triangle => "Triangle",
            Head::TriangleOutline => "Triangle (outline)",
            Head::Diamond => "Diamond",
            Head::DiamondOutline => "Diamond (outline)",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        HEADS.get(v as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ArrowType {
    #[default]
    Straight,
    Curved,
    Elbow,
}

pub const ARROW_TYPES: [ArrowType; 3] = [ArrowType::Straight, ArrowType::Curved, ArrowType::Elbow];

impl ArrowType {
    pub fn name(self) -> &'static str {
        match self {
            ArrowType::Straight => "Straight",
            ArrowType::Curved => "Curved",
            ArrowType::Elbow => "Elbow",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        ARROW_TYPES.get(v as usize).copied().unwrap_or_default()
    }
}

/// Everything about how a shape looks. Colors are RGBA8 (straight alpha,
/// alpha = opacity is applied separately).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShapeStyle {
    pub kind: ShapeKind,
    pub stroke: u32,
    pub fill: u32,
    pub fill_style: FillStyle,
    pub dash: Dash,
    pub sloppiness: Sloppiness,
    /// Rounded corners.
    pub round: bool,
    /// Polygon sides / star points.
    pub sides: u8,
    pub start: Head,
    pub end: Head,
    pub arrow: ArrowType,
    /// 0..=255.
    pub opacity: u8,
    /// Gradients: the end color, and a linear one's direction (degrees,
    /// 0 = left to right, 90 = top to bottom).
    pub fill2: u32,
    pub grad: f32,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            kind: ShapeKind::Rect,
            stroke: rgba(28, 28, 36, 255),
            fill: rgba(255, 201, 120, 255),
            fill_style: FillStyle::None,
            dash: Dash::Solid,
            sloppiness: Sloppiness::Artist,
            round: true,
            sides: 5,
            start: Head::None,
            end: Head::Arrow,
            arrow: ArrowType::Straight,
            opacity: 255,
            fill2: rgba(120, 90, 220, 255),
            grad: 90.0,
        }
    }
}

/// `a` to `b` by `t` (RGBA8).
fn mix(a: u32, b: u32, t: f64) -> u32 {
    let (a, b) = (a.to_le_bytes(), b.to_le_bytes());
    let c: [u8; 4] =
        std::array::from_fn(|i| (a[i] as f64 + (b[i] as f64 - a[i] as f64) * t).round() as u8);
    u32::from_le_bytes(c)
}

/// Gradient bands: the closed `contours` cut into strips (linear, at
/// `angle_deg`) or rings (radial), each filled with its color from
/// `from` to `to`. Each band reaches a little into the next, so no seam
/// shows between them.
pub fn gradient_bands(
    contours: &[Vec<[f64; 2]>],
    from: u32,
    to: u32,
    radial: bool,
    angle_deg: f32,
) -> Vec<Piece> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::core::overlay_rule::OverlayRule;
    use i_overlay::float::single::SingleFloatOverlay;
    const BANDS: usize = 40;
    let all: Vec<[f64; 2]> = contours.iter().flatten().copied().collect();
    if all.len() < 3 {
        return vec![];
    }
    let (c, half) = bounds_of(&all);
    let reach = half[0].hypot(half[1]) * 1.01 + 1e-12;
    let mut out = Vec::new();
    for i in 0..BANDS {
        let (t0, t1) = (i as f64 / BANDS as f64, (i + 1) as f64 / BANDS as f64);
        let t1 = (t1 + 0.4 / BANDS as f64).min(1.0);
        let band: Vec<Vec<[f64; 2]>> = if radial {
            let ring = |r: f64| -> Vec<[f64; 2]> {
                (0..72)
                    .map(|k| {
                        let a = k as f64 / 72.0 * TAU;
                        [c[0] + r * a.cos(), c[1] + r * a.sin()]
                    })
                    .collect()
            };
            let mut b = vec![ring(reach * t1.max(1e-9))];
            if i > 0 {
                b.push(ring(reach * t0).into_iter().rev().collect());
            }
            if i == BANDS - 1 {
                b[0] = ring(reach * 1.5);
            }
            b
        } else {
            let a = (angle_deg as f64).to_radians();
            let (d, n) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
            let at = |t: f64, s: f64| {
                let u = -reach + 2.0 * reach * t;
                [c[0] + d[0] * u + n[0] * s, c[1] + d[1] * u + n[1] * s]
            };
            let (t0, t1) = (
                if i == 0 { -0.1 } else { t0 },
                if i == BANDS - 1 { 1.1 } else { t1 },
            );
            vec![vec![
                at(t0, -reach),
                at(t1, -reach),
                at(t1, reach),
                at(t0, reach),
            ]]
        };
        let parts: Vec<Vec<[f64; 2]>> = contours
            .to_vec()
            .overlay(&band, OverlayRule::Intersect, FillRule::EvenOdd)
            .into_iter()
            .flatten()
            .filter(|p| p.len() >= 3)
            .collect();
        if parts.is_empty() {
            continue;
        }
        let (pts, bridges) = crate::font::join(parts);
        out.push(Piece {
            pts,
            width: 0.0,
            brush: Brush::Fill,
            dash: Dash::Solid,
            color: mix(from, to, (i as f64 + 0.5) / BANDS as f64),
            bridges,
        });
    }
    out
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

/// Where a shape is, in frame units.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Geom {
    pub center: [f64; 2],
    /// Half width and height before rotation.
    pub half: [f64; 2],
    pub rot: f64,
    /// Lines and arrows: their points (absolute, frame units).
    pub pts: Vec<[f64; 2]>,
}

/// One stroke to store.
#[derive(Clone, Debug)]
pub struct Piece {
    pub pts: Vec<[f64; 2]>,
    pub width: f64,
    pub brush: Brush,
    pub dash: Dash,
    pub color: u32,
    /// Fill only: points whose incoming edge just joins two contours (a
    /// letter's hole or second part), drawn without an edge.
    pub bridges: Vec<u32>,
}

fn with_opacity(c: u32, o: u8) -> u32 {
    let [r, g, b, a] = c.to_le_bytes();
    u32::from_le_bytes([r, g, b, ((a as u32 * o as u32) / 255) as u8])
}

/// Small seeded random numbers (xorshift), so wobble is repeatable.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self((seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    /// In [-1, 1).
    pub fn f_pub(&mut self) -> f64 {
        self.f()
    }

    fn f(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        v as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] * k, a[1] * k]
}
fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// The closed outline of a box shape, in its own unrotated frame centred on
/// the origin (before rounding).
fn base_outline(kind: ShapeKind, half: [f64; 2], sides: u8) -> Vec<[f64; 2]> {
    let [w, h] = half;
    match kind {
        ShapeKind::Rect | ShapeKind::Frame | ShapeKind::Pin => {
            vec![[-w, -h], [w, -h], [w, h], [-w, h]]
        }
        ShapeKind::Diamond => vec![[0.0, -h], [w, 0.0], [0.0, h], [-w, 0.0]],
        ShapeKind::Triangle => vec![[0.0, -h], [w, h], [-w, h]],
        ShapeKind::Ellipse => (0..96)
            .map(|i| {
                let a = i as f64 / 96.0 * TAU;
                [w * a.cos(), h * a.sin()]
            })
            .collect(),
        ShapeKind::Star => {
            let n = sides.clamp(3, 16) as usize;
            (0..2 * n)
                .map(|i| {
                    let a = -PI / 2.0 + i as f64 * PI / n as f64;
                    let k = if i % 2 == 0 { 1.0 } else { 0.45 };
                    [w * k * a.cos(), h * k * a.sin()]
                })
                .collect()
        }
        ShapeKind::Polygon => {
            let n = sides.clamp(3, 16) as usize;
            (0..n)
                .map(|i| {
                    let a = -PI / 2.0 + i as f64 * TAU / n as f64;
                    [w * a.cos(), h * a.sin()]
                })
                .collect()
        }
        ShapeKind::Line | ShapeKind::Arrow | ShapeKind::Path => vec![],
    }
}

/// A pen path's parts, flattened: each a polyline and whether it closes.
///
/// `pts` are anchors, each followed by its two curve controls towards the
/// next anchor (cubic Béziers): a, c1, c2, a, c1, c2, a. A part closes when
/// an anchor lands back on the part's first one; the segment after that
/// only jumps to the next part's first anchor.
pub fn subpaths(pts: &[[f64; 2]]) -> Vec<(Vec<[f64; 2]>, bool)> {
    let mut out = Vec::new();
    if pts.is_empty() {
        return out;
    }
    let mut cur = vec![pts[0]];
    let mut start = pts[0];
    let mut segs = 0;
    let mut i = 0;
    while i + 3 < pts.len() {
        let (a, c1, c2, b) = (pts[i], pts[i + 1], pts[i + 2], pts[i + 3]);
        if cur.is_empty() {
            // The jump to the next part.
            cur.push(b);
            start = b;
            segs = 0;
            i += 3;
            continue;
        }
        let straight = c1 == a && c2 == b;
        if straight {
            cur.push(b);
        } else {
            let n = 16;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                let u = 1.0 - t;
                let f = |j: usize| {
                    u * u * u * a[j]
                        + 3.0 * u * u * t * c1[j]
                        + 3.0 * u * t * t * c2[j]
                        + t * t * t * b[j]
                };
                cur.push([f(0), f(1)]);
            }
        }
        segs += 1;
        if b == start && segs >= 2 {
            cur.pop();
            out.push((std::mem::take(&mut cur), true));
        }
        i += 3;
    }
    if cur.len() >= 2 {
        out.push((cur, false));
    }
    out
}

/// A pen path from closed polygons (straight segments; see `subpaths`).
pub fn path_of_polygons(polys: &[Vec<[f64; 2]>]) -> Vec<[f64; 2]> {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for poly in polys.iter().filter(|p| p.len() >= 3) {
        if let Some(&last) = pts.last() {
            // The jump: straight controls.
            pts.push(last);
            pts.push(poly[0]);
        }
        pts.push(poly[0]);
        for w in poly
            .windows(2)
            .chain(std::iter::once(&[poly[poly.len() - 1], poly[0]][..]))
        {
            pts.push(w[0]);
            pts.push(w[1]);
            pts.push(w[1]);
        }
    }
    pts
}

/// The box around points: center and half size.
pub fn bounds_of(pts: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if lo[0] > hi[0] {
        return ([0.0; 2], [0.0; 2]);
    }
    (
        [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5],
        [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5],
    )
}

/// Replace each corner of a closed polygon by a circular-ish arc.
fn round_corners(poly: &[[f64; 2]], frac: f64) -> Vec<[f64; 2]> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let prev = poly[(i + n - 1) % n];
        let cur = poly[i];
        let next = poly[(i + 1) % n];
        let (a, b) = (sub(prev, cur), sub(next, cur));
        let r = (len(a).min(len(b)) * frac).max(0.0);
        let p0 = add(cur, mul(a, r / len(a).max(1e-12)));
        let p1 = add(cur, mul(b, r / len(b).max(1e-12)));
        // Quadratic curve p0 -> cur -> p1.
        for k in 0..=6 {
            let t = k as f64 / 6.0;
            let q = add(
                add(
                    mul(p0, (1.0 - t) * (1.0 - t)),
                    mul(cur, 2.0 * t * (1.0 - t)),
                ),
                mul(p1, t * t),
            );
            out.push(q);
        }
    }
    out
}

/// A shape's closed outline in frame units (rotated and placed). A pen
/// path's is its first closed part.
pub fn outline(st: &ShapeStyle, g: &Geom) -> Vec<[f64; 2]> {
    if st.kind == ShapeKind::Path {
        return subpaths(&g.pts)
            .into_iter()
            .find(|p| p.1)
            .map(|p| p.0)
            .unwrap_or_default();
    }
    let mut poly = base_outline(st.kind, g.half, st.sides);
    if st.round && !matches!(st.kind, ShapeKind::Ellipse) {
        poly = round_corners(&poly, 0.22);
    }
    let (s, c) = g.rot.sin_cos();
    poly.iter()
        .map(|p| {
            [
                g.center[0] + p[0] * c - p[1] * s,
                g.center[1] + p[0] * s + p[1] * c,
            ]
        })
        .collect()
}

/// The path of a line or arrow, by arrow type.
pub fn line_path(st: &ShapeStyle, g: &Geom) -> Vec<[f64; 2]> {
    let pts = &g.pts;
    if pts.len() < 2 {
        return pts.clone();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    if pts.len() > 2 {
        return match st.arrow {
            ArrowType::Straight => pts.clone(),
            // Through every joint: a Catmull-Rom curve.
            ArrowType::Curved => {
                let n = pts.len();
                let at = |i: isize| pts[i.clamp(0, n as isize - 1) as usize];
                let mut out = vec![pts[0]];
                for i in 0..n - 1 {
                    let (p0, p1, p2, p3) = (
                        at(i as isize - 1),
                        at(i as isize),
                        at(i as isize + 1),
                        at(i as isize + 2),
                    );
                    for k in 1..=12 {
                        let t = k as f64 / 12.0;
                        let (t2, t3) = (t * t, t * t * t);
                        let f = |a: f64, b: f64, c: f64, d: f64| {
                            0.5 * (2.0 * b
                                + (-a + c) * t
                                + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                                + (-a + 3.0 * b - 3.0 * c + d) * t3)
                        };
                        out.push([f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]);
                    }
                }
                out
            }
            // Square corners between each pair of points.
            ArrowType::Elbow => {
                let mut out = vec![pts[0]];
                for w in pts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let corner = if (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() {
                        [b[0], a[1]]
                    } else {
                        [a[0], b[1]]
                    };
                    out.push(corner);
                    out.push(b);
                }
                out
            }
        };
    }
    match st.arrow {
        ArrowType::Straight => pts.clone(),
        ArrowType::Curved => {
            let m = lerp(a, b, 0.5);
            let d = sub(b, a);
            let ctrl = add(m, mul([-d[1], d[0]], 0.25));
            (0..=32)
                .map(|i| {
                    let t = i as f64 / 32.0;
                    add(
                        add(
                            mul(a, (1.0 - t) * (1.0 - t)),
                            mul(ctrl, 2.0 * t * (1.0 - t)),
                        ),
                        mul(b, t * t),
                    )
                })
                .collect()
        }
        ArrowType::Elbow => {
            let m = lerp(a, b, 0.5);
            if (b[0] - a[0]).abs() >= (b[1] - a[1]).abs() {
                vec![a, [m[0], a[1]], [m[0], b[1]], b]
            } else {
                vec![a, [a[0], m[1]], [b[0], m[1]], b]
            }
        }
    }
}

/// The size used to scale wobble: the shape's larger side.
fn size_of(pts: &[[f64; 2]]) -> f64 {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pts {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-12)
}

/// Hand-drawn passes over a polyline: subdivided, nudged sideways by smooth
/// noise, closed shapes overshooting their start a little.
fn rough(
    pts: &[[f64; 2]],
    closed: bool,
    amp: f64,
    passes: usize,
    rng: &mut Rng,
) -> Vec<Vec<[f64; 2]>> {
    if amp <= 0.0 || pts.len() < 2 {
        let mut p = pts.to_vec();
        if closed && !p.is_empty() {
            p.push(p[0]);
        }
        return vec![p];
    }
    // Subdivide so the wobble has room to bend.
    let mut path = pts.to_vec();
    if closed {
        path.push(pts[0]);
    }
    let total: f64 = path.windows(2).map(|w| len(sub(w[1], w[0]))).sum();
    let step = total / 48.0;
    let mut dense = vec![path[0]];
    for w in path.windows(2) {
        let n = ((len(sub(w[1], w[0])) / step).ceil() as usize).clamp(1, 64);
        for k in 1..=n {
            dense.push(lerp(w[0], w[1], k as f64 / n as f64));
        }
    }
    let mut out = Vec::new();
    for _ in 0..passes {
        // Smooth noise: random knots every few points, interpolated.
        let every = 6;
        let knots: Vec<f64> = (0..dense.len() / every + 2)
            .map(|_| rng.f() * amp)
            .collect();
        let mut p: Vec<[f64; 2]> = dense
            .iter()
            .enumerate()
            .map(|(i, &q)| {
                let k = i / every;
                let t = (i % every) as f64 / every as f64;
                let off = knots[k] * (1.0 - t) + knots[k + 1] * t;
                let a = dense[i.saturating_sub(1)];
                let b = dense[(i + 1).min(dense.len() - 1)];
                let d = sub(b, a);
                let l = len(d).max(1e-12);
                add(q, mul([-d[1] / l, d[0] / l], off))
            })
            .collect();
        if closed {
            // Overshoot past the start, like a quick hand closing a loop.
            let extra = (dense.len() / 20).max(2);
            for i in 1..=extra {
                let q = p[i];
                p.push(add(q, [rng.f() * amp * 0.3, rng.f() * amp * 0.3]));
            }
        } else {
            // Ends land a little off.
            let n = p.len();
            p[0] = add(p[0], [rng.f() * amp * 0.5, rng.f() * amp * 0.5]);
            p[n - 1] = add(p[n - 1], [rng.f() * amp * 0.5, rng.f() * amp * 0.5]);
        }
        out.push(p);
    }
    out
}

/// Parallel lines at `angle` clipped to a polygon (even-odd), `gap` apart.
fn hatch(poly: &[[f64; 2]], angle: f64, gap: f64) -> Vec<[[f64; 2]; 2]> {
    let (s, c) = angle.sin_cos();
    // Rotate the polygon so the hatch lines are horizontal.
    let rp: Vec<[f64; 2]> = poly
        .iter()
        .map(|p| [p[0] * c + p[1] * s, -p[0] * s + p[1] * c])
        .collect();
    let (y0, y1) = rp.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p[1]), hi.max(p[1]))
    });
    let mut out = Vec::new();
    let n = rp.len();
    let mut y = y0 + gap * 0.5;
    let mut guard = 0;
    while y < y1 && guard < 2000 {
        guard += 1;
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let (a, b) = (rp[i], rp[(i + 1) % n]);
            if (a[1] > y) != (b[1] > y) {
                xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            let back = |x: f64| [x * c - y * s, x * s + y * c];
            out.push([back(pair[0]), back(pair[1])]);
        }
        y += gap;
    }
    out
}

/// All the strokes for a shape whose outline is `width` thick (frame units).
pub fn pieces(st: &ShapeStyle, g: &Geom, width: f64, seed: u32) -> Vec<Piece> {
    let mut rng = Rng::new(seed);
    let stroke = with_opacity(st.stroke, st.opacity);
    let fill = with_opacity(st.fill, st.opacity);
    let mut out = Vec::new();
    let line = |pts: Vec<[f64; 2]>, w: f64, dash: Dash, color: u32| Piece {
        pts,
        width: w,
        brush: Brush::Marker,
        dash,
        color,
        bridges: vec![],
    };

    if st.kind.is_linear() {
        let path = line_path(st, g);
        if path.len() < 2 {
            return out;
        }
        let size = size_of(&path);
        let (amp, passes) = st.sloppiness.amount();
        let head = (width * 4.0 + size * 0.04).min(size * 0.35);
        // Shorten the line under solid / outline heads so it does not poke through.
        let mut body = path.clone();
        let n = body.len();
        if st.kind == ShapeKind::Arrow {
            if shortens(st.end) {
                body[n - 1] = towards(body[n - 1], body[n - 2], head * 0.8);
            }
            if shortens(st.start) {
                body[0] = towards(body[0], body[1], head * 0.8);
            }
        }
        for p in rough(&body, false, amp * size, passes, &mut rng) {
            out.push(line(p, width, st.dash, stroke));
        }
        if st.kind == ShapeKind::Arrow {
            let n = path.len();
            arrowhead(
                &mut out,
                st.end,
                path[n - 1],
                path[n - 2],
                head,
                width,
                stroke,
            );
            arrowhead(&mut out, st.start, path[0], path[1], head, width, stroke);
        }
        return out;
    }

    if st.kind == ShapeKind::Pin {
        // An unseen dot: a fill with no color and no area.
        return vec![Piece {
            pts: vec![g.center; 3],
            width: 0.0,
            brush: Brush::Fill,
            dash: Dash::Solid,
            color: 0,
            bridges: vec![],
        }];
    }
    if st.kind == ShapeKind::Path {
        let parts = subpaths(&g.pts);
        let closed: Vec<Vec<[f64; 2]>> = parts
            .iter()
            .filter(|p| p.1 && p.0.len() >= 3)
            .map(|p| p.0.clone())
            .collect();
        let all: Vec<[f64; 2]> = parts.iter().flat_map(|p| p.0.iter().copied()).collect();
        if all.len() < 2 {
            return out;
        }
        let size = size_of(&all);
        let (amp, passes) = st.sloppiness.amount();
        if !closed.is_empty() && st.fill_style.gradient() {
            out.extend(gradient_bands(
                &closed,
                fill,
                with_opacity(st.fill2, st.opacity),
                st.fill_style == FillStyle::Radial,
                st.grad,
            ));
        } else if !closed.is_empty() && st.fill_style != FillStyle::None {
            if st.fill_style == FillStyle::Solid || closed.len() > 1 {
                // Holes and separate parts: one fill, joined by bridges.
                let (pts, bridges) = crate::font::join(closed.clone());
                out.push(Piece {
                    pts,
                    width: 0.0,
                    brush: Brush::Fill,
                    dash: Dash::Solid,
                    color: fill,
                    bridges,
                });
            } else {
                let gap = (width * 5.0).max(size * 0.03);
                let mut lines = hatch(&closed[0], -0.716, gap);
                if st.fill_style == FillStyle::CrossHatch {
                    lines.extend(hatch(&closed[0], -0.716 + PI / 2.0, gap));
                }
                for l in lines {
                    for p in rough(&l, false, amp * size * 0.3, 1, &mut rng) {
                        out.push(line(p, width * 0.6, Dash::Solid, fill));
                    }
                }
            }
        }
        for (p, c) in &parts {
            for q in rough(p, *c, amp * size, passes, &mut rng) {
                out.push(line(q, width, st.dash, stroke));
            }
        }
        return out;
    }

    let poly = outline(st, g);
    if poly.len() < 3 {
        return out;
    }
    let size = size_of(&poly);
    let (amp, passes) = st.sloppiness.amount();
    let gap = (width * 5.0).max(size * 0.03);
    // Hachure leans like Excalidraw's (about -41 degrees).
    let angle = -0.716 + g.rot;
    match st.fill_style {
        FillStyle::None => {}
        FillStyle::Solid => out.push(Piece {
            pts: poly.clone(),
            width: 0.0,
            brush: Brush::Fill,
            dash: Dash::Solid,
            color: fill,
            bridges: vec![],
        }),
        FillStyle::Linear | FillStyle::Radial => out.extend(gradient_bands(
            std::slice::from_ref(&poly),
            fill,
            with_opacity(st.fill2, st.opacity),
            st.fill_style == FillStyle::Radial,
            st.grad + g.rot.to_degrees() as f32,
        )),
        FillStyle::Hachure | FillStyle::CrossHatch => {
            let mut lines = hatch(&poly, angle, gap);
            if st.fill_style == FillStyle::CrossHatch {
                lines.extend(hatch(&poly, angle + PI / 2.0, gap));
            }
            for l in lines {
                for p in rough(&l, false, amp * size * 0.3, 1, &mut rng) {
                    out.push(line(p, width * 0.6, Dash::Solid, fill));
                }
            }
        }
        FillStyle::Zigzag => {
            let lines = hatch(&poly, angle, gap * 0.9);
            let mut zig = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                if i % 2 == 0 {
                    zig.extend_from_slice(l);
                } else {
                    zig.push(l[1]);
                    zig.push(l[0]);
                }
            }
            if zig.len() >= 2 {
                for p in rough(&zig, false, amp * size * 0.3, 1, &mut rng) {
                    out.push(line(p, width * 0.6, Dash::Solid, fill));
                }
            }
        }
    }
    for p in rough(&poly, true, amp * size, passes, &mut rng) {
        out.push(line(p, width, st.dash, stroke));
    }
    out
}

fn shortens(h: Head) -> bool {
    matches!(
        h,
        Head::Triangle
            | Head::TriangleOutline
            | Head::Diamond
            | Head::DiamondOutline
            | Head::Circle
    )
}

/// `from` moved `d` towards `to`.
fn towards(from: [f64; 2], to: [f64; 2], d: f64) -> [f64; 2] {
    let v = sub(to, from);
    let l = len(v).max(1e-12);
    add(from, mul(v, (d / l).min(0.45)))
}

/// An arrowhead at `tip`, pointing away from `from`, `size` long.
fn arrowhead(
    out: &mut Vec<Piece>,
    h: Head,
    tip: [f64; 2],
    from: [f64; 2],
    size: f64,
    width: f64,
    color: u32,
) {
    let v = sub(tip, from);
    let l = len(v).max(1e-12);
    let d = mul(v, 1.0 / l);
    let n = [-d[1], d[0]];
    let back = add(tip, mul(d, -size));
    let fill = |pts: Vec<[f64; 2]>| Piece {
        pts,
        width: 0.0,
        brush: Brush::Fill,
        dash: Dash::Solid,
        color,
        bridges: vec![],
    };
    let stroke = |pts: Vec<[f64; 2]>| Piece {
        pts,
        width,
        brush: Brush::Marker,
        dash: Dash::Solid,
        color,
        bridges: vec![],
    };
    let w = size * 0.5;
    match h {
        Head::None => {}
        Head::Arrow => {
            out.push(stroke(vec![
                add(back, mul(n, w)),
                tip,
                add(back, mul(n, -w)),
            ]));
        }
        Head::Bar => out.push(stroke(vec![add(tip, mul(n, w)), add(tip, mul(n, -w))])),
        Head::Dot | Head::Circle => {
            let c = add(tip, mul(d, -size * 0.35));
            let r = size * 0.35;
            let ring: Vec<[f64; 2]> = (0..=32)
                .map(|i| {
                    let a = i as f64 / 32.0 * TAU;
                    add(c, [r * a.cos(), r * a.sin()])
                })
                .collect();
            if h == Head::Dot {
                out.push(fill(ring[..32].to_vec()));
            }
            out.push(stroke(ring));
        }
        Head::Triangle | Head::TriangleOutline => {
            let tri = vec![tip, add(back, mul(n, w)), add(back, mul(n, -w))];
            if h == Head::Triangle {
                out.push(fill(tri.clone()));
            }
            let mut t = tri;
            t.push(t[0]);
            out.push(stroke(t));
        }
        Head::Diamond | Head::DiamondOutline => {
            let mid = add(tip, mul(d, -size * 0.5));
            let dia = vec![
                tip,
                add(mid, mul(n, w * 0.6)),
                back,
                add(mid, mul(n, -w * 0.6)),
            ];
            if h == Head::Diamond {
                out.push(fill(dia.clone()));
            }
            let mut t = dia;
            t.push(t[0]);
            out.push(stroke(t));
        }
    }
}

/// A box geometry spanning two corners (drag start and end), optionally
/// square (Shift) or from the centre (Alt).
pub fn box_from_drag(a: [f64; 2], b: [f64; 2], square: bool, centered: bool) -> Geom {
    let mut d = sub(b, a);
    if square {
        let m = d[0].abs().max(d[1].abs());
        d = [m * d[0].signum(), m * d[1].signum()];
    }
    let (center, half) = if centered {
        (a, [d[0].abs(), d[1].abs()])
    } else {
        (add(a, mul(d, 0.5)), [d[0].abs() * 0.5, d[1].abs() * 0.5])
    };
    Geom {
        center,
        half,
        rot: 0.0,
        pts: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom() -> Geom {
        Geom {
            center: [0.5, 0.5],
            half: [0.3, 0.2],
            rot: 0.3,
            pts: vec![[0.1, 0.1], [0.9, 0.6]],
        }
    }

    #[test]
    fn every_shape_and_fill_makes_ink() {
        for kind in SHAPES {
            for fill_style in FILLS {
                for sloppiness in SLOPPINESS {
                    let st = ShapeStyle {
                        kind,
                        fill_style,
                        sloppiness,
                        start: Head::Dot,
                        end: Head::Triangle,
                        ..Default::default()
                    };
                    let mut g = geom();
                    if kind == ShapeKind::Path {
                        g.pts = path_through(&[[0.1, 0.1], [0.9, 0.6], [0.3, 0.8]], true, true);
                    }
                    let p = pieces(&st, &g, 0.005, 7);
                    assert!(!p.is_empty(), "{kind:?} {fill_style:?}");
                    for piece in &p {
                        assert!(!piece.pts.is_empty());
                        assert!(piece
                            .pts
                            .iter()
                            .all(|q| q[0].is_finite() && q[1].is_finite()));
                    }
                    if !kind.is_linear() && fill_style == FillStyle::Solid {
                        assert!(p.iter().any(|q| q.brush == Brush::Fill));
                    }
                }
            }
        }
    }

    #[test]
    fn same_seed_same_wobble() {
        let st = ShapeStyle {
            sloppiness: Sloppiness::Cartoonist,
            ..Default::default()
        };
        let a = pieces(&st, &geom(), 0.005, 42);
        let b = pieces(&st, &geom(), 0.005, 42);
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].pts, b[0].pts);
    }

    #[test]
    fn hachure_stays_inside() {
        let sq = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let lines = hatch(&sq, -0.7, 0.1);
        assert!(lines.len() > 5);
        for l in lines {
            for p in l {
                assert!(p[0] > -1e-9 && p[0] < 1.0 + 1e-9 && p[1] > -1e-9 && p[1] < 1.0 + 1e-9);
            }
        }
    }

    #[test]
    fn gradients_fill_in_bands_from_one_color_to_the_other() {
        let st = ShapeStyle {
            fill_style: FillStyle::Linear,
            fill: rgba(255, 0, 0, 255),
            fill2: rgba(0, 0, 255, 255),
            grad: 0.0,
            sloppiness: Sloppiness::Architect,
            round: false,
            ..Default::default()
        };
        let g = Geom {
            center: [0.0, 0.0],
            half: [10.0, 5.0],
            ..Default::default()
        };
        let p = pieces(&st, &g, 0.1, 1);
        let fills: Vec<&Piece> = p.iter().filter(|q| q.brush == Brush::Fill).collect();
        assert!(fills.len() >= 20, "{}", fills.len());
        // Left to right: red first, blue last; inside the box.
        let first = fills[0].color.to_le_bytes();
        let last = fills[fills.len() - 1].color.to_le_bytes();
        assert!(first[0] > 200 && first[2] < 60, "{first:?}");
        assert!(last[2] > 200 && last[0] < 60, "{last:?}");
        assert!(fills.iter().all(|f| f
            .pts
            .iter()
            .all(|q| q[0].abs() <= 10.001 && q[1].abs() <= 5.001)));
        // Radial: the middle band is the fill color.
        let st = ShapeStyle {
            fill_style: FillStyle::Radial,
            ..st
        };
        let p = pieces(&st, &g, 0.1, 1);
        let mid = p
            .iter()
            .find(|q| q.brush == Brush::Fill)
            .unwrap()
            .color
            .to_le_bytes();
        assert!(mid[0] > 200, "{mid:?}");
    }
}
