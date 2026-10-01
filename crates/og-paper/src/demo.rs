// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The try-mode demo canvas: a home page with notes and doodles, and a chain
//! of nested "worlds" — each one hidden inside a dot of the one above, 1024x
//! smaller — that reaches about 10^45 zoom. Text is drawn with a tiny
//! single-stroke font so every letter is real ink you can erase or recolor.

use std::f64::consts::TAU;

use ogpaper_core::{Brush, Camera, CellAddr, Scene};

use crate::timeline::{Bookmark, Event, Timeline};

/// Levels between one world and the next (2^10 = 1024x).
const STEP: u32 = 10;
/// Worlds below home. 15 x 1024x is about 10^45.
pub const WORLDS: usize = 15;
/// Pixels per level-0 cell at scale 1 (same as the app's home camera).
const BASE_PX: f64 = 800.0;

const INK: u32 = rgba(28, 28, 36, 255);
const ACCENT: u32 = rgba(200, 40, 90, 255);
const BLUE: u32 = rgba(30, 90, 200, 255);
const GREEN: u32 = rgba(30, 150, 90, 255);
const ORANGE: u32 = rgba(235, 120, 20, 255);
const PURPLE: u32 = rgba(120, 60, 190, 255);
const MUTED: u32 = rgba(110, 110, 125, 255);
const YELLOW_HL: u32 = rgba(255, 214, 0, 255);
const PINK_HL: u32 = rgba(255, 140, 190, 255);
const DOT: u32 = rgba(255, 200, 40, 255);

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

/// Where the portal to the next world sits in each world (local [0,1] space),
/// as an index of a cell `STEP` levels down. Varies so the chain wanders.
fn portal_cell(k: usize) -> (i64, i64) {
    const SPOTS: [(i64, i64); 4] = [(790, 700), (230, 760), (780, 520), (512, 800)];
    SPOTS[k % SPOTS.len()]
}

/// The home world's portal: the dot of the big "i".
const HOME_PORTAL: (i64, i64) = (176, 410);

/// Draws strokes given in one cell's local space; anchors each stroke to the
/// smallest cell that holds it, like hand-drawn ink.
struct Pen<'a> {
    scene: &'a mut Scene,
    base: CellAddr,
}

impl Pen<'_> {
    fn line(&mut self, pts: &[[f64; 2]], width: f64, color: u32, brush: Brush) {
        if pts.is_empty() {
            return;
        }
        let (cell, local, side) = Scene::anchor_for_min(&self.base, pts, width);
        let pts: Vec<[f32; 4]> = local.iter().map(|l| [l[0], l[1], 1.0, 0.0]).collect();
        self.scene
            .add_stroke_styled(&cell, &pts, (width / side) as f32, color, brush, 0);
    }

    fn pen(&mut self, pts: &[[f64; 2]], width: f64, color: u32) {
        self.line(pts, width, color, Brush::Marker);
    }

    fn circle(&mut self, c: [f64; 2], r: f64, width: f64, color: u32) {
        let pts: Vec<[f64; 2]> = (0..=48)
            .map(|i| {
                let a = i as f64 / 48.0 * TAU;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect();
        self.pen(&pts, width, color);
    }

    /// A filled disc: one fat round-nib stroke.
    fn disc(&mut self, c: [f64; 2], r: f64, color: u32) {
        self.pen(
            &[[c[0] - 1e-4 * r, c[1]], [c[0] + 1e-4 * r, c[1]]],
            r * 2.0,
            color,
        );
    }

    /// A portal dot: reads as a solid dot from afar, but is a thick ring so
    /// the world inside sits on plain paper.
    fn portal(&mut self, c: [f64; 2], r: f64) {
        self.circle(c, r * 0.72, r * 0.56, DOT);
    }

    fn arrow(&mut self, from: [f64; 2], to: [f64; 2], width: f64, color: u32) {
        self.pen(&[from, to], width, color);
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let len = dx.hypot(dy).max(1e-12);
        let (ux, uy) = (dx / len, dy / len);
        let h = (len * 0.25).min(width * 6.0);
        for s in [-1.0, 1.0] {
            let (rx, ry) = (-ux * 0.8 + s * -uy * 0.6, -uy * 0.8 + s * ux * 0.6);
            self.pen(&[to, [to[0] + rx * h, to[1] + ry * h]], width, color);
        }
    }

    /// Text with its top-left at `at`, capital letters `h` tall. `^` raises
    /// the digits after it (10^45). Returns the width drawn.
    fn text(&mut self, at: [f64; 2], h: f64, color: u32, s: &str) -> f64 {
        let u = h / 6.0;
        let width = u * 0.75;
        let mut x = at[0];
        let mut sup = false;
        for ch in s.chars() {
            if ch == '^' {
                sup = true;
                continue;
            }
            if sup && !ch.is_ascii_digit() {
                sup = false;
            }
            let (scale, dy) = if sup { (0.6, -1.6) } else { (1.0, 0.0) };
            let uu = u * scale;
            for stroke in glyph(ch.to_ascii_uppercase()) {
                let pts: Vec<[f64; 2]> = stroke
                    .iter()
                    .map(|p| [x + p[0] * uu, at[1] + (p[1] + dy) * uu])
                    .collect();
                self.pen(&pts, width * scale, color);
            }
            x += 6.0 * uu;
        }
        x - at[0]
    }

    /// Like `text`, centred on `cx`.
    fn text_c(&mut self, cx: f64, y: f64, h: f64, color: u32, s: &str) {
        let w = text_width(s, h);
        self.text([cx - w * 0.5, y], h, color, s);
    }

    fn highlight(&mut self, from: [f64; 2], to: [f64; 2], width: f64, color: u32) {
        self.line(&[from, to], width, color, Brush::Highlighter);
    }
}

fn text_width(s: &str, h: f64) -> f64 {
    let u = h / 6.0;
    let mut w = 0.0;
    let mut sup = false;
    for ch in s.chars() {
        if ch == '^' {
            sup = true;
            continue;
        }
        if sup && !ch.is_ascii_digit() {
            sup = false;
        }
        w += 6.0 * u * if sup { 0.6 } else { 1.0 };
    }
    w
}

/// Single-stroke glyphs on a 4 x 6 grid (y down).
fn glyph(c: char) -> Vec<Vec<[f64; 2]>> {
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
        'x' | '×' => "1,2 3,4;3,2 1,4",
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

/// The demo canvas and the places worth flying to.
pub struct Demo {
    pub scene: Scene,
    /// Home, then each world down the chain (bookmarks to fly to).
    pub worlds: Vec<CellAddr>,
}

/// Camera framing `cell` (a world) on a `w` x `h` px viewport.
pub fn frame_cell(cell: &CellAddr, w: f64, h: f64) -> Camera {
    let mut c = Camera::new(cell.clone(), [0.5, 0.5], BASE_PX);
    let fit = (w.min(h) * 0.92 / BASE_PX).max(1e-3);
    c.zoom_at(fit, [0.0, 0.0]);
    c
}

/// Zoom depth (log10) of world `k`, as shown in its label.
fn depth_label(k: usize) -> i64 {
    (k as f64 * STEP as f64 * std::f64::consts::LOG10_2).round() as i64
}

fn child_at(base: &CellAddr, (ix, iy): (i64, i64)) -> CellAddr {
    CellAddr {
        level: base.level + STEP as i64,
        x: (&base.x << STEP) + ix,
        y: (&base.y << STEP) + iy,
    }
}

fn portal_centre((ix, iy): (i64, i64)) -> [f64; 2] {
    let n = (1u64 << STEP) as f64;
    [(ix as f64 + 0.5) / n, (iy as f64 + 0.5) / n]
}

impl Demo {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Ready-made bookmarks: home, the first world, a few depths, the bottom.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        let at = |k: usize, name: String| Bookmark {
            name,
            cam: frame_cell(&self.worlds[k], BASE_PX, BASE_PX),
            view_px: BASE_PX,
        };
        let mut v = vec![
            at(0, "Home".into()),
            at(1, format!("Inside the dot (10^{})", depth_label(1))),
        ];
        for k in [4, 8, 12] {
            v.push(at(k, format!("Zoom 10^{}", depth_label(k))));
        }
        v.push(at(
            WORLDS,
            format!("The bottom (10^{})", depth_label(WORLDS)),
        ));
        v
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// A made-up history so the timeline has something to replay: the demo
    /// is "drawn" one stroke every 150 ms, ending at `now`.
    pub fn timeline(&self, now: i64) -> Timeline {
        let n = self.scene.strokes.len() as i64;
        Timeline::from_events(
            (0..n)
                .map(|i| Event {
                    t: now - (n - i) * 150,
                    id: i as u32,
                    alive: true,
                })
                .collect(),
        )
    }
}

pub fn build() -> Demo {
    let mut scene = Scene::new();
    let home = CellAddr::new(0, 0, 0);
    let mut worlds = vec![home.clone()];

    home_world(&mut scene, &home);
    zoomed_out_notes(&mut scene);

    let mut parent = home;
    let mut portal = HOME_PORTAL;
    for k in 1..=WORLDS {
        let cell = child_at(&parent, portal);
        let next = portal_cell(k);
        deep_world(&mut scene, &cell, k, (k < WORLDS).then_some(next));
        // Inside the parent's portal, between the two worlds: a nudge.
        let mut p = Pen {
            scene: &mut scene,
            base: parent.clone(),
        };
        let c = portal_centre(portal);
        let s = 1.0 / (1u64 << STEP) as f64;
        p.text_c(c[0], c[1] - s * 3.2, s * 0.9, INK, "KEEP ZOOMING");
        p.circle(c, s * 1.6, s * 0.06, ACCENT);
        worlds.push(cell.clone());
        parent = cell;
        portal = next;
    }
    Demo { scene, worlds }
}

fn home_world(scene: &mut Scene, home: &CellAddr) {
    let mut p = Pen {
        scene,
        base: home.clone(),
    };
    // Title.
    p.highlight([0.08, 0.135], [0.62, 0.135], 0.05, YELLOW_HL);
    p.text([0.08, 0.09], 0.09, INK, "OG PAPER");
    p.text([0.08, 0.215], 0.028, MUTED, "TRY MODE - AN ENDLESS CANVAS");

    // The big lowercase "i" whose dot is the way down.
    let dot = portal_centre(HOME_PORTAL);
    p.portal(dot, 0.028);
    p.pen(
        &[[0.172, 0.5], [0.172, 0.72], [0.19, 0.74], [0.205, 0.73]],
        0.024,
        INK,
    );
    p.text([0.27, 0.36], 0.026, ACCENT, "ZOOM INTO THE DOT");
    p.text([0.27, 0.40], 0.026, ACCENT, "OF THE I");
    p.arrow([0.26, 0.38], [0.215, 0.395], 0.006, ACCENT);
    p.text([0.27, 0.46], 0.018, MUTED, "SCROLL, PINCH OR USE");
    p.text([0.27, 0.49], 0.018, MUTED, "THE FLY-TO BUTTONS");

    // How-to notes.
    let notes = [
        ("DRAW ANYWHERE WITH THE PEN", INK),
        ("RIGHT-DRAG OR TWO FINGERS TO PAN", INK),
        ("CTRL+Z OR TWO-FINGER TAP TO UNDO", INK),
        ("ZOOM OUT TOO - IT GOES UP FOREVER", INK),
    ];
    for (i, (s, c)) in notes.iter().enumerate() {
        let y = 0.62 + i as f64 * 0.05;
        p.disc([0.3, y + 0.011], 0.006, ACCENT);
        p.text([0.32, y], 0.022, *c, s);
    }

    // Doodles.
    star(&mut p, [0.82, 0.16], 0.07, ORANGE);
    spiral(&mut p, [0.83, 0.42], 0.075, BLUE);
    house(&mut p, [0.74, 0.86], 0.09, GREEN);
    heart(&mut p, [0.9, 0.86], 0.045, ACCENT);
    p.highlight([0.31, 0.903], [0.6, 0.903], 0.02, PINK_HL);
    p.text([0.32, 0.89], 0.022, PURPLE, "HIGHLIGHTER TOO");

    // Off to the sides: the canvas keeps going.
    p.text([1.15, 0.45], 0.04, BLUE, "YOU PANNED RIGHT.");
    p.text([1.15, 0.52], 0.04, BLUE, "THERE IS NO EDGE.");
    p.arrow([1.15, 0.62], [1.6, 0.62], 0.008, BLUE);
    p.text([1.65, 0.6], 0.04, BLUE, "...KEEP GOING");
    p.text([12.0, 0.45], 0.06, GREEN, "TWELVE SCREENS FROM HOME!");
    p.text(
        [12.0, 0.55],
        0.03,
        MUTED,
        "PRESS THE HOME BUTTON TO GO BACK",
    );
    p.text([-1.0, 0.45], 0.04, PURPLE, "LEFT SIDE TOO");
    p.text([0.08, -0.4], 0.04, ORANGE, "AND UP HERE");
    p.text([0.08, 1.3], 0.04, ORANGE, "AND DOWN HERE");
}

/// Notes only readable once you zoom out from home.
fn zoomed_out_notes(scene: &mut Scene) {
    // Level -4: a cell 16x the size of home, home sits in its top-left part.
    let big = CellAddr::new(-4, -1, -1);
    let mut p = Pen { scene, base: big };
    // In this cell's units home's origin is (1, 1) and its side is 1/16.
    let home_o = [1.0, 1.0];
    let s = 1.0 / 16.0;
    p.circle(
        [home_o[0] + s * 0.5, home_o[1] + s * 0.5],
        s * 2.2,
        s * 0.06,
        ACCENT,
    );
    p.arrow(
        [home_o[0] - s * 4.5, home_o[1] - s * 3.2],
        [home_o[0] - s * 1.3, home_o[1] - s * 1.2],
        s * 0.08,
        ACCENT,
    );
    p.text(
        [home_o[0] - s * 10.0, home_o[1] - s * 5.2],
        s * 0.5,
        ACCENT,
        "THE WHOLE HOME PAGE IS IN THERE",
    );
    p.text(
        [home_o[0] - s * 10.0, home_o[1] - s * 4.4],
        s * 0.3,
        MUTED,
        "ZOOM OUT AS FAR AS YOU LIKE - THEN BACK IN",
    );
}

fn deep_world(scene: &mut Scene, cell: &CellAddr, k: usize, portal: Option<(i64, i64)>) {
    const CAPTIONS: [&str; WORLDS] = [
        "HELLO FROM INSIDE THE DOT",
        "A MILLION TIMES DEEPER",
        "A BILLION. STILL CRISP.",
        "WRITE A NOTE DOWN HERE",
        "NO PRECISION LOSS",
        "EVERY LEVEL IS REAL INK",
        "PLAIN FLOATS BROKE LONG AGO",
        "TRY THE ERASER ON ME",
        "SMALLER THAN ATOMS NOW",
        "HALFWAY TO THE BOTTOM",
        "PAN AROUND - IT IS WIDE TOO",
        "UNDO WORKS AT ANY DEPTH",
        "ALMOST THERE",
        "ONE MORE DOT",
        "THE BOTTOM? THERE IS NONE.",
    ];
    let colors = [BLUE, GREEN, PURPLE, ORANGE, ACCENT];
    let col = colors[k % colors.len()];
    let mut p = Pen {
        scene,
        base: cell.clone(),
    };
    // Frame.
    p.pen(
        &[
            [0.03, 0.03],
            [0.97, 0.03],
            [0.97, 0.97],
            [0.03, 0.97],
            [0.03, 0.03],
        ],
        0.004,
        MUTED,
    );
    let label = format!("ZOOM 10^{}", depth_label(k));
    p.highlight(
        [0.07, 0.14],
        [0.07 + text_width(&label, 0.08), 0.14],
        0.05,
        YELLOW_HL,
    );
    p.text([0.07, 0.1], 0.08, INK, &label);
    let cap = CAPTIONS[k - 1];
    let h = (0.86 / text_width(cap, 1.0)).min(0.045);
    p.text([0.07, 0.25], h, col, cap);
    p.text(
        [0.07, 0.32],
        0.02,
        MUTED,
        &format!("WORLD {k} OF {WORLDS} - EACH ONE IS 1024X SMALLER"),
    );

    // A doodle, different per world, on the side away from the portal.
    let left = portal.is_none_or(|n| portal_centre(n)[0] >= 0.45);
    let c = [if left { 0.27 } else { 0.72 }, 0.58];
    match k % 4 {
        0 => star(&mut p, c, 0.13, col),
        1 => spiral(&mut p, c, 0.14, col),
        2 => heart(&mut p, c, 0.1, col),
        _ => house(&mut p, [c[0], c[1] + 0.12], 0.2, col),
    }

    match portal {
        Some(next) => {
            let pc = portal_centre(next);
            p.portal(pc, 0.022);
            let side = if pc[0] > 0.5 { -1.0 } else { 1.0 };
            let tx = pc[0] + side * 0.07;
            for (i, line) in ["NEXT WORLD", "IN THIS DOT"].iter().enumerate() {
                let w = text_width(line, 0.022);
                let x0 = if side < 0.0 { tx - w } else { tx };
                p.text([x0, pc[1] - 0.105 + i as f64 * 0.035], 0.022, ACCENT, line);
            }
            p.arrow(
                [tx, pc[1] - 0.04],
                [pc[0] + side * 0.03, pc[1] - 0.012],
                0.004,
                ACCENT,
            );
        }
        None => {
            p.text([0.55, 0.55], 0.03, ACCENT, "YOU MADE IT.");
            p.text([0.55, 0.6], 0.022, INK, "THIS IS 10^45 DEEP.");
            p.text([0.55, 0.64], 0.022, INK, "DRAW SOMETHING HERE,");
            p.text([0.55, 0.68], 0.022, INK, "THEN FLY HOME AND");
            p.text([0.55, 0.72], 0.022, INK, "COME BACK TO FIND IT.");
            star(&mut p, [0.75, 0.86], 0.06, ORANGE);
        }
    }
}

fn star(p: &mut Pen, c: [f64; 2], r: f64, color: u32) {
    let pts: Vec<[f64; 2]> = (0..=10)
        .map(|i| {
            let a = -TAU / 4.0 + i as f64 * TAU / 10.0;
            let rr = if i % 2 == 0 { r } else { r * 0.42 };
            [c[0] + rr * a.cos(), c[1] + rr * a.sin()]
        })
        .collect();
    p.pen(&pts, r * 0.06, color);
}

fn spiral(p: &mut Pen, c: [f64; 2], r: f64, color: u32) {
    let pts: Vec<[f64; 2]> = (0..=120)
        .map(|i| {
            let t = i as f64 / 120.0;
            let a = t * TAU * 3.0;
            [c[0] + r * t * a.cos(), c[1] + r * t * a.sin()]
        })
        .collect();
    p.pen(&pts, r * 0.05, color);
}

fn heart(p: &mut Pen, c: [f64; 2], r: f64, color: u32) {
    let pts: Vec<[f64; 2]> = (0..=64)
        .map(|i| {
            let t = i as f64 / 64.0 * TAU;
            let x = 16.0 * t.sin().powi(3);
            let y =
                13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
            [c[0] + x / 17.0 * r, c[1] - y / 17.0 * r]
        })
        .collect();
    p.pen(&pts, r * 0.07, color);
}

/// A house sitting on `base` (bottom-centre), `s` wide.
fn house(p: &mut Pen, base: [f64; 2], s: f64, color: u32) {
    let (x, y, h) = (base[0] - s * 0.5, base[1], s * 0.6);
    p.pen(
        &[[x, y - h], [x, y], [x + s, y], [x + s, y - h]],
        s * 0.03,
        color,
    );
    p.pen(
        &[
            [x - s * 0.08, y - h + s * 0.04],
            [x + s * 0.5, y - h - s * 0.4],
            [x + s * 1.08, y - h + s * 0.04],
        ],
        s * 0.03,
        color,
    );
    p.pen(
        &[
            [x + s * 0.4, y],
            [x + s * 0.4, y - h * 0.5],
            [x + s * 0.6, y - h * 0.5],
            [x + s * 0.6, y],
        ],
        s * 0.025,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogpaper_core::{query, DrawList, Params};

    #[test]
    fn every_world_is_visible_when_framed() {
        let demo = build();
        assert_eq!(demo.worlds.len(), WORLDS + 1);
        assert!(demo.scene.strokes.len() > 500);
        let mut out = DrawList::default();
        for (k, w) in demo.worlds.iter().enumerate() {
            let cam = frame_cell(w, 1280.0, 800.0);
            query(
                &demo.scene,
                &cam,
                1280.0,
                800.0,
                Params::default(),
                &mut out,
            );
            assert!(out.stats.strokes > 20, "world {k}: {:?}", out.stats);
        }
        // The last world is ~10^45 deep.
        let deepest = frame_cell(demo.worlds.last().unwrap(), 1280.0, 800.0);
        assert!(deepest.log10_zoom() > 44.0, "{}", deepest.log10_zoom());
    }

    #[test]
    fn every_glyph_parses() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,!?-':/<>()+=x".chars() {
            assert!(!glyph(c).is_empty(), "{c}");
        }
    }
}
