// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Hit-testing in screen space against the current frame's draw list, so it
//! only ever looks at what is visible.

use crate::scene::Scene;
use crate::visible::DrawList;

/// Strokes within `radius` pixels of screen point `p` (pixels from the top-left).
pub fn strokes_near(scene: &Scene, list: &DrawList, p: [f32; 2], radius: f32) -> Vec<u32> {
    let mut out = Vec::new();
    for inst in &list.strokes {
        let s = &scene.strokes[inst.stroke as usize];
        if s.deleted {
            continue;
        }
        let pts = scene.stroke_points(inst.stroke);
        let hw = s.width * inst.scale * 0.5;
        let r = radius + hw;
        let at = |q: &[f32; 4]| [inst.ox + q[0] * inst.scale, inst.oy + q[1] * inst.scale];
        let hit = if s.brush == crate::Brush::Fill {
            let poly: Vec<[f32; 2]> = pts.iter().map(at).collect();
            inside(p, &poly)
                || (0..poly.len())
                    .any(|i| seg_dist(p, poly[i], poly[(i + 1) % poly.len()]) <= radius)
        } else {
            pts.windows(2)
                .any(|w| seg_dist(p, at(&w[0]), at(&w[1])) <= r)
                || (pts.len() == 1 && seg_dist(p, at(&pts[0]), at(&pts[0])) <= r)
        };
        if hit {
            out.push(inst.stroke);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Point in polygon (even-odd).
pub fn inside(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut odd = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > p[0] {
                odd = !odd;
            }
        }
    }
    odd
}

pub fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (bx, by) = (b[0] - a[0], b[1] - a[1]);
    let (px, py) = (p[0] - a[0], p[1] - a[1]);
    let len2 = bx * bx + by * by;
    let t = if len2 > 0.0 {
        ((px * bx + py * by) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((px - bx * t).powi(2) + (py - by * t).powi(2)).sqrt()
}
