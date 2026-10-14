// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Measuring: the selection's place, size and turn as numbers (X, Y, W, H
//! and ° in the selection panel, typed to change them), and rulers along
//! the top and left edges (Menu › Rulers). Both count in home-view points,
//! the units the page server and AIs use: x to the right and y down from
//! the page's centre, as the home view shows it.

use crate::objects::{ObjData, ObjRef, Op};
use crate::App;

/// Which number of the selection was typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoField {
    X,
    Y,
    W,
    H,
    Turn,
}

/// The rulers' mapping this frame: home-view points at the screen's left
/// and top edge, and per UI point across and down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ruler {
    pub x0: f64,
    pub dx: f64,
    pub y0: f64,
    pub dy: f64,
}

impl App {
    /// Camera units at screen px `p`, as home-view points.
    pub(crate) fn px_to_home(&self, p: [f64; 2]) -> [f64; 2] {
        let home = crate::home_camera();
        let q = self.px_to_cam(p);
        let o = self.cam.cell.origin_in(&home.cell);
        let side = self.cam.cell.side_in(&home.cell);
        let u = [o[0] + q[0] * side, o[1] + q[1] * side];
        let ppc = home.ppc();
        [(u[0] - home.off[0]) * ppc, (u[1] - home.off[1]) * ppc]
    }

    pub(crate) fn ruler(&self) -> Ruler {
        let k = self.uipp();
        let a = self.px_to_home([0.0, 0.0]);
        let b = self.px_to_home([100.0 * k, 100.0 * k]);
        Ruler {
            x0: a[0],
            dx: (b[0] - a[0]) / 100.0,
            y0: a[1],
            dy: (b[1] - a[1]) / 100.0,
        }
    }

    /// The box around the selection in screen px (lo, hi).
    fn sel_rect_px(&self) -> Option<([f64; 2], [f64; 2])> {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for r in &self.edit.selection {
            let (a, b) = self.obj_rect(r)?;
            let (a, b) = (self.cam_to_px(a), self.cam_to_px(b));
            lo = [lo[0].min(a[0]), lo[1].min(a[1])];
            hi = [hi[0].max(b[0]), hi[1].max(b[1])];
        }
        (lo[0] <= hi[0]).then_some((lo, hi))
    }

    /// The selection's turn (degrees) when it is one thing with one.
    fn sel_turn(&self) -> f64 {
        match self.edit.selection[..] {
            [ObjRef::Group(g)] => match &self.objs.groups[g as usize].data {
                ObjData::Bundle { .. } => 0.0,
                d => d.geom().rot.to_degrees(),
            },
            _ => 0.0,
        }
    }

    /// X, Y (top left), W, H in home-view points and the turn in degrees.
    pub(crate) fn sel_numbers(&self) -> Option<[f64; 5]> {
        let (lo, hi) = self.sel_rect_px()?;
        let (a, b) = (self.px_to_home(lo), self.px_to_home(hi));
        Some([a[0], a[1], b[0] - a[0], b[1] - a[1], self.sel_turn()])
    }

    /// Set one of the selection's numbers (one undo step).
    pub(crate) fn set_sel_number(&mut self, field: GeoField, v: f64) {
        self.sel_alive();
        let (Some(now), Some((lo, hi))) = (self.sel_numbers(), self.sel_rect_px()) else {
            return;
        };
        if !v.is_finite() {
            return;
        }
        // Home points to camera units, by the box itself (any depth).
        let cam_lo = self.px_to_cam(lo);
        let cam_hi = self.px_to_cam(hi);
        let k = [
            if now[2].abs() > 1e-300 {
                (cam_hi[0] - cam_lo[0]) / now[2]
            } else {
                0.0
            },
            if now[3].abs() > 1e-300 {
                (cam_hi[1] - cam_lo[1]) / now[3]
            } else {
                0.0
            },
        ];
        let op = match field {
            GeoField::X => Op::Move([(v - now[0]) * k[0], 0.0]),
            GeoField::Y => Op::Move([0.0, (v - now[1]) * k[1]]),
            GeoField::W if now[2] > 0.0 && v > 0.0 => Op::Scale {
                pivot: cam_lo,
                sx: v / now[2],
                sy: 1.0,
                rot: 0.0,
            },
            GeoField::H if now[3] > 0.0 && v > 0.0 => Op::Scale {
                pivot: cam_lo,
                sx: 1.0,
                sy: v / now[3],
                rot: 0.0,
            },
            GeoField::Turn => Op::Rotate {
                pivot: [(cam_lo[0] + cam_hi[0]) * 0.5, (cam_lo[1] + cam_hi[1]) * 0.5],
                angle: (v - now[4]).to_radians(),
            },
            _ => return,
        };
        self.apply_op(&op);
        self.redraw();
    }
}

/// A round step for ruler ticks about `want` long: 1, 2 or 5 times a power
/// of ten.
pub fn nice_step(want: f64) -> f64 {
    if !(want > 0.0) || !want.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(want.log10().floor());
    let m = want / p;
    p * if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    }
}

/// A ruler label: short, exponent form when very big or small.
pub fn label(v: f64, step: f64) -> String {
    let a = v.abs();
    if a != 0.0 && (a >= 1e7 || step < 1e-4) {
        format!("{v:.2e}")
    } else if step >= 1.0 {
        format!("{v:.0}")
    } else {
        let dec = (-step.log10().floor()) as usize;
        format!("{v:.dec$}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round() {
        assert_eq!(nice_step(87.0), 100.0);
        assert_eq!(nice_step(0.3), 0.2);
        assert_eq!(nice_step(4.2), 5.0);
        assert_eq!(label(250.0, 50.0), "250");
        assert_eq!(label(0.25, 0.05), "0.25");
        assert!(label(3e-9, 1e-9).contains('e'));
    }

    #[test]
    fn numbers_place_size_and_turn_the_selection() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([1200.0, 800.0]);
        app.run_commands(&serde_json::json!([
            {"add": "shape", "kind": "rect", "x": 10, "y": 20, "w": 100, "h": 50, "neat": true}
        ]));
        let r = app.objs.obj_of(0);
        app.edit.selection = vec![r];
        let n = app.sel_numbers().unwrap();
        // The outline's width pads the box a little.
        assert!(
            (n[0] - 10.0).abs() < 3.0 && (n[2] - 100.0).abs() < 5.0,
            "{n:?}"
        );
        app.set_sel_number(GeoField::X, 200.0);
        let n = app.sel_numbers().unwrap();
        assert!((n[0] - 200.0).abs() < 0.01, "{n:?}");
        app.set_sel_number(GeoField::W, 300.0);
        let n = app.sel_numbers().unwrap();
        assert!(
            (n[2] - 300.0).abs() < 0.5 && (n[0] - 200.0).abs() < 0.01,
            "{n:?}"
        );
        app.set_sel_number(GeoField::Turn, 30.0);
        assert!((app.sel_numbers().unwrap()[4] - 30.0).abs() < 0.01);
        // Zoomed in: the same numbers.
        app.cam.zoom_at(37.0, [0.0, 0.0]);
        let m = app.sel_numbers().unwrap();
        assert!((m[4] - 30.0).abs() < 0.01);
        let r = app.ruler();
        assert!(r.dx > 0.0 && r.dx < 0.1, "{r:?}");
    }
}
