// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A camera that never loses precision.
//!
//! The camera lives in one cell. `off` is the viewport centre inside that cell
//! (in cell units, kept in [0, 1)); `scale` is kept in [1, 2). Whenever a zoom
//! pushes `scale` out of range the camera re-anchors to a child or parent cell,
//! so every float it holds stays small no matter how deep it goes.

use crate::addr::{CellAddr, Level};

#[derive(Clone, Debug)]
pub struct Camera {
    pub cell: CellAddr,
    pub off: [f64; 2],
    pub scale: f64,
    /// Screen pixels per camera cell at scale 1.
    pub base_px: f64,
}

impl Camera {
    pub fn new(cell: CellAddr, off: [f64; 2], base_px: f64) -> Self {
        let mut c = Self {
            cell,
            off,
            scale: 1.0,
            base_px,
        };
        c.normalize();
        c
    }

    /// Screen pixels per camera cell.
    pub fn ppc(&self) -> f64 {
        self.base_px * self.scale
    }

    /// Zoom depth as log10 of magnification relative to a level-0 cell of
    /// `base_px` pixels.
    pub fn log10_zoom(&self) -> f64 {
        (self.cell.level as f64 + self.scale.log2()) * std::f64::consts::LOG10_2
    }

    pub fn level(&self) -> Level {
        self.cell.level
    }

    /// Pan by a screen-space delta (content follows the pointer).
    pub fn pan_px(&mut self, dx: f64, dy: f64) {
        let p = self.ppc();
        self.off[0] -= dx / p;
        self.off[1] -= dy / p;
        self.normalize();
    }

    /// Zoom by `factor` keeping the point at `at` (pixels from viewport centre) fixed.
    pub fn zoom_at(&mut self, factor: f64, at: [f64; 2]) {
        let p0 = self.ppc();
        let q = [self.off[0] + at[0] / p0, self.off[1] + at[1] / p0];
        self.scale *= factor;
        let p1 = self.ppc();
        self.off = [q[0] - at[0] / p1, q[1] - at[1] / p1];
        self.normalize();
    }

    /// Restore invariants: off in [0,1), scale in [1,2).
    pub fn normalize(&mut self) {
        loop {
            self.wrap_off();
            if self.scale >= 2.0 {
                // Zoomed in past 2x: move into the child cell under the centre.
                self.scale *= 0.5;
                let qx = if self.off[0] >= 0.5 { 1 } else { 0 };
                let qy = if self.off[1] >= 0.5 { 1 } else { 0 };
                self.off = [self.off[0] * 2.0 - qx as f64, self.off[1] * 2.0 - qy as f64];
                self.cell = self.cell.child(qx | (qy << 1));
            } else if self.scale < 1.0 {
                // Zoomed out past 1x: move up to the parent cell.
                self.scale *= 2.0;
                let q = self.cell.quadrant();
                self.off = [
                    (self.off[0] + (q & 1) as f64) * 0.5,
                    (self.off[1] + (q >> 1) as f64) * 0.5,
                ];
                self.cell = self.cell.parent();
            } else {
                break;
            }
        }
    }

    fn wrap_off(&mut self) {
        for axis in 0..2 {
            let f = self.off[axis].floor();
            if f != 0.0 {
                let n = f as i64;
                self.off[axis] -= f;
                if axis == 0 {
                    self.cell.x += n;
                } else {
                    self.cell.y += n;
                }
            }
            // Guard against off == 1.0 after rounding.
            if self.off[axis] >= 1.0 {
                self.off[axis] = 0.0;
                if axis == 0 {
                    self.cell.x += 1;
                } else {
                    self.cell.y += 1;
                }
            }
        }
    }

    /// Screen position (pixels from viewport centre) of a point given in the
    /// local [0,1] space of `cell`.
    pub fn to_screen(&self, cell: &CellAddr, local: [f64; 2]) -> [f64; 2] {
        let o = cell.origin_in(&self.cell);
        let s = cell.side_in(&self.cell);
        let p = self.ppc();
        [
            (o[0] + local[0] * s - self.off[0]) * p,
            (o[1] + local[1] * s - self.off[1]) * p,
        ]
    }

    /// Position in camera-cell units (relative to the camera cell origin) of a
    /// screen point given in pixels from viewport centre.
    pub fn screen_to_cam(&self, at: [f64; 2]) -> [f64; 2] {
        let p = self.ppc();
        [self.off[0] + at[0] / p, self.off[1] + at[1] / p]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2], eps: f64) -> bool {
        (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps
    }

    #[test]
    fn zoom_in_and_out_1e30_is_lossless() {
        let mut cam = Camera::new(CellAddr::new(0, 0, 0), [0.3, 0.7], 512.0);
        let start = cam.clone();
        // 2^100 ~ 1.27e30, done in 1.05x steps around an off-centre point.
        let steps = (100.0 * 2f64.ln() / 1.05f64.ln()).ceil() as usize;
        for _ in 0..steps {
            cam.zoom_at(1.05, [123.0, -45.0]);
        }
        assert!(cam.log10_zoom() > 30.0, "reached {}", cam.log10_zoom());
        for _ in 0..steps {
            cam.zoom_at(1.0 / 1.05, [123.0, -45.0]);
        }
        // Same zoom, and the viewport centre maps back to the same world point.
        assert!((cam.log10_zoom() - start.log10_zoom()).abs() < 1e-9);
        let o = cam.cell.origin_in(&start.cell);
        let s = cam.cell.side_in(&start.cell);
        let centre = [o[0] + cam.off[0] * s, o[1] + cam.off[1] * s];
        assert!(
            close(centre, start.off, 1e-9),
            "{centre:?} vs {:?}",
            start.off
        );
    }

    #[test]
    fn point_stays_put_under_cursor_at_depth() {
        // Zoom to ~1e36, then check a content point near the cursor stays
        // fixed on screen (sub-pixel) through further zooms and pans.
        let mut cam = Camera::new(CellAddr::new(0, 0, 0), [0.5, 0.5], 512.0);
        for _ in 0..3000 {
            cam.zoom_at(1.03, [0.0, 0.0]);
        }
        assert!(cam.log10_zoom() > 35.0);
        // Content: a point in a cell 3 levels below the camera.
        let cell = cam.cell.child(1).child(2).child(3);
        let local = [0.25, 0.75];
        let before = cam.to_screen(&cell, local);
        for _ in 0..50 {
            cam.zoom_at(1.01, before);
        }
        let after = cam.to_screen(&cell, local);
        assert!(close(before, after, 1e-6), "{before:?} -> {after:?}");
        // Pan by 1000 tiny steps of 0.37 px and back: no drift.
        for _ in 0..1000 {
            cam.pan_px(0.37, -0.21);
        }
        for _ in 0..1000 {
            cam.pan_px(-0.37, 0.21);
        }
        let back = cam.to_screen(&cell, local);
        assert!(close(before, back, 1e-6), "{before:?} -> {back:?}");
    }
}
