// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Fonts for the Text tool, all turned into ink so text stays sharp at any
//! zoom and erases, undoes and replays like everything else:
//!
//! - **Outline fonts** (TrueType / OpenType, bundled or your own): each
//!   letter's outline becomes one filled polygon.
//! - **Single-line fonts**: a small built-in stroke font (pen-plotter style)
//!   whose glyphs sit on a 4 x 6 grid (cap height 6, x-height 4, descenders
//!   to 8), y down. Fonts are known by name in a registry; text stores the
//!   name, so a canvas still shows (and edits) its text wherever it opens.
//!
//! Layout works in "grid units" where the cap height is 6 for every font.

use std::sync::{Arc, Mutex, OnceLock};

use crate::shapes::Rng;

/// A font's place in the registry.
pub type FontId = u16;

/// How a single-line font draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StrokeKind {
    /// Clean, proportional.
    Clean,
    /// Slanted and wobbly, like quick handwriting.
    Hand,
    /// Fixed width.
    Mono,
}

#[derive(Clone)]
enum Source {
    Stroke(StrokeKind),
    Outline(Arc<Vec<u8>>),
    /// Known by name (bundled and still loading, or used by a canvas but
    /// not on this device).
    Missing,
}

#[derive(Clone)]
struct Entry {
    name: String,
    category: String,
    source: Source,
    /// Added by the user.
    user: bool,
}

/// What the font picker shows.
#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo {
    pub id: FontId,
    pub name: String,
    pub category: String,
    pub available: bool,
    pub outline: bool,
    pub user: bool,
}

/// Fonts that ship with the app: name, file in `web/app/fonts/`, style.
pub const BUNDLED: [(&str, &str, &str); 11] = [
    (
        "Architects Daughter",
        "ArchitectsDaughter.ttf",
        "Hand-drawn",
    ),
    ("Patrick Hand", "PatrickHand.ttf", "Hand-drawn"),
    ("Indie Flower", "IndieFlower.ttf", "Hand-drawn"),
    ("Permanent Marker", "PermanentMarker.ttf", "Marker"),
    ("Nunito", "Nunito.ttf", "Sans"),
    ("Comic Neue", "ComicNeue.ttf", "Sans"),
    ("Lora", "Lora.ttf", "Serif"),
    ("JetBrains Mono", "JetBrainsMono.ttf", "Mono"),
    ("Courier Prime", "CourierPrime.ttf", "Mono"),
    ("Bangers", "Bangers.ttf", "Display"),
    ("Pacifico", "Pacifico.ttf", "Display"),
];

/// The font new text starts with.
pub const DEFAULT_FONT: &str = "Architects Daughter";

/// The desktop app carries the bundled fonts inside; the web page loads them
/// from `fonts/` and registers them as they arrive.
#[cfg(not(target_arch = "wasm32"))]
fn bundled_bytes(file: &str) -> Option<&'static [u8]> {
    macro_rules! fonts {
        ($($f:literal),*) => {
            match file {
                $($f => Some(include_bytes!(concat!("../../../web/app/fonts/", $f)).as_slice()),)*
                _ => None,
            }
        };
    }
    fonts!(
        "ArchitectsDaughter.ttf",
        "PatrickHand.ttf",
        "IndieFlower.ttf",
        "PermanentMarker.ttf",
        "Nunito.ttf",
        "ComicNeue.ttf",
        "Lora.ttf",
        "JetBrainsMono.ttf",
        "CourierPrime.ttf",
        "Bangers.ttf",
        "Pacifico.ttf"
    )
}

#[cfg(target_arch = "wasm32")]
fn bundled_bytes(_file: &str) -> Option<&'static [u8]> {
    None
}

fn registry() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    static REG: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    REG.get_or_init(|| {
        // Ids 0-2 are the single-line fonts (older canvases refer to them by
        // these numbers).
        let mut v = vec![
            Entry {
                name: "Single-line".into(),
                category: "Single-line".into(),
                source: Source::Stroke(StrokeKind::Clean),
                user: false,
            },
            Entry {
                name: "Single-line hand".into(),
                category: "Single-line".into(),
                source: Source::Stroke(StrokeKind::Hand),
                user: false,
            },
            Entry {
                name: "Single-line mono".into(),
                category: "Single-line".into(),
                source: Source::Stroke(StrokeKind::Mono),
                user: false,
            },
        ];
        for (name, file, category) in BUNDLED {
            let source = match bundled_bytes(file) {
                Some(b) => Source::Outline(Arc::new(b.to_vec())),
                None => Source::Missing,
            };
            v.push(Entry {
                name: name.into(),
                category: category.into(),
                source,
                user: false,
            });
        }
        Mutex::new(v)
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

/// The family name stored in a font file.
pub fn family_name(bytes: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let mut best = None;
    for n in face.names() {
        let id = n.name_id;
        if id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY || id == ttf_parser::name_id::FAMILY {
            if let Some(s) = n.to_string() {
                if id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY || best.is_none() {
                    best = Some(s);
                }
            }
        }
    }
    best.filter(|s| !s.trim().is_empty())
}

/// Add (or replace) an outline font from a TrueType / OpenType file. With
/// no name, the file's own family name is used. Returns its id and name.
pub fn register(
    name: Option<&str>,
    category: &str,
    bytes: Vec<u8>,
    user: bool,
) -> Result<(FontId, String), String> {
    if ttf_parser::Face::parse(&bytes, 0).is_err() {
        return Err("not a TrueType / OpenType font (WOFF files are not supported yet)".into());
    }
    let name = match name {
        Some(n) => n.to_string(),
        None => family_name(&bytes).ok_or("the font has no name")?,
    };
    let mut reg = registry();
    let entry = Entry {
        name: name.clone(),
        category: category.into(),
        source: Source::Outline(Arc::new(bytes)),
        user,
    };
    let id = match reg.iter().position(|e| e.name == name) {
        Some(i) => {
            let keep_cat = reg[i].category.clone();
            reg[i] = Entry {
                category: if user {
                    entry.category.clone()
                } else {
                    keep_cat
                },
                ..entry
            };
            i
        }
        None => {
            reg.push(entry);
            reg.len() - 1
        }
    };
    Ok((id as FontId, name))
}

/// The id of a font by name (registering an unavailable placeholder for a
/// name this device has never seen).
pub fn id_of(name: &str) -> FontId {
    let mut reg = registry();
    if let Some(i) = reg.iter().position(|e| e.name == name) {
        return i as FontId;
    }
    reg.push(Entry {
        name: name.into(),
        category: "Not on this device".into(),
        source: Source::Missing,
        user: false,
    });
    (reg.len() - 1) as FontId
}

pub fn name_of(id: FontId) -> String {
    registry()
        .get(id as usize)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "Single-line".into())
}

pub fn default_font() -> FontId {
    id_of(DEFAULT_FONT)
}

pub fn list() -> Vec<FontInfo> {
    registry()
        .iter()
        .enumerate()
        .map(|(i, e)| FontInfo {
            id: i as FontId,
            name: e.name.clone(),
            category: e.category.clone(),
            available: !matches!(e.source, Source::Missing),
            outline: matches!(e.source, Source::Outline(_)),
            user: e.user,
        })
        .collect()
}

/// Outline fonts' bytes by name (to show font names in their own font).
pub fn outline_data(name: &str) -> Option<Arc<Vec<u8>>> {
    registry()
        .iter()
        .find(|e| e.name == name)
        .and_then(|e| match &e.source {
            Source::Outline(b) => Some(b.clone()),
            _ => None,
        })
}

/// What a font draws with, falling back when it is not available here.
fn source(id: FontId) -> Source {
    let reg = registry();
    match reg.get(id as usize).map(|e| &e.source) {
        Some(Source::Missing) | None => {
            // The default font if loaded, else the single-line font.
            reg.iter()
                .find(|e| e.name == DEFAULT_FONT)
                .map(|e| e.source.clone())
                .filter(|s| !matches!(s, Source::Missing))
                .unwrap_or(Source::Stroke(StrokeKind::Clean))
        }
        Some(s) => s.clone(),
    }
}

/// Whether text in this font is drawn as strokes (single-line) rather than
/// filled outlines.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn is_single_line(id: FontId) -> bool {
    matches!(source(id), Source::Stroke(_))
}

/// Laid-out ink, in grid units with the text box's top-left at the origin.
#[derive(Clone, Debug)]
pub enum Mark {
    /// A single-line stroke.
    Line(Vec<[f64; 2]>),
    /// A filled letter: its outline points, and the indexes of points whose
    /// incoming edge only joins one contour to the next (holes and separate
    /// parts share one polygon; those joins carry no ink).
    Fill(Vec<[f64; 2]>, Vec<u32>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

pub const ALIGNS: [Align; 3] = [Align::Left, Align::Center, Align::Right];

impl Align {
    pub fn from_u8(v: u8) -> Self {
        ALIGNS.get(v as usize).copied().unwrap_or_default()
    }
}

/// Lay out `text` (lines split on newlines) in `font`. Returns the ink and
/// the box size (grid units, cap height 6).
pub fn layout(text: &str, font: FontId, align: Align, seed: u32) -> (Vec<Mark>, [f64; 2]) {
    match source(font) {
        Source::Outline(bytes) => match ttf_parser::Face::parse(&bytes, 0) {
            Ok(face) => outline_layout(&face, text, align),
            Err(_) => stroke_layout(text, StrokeKind::Clean, align, seed),
        },
        Source::Stroke(k) => stroke_layout(text, k, align, seed),
        Source::Missing => stroke_layout(text, StrokeKind::Clean, align, seed),
    }
}

/// Most points a filled letter may have (the shader tests each pixel
/// against every edge).
pub const FILL_MAX_PTS: usize = 1024;

/// Collects a glyph's outline as polygons (curves flattened).
struct Contours {
    polys: Vec<Vec<[f64; 2]>>,
    cur: Vec<[f64; 2]>,
    /// Curve step: font units per segment.
    step: f64,
}

impl Contours {
    fn last(&self) -> [f64; 2] {
        *self.cur.last().unwrap_or(&[0.0, 0.0])
    }
    fn segs(&self, len: f64) -> usize {
        ((len / self.step).ceil() as usize).clamp(2, 16)
    }
}

impl ttf_parser::OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.cur.len() > 2 {
            self.polys.push(std::mem::take(&mut self.cur));
        }
        self.cur = vec![[x as f64, y as f64]];
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push([x as f64, y as f64]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last();
        let (p1, p2) = ([x1 as f64, y1 as f64], [x as f64, y as f64]);
        let n =
            self.segs((p1[0] - p0[0]).hypot(p1[1] - p0[1]) + (p2[0] - p1[0]).hypot(p2[1] - p1[1]));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            self.cur.push([
                u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0],
                u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1],
            ]);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last();
        let (p1, p2, p3) = (
            [x1 as f64, y1 as f64],
            [x2 as f64, y2 as f64],
            [x as f64, y as f64],
        );
        let len = (p1[0] - p0[0]).hypot(p1[1] - p0[1])
            + (p2[0] - p1[0]).hypot(p2[1] - p1[1])
            + (p3[0] - p2[0]).hypot(p3[1] - p2[1]);
        let n = self.segs(len);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.cur.push([
                a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
            ]);
        }
    }
    fn close(&mut self) {
        if self.cur.len() > 2 {
            self.polys.push(std::mem::take(&mut self.cur));
        }
        self.cur.clear();
    }
}

/// Join a glyph's contours into one polygon: each contour closed, then a
/// zero-width bridge to the next, and the bridges walked back at the end so
/// they cancel. Returns the points and the bridge indexes.
fn join(polys: Vec<Vec<[f64; 2]>>) -> (Vec<[f64; 2]>, Vec<u32>) {
    let mut pts = Vec::new();
    let mut bridges = Vec::new();
    let starts: Vec<[f64; 2]> = polys.iter().map(|p| p[0]).collect();
    for (k, poly) in polys.iter().enumerate() {
        if k > 0 {
            bridges.push(pts.len() as u32);
        }
        pts.extend_from_slice(poly);
        pts.push(poly[0]);
    }
    for k in (0..starts.len().saturating_sub(1)).rev() {
        bridges.push(pts.len() as u32);
        pts.push(starts[k]);
    }
    (pts, bridges)
}

fn outline_layout(face: &ttf_parser::Face, text: &str, align: Align) -> (Vec<Mark>, [f64; 2]) {
    let upm = face.units_per_em() as f64;
    let cap = face
        .capital_height()
        .filter(|&c| c > 0)
        .map(f64::from)
        .unwrap_or(upm * 0.7);
    let s = 6.0 / cap;
    let desc = (-(face.descender() as f64)).max(0.0) * s;
    let line_h =
        ((face.ascender() as f64 - face.descender() as f64 + face.line_gap() as f64) * s).max(7.0);
    let space = face
        .glyph_index(' ')
        .and_then(|g| face.glyph_hor_advance(g))
        .map(|a| a as f64 * s)
        .unwrap_or(2.5);
    let adv = |c: char| -> f64 {
        face.glyph_index(c)
            .and_then(|g| face.glyph_hor_advance(g))
            .map(|a| a as f64 * s)
            .unwrap_or(space)
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let widths: Vec<f64> = lines.iter().map(|l| l.chars().map(adv).sum()).collect();
    let box_w = widths.iter().cloned().fold(0.0, f64::max).max(1.0);
    let box_h = 6.0 + (lines.len() as f64 - 1.0) * line_h + desc;
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut x = match align {
            Align::Left => 0.0,
            Align::Center => (box_w - widths[i]) * 0.5,
            Align::Right => box_w - widths[i],
        };
        let base = 6.0 + i as f64 * line_h;
        for c in line.chars() {
            if let Some(g) = face.glyph_index(c) {
                let mut b = Contours {
                    polys: Vec::new(),
                    cur: Vec::new(),
                    step: upm * 0.03,
                };
                if face.outline_glyph(g, &mut b).is_some() {
                    if b.cur.len() > 2 {
                        let cur = std::mem::take(&mut b.cur);
                        b.polys.push(cur);
                    }
                    if !b.polys.is_empty() {
                        let (mut pts, mut bridges) = join(b.polys);
                        if pts.len() > FILL_MAX_PTS {
                            // Very detailed letters: keep every k-th point
                            // (and every contour join).
                            let k = pts.len().div_ceil(FILL_MAX_PTS);
                            let keep: Vec<usize> = (0..pts.len())
                                .filter(|&j| j % k == 0 || bridges.contains(&(j as u32)))
                                .collect();
                            let new_bridges = keep
                                .iter()
                                .enumerate()
                                .filter(|(_, &j)| bridges.contains(&(j as u32)))
                                .map(|(n, _)| n as u32)
                                .collect();
                            pts = keep.iter().map(|&j| pts[j]).collect();
                            bridges = new_bridges;
                            pts.truncate(FILL_MAX_PTS);
                        }
                        let pts = pts
                            .iter()
                            .map(|p| [x + p[0] * s, base - p[1] * s])
                            .collect();
                        out.push(Mark::Fill(pts, bridges));
                    }
                }
            }
            x += adv(c);
        }
    }
    (out, [box_w, box_h])
}

/// Grid units from one line's top to the next.
pub const LINE_UNITS: f64 = 11.0;

/// Horizontal extent of a glyph's ink (grid units).
fn ink_x(c: char) -> (f64, f64) {
    let g = glyph(c);
    let lo = g.iter().flatten().fold(f64::MAX, |m, p| m.min(p[0]));
    let hi = g.iter().flatten().fold(f64::MIN, |m, p| m.max(p[0]));
    if lo > hi {
        (0.0, 0.0)
    } else {
        (lo, hi)
    }
}

/// Horizontal advance of a glyph and how far to shift it left (grid units).
fn advance(c: char, font: StrokeKind) -> (f64, f64) {
    if font == StrokeKind::Mono {
        return (6.0, -1.0);
    }
    if c == ' ' {
        return (3.6, 0.0);
    }
    let (lo, hi) = ink_x(c);
    ((hi - lo + 1.6).max(1.6), lo)
}

/// Single-line layout: strokes in grid units, the box's top-left at the
/// origin. Returns the strokes and the box size.
fn stroke_layout(text: &str, font: StrokeKind, align: Align, seed: u32) -> (Vec<Mark>, [f64; 2]) {
    let lines: Vec<&str> = text.split('\n').collect();
    let widths: Vec<f64> = lines
        .iter()
        .map(|l| l.chars().map(|c| advance(c, font).0).sum::<f64>())
        .collect();
    let box_w = widths.iter().cloned().fold(0.0, f64::max).max(1.0);
    let box_h = LINE_UNITS * lines.len() as f64 - (LINE_UNITS - 8.0);
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut x = match align {
            Align::Left => 0.0,
            Align::Center => (box_w - widths[i]) * 0.5,
            Align::Right => box_w - widths[i],
        };
        let y = i as f64 * LINE_UNITS;
        for c in line.chars() {
            let (adv, shift) = advance(c, font);
            let dx = -shift;
            for stroke in glyph(c) {
                let pts = stroke
                    .iter()
                    .map(|p| {
                        let (mut px, mut py) = (x + dx + p[0], y + p[1]);
                        if font == StrokeKind::Hand {
                            // Slant and a small, repeatable wobble.
                            px += (6.0 - p[1]) * 0.16 + rng.f_pub() * 0.18;
                            py += rng.f_pub() * 0.18;
                        }
                        [px, py]
                    })
                    .collect();
                out.push(Mark::Line(pts));
            }
            x += adv;
        }
    }
    (out, [box_w, box_h])
}

/// Single-stroke glyphs on a 4 x 6 grid (y down).
pub fn glyph(c: char) -> Vec<Vec<[f64; 2]>> {
    let spec: &str = match c {
        'A' => "0,6 0,2 2,0 4,2 4,6;0,3.5 4,3.5",
        'B' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3;0,0 3,0 4,1 4,2 3,3",
        'C' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5",
        'D' => "0,0 0,6 2,6 4,4 4,2 2,0 0,0",
        'E' => "4,0 0,0 0,6 4,6;0,3 3,3",
        'F' => "4,0 0,0 0,6;0,3 3,3",
        'G' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5 4,3.5 2,3.5",
        'H' => "0,0 0,6;4,0 4,6;0,3 4,3",
        'I' => "1,0 3,0;2,0 2,6;1,6 3,6",
        'J' => "4,0 4,5 3,6 1,6 0,5",
        'K' => "0,0 0,6;4,0 0,4;1.3,3 4,6",
        'L' => "0,0 0,6 4,6",
        'M' => "0,6 0,0 2,3 4,0 4,6",
        'N' => "0,6 0,0 4,6 4,0",
        'O' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0",
        'P' => "0,6 0,0 3,0 4,1 4,2 3,3 0,3",
        'Q' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0;2.5,4.5 4,6",
        'R' => "0,6 0,0 3,0 4,1 4,2 3,3 0,3;2,3 4,6",
        'S' => "4,1 3,0 1,0 0,1 0,2 1,3 3,3 4,4 4,5 3,6 1,6 0,5",
        'T' => "0,0 4,0;2,0 2,6",
        'U' => "0,0 0,5 1,6 3,6 4,5 4,0",
        'V' => "0,0 2,6 4,0",
        'W' => "0,0 1,6 2,3 3,6 4,0",
        'X' => "0,0 4,6;4,0 0,6",
        'Y' => "0,0 2,3 4,0;2,3 2,6",
        'Z' => "0,0 4,0 0,6 4,6",
        '0' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0;0.5,5 3.5,1",
        '1' => "1,1 2,0 2,6;1,6 3,6",
        '2' => "0,1 1,0 3,0 4,1 4,2 0,6 4,6",
        '3' => "0,1 1,0 3,0 4,1 4,2 3,3 4,4 4,5 3,6 1,6 0,5;1.5,3 3,3",
        '4' => "3,6 3,0 0,4 4,4",
        '5' => "4,0 0,0 0,3 3,3 4,4 4,5 3,6 0,6",
        '6' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5 4,4 3,3 0,3",
        '7' => "0,0 4,0 1,6",
        '8' => "1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3 1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3",
        '9' => "4,3 1,3 0,2 0,1 1,0 3,0 4,1 4,5 3,6 1,6 0,5",
        '.' => "2,5.9 2,6",
        ',' => "2,5.5 2,6 1.4,7",
        '!' => "2,0 2,4;2,5.9 2,6",
        '?' => "0,1 1,0 3,0 4,1 4,2 2,3.5 2,4.2;2,5.9 2,6",
        '-' => "1,3 3,3",
        '\'' => "2,0 2,1.5",
        ':' => "2,1.9 2,2;2,4.9 2,5",
        '/' => "4,0 0,6",
        '>' => "0,0 4,3 0,6",
        '<' => "4,0 0,3 4,6",
        '(' => "3,0 2,1 1.5,2.5 1.5,3.5 2,5 3,6",
        ')' => "1,0 2,1 2.5,2.5 2.5,3.5 2,5 1,6",
        '+' => "2,1.5 2,4.5;0.5,3 3.5,3",
        '=' => "1,2 3,2;1,4 3,4",
        '×' => "1,2 3,4;3,2 1,4",
        'a' => "4,2 4,6;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'b' => "0,0 0,6;0,3 1,2 3,2 4,3 4,5 3,6 1,6 0,5",
        'c' => "4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'd' => "4,0 4,6;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'e' => "0,4 4,4 4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5.4",
        'f' => "3.6,0.4 3,0 2.2,0 1.4,0.8 1.4,6;0,2.4 3.2,2.4",
        'g' => "4,2 4,7 3,8 1,8 0.3,7.4;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'h' => "0,0 0,6;0,3 1,2 3,2 4,3 4,6",
        'i' => "2,2 2,6;2,0.3 2,0.6",
        'j' => "3,2 3,7 2.2,8 0.8,8;3,0.3 3,0.6",
        'k' => "0,0 0,6;3.8,2 0,4.6;1.3,3.7 4,6",
        'l' => "1.5,0 1.5,5.2 2.3,6 3.2,6",
        'm' => "0,6 0,2;0,3 0.7,2 1.4,2 2,3 2,6;2,3 2.6,2 3.3,2 4,3 4,6",
        'n' => "0,6 0,2;0,3 1,2 3,2 4,3 4,6",
        'o' => "1,2 3,2 4,3 4,5 3,6 1,6 0,5 0,3 1,2",
        'p' => "0,2 0,8;0,3 1,2 3,2 4,3 4,5 3,6 1,6 0,5",
        'q' => "4,2 4,8;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'r' => "0,6 0,2;0,3.6 1.6,2 3.6,2",
        's' => "4,2.6 3,2 1,2 0,2.8 1,3.9 3,4.1 4,5 3,6 1,6 0,5.4",
        't' => "1.5,0.5 1.5,5.2 2.3,6 3.6,6;0,2 3.4,2",
        'u' => "0,2 0,5 1,6 3,6 4,5;4,2 4,6",
        'v' => "0,2 2,6 4,2",
        'w' => "0,2 1,6 2,3 3,6 4,2",
        'x' => "0,2 4,6;4,2 0,6",
        'y' => "0,2 2,6.2;4,2 1,8 0.2,8",
        'z' => "0,2 4,2 0,6 4,6",
        ';' => "2,1.9 2,2.1;2,4.9 2,5.5 1.4,6.6",
        '"' => "1.2,0 1.2,1.6;2.8,0 2.8,1.6",
        '#' => "1.4,0.8 0.8,6;3.2,0.8 2.6,6;0,2.4 4,2.4;0,4.4 4,4.4",
        '*' => "2,1.4 2,4.6;0.6,2.2 3.4,3.8;3.4,2.2 0.6,3.8",
        '&' => "4,6 1.2,2.6 1.2,1 2,0 3,0.6 3,1.8 0,4.4 0,5.4 1,6 2.6,6 4,3.6",
        '%' => "4,0 0,6;0.8,0.4 0.8,1.6;3.2,4.4 3.2,5.6",
        '_' => "0,6.8 4,6.8",
        '[' => "3,0 1.6,0 1.6,6.6 3,6.6",
        ']' => "1,0 2.4,0 2.4,6.6 1,6.6",
        '{' => "3,0 2.2,0.4 2,2.6 1,3.3 2,4 2.2,6.2 3,6.6",
        '}' => "1,0 1.8,0.4 2,2.6 3,3.3 2,4 1.8,6.2 1,6.6",
        '@' => "3,4 3,2.4 2,2 1.3,3 1.5,4.2 2.5,4.4 3,4 3.6,4.4 4,3.4 4,2 3,0.5 1,0.5 0,2 0,4.6 1,6 3.6,6",
        '$' => "4,1 3,0.4 1,0.4 0,1.2 0,2.2 1,3.1 3,3.1 4,4 4,5 3,5.8 1,5.8 0,5.2;2,-0.5 2,6.6",
        '|' => "2,0 2,7",
        '~' => "0,3.6 1,2.8 3,4.2 4,3.4",
        '`' => "1.4,0 2.6,1.2",
        '\\' => "0,0 4,6",
        '^' => "1,2 2,0 3,2",
        _ => "",
    };
    spec.split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split_whitespace()
                .map(|p| {
                    let (x, y) = p.split_once(',').expect("glyph point");
                    [x.parse().expect("glyph x"), y.parse().expect("glyph y")]
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_ascii_has_glyphs() {
        for c in (33u8..127).map(char::from) {
            assert!(!glyph(c).is_empty(), "{c:?}");
        }
    }

    #[test]
    fn layout_measures_lines() {
        let (strokes, size) = layout("Hi\nthere", 0, Align::Center, 1);
        assert!(strokes.len() > 6);
        assert!(size[0] > 10.0 && size[1] > LINE_UNITS);
        let (_, mono) = layout("ab", 2, Align::Left, 1);
        assert_eq!(mono[0], 12.0);
    }

    fn area(p: &[[f64; 2]]) -> f64 {
        let n = p.len();
        (0..n)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            * 0.5
    }

    #[test]
    fn joined_contours_keep_their_area() {
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let far = vec![[20.0, 0.0], [22.0, 0.0], [22.0, 2.0]];
        let want = area(&outer) + area(&hole) + area(&far);
        let (pts, bridges) = join(vec![outer, hole, far]);
        assert!((area(&pts) - want).abs() < 1e-9, "{} vs {want}", area(&pts));
        assert_eq!(bridges.len(), 4);
    }

    #[test]
    fn bundled_fonts_parse_and_draw() {
        let manifest = include_str!("../../../web/app/fonts/fonts.json");
        for (name, file, category) in BUNDLED {
            assert!(
                manifest.contains(&format!(
                    "\"name\": \"{name}\", \"file\": \"{file}\", \"category\": \"{category}\""
                )),
                "{name} missing from fonts.json"
            );
            let bytes = bundled_bytes(file).expect(file);
            assert!(family_name(bytes).is_some(), "{name}");
            let id = id_of(name);
            assert!(!is_single_line(id), "{name} loads as an outline font");
            let (marks, size) = layout("Hello, wörld!\nQq", id, Align::Center, 1);
            let fills = marks.iter().filter(|m| matches!(m, Mark::Fill(..))).count();
            assert!(fills >= 12, "{name}: {fills} letters");
            assert!(size[0] > 20.0 && size[1] > 12.0, "{name}: {size:?}");
            for m in &marks {
                if let Mark::Fill(p, _) = m {
                    assert!(
                        p.len() <= FILL_MAX_PTS
                            && p.iter().all(|q| q[0].is_finite() && q[1].is_finite())
                    );
                }
            }
        }
    }

    #[test]
    fn user_fonts_register_by_family_name() {
        let bytes = bundled_bytes("Bangers.ttf").unwrap().to_vec();
        let (id, name) = register(None, "Yours", bytes, true).unwrap();
        assert_eq!(name, "Bangers");
        assert_eq!(
            id,
            id_of("Bangers"),
            "replaces the bundled entry, no duplicate"
        );
        assert!(register(None, "Yours", vec![1, 2, 3], true).is_err());
        let ghost = id_of("Some Font Nobody Has");
        assert!(!list()[ghost as usize].available);
        // Unknown fonts still lay out (with the fallback).
        assert!(!layout("ok", ghost, Align::Left, 1).0.is_empty());
    }
}
