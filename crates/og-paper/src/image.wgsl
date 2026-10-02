// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Pictures: one textured quad per instance, its corners given in screen
// pixels (top-left, top-right, bottom-right, bottom-left of the picture).

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
};

@vertex
fn vs_image(
    @builtin(vertex_index) vi: u32,
    @location(0) p01: vec4<f32>,
    @location(1) p23: vec4<f32>,
    // viewport w, h; opacity; unused
    @location(2) misc: vec4<f32>,
    // The part of the picture shown: u0, v0, u1, v1.
    @location(3) crop: vec4<f32>,
) -> VsOut {
    // Two triangles: 0 1 2, 0 2 3.
    var corner = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let k = corner[vi];
    var p = p01.xy;
    var uv = vec2<f32>(0.0, 0.0);
    if k == 1u {
        p = p01.zw;
        uv = vec2<f32>(1.0, 0.0);
    } else if k == 2u {
        p = p23.xy;
        uv = vec2<f32>(1.0, 1.0);
    } else if k == 3u {
        p = p23.zw;
        uv = vec2<f32>(0.0, 1.0);
    }
    var out: VsOut;
    out.pos = vec4<f32>(p.x / misc.x * 2.0 - 1.0, 1.0 - p.y / misc.y * 2.0, 0.0, 1.0);
    out.uv = mix(crop.xy, crop.zw, uv);
    out.alpha = misc.z;
    return out;
}

@fragment
fn fs_image(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    let a = c.a * in.alpha;
    return vec4<f32>(c.rgb * a, a);
}
