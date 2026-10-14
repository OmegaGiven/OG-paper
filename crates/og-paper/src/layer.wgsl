// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// A layer drawn on its own (premultiplied), laid over the frame: times its
// opacity, times the alpha of the layer it is clipped to (if any). The
// blend mode is the pipeline's blend state; Darken's Min needs what is
// transparent to read as white.

@group(0) @binding(0) var layer_t: texture_2d<f32>;
@group(0) @binding(1) var mask_t: texture_2d<f32>;

struct Out {
    @builtin(position) pos: vec4<f32>,
    // opacity, clipped (0/1), darken (0/1), unused
    @location(0) @interpolate(flat) p: vec4<f32>,
};

@vertex
fn vs_layer(@builtin(vertex_index) vi: u32, @location(0) p: vec4<f32>) -> Out {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    var o: Out;
    o.pos = vec4<f32>(x, y, 0.0, 1.0);
    o.p = p;
    return o;
}

@fragment
fn fs_layer(in: Out) -> @location(0) vec4<f32> {
    let at = vec2<i32>(in.pos.xy);
    var c = textureLoad(layer_t, at, 0) * in.p.x;
    if (in.p.y > 0.5) {
        c = c * textureLoad(mask_t, at, 0).a;
    }
    if (in.p.z > 0.5) {
        return vec4<f32>(c.rgb + vec3<f32>(1.0 - c.a), c.a);
    }
    return c;
}
