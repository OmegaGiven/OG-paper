// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Where the controls sit (Settings > Edit layout). Positions are fractions
//! of the screen, so a layout carries over between screen sizes; anything
//! not moved stays at its default spot. Fans open toward the middle of the
//! screen from wherever their button is: a quarter circle from a corner, a
//! half circle from an edge, a full ring from the middle.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// A screen corner, for the tool panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Corner {
    #[default]
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl Corner {
    fn key(self) -> &'static str {
        match self {
            Corner::BottomLeft => "bl",
            Corner::BottomRight => "br",
            Corner::TopLeft => "tl",
            Corner::TopRight => "tr",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        Some(match k {
            "bl" => Corner::BottomLeft,
            "br" => Corner::BottomRight,
            "tl" => Corner::TopLeft,
            "tr" => Corner::TopRight,
            _ => return None,
        })
    }

    /// The corner nearest a point given as screen fractions.
    pub fn nearest(f: [f32; 2]) -> Self {
        match (f[0] >= 0.5, f[1] >= 0.5) {
            (false, true) => Corner::BottomLeft,
            (true, true) => Corner::BottomRight,
            (false, false) => Corner::TopLeft,
            (true, false) => Corner::TopRight,
        }
    }

    pub fn is_right(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::TopRight)
    }

    pub fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Layout {
    /// Centres as screen fractions; None = the default spot.
    pub tool: Option<[f32; 2]>,
    pub app: Option<[f32; 2]>,
    /// The middle of the undo / redo pair.
    pub undo: Option<[f32; 2]>,
    /// The middle of the quick bar.
    pub bar: Option<[f32; 2]>,
    pub panel: Corner,
}

impl Layout {
    pub fn encode(&self) -> String {
        let mut parts = Vec::new();
        for (k, v) in [
            ("tool", self.tool),
            ("app", self.app),
            ("undo", self.undo),
            ("bar", self.bar),
        ] {
            if let Some([x, y]) = v {
                parts.push(format!("{k}:{x:.4},{y:.4}"));
            }
        }
        if self.panel != Corner::BottomLeft {
            parts.push(format!("panel:{}", self.panel.key()));
        }
        parts.join(";")
    }

    pub fn decode(s: &str) -> Self {
        let mut l = Layout::default();
        for part in s.split(';') {
            let Some((k, v)) = part.split_once(':') else {
                continue;
            };
            let xy = || -> Option<[f32; 2]> {
                let (x, y) = v.split_once(',')?;
                let p = [x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?];
                p.iter().all(|c| (0.0..=1.0).contains(c)).then_some(p)
            };
            match k.trim() {
                "tool" => l.tool = xy(),
                "app" => l.app = xy(),
                "undo" => l.undo = xy(),
                "bar" => l.bar = xy(),
                "panel" => l.panel = Corner::from_key(v.trim()).unwrap_or_default(),
                _ => {}
            }
        }
        l
    }
}

/// The arc a fan opens along from a button at screen fraction `f`: its
/// start angle and sweep (radians, y down). Toward the middle of the screen:
/// a quarter circle from a corner, half from an edge, a full ring from the
/// middle.
pub fn fan_arc(f: [f32; 2]) -> (f32, f32) {
    let side = |v: f32| -> f32 {
        if v < 1.0 / 3.0 {
            1.0
        } else if v > 2.0 / 3.0 {
            -1.0
        } else {
            0.0
        }
    };
    let (dx, dy) = (side(f[0]), side(f[1]));
    if dx == 0.0 && dy == 0.0 {
        return (-FRAC_PI_2, TAU);
    }
    let mid = dy.atan2(dx);
    let span = if dx != 0.0 && dy != 0.0 {
        FRAC_PI_2
    } else {
        PI
    };
    (mid - span * 0.5, span)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        let d = (a - b).rem_euclid(TAU);
        d < 1e-5 || TAU - d < 1e-5
    }

    #[test]
    fn layouts_round_trip() {
        let l = Layout {
            tool: Some([0.1, 0.9]),
            app: None,
            undo: Some([0.5, 0.05]),
            bar: Some([0.5, 0.02]),
            panel: Corner::TopRight,
        };
        assert_eq!(Layout::decode(&l.encode()), l);
        assert_eq!(Layout::decode(""), Layout::default());
        assert_eq!(Layout::decode("tool:2,3;junk").tool, None);
    }

    #[test]
    fn fans_open_toward_the_middle() {
        // Bottom right (the default tool button): left to up, as before.
        let (a, s) = fan_arc([0.95, 0.95]);
        assert!(close(a, PI) && close(s, FRAC_PI_2));
        // Top right (the default settings button): down to left.
        let (a, s) = fan_arc([0.95, 0.05]);
        assert!(close(a, FRAC_PI_2) && close(s, FRAC_PI_2));
        // Middle of the left edge: a half circle facing right.
        let (a, s) = fan_arc([0.02, 0.5]);
        assert!(close(a, -FRAC_PI_2) && close(s, PI));
        // The middle: all the way round.
        assert!(close(fan_arc([0.5, 0.5]).1, TAU));
    }
}
