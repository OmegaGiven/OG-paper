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
}

pub const SHAPES: [ShapeKind; 8] = [
    ShapeKind::Rect,
    ShapeKind::Ellipse,
    ShapeKind::Diamond,
    ShapeKind::Triangle,
    ShapeKind::Star,
    ShapeKind::Polygon,
    ShapeKind::Line,
    ShapeKind::Arrow,
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
        }
    }

    /// Lines and arrows are drawn from point to point, not in a box.
    pub fn is_linear(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow)
    }

    pub fn from_u8(v: u8) -> Self {
        SHAPES.get(v as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FillStyle {
    #[default]
    None,
    Hachure,
    CrossHatch,
    Zigzag,
    Solid,
}

pub const FILLS: [FillStyle; 5] = [
    FillStyle::None,
    FillStyle::Hachure,
    FillStyle::CrossHatch,
    FillStyle::Zigzag,
    FillStyle::Solid,
];

impl FillStyle {
    pub fn name(self) -> &'static str {
        match self {
            FillStyle::None => "No fill",
            FillStyle::Hachure => "Hachure",
            FillStyle::CrossHatch => "Cross-hatch",
            FillStyle::Zigzag => "Zigzag",
            FillStyle::Solid => "Solid",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        FILLS.get(v as usize).copied().unwrap_or_default()
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
        }
    }
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
        ShapeKind::Rect => vec![[-w, -h], [w, -h], [w, h], [-w, h]],
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
        ShapeKind::Line | ShapeKind::Arrow => vec![],
    }
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

/// A shape's closed outline in frame units (rotated and placed).
pub fn outline(st: &ShapeStyle, g: &Geom) -> Vec<[f64; 2]> {
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
                    let p = pieces(&st, &geom(), 0.005, 7);
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
}
