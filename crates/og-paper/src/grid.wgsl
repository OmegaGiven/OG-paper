// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Background grid, drawn under the ink. Lines (or dots) sit on the canvas's
// own power-of-two cells, so they line up with the world at any zoom: as you
// zoom in, the next finer grid fades in between the lines.

struct Grid {
    // Screen position (px) of a point where grid lines cross.
    origin: vec2<f32>,
    // Finest spacing drawn (px), and the spacing where lines start to show.
    fine: f32,
    appear: f32,
    // 1 = lines, 2 = dots.
    mode: u32,
    _a: f32,
    // Physical pixels per point.
    ppp: f32,
    _b: f32,
};

@group(0) @binding(0) var<uniform> g: Grid;

@vertex
fn vs_grid(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// Distance (px) to the nearest line of spacing `s`, per axis.
fn line_dist(p: vec2<f32>, s: f32) -> vec2<f32> {
    let r = (p - g.origin) / s;
    return abs(fract(r + 0.5) - 0.5) * s;
}

// A grid level's strength from its on-screen spacing alone, so nothing
// jumps as the zoom crosses a power of two: lines appear as their spacing
// grows past `target`, and coarser levels read darker.
fn level_alpha(spacing: f32) -> f32 {
    let t = g.appear;
    return smoothstep(t, 2.0 * t, spacing) * (0.10 + 0.12 * smoothstep(4.0 * t, 16.0 * t, spacing));
}

@fragment
fn fs_grid(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy;
    let ink = vec3<f32>(0.45, 0.45, 0.52);
    var a = 0.0;
    var s = g.fine;
    for (var k = 0; k < 5; k = k + 1) {
        let la = level_alpha(s);
        if (g.mode == 1u) {
            let w = 0.6 * g.ppp;
            let d = line_dist(p, s);
            a = max(a, (1.0 - smoothstep(w * 0.5, w * 0.5 + 1.0, min(d.x, d.y))) * la);
        } else {
            let r = (1.0 + 0.6 * f32(k) / 4.0) * g.ppp;
            let d = length(line_dist(p, s));
            a = max(a, (1.0 - smoothstep(r, r + 1.0, d)) * la * 2.6);
        }
        s = s * 2.0;
    }
    return vec4<f32>(ink * a, a);
}
