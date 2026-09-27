// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Synthetic test canvases for the Phase 0 spike.
//!
//! * A "mass page": a handwriting-like page of `mass` strokes (1M by default),
//!   one letter-sized stroke per level-12 cell, laid out in lines and words.
//! * A "zoom chain": `depth` nested scenes, each 16x smaller than the last and
//!   circled in its parent, labelled with its depth. 40 steps reach ~10^48 zoom.

use crate::addr::CellAddr;
use crate::scene::Scene;

pub const MAX_PTS: usize = 16;

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | (a as u32) << 24
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
    pub fn int(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A cursive-looking word fragment inside [x, x+w] x [y, y+h]: a left-to-right
/// wave with a second harmonic, so it reads like joined-up handwriting.
pub fn squiggle(rng: &mut Rng, x: f32, y: f32, w: f32, h: f32, n: usize) -> Vec<[f32; 2]> {
    let n = n.clamp(2, MAX_PTS);
    let tau = std::f32::consts::TAU;
    let f = rng.range(1.0, 2.6);
    let (p1, p2) = (rng.range(0.0, tau), rng.range(0.0, tau));
    let (a1, a2) = (rng.range(0.25, 0.4), rng.range(0.05, 0.15));
    let slant = rng.range(0.05, 0.2);
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            let wave = a1 * (tau * f * t + p1).sin() + a2 * (tau * 2.3 * f * t + p2).sin();
            let yy = 0.5 + wave;
            [x + w * (t + slant * (0.5 - yy)), y + h * yy]
        })
        .collect()
}

/// Seven-segment strokes for a decimal number with its top-left at (x, y).
pub fn digits(n: u64, x: f32, y: f32, dw: f32) -> Vec<Vec<[f32; 2]>> {
    // Segments: a top, b top-right, c bottom-right, d bottom, e bottom-left, f top-left, g middle.
    const SEG: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let dh = dw * 1.8;
    let mut out = Vec::new();
    for (i, ch) in n.to_string().bytes().enumerate() {
        let m = SEG[(ch - b'0') as usize];
        let ox = x + i as f32 * dw * 1.5;
        let p = |fx: f32, fy: f32| [ox + fx * dw, y + fy * dh];
        let segs: [(u8, [f32; 2], [f32; 2]); 7] = [
            (0x01, p(0.0, 0.0), p(1.0, 0.0)),
            (0x02, p(1.0, 0.0), p(1.0, 0.5)),
            (0x04, p(1.0, 0.5), p(1.0, 1.0)),
            (0x08, p(0.0, 1.0), p(1.0, 1.0)),
            (0x10, p(0.0, 0.5), p(0.0, 1.0)),
            (0x20, p(0.0, 0.0), p(0.0, 0.5)),
            (0x40, p(0.0, 0.5), p(1.0, 0.5)),
        ];
        for (bit, a, b) in segs {
            if m & bit != 0 {
                out.push(vec![a, b]);
            }
        }
    }
    out
}

/// A circle as two open half-arcs (each within MAX_PTS points).
pub fn ring(cx: f32, cy: f32, r: f32) -> Vec<Vec<[f32; 2]>> {
    (0..2)
        .map(|h| {
            (0..MAX_PTS)
                .map(|i| {
                    let t = (h as f32 + i as f32 / (MAX_PTS - 1) as f32) * std::f32::consts::PI;
                    [cx + r * t.cos(), cy + r * t.sin()]
                })
                .collect()
        })
        .collect()
}

pub struct Demo {
    pub scene: Scene,
    /// Cells of the zoom chain, outermost first; auto-zoom flies through them.
    pub chain: Vec<CellAddr>,
    /// Camera start: the level-0 cell holding both pages side by side.
    pub home: CellAddr,
}

pub fn build(mass: usize, depth: usize, seed: u64) -> Demo {
    let mut scene = Scene::new();
    let mut rng = Rng::new(seed);
    let ink = [
        rgba(30, 30, 40, 255),
        rgba(20, 60, 160, 255),
        rgba(170, 30, 40, 255),
        rgba(20, 120, 70, 255),
    ];

    // Page frames so each page reads as a sheet when zoomed out.
    for px in 0..2 {
        let page = CellAddr::new(0, px, 0);
        let frame = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
        scene.add_stroke(&page, &frame, 0.002, rgba(150, 150, 160, 255));
    }

    // Mass page: level-12 cells inside page (0, 1, 0), lines of words.
    const L: i64 = 12;
    let per = 1i64 << L;
    let base_x = per; // page x = 1
    let mut placed = 0usize;
    let mut line = 0i64;
    'outer: while placed < mass {
        let row = 40 + line * 10;
        if row >= per - 40 {
            break; // page full (~1.3M strokes)
        }
        let mut col = 60 + rng.int(20) as i64;
        let color = ink[(line as usize / 40) % ink.len()];
        while col < per - 60 {
            let word = 2 + rng.int(8) as i64;
            for i in 0..word {
                let cell = CellAddr::new(L, base_x + col + i, row);
                let n = 8 + rng.int(8) as usize;
                let pts = squiggle(&mut rng, 0.05, 0.1, 0.9, 0.8, n);
                scene.add_stroke(&cell, &pts, 0.08, color);
                placed += 1;
                if placed >= mass {
                    break 'outer;
                }
            }
            col += word + 1 + rng.int(2) as i64;
        }
        line += 1;
    }

    // Zoom chain in page (0, 0, 0): step k lives at level 2 + 4k.
    let mut chain = Vec::with_capacity(depth);
    let mut cell = CellAddr::new(2, 1, 1);
    for k in 0..depth {
        chain.push(cell.clone());
        let tx = 5 + rng.int(6) as i64;
        let ty = 7 + rng.int(4) as i64;
        let (cx, cy) = ((tx as f32 + 0.5) / 16.0, (ty as f32 + 0.5) / 16.0);
        let color = ink[k % ink.len()];
        for arc in ring(cx, cy, 1.3 / 16.0) {
            scene.add_stroke(&cell, &arc, 0.004, rgba(220, 120, 20, 255));
        }
        for seg in digits(k as u64, 0.06, 0.06, 0.045) {
            scene.add_stroke(&cell, &seg, 0.012, color);
        }
        // A few lines of "writing" above the ring.
        for w in 0..24 {
            let x = 0.3 + (w % 8) as f32 * 0.08;
            let y = 0.08 + (w / 8) as f32 * 0.07;
            let pts = squiggle(&mut rng, x, y, 0.06, 0.05, 10);
            scene.add_stroke(&cell, &pts, 0.006, color);
        }
        cell = CellAddr {
            level: cell.level + 4,
            x: (&cell.x << 4u32) + tx,
            y: (&cell.y << 4u32) + ty,
        };
    }

    Demo {
        scene,
        chain,
        home: CellAddr::new(-1, 0, 0),
    }
}
