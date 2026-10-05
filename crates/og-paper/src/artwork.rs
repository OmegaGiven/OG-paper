// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! "The Maker's Loop": an endless-zoom illustration on the demo canvas.
//!
//! A man in his man cave makes a drawing app. On his screen, the app shows a
//! sketch of his life: his job, taken by an AI. On that office computer is
//! the game he dreams of making, a wizard's quest. Inside the wizard's spell
//! is the god who makes spells for people, and in the god's eye, reflected,
//! is the man in his man cave again: a portal back to the start, so zooming
//! in never ends.
//!
//! Each scene is a frame 16:10 (u across 0..1, v down 0..0.625) drawn in
//! home-cell units; the next scene's frame is a rectangle inside it (a
//! screen, a spell, an eye) with the same shape, so zooming into it lands
//! exactly on the next scene. Everything is ordinary ink (fills, pens,
//! markers and text) you can select, recolor or erase.

use std::f64::consts::{PI, TAU};

use ogpaper_core::{Brush, Camera, CellAddr, Scene};

use crate::font::glyph;
use crate::timeline::Bookmark;

/// Pixels per home cell at scale 1 (as the app's home camera).
const BASE_PX: f64 = 800.0;
/// Frame height over width.
const ASPECT: f64 = 0.625;
/// Scene 0's top-left on the demo canvas, in home-cell units (left of home).
pub const ORIGIN: [f64; 2] = [-2.3, 0.0];
/// On a canvas of its own: centred on the home view.
pub const PAGE_ORIGIN: [f64; 2] = [0.5 - WIDTH0 * 0.5, 0.5 - WIDTH0 * ASPECT * 0.5];
const WIDTH0: f64 = 1.6;

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from_le_bytes([r, g, b, 255])
}
const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([r, g, b, a])
}

const INK: u32 = rgb(24, 22, 30);
const WHITE: u32 = rgb(250, 248, 242);

/// A scene's frame: world = o + uv * s.
#[derive(Clone, Copy)]
struct Frame {
    o: [f64; 2],
    s: f64,
}

impl Frame {
    /// The frame of rectangle (u0, v0)-(u1, ..) inside this one: the next
    /// scene (its height follows from the aspect).
    fn inner(&self, u0: f64, v0: f64, u1: f64) -> Frame {
        Frame {
            o: [self.o[0] + u0 * self.s, self.o[1] + v0 * self.s],
            s: (u1 - u0) * self.s,
        }
    }
    fn centre(&self) -> [f64; 2] {
        [self.o[0] + 0.5 * self.s, self.o[1] + 0.5 * ASPECT * self.s]
    }
    /// A camera showing the whole frame `view_px` tall.
    fn camera(&self, view_px: f64) -> Camera {
        let mut c = Camera::new(CellAddr::new(0, 0, 0), self.centre(), BASE_PX);
        c.zoom_at(view_px / (ASPECT * self.s * BASE_PX), [0.0, 0.0]);
        c
    }
}

/// Draws in a frame's uv space.
struct Art<'a> {
    scene: &'a mut Scene,
    f: Frame,
}

impl Art<'_> {
    fn w(&self, p: [f64; 2]) -> [f64; 2] {
        [self.f.o[0] + p[0] * self.f.s, self.f.o[1] + p[1] * self.f.s]
    }

    fn stroke(&mut self, pts: &[[f64; 2]], press: &[f32], width: f64, color: u32, brush: Brush) {
        if pts.is_empty() {
            return;
        }
        let home = CellAddr::new(0, 0, 0);
        let world: Vec<[f64; 2]> = pts.iter().map(|&p| self.w(p)).collect();
        let wd = width * self.f.s;
        let (cell, local, side) = Scene::anchor_for_min(&home, &world, wd.max(1e-12));
        let pts: Vec<[f32; 4]> = local
            .iter()
            .enumerate()
            .map(|(i, l)| [l[0], l[1], press.get(i).copied().unwrap_or(1.0), 0.0])
            .collect();
        self.scene
            .add_stroke_styled(&cell, &pts, (wd / side) as f32, color, brush, 0);
    }

    /// A constant-width line.
    fn line(&mut self, pts: &[[f64; 2]], width: f64, color: u32) {
        self.stroke(pts, &[], width, color, Brush::Marker);
    }
    /// A hand-drawn line: thin at both ends.
    fn taper(&mut self, pts: &[[f64; 2]], width: f64, color: u32) {
        let n = pts.len().max(2) - 1;
        let press: Vec<f32> = (0..pts.len())
            .map(|i| {
                let t = i as f64 / n as f64;
                (0.25 + 0.75 * (PI * t).sin()) as f32
            })
            .collect();
        self.stroke(pts, &press, width, color, Brush::Pen);
    }
    fn fill(&mut self, pts: &[[f64; 2]], color: u32) {
        self.stroke(pts, &[], 0.001, color, Brush::Fill);
    }
    fn closed(&mut self, pts: &[[f64; 2]], width: f64, color: u32) {
        let mut v = pts.to_vec();
        if let Some(&f) = pts.first() {
            v.push(f);
        }
        self.line(&v, width, color);
    }
    fn rect(&mut self, a: [f64; 2], b: [f64; 2], color: u32) {
        self.fill(&[a, [b[0], a[1]], b, [a[0], b[1]]], color);
    }
    fn rrect_pts(a: [f64; 2], b: [f64; 2], r: f64) -> Vec<[f64; 2]> {
        let r = r.min((b[0] - a[0]) * 0.5).min((b[1] - a[1]) * 0.5);
        let mut v = Vec::new();
        let corners = [
            ([b[0] - r, a[1] + r], -PI / 2.0),
            ([b[0] - r, b[1] - r], 0.0),
            ([a[0] + r, b[1] - r], PI / 2.0),
            ([a[0] + r, a[1] + r], PI),
        ];
        for (c, a0) in corners {
            for k in 0..=6 {
                let t = a0 + k as f64 / 6.0 * PI / 2.0;
                v.push([c[0] + r * t.cos(), c[1] + r * t.sin()]);
            }
        }
        v
    }
    fn rrect(&mut self, a: [f64; 2], b: [f64; 2], r: f64, color: u32) {
        let v = Self::rrect_pts(a, b, r);
        self.fill(&v, color);
    }
    fn rrect_line(&mut self, a: [f64; 2], b: [f64; 2], r: f64, width: f64, color: u32) {
        let v = Self::rrect_pts(a, b, r);
        self.closed(&v, width, color);
    }
    fn ellipse_pts(c: [f64; 2], rx: f64, ry: f64, a0: f64, a1: f64, n: usize) -> Vec<[f64; 2]> {
        (0..=n)
            .map(|i| {
                let t = a0 + (a1 - a0) * i as f64 / n as f64;
                [c[0] + rx * t.cos(), c[1] + ry * t.sin()]
            })
            .collect()
    }
    fn ellipse(&mut self, c: [f64; 2], rx: f64, ry: f64, color: u32) {
        let v = Self::ellipse_pts(c, rx, ry, 0.0, TAU, 40);
        self.fill(&v, color);
    }
    fn disc(&mut self, c: [f64; 2], r: f64, color: u32) {
        self.ellipse(c, r, r, color);
    }
    fn ring(&mut self, c: [f64; 2], rx: f64, ry: f64, width: f64, color: u32) {
        let v = Self::ellipse_pts(c, rx, ry, 0.0, TAU, 48);
        self.line(&v, width, color);
    }
    fn arc(&mut self, c: [f64; 2], r: f64, a0: f64, a1: f64, width: f64, color: u32) {
        let v = Self::ellipse_pts(c, r, r, a0, a1, 24);
        self.line(&v, width, color);
    }
    /// A soft glow: translucent discs, larger and fainter. Few discs of
    /// few sides: every disc whose edge crosses the view costs each pixel a
    /// walk round its outline, and glows are big.
    fn glow(&mut self, c: [f64; 2], r: f64, rgb3: (u8, u8, u8), strength: f64) {
        for i in 0..4 {
            let k = 1.0 - i as f64 / 4.0;
            let a = (strength * 36.0 * (1.0 - k * 0.7)) as u8;
            let v = Self::ellipse_pts(c, r * k, r * k, 0.0, TAU, 28);
            self.fill(&v, rgba(rgb3.0, rgb3.1, rgb3.2, a.max(5)));
        }
    }
    /// A four-point sparkle.
    fn sparkle(&mut self, c: [f64; 2], r: f64, color: u32) {
        let t = r * 0.18;
        self.fill(
            &[
                [c[0], c[1] - r],
                [c[0] + t, c[1] - t],
                [c[0] + r, c[1]],
                [c[0] + t, c[1] + t],
                [c[0], c[1] + r],
                [c[0] - t, c[1] + t],
                [c[0] - r, c[1]],
                [c[0] - t, c[1] - t],
            ],
            color,
        );
    }
    /// Text, capitals `h` tall, top-left at `at`.
    fn text(&mut self, at: [f64; 2], h: f64, color: u32, s: &str) -> f64 {
        let u = h / 6.0;
        let mut x = at[0];
        for ch in s.chars() {
            for stroke in glyph(ch.to_ascii_uppercase()) {
                let pts: Vec<[f64; 2]> = stroke
                    .iter()
                    .map(|p| [x + p[0] * u, at[1] + p[1] * u])
                    .collect();
                self.line(&pts, u * 0.8, color);
            }
            x += 6.0 * u;
        }
        x - at[0]
    }
    fn text_c(&mut self, cx: f64, y: f64, h: f64, color: u32, s: &str) {
        let w = s.chars().count() as f64 * h - h / 6.0;
        self.text([cx - w * 0.5, y], h, color, s);
    }
}

/// A tiny deterministic random source (the art is the same every time).
struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The scenes' frames, outermost first, and where the loop's portal sits
/// (a frame inside the last scene).
fn frames(origin: [f64; 2]) -> ([Frame; 5], Frame) {
    let f0 = Frame {
        o: origin,
        s: WIDTH0,
    };
    let f1 = f0.inner(SCREEN0.0, SCREEN0.1, SCREEN0.2);
    let f2 = f1.inner(SCREEN1.0, SCREEN1.1, SCREEN1.2);
    let f3 = f2.inner(SPELL2.0, SPELL2.1, SPELL2.2);
    let f4 = f3.inner(EYE3.0, EYE3.1, EYE3.2);
    let portal = f4.inner(PUPIL4.0, PUPIL4.1, PUPIL4.2);
    ([f0, f1, f2, f3, f4], portal)
}

// Where each next scene sits in the one before: (u0, v0, u1).
const SCREEN0: (f64, f64, f64) = (0.45, 0.17, 0.69); // the man's monitor
const SCREEN1: (f64, f64, f64) = (0.47, 0.215, 0.63); // the office computer in the sketch
const SPELL2: (f64, f64, f64) = (0.55, 0.25625, 0.69); // the wizard's spell orb
const EYE3: (f64, f64, f64) = (0.445, 0.2025, 0.485); // the god's left eye
const PUPIL4: (f64, f64, f64) = (0.44, 0.2725, 0.56); // the reflection in the pupil

pub const TITLES: [&str; 5] = [
    "The Maker's Loop: the man cave",
    "The Maker's Loop: his app, his life",
    "The Maker's Loop: the game he dreams of",
    "The Maker's Loop: the god of spells",
    "The Maker's Loop: in the god's eye",
];

/// Bookmarks to each scene, framed whole.
pub fn bookmarks(origin: [f64; 2]) -> Vec<Bookmark> {
    let (fs, _) = frames(origin);
    fs.iter()
        .zip(TITLES)
        .map(|(f, name)| Bookmark {
            name: name.into(),
            cam: f.camera(700.0),
            view_px: 700.0,
            when: None,
        })
        .collect()
}

/// The camera `x` along the endless zoom (0..5, then it repeats): scene
/// floor(x), zoomed toward the next by the fraction of x, about the point
/// the two frames share, so it is one smooth exponential zoom that lands
/// on each scene centred, and at 5 (the pupil) shows scene 0 again.
/// `view_px` is how tall the frame is on screen.
pub fn loop_camera(x: f64, view_px: f64, origin: [f64; 2]) -> Camera {
    let (fs, portal) = frames(origin);
    let chain = [fs[0], fs[1], fs[2], fs[3], fs[4], portal];
    let x = x.rem_euclid(5.0);
    let i = (x.floor() as usize).min(4);
    let t = x - i as f64;
    let (a, b) = (chain[i], chain[i + 1]);
    let k = b.s / a.s;
    let z = [
        (b.o[0] - k * a.o[0]) / (1.0 - k),
        (b.o[1] - k * a.o[1]) / (1.0 - k),
    ];
    let kt = k.powf(t);
    Frame {
        o: [z[0] + (a.o[0] - z[0]) * kt, z[1] + (a.o[1] - z[1]) * kt],
        s: a.s * kt,
    }
    .camera(view_px)
}

/// Draw the whole loop; returns the portal (to add to the canvas's objects).
pub fn draw(scene: &mut Scene, origin: [f64; 2]) -> crate::objects::Group {
    let (fs, portal) = frames(origin);
    title_card(&mut Art { scene, f: fs[0] });
    man_cave(&mut Art { scene, f: fs[0] });
    app_and_life(&mut Art { scene, f: fs[1] });
    wizard_game(&mut Art { scene, f: fs[2] });
    god_of_spells(&mut Art { scene, f: fs[3] });
    gods_eye(&mut Art { scene, f: fs[4] });
    loop_portal(scene, &portal, &fs[0])
}

/// The way in, above the first frame.
fn title_card(a: &mut Art) {
    a.text([0.0, -0.085], 0.045, INK, "THE MAKER'S LOOP");
    a.text(
        [0.0, -0.03],
        0.016,
        rgb(110, 110, 125),
        "ZOOM INTO HIS SCREEN - AND KEEP GOING. IT NEVER ENDS.",
    );
}

// ---- scene 0: the man cave ------------------------------------------------

fn man_cave(a: &mut Art) {
    let mut r = Rng(0x5eed_1234_abcd_0001);
    // Walls, a little lighter lower down, and the floor.
    let bands = [
        rgb(30, 24, 48),
        rgb(36, 29, 57),
        rgb(42, 34, 66),
        rgb(47, 38, 73),
    ];
    for (i, c) in bands.iter().enumerate() {
        let v0 = i as f64 * 0.47 / 4.0;
        a.rect([0.0, v0], [1.0, v0 + 0.47 / 4.0 + 0.001], *c);
    }
    a.rect([0.0, 0.47], [1.0, ASPECT], rgb(104, 72, 46));
    for v in [0.5, 0.538, 0.582] {
        a.line(&[[0.0, v], [1.0, v]], 0.0018, rgb(80, 55, 35));
    }
    for i in 0..14 {
        let row = i % 3;
        let v0 = [0.47, 0.5, 0.538][row];
        let v1 = [0.5, 0.538, 0.582][row];
        let u = (i as f64 * 0.137 + row as f64 * 0.05) % 1.0;
        a.line(&[[u, v0], [u, v1]], 0.0015, rgb(80, 55, 35));
    }
    a.rect([0.0, 0.462], [1.0, 0.474], rgb(36, 26, 20));
    // The rug, and a cat asleep on it.
    a.ellipse([0.36, 0.565], 0.27, 0.045, rgb(48, 78, 108));
    a.ring([0.36, 0.565], 0.24, 0.037, 0.003, rgb(214, 170, 74));
    a.ring([0.36, 0.565], 0.2, 0.029, 0.002, rgb(214, 170, 74));
    cat(a, [0.17, 0.555]);

    // The window: night sky, moon, stars, curtains.
    a.rect([0.05, 0.07], [0.21, 0.27], rgb(18, 32, 78));
    a.rect([0.05, 0.19], [0.21, 0.27], rgb(28, 44, 96));
    a.glow([0.165, 0.115], 0.05, (244, 233, 193), 0.5);
    a.disc([0.165, 0.115], 0.024, rgb(244, 233, 193));
    a.disc([0.158, 0.108], 0.005, rgb(222, 210, 170));
    a.disc([0.172, 0.124], 0.004, rgb(222, 210, 170));
    for _ in 0..14 {
        let c = [0.06 + r.f() * 0.14, 0.08 + r.f() * 0.1];
        a.sparkle(c, 0.003 + r.f() * 0.004, rgb(255, 250, 220));
    }
    // City skyline far away, lit windows.
    let mut u = 0.05;
    while u < 0.21 {
        let w = 0.012 + r.f() * 0.016;
        let h = 0.02 + r.f() * 0.05;
        a.rect([u, 0.27 - h], [(u + w).min(0.21), 0.27], rgb(12, 18, 40));
        for k in 0..3 {
            if r.f() < 0.6 {
                let y = 0.27 - h + 0.006 + k as f64 * 0.012;
                if y < 0.265 {
                    a.rect([u + 0.003, y], [u + 0.006, y + 0.004], rgb(255, 214, 120));
                }
            }
        }
        u += w + 0.002;
    }
    a.rrect_line([0.05, 0.07], [0.21, 0.27], 0.004, 0.006, rgb(201, 178, 138));
    a.line(&[[0.13, 0.07], [0.13, 0.27]], 0.004, rgb(201, 178, 138));
    a.line(&[[0.05, 0.17], [0.21, 0.17]], 0.004, rgb(201, 178, 138));
    for (x0, x1) in [(0.025, 0.07), (0.19, 0.235)] {
        a.fill(
            &[
                [x0, 0.055],
                [x1, 0.055],
                [x1 - 0.004, 0.29],
                [x0 + 0.004, 0.3],
            ],
            rgb(122, 47, 74),
        );
        for k in 1..4 {
            let x = x0 + (x1 - x0) * k as f64 / 4.0;
            a.taper(
                &[[x, 0.06], [x + 0.002, 0.18], [x - 0.001, 0.29]],
                0.0022,
                rgb(90, 30, 52),
            );
        }
    }
    a.line(&[[0.015, 0.056], [0.245, 0.056]], 0.005, rgb(160, 130, 90));

    // String lights along the top.
    let lights: Vec<[f64; 2]> = (0..=40)
        .map(|i| {
            let t = i as f64 / 40.0;
            [0.02 + 0.96 * t, 0.028 + 0.03 * (t * 3.0 * PI).sin().abs()]
        })
        .collect();
    a.line(&lights, 0.0015, rgb(20, 20, 20));
    let bulbs = [
        (255, 90, 120),
        (255, 210, 80),
        (90, 220, 160),
        (110, 170, 255),
        (220, 120, 255),
    ];
    for (i, p) in lights.iter().enumerate().step_by(2) {
        let c = bulbs[(i / 2) % bulbs.len()];
        a.glow([p[0], p[1] + 0.008], 0.018, c, 0.7);
        a.ellipse([p[0], p[1] + 0.008], 0.004, 0.006, rgb(c.0, c.1, c.2));
    }

    // A neon sign: MAKE THINGS.
    a.glow([0.6, 0.095], 0.11, (255, 70, 170), 0.35);
    a.rrect_line(
        [0.465, 0.066],
        [0.735, 0.124],
        0.01,
        0.003,
        rgb(255, 120, 200),
    );
    a.text_c(0.6, 0.083, 0.022, rgb(255, 120, 210), "MAKE THINGS");
    // Posters: LEVEL UP (with a pixel ghost) and DREAM BIG.
    a.rect([0.785, 0.085], [0.905, 0.25], rgb(232, 197, 71));
    a.text_c(0.845, 0.1, 0.018, INK, "LEVEL UP");
    let ghost = [
        "..XXXX..", ".XXXXXX.", "XX.XX.XX", "XXXXXXXX", "XXXXXXXX", "X.XX.XX.",
    ];
    for (row, line) in ghost.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch == 'X' {
                let x = 0.81 + col as f64 * 0.0088;
                let y = 0.135 + row as f64 * 0.0088;
                a.rect([x, y], [x + 0.0085, y + 0.0085], rgb(233, 80, 110));
            }
        }
    }
    a.rect([0.25, 0.09], [0.33, 0.2], rgb(40, 160, 160));
    a.text_c(0.29, 0.11, 0.012, WHITE, "DREAM");
    a.text_c(0.29, 0.13, 0.016, WHITE, "BIG");
    star(a, [0.29, 0.172], 0.014, rgb(255, 230, 120));

    // The shelf: books, a plant, a trophy.
    a.rect([0.77, 0.3], [0.975, 0.31], rgb(120, 80, 50));
    let mut u = 0.78;
    let colors = [
        rgb(200, 60, 70),
        rgb(60, 120, 200),
        rgb(230, 170, 60),
        rgb(80, 160, 100),
        rgb(150, 90, 180),
    ];
    for i in 0..9 {
        let w = 0.008 + r.f() * 0.006;
        let h = 0.035 + r.f() * 0.02;
        a.rect([u, 0.3 - h], [u + w, 0.3], colors[i % colors.len()]);
        a.line(
            &[[u + w * 0.2, 0.3 - h * 0.8], [u + w * 0.8, 0.3 - h * 0.8]],
            0.001,
            rgb(250, 240, 210),
        );
        u += w + 0.001;
    }
    a.fill(
        &[[0.9, 0.3], [0.93, 0.3], [0.935, 0.275], [0.895, 0.275]],
        rgb(190, 100, 60),
    );
    for k in 0..6 {
        let ang = -PI / 2.0 + (k as f64 - 2.5) * 0.35;
        let tip = [0.915 + 0.035 * ang.cos(), 0.272 + 0.035 * ang.sin()];
        a.taper(
            &[
                [0.915, 0.277],
                [(0.915 + tip[0]) / 2.0 + 0.004, (0.277 + tip[1]) / 2.0],
                tip,
            ],
            0.006,
            rgb(70, 160, 90),
        );
    }
    a.fill(
        &[[0.945, 0.3], [0.965, 0.3], [0.958, 0.29], [0.952, 0.29]],
        rgb(220, 180, 60),
    );
    a.line(&[[0.955, 0.29], [0.955, 0.278]], 0.002, rgb(220, 180, 60));
    a.ellipse([0.955, 0.27], 0.01, 0.009, rgb(240, 200, 70));

    // A guitar leaning on the wall.
    a.line(&[[0.93, 0.33], [0.9, 0.47]], 0.006, rgb(90, 55, 30));
    a.ellipse([0.895, 0.5], 0.03, 0.035, rgb(200, 100, 44));
    a.ellipse([0.9, 0.465], 0.022, 0.025, rgb(200, 100, 44));
    a.disc([0.898, 0.49], 0.008, rgb(40, 24, 14));
    for d in [-0.002, 0.0, 0.002] {
        a.line(
            &[[0.928 + d, 0.335], [0.896 + d, 0.505]],
            0.0005,
            rgb(230, 230, 230),
        );
    }

    // The desk and what is on it.
    let desk = rgb(139, 90, 60);
    a.rect([0.3, 0.385], [0.88, 0.405], desk);
    a.rect([0.3, 0.403], [0.88, 0.41], rgb(100, 64, 42));
    a.rect([0.32, 0.41], [0.34, 0.5], rgb(100, 64, 42));
    a.rect([0.84, 0.41], [0.86, 0.5], rgb(100, 64, 42));
    a.rect([0.72, 0.41], [0.84, 0.47], rgb(120, 78, 52));
    for v in [0.43, 0.452] {
        a.line(&[[0.765, v], [0.795, v]], 0.003, rgb(220, 190, 140));
    }
    // The glow of the big screen on the wall behind it.
    a.glow([0.57, 0.245], 0.24, (110, 170, 255), 0.45);
    // The second monitor: code.
    a.rrect([0.3, 0.205], [0.4, 0.3], 0.006, rgb(18, 18, 22));
    a.rect([0.305, 0.21], [0.395, 0.293], rgb(16, 22, 40));
    let code = [
        rgb(120, 220, 140),
        rgb(255, 140, 190),
        rgb(255, 214, 102),
        rgb(130, 180, 255),
    ];
    for i in 0..9 {
        let y = 0.218 + i as f64 * 0.0085;
        let x0 = 0.31 + (i % 3) as f64 * 0.006;
        let len = 0.02 + r.f() * 0.05;
        a.line(
            &[[x0, y], [(x0 + len).min(0.39), y]],
            0.0025,
            code[i % code.len()],
        );
    }
    a.rect([0.345, 0.3], [0.355, 0.385], rgb(30, 30, 34));
    // The big monitor: its screen is the next scene.
    a.rrect([0.44, 0.16], [0.7, 0.33], 0.008, rgb(14, 14, 18));
    a.rect([0.564, 0.33], [0.576, 0.375], rgb(30, 30, 34));
    a.ellipse([0.57, 0.38], 0.04, 0.006, rgb(30, 30, 34));
    // Keyboard, mouse, a drawing tablet and pen, a mug, a can, the lamp.
    a.rrect([0.47, 0.37], [0.62, 0.384], 0.003, rgb(30, 30, 36));
    for k in 0..14 {
        for row in 0..2 {
            let x = 0.476 + k as f64 * 0.0098;
            let y = 0.373 + row as f64 * 0.005;
            a.rect([x, y], [x + 0.007, y + 0.0035], rgb(70, 70, 80));
        }
    }
    a.ellipse([0.64, 0.378], 0.008, 0.005, rgb(40, 40, 46));
    a.rrect([0.66, 0.368], [0.75, 0.385], 0.003, rgb(36, 36, 44));
    a.rect([0.67, 0.371], [0.74, 0.382], rgb(60, 60, 72));
    a.line(&[[0.72, 0.36], [0.745, 0.378]], 0.003, rgb(200, 40, 90));
    a.rrect([0.36, 0.355], [0.38, 0.385], 0.003, rgb(240, 236, 228));
    a.arc(
        [0.38, 0.37],
        0.008,
        -PI / 2.0,
        PI / 2.0,
        0.003,
        rgb(240, 236, 228),
    );
    a.text([0.364, 0.365], 0.006, rgb(200, 40, 90), "OG");
    for k in 0..3 {
        let x = 0.364 + k as f64 * 0.006;
        a.taper(
            &[
                [x, 0.35],
                [x + 0.004, 0.335],
                [x - 0.002, 0.32],
                [x + 0.003, 0.305],
            ],
            0.0025,
            rgba(230, 230, 240, 150),
        );
    }
    a.rrect([0.795, 0.355], [0.81, 0.385], 0.002, rgb(60, 200, 110));
    a.text([0.797, 0.365], 0.006, INK, "ZAP");
    // Desk lamp and its light.
    a.ellipse([0.84, 0.383], 0.025, 0.005, rgb(40, 40, 46));
    a.line(
        &[[0.84, 0.38], [0.86, 0.3], [0.8, 0.255]],
        0.006,
        rgb(40, 40, 46),
    );
    a.fill(
        &[[0.785, 0.24], [0.82, 0.25], [0.81, 0.275], [0.775, 0.262]],
        rgb(220, 60, 70),
    );
    a.fill(
        &[[0.78, 0.268], [0.81, 0.276], [0.86, 0.385], [0.7, 0.385]],
        rgba(255, 230, 150, 40),
    );

    // Him: hoodie, headphones, the gaming chair.
    let hoodie = rgb(58, 74, 90);
    let skin = rgb(224, 180, 138);
    // Arm out to the tablet pen.
    a.fill(
        &[
            [0.43, 0.295],
            [0.48, 0.31],
            [0.66, 0.36],
            [0.665, 0.375],
            [0.47, 0.335],
            [0.425, 0.32],
        ],
        hoodie,
    );
    a.ellipse([0.67, 0.367], 0.012, 0.008, skin);
    // Chair back, stripes, headrest.
    a.rrect([0.34, 0.27], [0.455, 0.47], 0.03, rgb(179, 38, 58));
    a.rrect([0.38, 0.29], [0.415, 0.46], 0.012, rgb(30, 30, 34));
    a.rrect([0.36, 0.255], [0.435, 0.285], 0.012, rgb(150, 30, 48));
    // Shoulders and head over the chair.
    a.fill(
        &[[0.35, 0.3], [0.37, 0.27], [0.425, 0.268], [0.445, 0.3]],
        hoodie,
    );
    a.ellipse([0.397, 0.272], 0.02, 0.008, rgb(48, 62, 76));
    a.disc([0.398, 0.235], 0.03, rgb(70, 46, 30));
    for k in 0..9 {
        let ang = -PI + k as f64 / 8.0 * PI;
        let base = [0.398 + 0.026 * ang.cos(), 0.235 + 0.026 * ang.sin()];
        let tip = [
            0.398 + 0.04 * (ang + 0.15).cos(),
            0.235 + 0.04 * (ang + 0.15).sin(),
        ];
        a.taper(&[base, tip], 0.012, rgb(70, 46, 30));
    }
    a.ellipse([0.425, 0.24], 0.006, 0.01, skin);
    a.arc(
        [0.398, 0.235],
        0.034,
        -PI * 0.95,
        -PI * 0.05,
        0.006,
        rgb(25, 25, 28),
    );
    a.ellipse([0.366, 0.24], 0.008, 0.013, rgb(25, 25, 28));
    a.ellipse([0.431, 0.24], 0.008, 0.013, rgb(25, 25, 28));
    a.disc([0.431, 0.24], 0.004, rgb(90, 220, 160));
    // Chair base.
    a.rect([0.393, 0.47], [0.403, 0.52], rgb(30, 30, 34));
    for (x0, x1) in [(0.398, 0.34), (0.398, 0.46), (0.398, 0.37), (0.398, 0.43)] {
        a.line(&[[x0, 0.52], [x1, 0.545]], 0.006, rgb(30, 30, 34));
        a.disc([x1, 0.548], 0.006, rgb(20, 20, 24));
    }
}

fn cat(a: &mut Art, c: [f64; 2]) {
    let fur = rgb(232, 140, 60);
    a.ellipse(c, 0.04, 0.016, fur);
    a.disc([c[0] + 0.035, c[1] - 0.004], 0.014, fur);
    a.fill(
        &[
            [c[0] + 0.026, c[1] - 0.014],
            [c[0] + 0.03, c[1] - 0.026],
            [c[0] + 0.036, c[1] - 0.016],
        ],
        fur,
    );
    a.fill(
        &[
            [c[0] + 0.038, c[1] - 0.016],
            [c[0] + 0.045, c[1] - 0.026],
            [c[0] + 0.047, c[1] - 0.012],
        ],
        fur,
    );
    for d in [-0.015, 0.0, 0.015] {
        a.taper(
            &[[c[0] + d, c[1] - 0.014], [c[0] + d + 0.004, c[1] + 0.0]],
            0.003,
            rgb(190, 100, 40),
        );
    }
    a.taper(
        &[
            [c[0] - 0.04, c[1]],
            [c[0] - 0.05, c[1] + 0.012],
            [c[0] - 0.03, c[1] + 0.016],
            [c[0] + 0.0, c[1] + 0.014],
        ],
        0.008,
        fur,
    );
    a.arc(
        [c[0] + 0.031, c[1] - 0.004],
        0.003,
        0.2,
        PI - 0.2,
        0.0012,
        INK,
    );
    a.arc(
        [c[0] + 0.04, c[1] - 0.004],
        0.003,
        0.2,
        PI - 0.2,
        0.0012,
        INK,
    );
    a.text([c[0] + 0.05, c[1] - 0.05], 0.008, rgb(200, 200, 230), "Z");
    a.text([c[0] + 0.06, c[1] - 0.065], 0.011, rgb(200, 200, 230), "Z");
}

fn star(a: &mut Art, c: [f64; 2], r: f64, color: u32) {
    let pts: Vec<[f64; 2]> = (0..10)
        .map(|i| {
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            let t = -PI / 2.0 + i as f64 * PI / 5.0;
            [c[0] + rr * t.cos(), c[1] + rr * t.sin()]
        })
        .collect();
    a.fill(&pts, color);
}

// ---- scene 1: the app on his screen, sketching his life --------------------

fn app_and_life(a: &mut Art) {
    let paper = rgb(244, 239, 230);
    a.rect([0.0, 0.0], [1.0, ASPECT], paper);
    // The app: gear top right, the tool panel with its colour wheel, the
    // toolbar at the bottom.
    a.disc([0.955, 0.05], 0.028, WHITE);
    a.ring([0.955, 0.05], 0.028, 0.028, 0.002, rgb(200, 196, 188));
    gear(a, [0.955, 0.05], 0.016);
    a.text_c(0.955, 0.088, 0.01, INK, "10^0.0");
    a.rrect([0.02, 0.09], [0.2, 0.58], 0.012, WHITE);
    a.rrect_line([0.02, 0.09], [0.2, 0.58], 0.012, 0.0015, rgb(214, 208, 198));
    a.text([0.035, 0.105], 0.014, INK, "BRUSH");
    a.taper(
        &[[0.04, 0.16], [0.08, 0.145], [0.12, 0.165], [0.17, 0.148]],
        0.008,
        INK,
    );
    for (i, y) in [0.2, 0.235, 0.27].iter().enumerate() {
        a.line(&[[0.04, *y], [0.18, *y]], 0.003, rgb(220, 216, 208));
        a.disc([0.07 + i as f64 * 0.04, *y], 0.007, rgb(180, 176, 168));
    }
    let wc = [0.11, 0.41];
    for k in 0..36 {
        let t0 = k as f64 / 36.0 * TAU;
        let t1 = (k + 1) as f64 / 36.0 * TAU + 0.02;
        let (rr, gg, bb) = hue(k as f64 / 36.0);
        let pts = vec![
            [wc[0] + 0.07 * t0.cos(), wc[1] + 0.07 * t0.sin()],
            [wc[0] + 0.07 * t1.cos(), wc[1] + 0.07 * t1.sin()],
            [wc[0] + 0.05 * t1.cos(), wc[1] + 0.05 * t1.sin()],
            [wc[0] + 0.05 * t0.cos(), wc[1] + 0.05 * t0.sin()],
        ];
        a.fill(&pts, rgb(rr, gg, bb));
    }
    a.disc(wc, 0.022, INK);
    for k in 0..8 {
        let t = k as f64 / 8.0 * TAU;
        let (rr, gg, bb) = hue(k as f64 / 8.0);
        a.disc(
            [wc[0] + 0.036 * t.cos(), wc[1] + 0.036 * t.sin()],
            0.006,
            rgb(rr, gg, bb),
        );
    }
    a.rrect([0.33, 0.56], [0.67, 0.608], 0.01, WHITE);
    for k in 0..9 {
        let x = 0.345 + k as f64 * 0.036;
        a.rrect(
            [x, 0.566],
            [x + 0.03, 0.6],
            0.005,
            if k == 1 {
                rgb(255, 228, 236)
            } else {
                rgb(250, 248, 244)
            },
        );
        a.rrect_line(
            [x, 0.566],
            [x + 0.03, 0.6],
            0.005,
            0.0012,
            if k == 1 {
                rgb(200, 40, 90)
            } else {
                rgb(214, 208, 198)
            },
        );
        let c = [x + 0.015, 0.583];
        match k {
            0 => a.taper(
                &[[c[0] - 0.008, c[1] + 0.008], [c[0] + 0.008, c[1] - 0.008]],
                0.003,
                INK,
            ),
            1 => a.taper(
                &[
                    [c[0] - 0.008, c[1] + 0.004],
                    [c[0], c[1] - 0.004],
                    [c[0] + 0.008, c[1] + 0.002],
                ],
                0.004,
                rgb(200, 40, 90),
            ),
            2 => a.line(
                &[[c[0] - 0.008, c[1]], [c[0] + 0.008, c[1]]],
                0.006,
                rgba(255, 214, 0, 160),
            ),
            3 => a.rrect(
                [c[0] - 0.007, c[1] - 0.005],
                [c[0] + 0.007, c[1] + 0.005],
                0.001,
                rgb(240, 140, 160),
            ),
            4 => a.ring(c, 0.007, 0.007, 0.0015, INK),
            5 => star(a, c, 0.008, rgb(255, 190, 60)),
            6 => {
                a.text([c[0] - 0.007, c[1] - 0.005], 0.01, INK, "A");
            }
            7 => a.ring(c, 0.007, 0.004, 0.0015, rgb(120, 60, 190)),
            _ => a.sparkle(c, 0.008, rgb(30, 90, 200)),
        }
    }

    // On the canvas: MY LIFE, sketched with the app's own tools.
    let ink = rgb(40, 38, 48);
    let red = rgb(200, 40, 90);
    a.text([0.27, 0.05], 0.035, red, "MY LIFE");
    a.taper(
        &[[0.27, 0.098], [0.4, 0.094], [0.5, 0.1]],
        0.004,
        rgb(255, 214, 0),
    );
    a.text([0.27, 0.115], 0.013, rgb(110, 110, 125), "THE DAY JOB...");
    // The office: cubicle walls, the desk he used to have.
    a.fill(
        &[[0.25, 0.17], [0.75, 0.17], [0.75, 0.52], [0.25, 0.52]],
        rgb(236, 232, 224),
    );
    a.taper(&[[0.25, 0.17], [0.25, 0.52]], 0.004, ink);
    a.taper(&[[0.75, 0.17], [0.75, 0.52]], 0.004, ink);
    a.taper(&[[0.24, 0.17], [0.5, 0.168], [0.76, 0.171]], 0.004, ink);
    a.taper(&[[0.26, 0.4], [0.74, 0.402]], 0.004, ink);
    a.fill(
        &[[0.26, 0.4], [0.74, 0.402], [0.74, 0.415], [0.26, 0.415]],
        rgba(150, 110, 70, 120),
    );
    a.taper(&[[0.3, 0.415], [0.3, 0.5]], 0.003, ink);
    a.taper(&[[0.7, 0.415], [0.7, 0.5]], 0.003, ink);
    // Sticky notes on the cubicle wall.
    a.rect([0.27, 0.19], [0.32, 0.235], rgb(255, 230, 120));
    a.text([0.274, 0.198], 0.008, ink, "DEADLINE");
    a.text([0.274, 0.214], 0.008, ink, "FRIDAY");
    a.rect([0.67, 0.19], [0.73, 0.24], rgb(255, 170, 200));
    a.text([0.674, 0.198], 0.008, ink, "AI WILL");
    a.text([0.674, 0.212], 0.008, ink, "DO IT");
    a.text([0.674, 0.226], 0.008, red, "-BOSS");
    // The office computer (its screen is the next scene), on the desk.
    a.rrect([0.462, 0.208], [0.638, 0.322], 0.006, rgb(60, 60, 70));
    a.taper(
        &[
            [0.462, 0.208],
            [0.638, 0.209],
            [0.638, 0.322],
            [0.462, 0.321],
            [0.462, 0.208],
        ],
        0.003,
        ink,
    );
    a.fill(
        &[[0.54, 0.322], [0.56, 0.322], [0.565, 0.398], [0.535, 0.398]],
        rgb(90, 90, 100),
    );
    a.ellipse([0.55, 0.399], 0.04, 0.005, rgb(90, 90, 100));
    // The robot at his desk, typing.
    let metal = rgb(176, 190, 204);
    a.rrect([0.35, 0.29], [0.43, 0.4], 0.01, metal);
    a.rrect([0.36, 0.215], [0.425, 0.285], 0.01, metal);
    a.taper(&[[0.3925, 0.215], [0.3925, 0.19]], 0.003, ink);
    a.disc([0.3925, 0.188], 0.006, red);
    a.rrect([0.368, 0.235], [0.418, 0.262], 0.006, rgb(30, 40, 60));
    a.disc([0.382, 0.248], 0.006, rgb(90, 230, 255));
    a.disc([0.404, 0.248], 0.006, rgb(90, 230, 255));
    a.taper(
        &[[0.38, 0.272], [0.3925, 0.276], [0.405, 0.272]],
        0.003,
        ink,
    );
    a.taper(&[[0.43, 0.32], [0.47, 0.37], [0.5, 0.39]], 0.009, metal);
    a.taper(&[[0.35, 0.33], [0.33, 0.37], [0.36, 0.395]], 0.009, metal);
    a.rrect_line([0.35, 0.29], [0.43, 0.4], 0.01, 0.003, ink);
    a.rrect_line([0.36, 0.215], [0.425, 0.285], 0.01, 0.003, ink);
    a.text([0.37, 0.33], 0.012, rgb(60, 70, 90), "AI");
    for k in 0..3 {
        let x = 0.47 + k as f64 * 0.012;
        a.text([x, 0.36 - k as f64 * 0.012], 0.008, rgb(30, 90, 200), "01");
    }
    // Him, walking out with his box.
    let him = [0.83, 0.42];
    a.disc([him[0], him[1] - 0.13], 0.025, rgb(224, 180, 138));
    a.taper(
        &[
            [him[0] - 0.02, him[1] - 0.15],
            [him[0], him[1] - 0.162],
            [him[0] + 0.022, him[1] - 0.148],
        ],
        0.012,
        rgb(70, 46, 30),
    );
    a.taper(
        &[
            [him[0] - 0.008, him[1] - 0.122],
            [him[0] - 0.004, him[1] - 0.118],
        ],
        0.003,
        ink,
    );
    a.taper(
        &[
            [him[0] + 0.008, him[1] - 0.122],
            [him[0] + 0.012, him[1] - 0.118],
        ],
        0.003,
        ink,
    );
    a.arc(
        [him[0] + 0.002, him[1] - 0.104],
        0.008,
        PI + 0.4,
        TAU - 0.4,
        0.002,
        ink,
    );
    a.disc([him[0] + 0.022, him[1] - 0.14], 0.004, rgb(120, 190, 255));
    a.fill(
        &[
            [him[0] - 0.03, him[1] - 0.1],
            [him[0] + 0.03, him[1] - 0.1],
            [him[0] + 0.035, him[1]],
            [him[0] - 0.035, him[1]],
        ],
        rgb(58, 74, 90),
    );
    a.taper(
        &[[him[0] - 0.015, him[1]], [him[0] - 0.02, him[1] + 0.08]],
        0.012,
        ink,
    );
    a.taper(
        &[[him[0] + 0.015, him[1]], [him[0] + 0.025, him[1] + 0.08]],
        0.012,
        ink,
    );
    a.fill(
        &[
            [him[0] - 0.05, him[1] - 0.06],
            [him[0] + 0.03, him[1] - 0.06],
            [him[0] + 0.025, him[1] - 0.005],
            [him[0] - 0.045, him[1] - 0.005],
        ],
        rgb(200, 160, 100),
    );
    a.taper(
        &[
            [him[0] - 0.05, him[1] - 0.06],
            [him[0] + 0.03, him[1] - 0.06],
        ],
        0.003,
        ink,
    );
    a.taper(
        &[
            [him[0] - 0.03, him[1] - 0.06],
            [him[0] - 0.035, him[1] - 0.09],
            [him[0] - 0.025, him[1] - 0.1],
        ],
        0.004,
        rgb(70, 160, 90),
    );
    a.taper(
        &[
            [him[0] - 0.03, him[1] - 0.06],
            [him[0] - 0.02, him[1] - 0.085],
        ],
        0.004,
        rgb(70, 160, 90),
    );
    a.rect(
        [him[0] - 0.005, him[1] - 0.08],
        [him[0] + 0.015, him[1] - 0.062],
        rgb(240, 236, 228),
    );
    a.text([him[0] - 0.12, him[1] + 0.09], 0.012, red, "REPLACED.");
    // Arrow from his old desk to the dream: "BUT AT NIGHT..."
    a.taper(&[[0.64, 0.14], [0.6, 0.16], [0.57, 0.2]], 0.004, red);
    a.taper(&[[0.57, 0.2], [0.565, 0.185]], 0.004, red);
    a.taper(&[[0.57, 0.2], [0.585, 0.195]], 0.004, red);
    a.text([0.63, 0.12], 0.012, red, "BUT HIS DREAM...");
}

fn gear(a: &mut Art, c: [f64; 2], r: f64) {
    let pts: Vec<[f64; 2]> = (0..48)
        .map(|i| {
            let t = i as f64 / 48.0 * TAU;
            let rr = if (i / 3) % 2 == 0 { r } else { r * 0.78 };
            [c[0] + rr * t.cos(), c[1] + rr * t.sin()]
        })
        .collect();
    a.fill(&pts, INK);
    a.disc(c, r * 0.42, WHITE);
}

fn hue(h: f64) -> (u8, u8, u8) {
    let f = |n: f64| {
        let k = (n + h * 6.0) % 6.0;
        let v = 1.0 - (k.min(4.0 - k).clamp(0.0, 1.0));
        (255.0 * (0.92 - 0.8 * v)) as u8
    };
    (f(5.0), f(3.0), f(1.0))
}

// ---- scene 2: the game he wants to make --------------------------------------

const ORB: u32 = rgb(42, 22, 80);

fn wizard_game(a: &mut Art) {
    let mut r = Rng(0x0b5e_55ed_7777_2222);
    let sky = [
        rgb(16, 12, 48),
        rgb(26, 18, 64),
        rgb(40, 26, 84),
        rgb(62, 36, 100),
        rgb(96, 52, 112),
    ];
    for (i, c) in sky.iter().enumerate() {
        let v0 = i as f64 * ASPECT / 5.0;
        a.rect([0.0, v0], [1.0, v0 + ASPECT / 5.0 + 0.001], *c);
    }
    for _ in 0..60 {
        let c = [r.f(), r.f() * 0.35];
        a.sparkle(
            c,
            0.002 + r.f() * 0.004,
            rgba(255, 250, 230, 200 + (r.f() * 55.0) as u8),
        );
    }
    a.glow([0.15, 0.12], 0.08, (255, 240, 200), 0.6);
    a.disc([0.15, 0.12], 0.04, rgb(250, 240, 210));
    a.disc([0.168, 0.11], 0.035, rgb(26, 18, 64));
    // Title, in gold.
    a.text_c(0.5, 0.035, 0.05, rgb(255, 210, 90), "WIZARD QUEST");
    a.text_c(
        0.5,
        0.1,
        0.014,
        rgb(220, 200, 255),
        "A GAME BY ME - SOMEDAY",
    );
    // Mountains, far and near.
    a.fill(
        &[
            [0.0, 0.45],
            [0.12, 0.3],
            [0.22, 0.4],
            [0.35, 0.26],
            [0.5, 0.42],
            [0.62, 0.33],
            [0.78, 0.44],
            [0.9, 0.31],
            [1.0, 0.4],
            [1.0, ASPECT],
            [0.0, ASPECT],
        ],
        rgb(46, 30, 90),
    );
    for (x, y) in [(0.35, 0.26), (0.9, 0.31), (0.12, 0.3)] {
        a.fill(
            &[
                [x, y],
                [x + 0.035, y + 0.04],
                [x + 0.01, y + 0.035],
                [x - 0.012, y + 0.042],
                [x - 0.03, y + 0.035],
            ],
            rgb(220, 220, 255),
        );
    }
    // The castle on the far hill, lit.
    let cx = 0.82;
    a.fill(
        &[
            [cx - 0.06, 0.44],
            [cx + 0.06, 0.44],
            [cx + 0.06, 0.36],
            [cx - 0.06, 0.36],
        ],
        rgb(30, 20, 60),
    );
    for dx in [-0.06, -0.015, 0.03] {
        a.rect([cx + dx, 0.32], [cx + dx + 0.03, 0.44], rgb(30, 20, 60));
        a.fill(
            &[
                [cx + dx - 0.004, 0.32],
                [cx + dx + 0.034, 0.32],
                [cx + dx + 0.015, 0.28],
            ],
            rgb(130, 40, 80),
        );
        a.rect(
            [cx + dx + 0.011, 0.34],
            [cx + dx + 0.019, 0.355],
            rgb(255, 200, 90),
        );
    }
    a.fill(
        &[
            [0.0, 0.5],
            [0.2, 0.47],
            [0.45, 0.52],
            [0.7, 0.49],
            [1.0, 0.53],
            [1.0, ASPECT],
            [0.0, ASPECT],
        ],
        rgb(28, 18, 56),
    );
    // The cliff and the wizard on it.
    a.fill(
        &[
            [0.0, 0.42],
            [0.18, 0.4],
            [0.32, 0.43],
            [0.36, 0.5],
            [0.34, ASPECT],
            [0.0, ASPECT],
        ],
        rgb(20, 14, 40),
    );
    let robe = rgb(110, 50, 180);
    let wz = [0.24, 0.4];
    a.fill(
        &[
            [wz[0] - 0.05, wz[1]],
            [wz[0] + 0.045, wz[1]],
            [wz[0] + 0.02, wz[1] - 0.15],
            [wz[0] - 0.015, wz[1] - 0.15],
        ],
        robe,
    );
    a.fill(
        &[
            [wz[0] - 0.05, wz[1]],
            [wz[0] - 0.02, wz[1] - 0.06],
            [wz[0] - 0.01, wz[1]],
        ],
        rgb(84, 36, 140),
    );
    for k in 0..5 {
        star(
            a,
            [
                wz[0] - 0.03 + k as f64 * 0.015,
                wz[1] - 0.03 - (k % 2) as f64 * 0.04,
            ],
            0.007,
            rgb(255, 214, 102),
        );
    }
    a.disc([wz[0], wz[1] - 0.165], 0.022, rgb(240, 200, 160));
    a.fill(
        &[
            [wz[0] - 0.022, wz[1] - 0.16],
            [wz[0] + 0.022, wz[1] - 0.16],
            [wz[0] + 0.01, wz[1] - 0.08],
            [wz[0], wz[1] - 0.06],
            [wz[0] - 0.012, wz[1] - 0.085],
        ],
        rgb(240, 240, 250),
    );
    a.fill(
        &[
            [wz[0] - 0.045, wz[1] - 0.175],
            [wz[0] + 0.045, wz[1] - 0.175],
            [wz[0] + 0.03, wz[1] - 0.19],
            [wz[0] + 0.03, wz[1] - 0.22],
            [wz[0] + 0.06, wz[1] - 0.27],
            [wz[0] - 0.01, wz[1] - 0.23],
            [wz[0] - 0.025, wz[1] - 0.19],
        ],
        robe,
    );
    star(a, [wz[0] + 0.005, wz[1] - 0.205], 0.009, rgb(255, 214, 102));
    a.disc([wz[0] - 0.008, wz[1] - 0.168], 0.0035, INK);
    a.disc([wz[0] + 0.008, wz[1] - 0.168], 0.0035, INK);
    // The staff, raised to the spell.
    a.taper(
        &[
            [wz[0] + 0.03, wz[1] - 0.01],
            [wz[0] + 0.12, wz[1] - 0.12],
            [0.5, 0.28],
        ],
        0.009,
        rgb(120, 80, 40),
    );
    a.fill(
        &[
            [wz[0] + 0.03, wz[1] - 0.12],
            [wz[0] + 0.07, wz[1] - 0.13],
            [wz[0] + 0.1, wz[1] - 0.115],
            [wz[0] + 0.06, wz[1] - 0.09],
        ],
        robe,
    );
    a.disc([wz[0] + 0.1, wz[1] - 0.118], 0.01, rgb(240, 200, 160));
    // The spell: a glowing orb with runes around it; the next scene is in it.
    let oc = [0.62, 0.3];
    a.glow(oc, 0.2, (200, 140, 255), 0.7);
    a.glow(oc, 0.13, (120, 220, 255), 0.6);
    for k in 0..24 {
        let t = k as f64 / 24.0 * TAU;
        let len = 0.12 + 0.03 * ((k * 7) % 5) as f64 / 5.0;
        a.taper(
            &[
                [oc[0] + 0.095 * t.cos(), oc[1] + 0.095 * t.sin()],
                [oc[0] + len * t.cos(), oc[1] + len * t.sin()],
            ],
            0.004,
            rgba(220, 200, 255, 180),
        );
    }
    a.disc(oc, 0.092, rgb(150, 110, 230));
    a.disc(oc, 0.088, ORB);
    a.ring(oc, 0.1, 0.1, 0.002, rgb(255, 214, 102));
    let runes = ["ᚠ", "ᚢ", "ᚦ", "ᚨ", "ᚱ", "ᚲ"];
    for (k, _) in runes.iter().enumerate() {
        let t = k as f64 / runes.len() as f64 * TAU + 0.3;
        let c = [oc[0] + 0.112 * t.cos(), oc[1] + 0.112 * t.sin()];
        a.taper(
            &[
                [c[0] - 0.006, c[1] - 0.008],
                [c[0], c[1] + 0.008],
                [c[0] + 0.006, c[1] - 0.008],
            ],
            0.0025,
            rgb(255, 214, 102),
        );
        a.taper(
            &[[c[0] - 0.004, c[1]], [c[0] + 0.004, c[1]]],
            0.0025,
            rgb(255, 214, 102),
        );
    }
    // Sparkles trailing from staff to orb.
    for k in 0..14 {
        let t = k as f64 / 13.0;
        let c = [
            0.37 + (oc[0] - 0.1 - 0.37) * t + 0.01 * (t * 9.0).sin(),
            0.28 - 0.02 * (t * PI).sin(),
        ];
        a.sparkle(c, 0.004 + 0.004 * r.f(), rgb(255, 240, 180));
    }
    a.text([0.73, 0.5], 0.012, rgb(255, 214, 102), "SPELL: ZOOM");
    a.text(
        [0.73, 0.52],
        0.009,
        rgb(220, 200, 255),
        "SEE WHO MAKES THE MAGIC",
    );
}

// ---- scene 3: the god who makes spells -----------------------------------------

const GOD_SKIN: u32 = rgb(242, 211, 166);

fn god_of_spells(a: &mut Art) {
    let mut r = Rng(0x60d0_f5be_11e5_3333);
    a.rect([0.0, 0.0], [1.0, ASPECT], ORB);
    // Nebulae and stars.
    for (c, rr, col) in [
        ([0.2, 0.15], 0.25, (220, 80, 200)),
        ([0.8, 0.45], 0.3, (60, 160, 255)),
        ([0.55, 0.55], 0.2, (255, 120, 90)),
    ] {
        a.glow(c, rr, col, 0.45);
    }
    for _ in 0..80 {
        a.sparkle(
            [r.f(), r.f() * ASPECT],
            0.002 + r.f() * 0.004,
            rgba(255, 250, 235, 220),
        );
    }
    // Constellations: a wizard hat and a star.
    let con = [[0.08, 0.5], [0.12, 0.42], [0.16, 0.5], [0.08, 0.5]];
    a.line(&con, 0.0015, rgba(200, 200, 255, 160));
    for p in con {
        a.disc(p, 0.004, rgb(255, 255, 255));
    }
    // The halo and the god.
    let head = [0.5, 0.22];
    a.glow(head, 0.28, (255, 220, 120), 0.8);
    a.ring(head, 0.14, 0.14, 0.008, rgb(255, 214, 102));
    a.ring(head, 0.155, 0.155, 0.002, rgb(255, 240, 180));
    // Robe and shoulders, with golden trim.
    a.fill(
        &[
            [0.28, ASPECT],
            [0.32, 0.36],
            [0.42, 0.31],
            [0.58, 0.31],
            [0.68, 0.36],
            [0.72, ASPECT],
        ],
        rgb(248, 244, 236),
    );
    a.fill(
        &[[0.47, 0.31], [0.53, 0.31], [0.52, ASPECT], [0.48, ASPECT]],
        rgb(255, 214, 102),
    );
    for k in 0..6 {
        let x = 0.33 + k as f64 * 0.07;
        a.taper(
            &[[x, 0.4], [x + 0.01, 0.5], [x - 0.005, 0.6]],
            0.004,
            rgb(214, 206, 190),
        );
    }
    // Hair and beard, flowing.
    let hair = rgb(240, 240, 248);
    a.fill(
        &[
            [0.41, 0.17],
            [0.44, 0.1],
            [0.5, 0.08],
            [0.56, 0.1],
            [0.59, 0.17],
            [0.61, 0.32],
            [0.39, 0.32],
        ],
        hair,
    );
    // The face.
    a.ellipse(head, 0.06, 0.075, GOD_SKIN);
    // Brows.
    a.taper(&[[0.44, 0.188], [0.465, 0.18], [0.49, 0.19]], 0.008, hair);
    a.taper(&[[0.51, 0.19], [0.535, 0.18], [0.56, 0.188]], 0.008, hair);
    // The right eye (the left one is the next scene, drawn there).
    // Sized like the other: scene 4's eye at this scale.
    a.ellipse([0.535, 0.215], 0.0128, 0.0052, WHITE);
    a.disc([0.535, 0.215], 0.0054, rgb(214, 150, 40));
    a.disc([0.535, 0.215], 0.0035, INK);
    a.sparkle([0.5325, 0.2125], 0.0015, WHITE);
    // Nose, beard, a kind smile.
    a.taper(
        &[[0.5, 0.215], [0.495, 0.245], [0.505, 0.25]],
        0.003,
        rgb(200, 160, 120),
    );
    a.fill(
        &[
            [0.44, 0.25],
            [0.56, 0.25],
            [0.58, 0.3],
            [0.55, 0.38],
            [0.5, 0.42],
            [0.45, 0.38],
            [0.42, 0.3],
        ],
        hair,
    );
    for k in 0..7 {
        let x = 0.45 + k as f64 * 0.017;
        a.taper(
            &[[x, 0.27], [x + 0.003, 0.33], [x + (x - 0.5) * 0.1, 0.39]],
            0.0025,
            rgb(214, 214, 228),
        );
    }
    a.taper(
        &[[0.47, 0.262], [0.5, 0.272], [0.53, 0.262]],
        0.004,
        rgb(170, 90, 80),
    );
    // Hands: one forging a spell over a star anvil, one letting spells go.
    let hand = GOD_SKIN;
    a.fill(
        &[
            [0.32, 0.36],
            [0.22, 0.4],
            [0.18, 0.44],
            [0.2, 0.47],
            [0.27, 0.44],
            [0.34, 0.42],
        ],
        rgb(248, 244, 236),
    );
    a.ellipse([0.19, 0.455], 0.025, 0.018, hand);
    a.fill(
        &[
            [0.68, 0.36],
            [0.78, 0.4],
            [0.82, 0.44],
            [0.8, 0.47],
            [0.73, 0.44],
            [0.66, 0.42],
        ],
        rgb(248, 244, 236),
    );
    a.ellipse([0.81, 0.455], 0.025, 0.018, hand);
    // The anvil of starlight and the spell being made on it.
    a.fill(
        &[
            [0.1, 0.56],
            [0.24, 0.56],
            [0.22, 0.54],
            [0.2, 0.52],
            [0.13, 0.52],
            [0.11, 0.54],
        ],
        rgb(120, 140, 220),
    );
    a.glow([0.17, 0.5], 0.06, (140, 230, 255), 0.9);
    a.disc([0.17, 0.5], 0.015, rgb(200, 245, 255));
    for k in 0..8 {
        let t = k as f64 / 8.0 * TAU;
        a.taper(
            &[
                [0.17 + 0.02 * t.cos(), 0.5 + 0.02 * t.sin()],
                [0.17 + 0.045 * t.cos(), 0.5 + 0.045 * t.sin()],
            ],
            0.003,
            rgb(200, 245, 255),
        );
    }
    // Spells drifting down to people on a floating island.
    let colors = [
        (255, 120, 160),
        (120, 220, 255),
        (255, 214, 102),
        (160, 255, 160),
        (200, 140, 255),
    ];
    for k in 0..5 {
        let c = [0.84 + (k as f64 - 2.0) * 0.03, 0.5 + k as f64 * 0.015];
        let col = colors[k];
        a.glow(c, 0.02, col, 0.8);
        a.disc(c, 0.006, rgb(col.0, col.1, col.2));
    }
    a.fill(
        &[
            [0.76, 0.6],
            [0.96, 0.6],
            [0.93, 0.615],
            [0.86, ASPECT],
            [0.8, 0.615],
        ],
        rgb(90, 70, 120),
    );
    a.ellipse([0.86, 0.6], 0.1, 0.008, rgb(90, 180, 110));
    for k in 0..4 {
        let x = 0.8 + k as f64 * 0.035;
        let body = [
            rgb(230, 90, 90),
            rgb(60, 140, 220),
            rgb(240, 180, 60),
            rgb(120, 200, 120),
        ][k];
        a.disc([x, 0.575], 0.006, rgb(230, 190, 150));
        a.fill(
            &[
                [x - 0.007, 0.598],
                [x + 0.007, 0.598],
                [x + 0.004, 0.582],
                [x - 0.004, 0.582],
            ],
            body,
        );
        a.taper(&[[x + 0.004, 0.585], [x + 0.012, 0.57]], 0.002, INK);
        a.sparkle([x + 0.013, 0.568], 0.004, rgb(255, 240, 160));
    }
    a.text_c(
        0.5,
        0.025,
        0.018,
        rgb(255, 230, 160),
        "THE ONE WHO MAKES SPELLS FOR US",
    );
}

// ---- scene 4: the god's eye -------------------------------------------------------

fn gods_eye(a: &mut Art) {
    a.rect([0.0, 0.0], [1.0, ASPECT], GOD_SKIN);
    // Shading around the eye, the brow above.
    a.ellipse([0.5, 0.32], 0.46, 0.22, rgb(232, 196, 150));
    for k in 0..26 {
        let t = k as f64 / 25.0;
        let x = 0.12 + t * 0.76;
        let y = 0.1 - 0.05 * (t * PI).sin();
        a.taper(
            &[[x, y + 0.02], [x + 0.03, y - 0.005], [x + 0.06, y + 0.01]],
            0.012,
            rgb(240, 240, 248),
        );
    }
    // The eye: white, then the iris rings, streaks, the pupil.
    let c = [0.5, 0.31];
    let lid: Vec<[f64; 2]> = (0..=40)
        .map(|i| {
            let t = i as f64 / 40.0 * TAU;
            [
                c[0] + 0.32 * t.cos(),
                c[1] + 0.13 * t.sin() * (1.0 - 0.2 * t.cos().abs()),
            ]
        })
        .collect();
    a.fill(&lid, rgb(250, 247, 240));
    a.ellipse([c[0], c[1] + 0.09], 0.28, 0.03, rgba(214, 170, 140, 90));
    a.disc(c, 0.135, rgb(150, 90, 20));
    a.disc(c, 0.125, rgb(214, 150, 40));
    a.disc(c, 0.1, rgb(240, 186, 70));
    let mut r = Rng(0xeeee_0001_1234_4444);
    for k in 0..72 {
        let t = k as f64 / 72.0 * TAU + r.f() * 0.04;
        let r0 = 0.09 + r.f() * 0.01;
        let r1 = 0.125 - r.f() * 0.015;
        let col = if k % 3 == 0 {
            rgb(120, 70, 10)
        } else {
            rgb(255, 214, 120)
        };
        a.taper(
            &[
                [c[0] + r0 * t.cos(), c[1] + r0 * t.sin()],
                [c[0] + r1 * t.cos(), c[1] + r1 * t.sin()],
            ],
            0.003,
            col,
        );
    }
    a.disc(c, 0.088, rgb(10, 8, 14));
    // Upper and lower lids, lashes.
    let upper: Vec<[f64; 2]> = (0..=30)
        .map(|i| {
            let t = PI + i as f64 / 30.0 * PI;
            [c[0] + 0.33 * t.cos(), c[1] + 0.14 * t.sin()]
        })
        .collect();
    a.taper(&upper, 0.02, rgb(120, 70, 50));
    for (i, p) in upper.iter().enumerate().step_by(2) {
        let t = PI + i as f64 / 30.0 * PI;
        a.taper(
            &[*p, [p[0] + 0.05 * t.cos(), p[1] + 0.06 * t.sin() - 0.01]],
            0.006,
            rgb(60, 40, 30),
        );
    }
    let lower: Vec<[f64; 2]> = (0..=30)
        .map(|i| {
            let t = i as f64 / 30.0 * PI;
            [c[0] + 0.32 * t.cos(), c[1] + 0.125 * t.sin()]
        })
        .collect();
    a.taper(&lower, 0.008, rgb(190, 130, 100));
    // Catchlights; the reflection in the pupil is the portal, added apart.
    a.ellipse([0.43, 0.25], 0.03, 0.02, rgba(255, 255, 255, 220));
    a.sparkle([0.58, 0.37], 0.015, rgba(255, 255, 255, 200));
    a.text_c(
        0.5,
        0.55,
        0.016,
        rgb(150, 100, 60),
        "IN HIS EYE: WHERE IT ALL BEGINS",
    );
}

/// The reflection in the god's pupil: a portal showing the man cave, so
/// zooming into it lands at the start.
fn loop_portal(scene: &mut Scene, at: &Frame, start: &Frame) -> crate::objects::Group {
    let home = CellAddr::new(0, 0, 0);
    let view_px = 900.0;
    let data = crate::objects::ObjData::Portal {
        style: crate::shapes::ShapeStyle {
            kind: crate::shapes::ShapeKind::Rect,
            fill: crate::objects::PORTAL_PAPER,
            opacity: 0,
            ..Default::default()
        },
        geom: crate::shapes::Geom {
            center: at.centre(),
            half: [at.s * 0.5, at.s * ASPECT * 0.5],
            rot: 0.0,
            pts: vec![],
        },
        width: at.s * 1e-3,
        seed: 7,
        view: crate::objects::PortalView {
            name: TITLES[0].into(),
            cam: start.camera(view_px),
            view_px,
        },
    };
    let z = scene.z_top + 1.0;
    let ids = crate::objects::emit(scene, &home, &data, (z, z + 4.0));
    crate::objects::Group {
        cell: home,
        data,
        strokes: ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_scene_sits_inside_the_one_before_and_the_eye_leads_home() {
        let (fs, portal) = frames(ORIGIN);
        for w in fs.windows(2) {
            let (outer, inner) = (w[0], w[1]);
            assert!(inner.s < outer.s * 0.3, "each scene is much smaller");
            assert!(inner.o[0] >= outer.o[0] && inner.o[1] >= outer.o[1]);
            assert!(inner.o[0] + inner.s <= outer.o[0] + outer.s);
            assert!(inner.o[1] + inner.s * ASPECT <= outer.o[1] + outer.s * ASPECT);
        }
        // The portal is inside the last scene and shows the first one whole.
        assert!(portal.s < fs[4].s * 0.2);
        let mut scene = Scene::new();
        let g = draw(&mut scene, ORIGIN);
        let crate::objects::ObjData::Portal { view, geom, .. } = &g.data else {
            panic!()
        };
        assert_eq!(view.name, TITLES[0]);
        assert!(
            (geom.half[1] / geom.half[0] - ASPECT).abs() < 1e-9,
            "same shape as the scenes"
        );
        assert!(
            scene.strokes.len() > 800,
            "a rich picture: {} strokes",
            scene.strokes.len()
        );
        assert_eq!(bookmarks(ORIGIN).len(), 5);
    }
}
