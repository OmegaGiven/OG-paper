// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Symbols (Menu › Symbols, the Diagram symbols plugin, and `{"add":
//! "symbol"}`): flowchart, electrical and mechanical symbols, like
//! draw.io's. A symbol is a group of plain shapes and texts with pins
//! (`ShapeKind::Pin`): in diagram mode, lines and arrows drawn to a pin
//! join it and stay on it as the symbol moves, turns or grows.
//!
//! Each is drawn on a grid (one unit is ten points at its usual size) in a
//! small language, one element per `;`:
//!
//! - `L x y x y …` a line through the points; `P …` a closed outline;
//!   `F …` a filled outline
//! - `C cx cy r` a circle; `D cx cy r` a dot (filled); `E cx cy rx ry` an
//!   ellipse
//! - `R x y w h` a rectangle
//! - `A cx cy r from to` an arc (degrees, clockwise from the right, y down)
//! - `T x y size text` words centred on (x, y)
//! - `N x y` a pin

use crate::objects::{ObjData, ObjRef};
use crate::shapes::{self, FillStyle, Geom, ShapeKind, ShapeStyle, Sloppiness};
use crate::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    General,
    Electrical,
    Mechanical,
}

impl Category {
    pub const ALL: [Category; 3] = [
        Category::General,
        Category::Electrical,
        Category::Mechanical,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::General => "Flowchart",
            Category::Electrical => "Electrical",
            Category::Mechanical => "Mechanical",
        }
    }
}

pub struct Symbol {
    /// Its name in commands (and, with `-` as spaces, in the panel).
    pub key: &'static str,
    pub cat: Category,
    pub spec: &'static str,
}

use Category::*;

macro_rules! sym {
    ($key:expr, $cat:expr, $spec:expr) => {
        Symbol {
            key: $key,
            cat: $cat,
            spec: $spec,
        }
    };
}

/// Pins at the middles of a 12 x 6 box's sides.
macro_rules! box_pins {
    () => {
        "N 0 3; N 12 3; N 6 0; N 6 6"
    };
}

pub const SYMBOLS: &[Symbol] = &[
    // ---- Flowchart ----------------------------------------------------------
    sym!("process", General, concat!("R 0 0 12 6;", box_pins!())),
    sym!("decision", General, concat!("P 6 0 12 3 6 6 0 3;", box_pins!())),
    sym!(
        "terminator",
        General,
        concat!("A 3 3 3 90 270; L 3 0 9 0; A 9 3 3 270 450; L 9 6 3 6;", box_pins!())
    ),
    sym!("data", General, concat!("P 2 0 12 0 10 6 0 6;", "N 1 3; N 11 3; N 7 0; N 5 6")),
    sym!(
        "document",
        General,
        concat!(
            "L 0 5.2 0 0 12 0 12 5.2; L 0 5.2 1.5 5.8 3 6 4.5 5.8 6 5.2 7.5 4.6 9 4.4 10.5 4.6 12 5.2;",
            "N 0 3; N 12 3; N 6 0; N 6 5.2"
        )
    ),
    sym!(
        "database",
        General,
        concat!(
            "E 4 1 4 1; L 0 1 0 7; L 8 1 8 7; A 4 7 4 0 180;",
            "N 0 4; N 8 4; N 4 0; N 4 8"
        )
    ),
    sym!(
        "predefined-process",
        General,
        concat!("R 0 0 12 6; L 1.2 0 1.2 6; L 10.8 0 10.8 6;", box_pins!())
    ),
    sym!("manual-input", General, concat!("P 0 1.5 12 0 12 6 0 6;", "N 0 3.75; N 12 3; N 6 0.75; N 6 6")),
    sym!("preparation", General, concat!("P 2 0 10 0 12 3 10 6 2 6 0 3;", box_pins!())),
    sym!("delay", General, concat!("L 9 0 0 0 0 6 9 6; A 9 3 3 270 450;", box_pins!())),
    sym!("connector", General, "C 3 3 3; N 0 3; N 6 3; N 3 0; N 3 6"),
    sym!("off-page", General, "P 0 0 6 0 6 4 3 6 0 4; N 0 2; N 6 2; N 3 0; N 3 6"),
    sym!("note", General, concat!("P 0 0 9.5 0 12 2.5 12 6 0 6; L 9.5 0 9.5 2.5 12 2.5;", box_pins!())),
    sym!(
        "actor",
        General,
        "C 2 1 1; L 2 2 2 4.6; L 0 3 4 3; L 2 4.6 0.4 7; L 2 4.6 3.6 7; N 2 0; N 2 7; N 0 3; N 4 3"
    ),
    // ---- Electrical (pins at the ends of the leads) ---------------------------
    sym!(
        "resistor",
        Electrical,
        "L 0 1 1.2 1 1.5 0.2 2.1 1.8 2.7 0.2 3.3 1.8 3.9 0.2 4.5 1.8 4.8 1 6 1; N 0 1; N 6 1"
    ),
    sym!(
        "potentiometer",
        Electrical,
        "L 0 1 1.2 1 1.5 0.2 2.1 1.8 2.7 0.2 3.3 1.8 3.9 0.2 4.5 1.8 4.8 1 6 1; L 3 3 3 2; F 3 1.7 2.7 2.3 3.3 2.3; N 0 1; N 6 1; N 3 3"
    ),
    sym!(
        "capacitor",
        Electrical,
        "L 0 1.5 2.7 1.5; L 2.7 0 2.7 3; L 3.3 0 3.3 3; L 3.3 1.5 6 1.5; N 0 1.5; N 6 1.5"
    ),
    sym!(
        "polarized-capacitor",
        Electrical,
        "L 0 1.5 2.7 1.5; L 2.7 0 2.7 3; A 4.6 1.5 1.5 145 215; L 3.15 1.5 6 1.5; T 2 0.4 0.7 +; N 0 1.5; N 6 1.5"
    ),
    sym!(
        "inductor",
        Electrical,
        "L 0 1.5 1 1.5; A 1.5 1.5 0.5 180 360; A 2.5 1.5 0.5 180 360; A 3.5 1.5 0.5 180 360; A 4.5 1.5 0.5 180 360; L 5 1.5 6 1.5; N 0 1.5; N 6 1.5"
    ),
    sym!("diode", Electrical, "L 0 1 2 1; F 2 0 2 2 4 1; L 4 0 4 2; L 4 1 6 1; N 0 1; N 6 1"),
    sym!(
        "led",
        Electrical,
        "L 0 2 2 2; F 2 1 2 3 4 2; L 4 1 4 3; L 4 2 6 2; L 3.2 0.9 4.2 0; F 4.4 -0.2 3.9 0 4.2 0.3; L 4 1.3 5 0.4; F 5.2 0.2 4.7 0.4 5 0.7; N 0 2; N 6 2"
    ),
    sym!(
        "battery",
        Electrical,
        "L 0 1.5 2.6 1.5; L 2.6 0 2.6 3; L 3.3 0.8 3.3 2.2; L 3.3 1.5 6 1.5; T 1.9 0.4 0.7 +; N 0 1.5; N 6 1.5"
    ),
    sym!(
        "dc-source",
        Electrical,
        "L 0 1.5 1.5 1.5; C 3 1.5 1.5; L 4.5 1.5 6 1.5; T 2.3 1.5 0.9 +; T 3.8 1.5 0.9 -; N 0 1.5; N 6 1.5"
    ),
    sym!(
        "ac-source",
        Electrical,
        "L 0 1.5 1.5 1.5; C 3 1.5 1.5; L 4.5 1.5 6 1.5; L 2.1 1.5 2.4 1.05 2.7 0.95 3 1.5 3.3 2.05 3.6 1.95 3.9 1.5; N 0 1.5; N 6 1.5"
    ),
    sym!("ground", Electrical, "L 1.5 0 1.5 1.2; L 0 1.2 3 1.2; L 0.5 1.9 2.5 1.9; L 1 2.6 2 2.6; N 1.5 0"),
    sym!(
        "switch",
        Electrical,
        "L 0 1.6 1.8 1.6; D 2 1.6 0.2; L 2 1.6 4 0.4; D 4.2 1.6 0.2; L 4.2 1.6 6 1.6; N 0 1.6; N 6 1.6"
    ),
    sym!(
        "push-button",
        Electrical,
        "L 0 2 1.8 2; D 2 2 0.2; D 4 2 0.2; L 4 2 6 2; L 1.6 1.2 4.4 1.2; L 3 1.2 3 0; N 0 2; N 6 2"
    ),
    sym!(
        "lamp",
        Electrical,
        "L 0 1.5 1.5 1.5; C 3 1.5 1.5; L 1.94 0.44 4.06 2.56; L 4.06 0.44 1.94 2.56; L 4.5 1.5 6 1.5; N 0 1.5; N 6 1.5"
    ),
    sym!("fuse", Electrical, "R 1.5 0.6 3 1.2; L 0 1.2 6 1.2; N 0 1.2; N 6 1.2"),
    sym!(
        "crystal",
        Electrical,
        "L 0 1.5 2.2 1.5; L 2.2 0.3 2.2 2.7; R 2.6 0 0.8 3; L 3.8 0.3 3.8 2.7; L 3.8 1.5 6 1.5; N 0 1.5; N 6 1.5"
    ),
    sym!(
        "npn-transistor",
        Electrical,
        "C 2.3 2 1.6; L 0 2 1.6 2; L 1.6 1 1.6 3; L 1.6 1.5 2.8 0.7 2.8 0; L 1.6 2.5 2.8 3.3 2.8 4; F 2.8 3.3 2.25 3.2 2.55 2.8; N 0 2; N 2.8 0; N 2.8 4"
    ),
    sym!(
        "pnp-transistor",
        Electrical,
        "C 2.3 2 1.6; L 0 2 1.6 2; L 1.6 1 1.6 3; L 1.6 1.5 2.8 0.7 2.8 0; L 1.6 2.5 2.8 3.3 2.8 4; F 1.6 2.5 2.15 2.65 1.9 3.0; N 0 2; N 2.8 0; N 2.8 4"
    ),
    sym!(
        "op-amp",
        Electrical,
        "P 1 0 1 5 5 2.5; L 0 1.2 1 1.2; L 0 3.8 1 3.8; L 5 2.5 6 2.5; T 1.7 1.2 0.9 -; T 1.7 3.8 0.9 +; N 0 1.2; N 0 3.8; N 6 2.5"
    ),
    sym!(
        "and-gate",
        Electrical,
        "L 3 0 1 0 1 4 3 4; A 3 2 2 270 450; L 0 1 1 1; L 0 3 1 3; L 5 2 6 2; N 0 1; N 0 3; N 6 2"
    ),
    sym!(
        "nand-gate",
        Electrical,
        "L 3 0 1 0 1 4 3 4; A 3 2 2 270 450; C 5.3 2 0.3; L 0 1 1 1; L 0 3 1 3; L 5.6 2 6.5 2; N 0 1; N 0 3; N 6.5 2"
    ),
    sym!(
        "or-gate",
        Electrical,
        "L 1 0 2.5 0.1 3.8 0.7 4.6 1.4 5 2 4.6 2.6 3.8 3.3 2.5 3.9 1 4; L 1 0 1.5 1 1.7 2 1.5 3 1 4; L 0 1 1.55 1; L 0 3 1.55 3; L 5 2 6 2; N 0 1; N 0 3; N 6 2"
    ),
    sym!("not-gate", Electrical, "P 1 0.5 1 3.5 4 2; C 4.3 2 0.3; L 0 2 1 2; L 4.6 2 6 2; N 0 2; N 6 2"),
    sym!(
        "speaker",
        Electrical,
        "R 1 1 1 2; P 2 1 3.5 0 3.5 4 2 3; L 0 1.5 1 1.5; L 0 2.5 1 2.5; N 0 1.5; N 0 2.5"
    ),
    sym!("microphone", Electrical, "C 2.4 2 1.4; L 1 0.6 1 3.4; L 0 1.5 1 1.5; L 0 2.5 1 2.5; N 0 1.5; N 0 2.5"),
    sym!("antenna", Electrical, "L 1.5 4 1.5 1; L 0 0 1.5 1.5 3 0; N 1.5 4"),
    sym!(
        "motor",
        Electrical,
        "L 0 2 1.4 2; C 3 2 1.6; T 3 2 1.4 M; L 4.6 2 6 2; N 0 2; N 6 2"
    ),
    sym!(
        "transformer",
        Electrical,
        "L 0 0 1.5 0; A 1.5 0.5 0.5 270 450; A 1.5 1.5 0.5 270 450; A 1.5 2.5 0.5 270 450; A 1.5 3.5 0.5 270 450; L 1.5 4 0 4; L 2.4 0 2.4 4; L 2.8 0 2.8 4; L 5.2 0 3.7 0; A 3.7 0.5 0.5 90 270; A 3.7 1.5 0.5 90 270; A 3.7 2.5 0.5 90 270; A 3.7 3.5 0.5 90 270; L 3.7 4 5.2 4; N 0 0; N 0 4; N 5.2 0; N 5.2 4"
    ),
    // ---- Mechanical -------------------------------------------------------------
    sym!("gear", Mechanical, "GEAR; C 3 3 0.7; N 3 3"),
    sym!(
        "spring",
        Mechanical,
        "L 0 1 1 1 1.5 0 2.5 2 3.5 0 4.5 2 5.5 0 6.5 2 7 1 8 1; N 0 1; N 8 1"
    ),
    sym!(
        "damper",
        Mechanical,
        "L 0 1.5 3 1.5; L 3 0.5 3 2.5; L 2 0 5 0 5 3 2 3; L 5 1.5 8 1.5; N 0 1.5; N 8 1.5"
    ),
    sym!("bearing", Mechanical, "C 2 2 2; C 2 2 1; BALLS; N 2 2"),
    sym!("pulley", Mechanical, "C 2 2 2; C 2 2 1.6; D 2 2 0.35; N 2 2"),
    sym!(
        "belt-drive",
        Mechanical,
        "C 2 2 2; D 2 2 0.3; C 9 2 1.2; D 9 2 0.25; L 2 0 9 0.8; L 2 4 9 3.2; N 2 2; N 9 2"
    ),
    sym!("shaft", Mechanical, "R 0 0.6 8 0.8; L 0 1 8 1; N 0 1; N 8 1"),
    sym!(
        "fixed-support",
        Mechanical,
        "L 0 0 6 0; L 0.6 0 0 0.8; L 1.6 0 1 0.8; L 2.6 0 2 0.8; L 3.6 0 3 0.8; L 4.6 0 4 0.8; L 5.6 0 5 0.8; N 3 0"
    ),
    sym!(
        "pin-support",
        Mechanical,
        "P 2 1 0.6 3 3.4 3; C 2 1 0.35; L 0 3 4 3; L 0.6 3 0.2 3.6; L 1.6 3 1.2 3.6; L 2.6 3 2.2 3.6; L 3.6 3 3.2 3.6; N 2 1"
    ),
    sym!(
        "roller-support",
        Mechanical,
        "P 2 1 0.8 2.6 3.2 2.6; C 2 1 0.35; C 1.2 3 0.4; C 2.8 3 0.4; L 0 3.4 4 3.4; N 2 1"
    ),
    sym!(
        "electric-motor",
        Mechanical,
        "R 0 0 5 3; R 5 1.2 2 0.6; T 2.5 1.5 1.2 M; N 7 1.5; N 2.5 3"
    ),
    sym!("pump", Mechanical, "C 2 2 2; P 1.1 0.8 1.1 3.2 3.6 2; L -1 2 0 2; L 4 2 5 2; N -1 2; N 5 2"),
    sym!("valve", Mechanical, "P 0 0 2 1 0 2; P 4 0 2 1 4 2; L 2 1 2 -0.6; L 1.3 -0.6 2.7 -0.6; N 0 1; N 4 1"),
    sym!("check-valve", Mechanical, "F 0 0 2 1 0 2; P 4 0 2 1 4 2; N 0 1; N 4 1"),
    sym!(
        "cylinder",
        Mechanical,
        "R 0 0 6 2; L 2 0 2 2; L 2 1 9 1; N 0 1; N 9 1"
    ),
    sym!("cam", Mechanical, "E 2.4 2 2.4 1.7; D 1.5 2 0.3; N 1.5 2"),
    sym!(
        "wheel",
        Mechanical,
        "C 2 2 2; C 2 2 1.7; D 2 2 0.35; L 2 0.3 2 3.7; L 0.3 2 3.7 2; L 0.8 0.8 3.2 3.2; L 3.2 0.8 0.8 3.2; N 2 2"
    ),
];

/// A symbol by its key (or its name with spaces).
pub fn find(name: &str) -> Option<&'static Symbol> {
    let k = name.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    SYMBOLS.iter().find(|s| s.key == k)
}

impl Symbol {
    /// Its name in the panel: "Npn transistor".
    pub fn label(&self) -> String {
        let s: Vec<String> = self
            .key
            .split('-')
            .map(|w| match w {
                "led" | "dc" | "ac" | "npn" | "pnp" | "and" | "nand" | "or" | "not" => {
                    w.to_uppercase()
                }
                "polarized" => "polar.".into(),
                w => w.to_string(),
            })
            .collect();
        let s = s.join(" ").replace("op amp", "op-amp");
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => s,
        }
    }
}

/// One drawn part of a symbol, in grid units.
#[derive(Clone, Debug, PartialEq)]
pub enum Elem {
    /// Points; closed; filled.
    Line(Vec<[f64; 2]>, bool, bool),
    /// Centre, radii; filled.
    Ellipse([f64; 2], [f64; 2], bool),
    Rect([f64; 2], [f64; 2]),
    /// Centre, cap height, words.
    Text([f64; 2], f64, String),
    Pin([f64; 2]),
}

fn arc(c: [f64; 2], r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let n = (((a1 - a0).abs() / 10.0).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// The parts of a symbol's spec.
pub fn parse(spec: &str) -> Vec<Elem> {
    let mut out = Vec::new();
    for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut w = part.split_whitespace();
        let op = w.next().unwrap_or("");
        if op == "T" {
            let n: Vec<f64> = w.by_ref().take(3).filter_map(|v| v.parse().ok()).collect();
            let text: Vec<&str> = w.collect();
            if n.len() == 3 {
                out.push(Elem::Text([n[0], n[1]], n[2], text.join(" ")));
            }
            continue;
        }
        let n: Vec<f64> = w.filter_map(|v| v.parse().ok()).collect();
        let pts = |n: &[f64]| {
            n.chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| [c[0], c[1]])
                .collect::<Vec<_>>()
        };
        match (op, n.len()) {
            ("L", _) => out.push(Elem::Line(pts(&n), false, false)),
            ("P", _) => out.push(Elem::Line(pts(&n), true, false)),
            ("F", _) => out.push(Elem::Line(pts(&n), true, true)),
            ("C", 3) => out.push(Elem::Ellipse([n[0], n[1]], [n[2], n[2]], false)),
            ("D", 3) => out.push(Elem::Ellipse([n[0], n[1]], [n[2], n[2]], true)),
            ("E", 4) => out.push(Elem::Ellipse([n[0], n[1]], [n[2], n[3]], false)),
            ("R", 4) => out.push(Elem::Rect([n[0], n[1]], [n[2], n[3]])),
            ("A", 5) => out.push(Elem::Line(
                arc([n[0], n[1]], n[2], n[3], n[4]),
                false,
                false,
            )),
            ("N", 2) => out.push(Elem::Pin([n[0], n[1]])),
            ("GEAR", _) => {
                // Twelve teeth round a wheel of radius 3 about (3, 3).
                let mut p = Vec::new();
                for i in 0..12 {
                    let a = i as f64 * 30.0;
                    for (da, r) in [(-9.0, 2.4), (-6.0, 3.0), (6.0, 3.0), (9.0, 2.4)] {
                        let t = (a + da).to_radians();
                        p.push([3.0 + r * t.cos(), 3.0 + r * t.sin()]);
                    }
                }
                out.push(Elem::Line(p, true, false));
            }
            ("BALLS", _) => {
                for i in 0..8 {
                    let t = (i as f64 * 45.0).to_radians();
                    out.push(Elem::Ellipse(
                        [2.0 + 1.5 * t.cos(), 2.0 + 1.5 * t.sin()],
                        [0.38, 0.38],
                        false,
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// The box round a symbol's parts (grid units).
pub fn bounds(elems: &[Elem]) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    let mut grow = |p: [f64; 2]| {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    };
    for e in elems {
        match e {
            Elem::Line(p, ..) => p.iter().for_each(|&q| grow(q)),
            Elem::Ellipse(c, r, _) => {
                grow([c[0] - r[0], c[1] - r[1]]);
                grow([c[0] + r[0], c[1] + r[1]]);
            }
            Elem::Rect(a, s) => {
                grow(*a);
                grow([a[0] + s[0], a[1] + s[1]]);
            }
            Elem::Text(c, ..) | Elem::Pin(c) => grow(*c),
        }
    }
    (lo, hi)
}

/// A symbol's parts as objects (camera units): the grid's (0, 0) at `at`,
/// `u` camera units per grid unit, drawn in `color`.
pub fn objects(sym: &Symbol, at: [f64; 2], u: f64, color: u32) -> Vec<ObjData> {
    let p = |q: [f64; 2]| [at[0] + q[0] * u, at[1] + q[1] * u];
    let style = |kind: ShapeKind, filled: bool| ShapeStyle {
        kind,
        stroke: color,
        fill: color,
        fill_style: if filled {
            FillStyle::Solid
        } else {
            FillStyle::None
        },
        sloppiness: Sloppiness::Architect,
        round: false,
        ..ShapeStyle::default()
    };
    let width = 0.14 * u;
    let shape = |style: ShapeStyle, geom: Geom| ObjData::Shape {
        style,
        geom,
        width,
        seed: 1,
    };
    parse(sym.spec)
        .into_iter()
        .map(|e| match e {
            Elem::Line(pts, closed, filled) => {
                let anchors: Vec<[f64; 2]> = pts.into_iter().map(p).collect();
                let pts = shapes::path_through(&anchors, false, closed);
                let (center, half) = shapes::bounds_of(&pts);
                shape(
                    style(ShapeKind::Path, filled),
                    Geom {
                        center,
                        half,
                        rot: 0.0,
                        pts,
                    },
                )
            }
            Elem::Ellipse(c, r, filled) => shape(
                style(ShapeKind::Ellipse, filled),
                Geom {
                    center: p(c),
                    half: [r[0] * u, r[1] * u],
                    ..Default::default()
                },
            ),
            Elem::Rect(a, s) => shape(
                style(ShapeKind::Rect, false),
                Geom {
                    center: p([a[0] + s[0] * 0.5, a[1] + s[1] * 0.5]),
                    half: [s[0] * 0.5 * u, s[1] * 0.5 * u],
                    ..Default::default()
                },
            ),
            Elem::Text(c, size, text) => {
                let st = crate::objects::TextStyle {
                    font: crate::font::id_of("Nunito"),
                    align: crate::font::Align::Center,
                    color,
                    ..Default::default()
                };
                let size = size * u * 0.7;
                let b = crate::objects::text_box(&text, &st, size);
                ObjData::Text {
                    text,
                    style: st,
                    geom: Geom {
                        center: p(c),
                        half: [b[0] * 0.5, b[1] * 0.5],
                        ..Default::default()
                    },
                    size,
                    seed: 1,
                }
            }
            Elem::Pin(c) => shape(
                style(ShapeKind::Pin, false),
                Geom {
                    center: p(c),
                    half: [0.0, 0.0],
                    ..Default::default()
                },
            ),
        })
        .collect()
}

impl App {
    /// Put symbol `sym` on the page, centred on `center` (camera units), `u`
    /// camera units per grid unit: one group, its strokes (unrecorded).
    pub(crate) fn add_symbol(
        &mut self,
        sym: &Symbol,
        center: [f64; 2],
        u: f64,
        color: u32,
    ) -> Option<(ObjRef, Vec<u32>)> {
        let (lo, hi) = bounds(&parse(sym.spec));
        let at = [
            center[0] - (lo[0] + hi[0]) * 0.5 * u,
            center[1] - (lo[1] + hi[1]) * 0.5 * u,
        ];
        let mut members = Vec::new();
        let mut ids = Vec::new();
        for d in objects(sym, at, u, color) {
            let (g, s) = self.add_group(&d, None);
            members.push(ObjRef::Group(g));
            ids.extend(s);
        }
        let b = self.make_bundle(&members)?;
        Some((b, ids))
    }

    /// Menu › Symbols: symbol `i` in the middle of the screen, selected.
    pub(crate) fn place_symbol(&mut self, i: usize) {
        let Some(sym) = SYMBOLS.get(i) else { return };
        let [w, h] = self.size();
        let center = self.px_to_cam([w * 0.5, h * 0.5]);
        let u = 10.0 * self.ppp() / self.cam.ppc();
        let color = self.ui.shape.stroke;
        if let Some((b, ids)) = self.add_symbol(sym, center, u, color) {
            self.record_edit(vec![], ids);
            self.edit.selection = vec![b];
            if !self.ui.diagram {
                self.say("Turn on Diagram (Menu) to join wires to its pins");
            }
        }
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_symbol_draws_and_has_pins() {
        for s in SYMBOLS {
            let e = parse(s.spec);
            assert!(
                e.iter().any(|x| !matches!(x, Elem::Pin(_))),
                "{} draws",
                s.key
            );
            assert!(
                e.iter().any(|x| matches!(x, Elem::Pin(_))),
                "{} has pins",
                s.key
            );
            let (lo, hi) = bounds(&e);
            assert!(hi[0] > lo[0] && hi[1] > lo[1], "{}", s.key);
        }
        let mut keys: Vec<&str> = SYMBOLS.iter().map(|s| s.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), SYMBOLS.len(), "keys are unique");
        assert!(find("NPN transistor").is_some());
    }

    /// Alive groups' data (camera units) matching `f`.
    fn alive(app: &App, f: impl Fn(&ObjData) -> bool) -> Vec<ObjData> {
        app.objs
            .groups
            .iter()
            .filter(|g| {
                g.strokes
                    .first()
                    .is_some_and(|&s| !app.scene.strokes[s as usize].deleted)
            })
            .map(|g| crate::objects::to_cam(&g.cell, &g.data, &app.cam))
            .filter(|d| f(d))
            .collect()
    }

    fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
        (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
    }

    #[test]
    fn wires_stay_on_pins_as_a_symbol_turns() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.ui.diagram = true;
        let r = app.run_commands(&json!([
            {"add": "symbol", "name": "resistor", "x": 0, "y": 0},
            {"add": "symbol", "name": "ground", "x": 200, "y": 100},
        ]));
        assert_eq!(r[0]["ok"], true, "{r}");
        let res = r[0]["id"].as_str().unwrap().to_string();
        let ppc = app.cam.ppc();
        let off = app.cam.off;
        let home = |q: [f64; 2]| [(q[0] - off[0]) * ppc, (q[1] - off[1]) * ppc];
        let pins = |app: &App, n: usize| -> Vec<[f64; 2]> {
            alive(
                app,
                |d| matches!(d, ObjData::Bundle { geom, .. } if geom.pts.len() == n),
            )
            .first()
            .map(|d| d.geom().pts.clone())
            .unwrap_or_default()
        };
        let rp = pins(&app, 2);
        let gp = pins(&app, 1);
        assert_eq!((rp.len(), gp.len()), (2, 1));
        let (a, b) = (home(rp[1]), home(gp[0]));
        app.run_commands(&json!([{"add": "shape", "kind": "line", "x": a[0], "y": a[1], "w": b[0] - a[0], "h": b[1] - a[1]}]));
        let wire = |app: &App| -> Vec<[f64; 2]> {
            alive(
                app,
                |d| matches!(d, ObjData::Shape { style, .. } if style.kind == ShapeKind::Line),
            )[0]
            .geom()
            .pts
            .clone()
        };
        let tol = 1e-3 / ppc;
        assert!(near(wire(&app)[0], rp[1], tol));
        // Moved down: the wire's end goes with the pin; the other stays.
        app.run_commands(&json!([{"move": res, "dx": 0, "dy": 40}]));
        let rp = pins(&app, 2);
        let w = wire(&app);
        assert!(near(w[0], rp[1], tol), "{w:?} {rp:?}");
        assert!(near(w[1], gp[0], tol));
        // Turned a quarter: the same pin, now below the middle.
        let id = app.run_commands(&json!([{"get": "objects", "kinds": ["group"]}]))[0]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["members"].as_u64() > Some(2))
            .unwrap()["id"]
            .clone();
        app.run_commands(&json!([{"set": id, "rotate": 90}]));
        let rp2 = pins(&app, 2);
        let w = wire(&app);
        assert!(near(w[0], rp2[1], tol), "{w:?} {rp2:?}");
        assert!(
            (rp2[1][0] - rp2[0][0]).abs() < tol,
            "the pins are one above the other now"
        );
    }

    /// `OGP_DUMP=dir cargo test -- --ignored symbol_sheet`: all of them.
    #[test]
    #[ignore]
    fn symbol_sheet() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([1400.0, 900.0]);
        let mut cmds = Vec::new();
        for (i, s) in SYMBOLS.iter().enumerate() {
            let (x, y) = (
                (i % 10) as f64 * 130.0 - 650.0,
                (i / 10) as f64 * 140.0 - 420.0,
            );
            cmds.push(json!({"add": "symbol", "name": s.key, "cx": x, "cy": y, "size": 8}));
            cmds.push(
                json!({"add": "text", "text": s.label(), "x": x - 50.0, "y": y + 45.0, "size": 10}),
            );
        }
        let r = app.run_commands(&serde_json::Value::Array(cmds));
        assert!(r.as_array().unwrap().iter().all(|x| x["ok"] == true));
        ogpaper_core::query(
            &app.scene,
            &app.cam,
            1400.0,
            900.0,
            crate::VIEW,
            &mut app.draw,
        );
        let opts = crate::export::Opts {
            selection: false,
            background: true,
            scale: 1.0,
        };
        let dir = std::env::var("OGP_DUMP")
            .unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        std::fs::write(
            format!("{dir}/symbols.png"),
            app.export(crate::export::Format::Png, &opts).unwrap(),
        )
        .unwrap();
    }
}
