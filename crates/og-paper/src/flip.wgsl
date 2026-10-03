// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Dark mode: the finished frame, copied to the screen with its lightness
// flipped and its hues kept (in YIQ, Y becomes 1 - Y), so paper turns dark,
// black ink turns white and red stays red. `SRGB` is prepended by the
// renderer: true when the frame is stored as sRGB (sampled as linear).

@group(0) @binding(0) var frame: texture_2d<f32>;

@vertex
fn vs_flip(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the screen.
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_flip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var c = textureLoad(frame, vec2<i32>(pos.xy), 0).rgb;
    if (SRGB) {
        c = to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    let y = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    let i = dot(c, vec3<f32>(0.596, -0.274, -0.322));
    let q = dot(c, vec3<f32>(0.211, -0.523, 0.312));
    let y2 = 1.0 - y;
    var o = vec3<f32>(
        y2 + 0.956 * i + 0.621 * q,
        y2 - 0.272 * i - 0.647 * q,
        y2 - 1.106 * i + 1.703 * q,
    );
    o = clamp(o, vec3<f32>(0.0), vec3<f32>(1.0));
    if (SRGB) {
        o = to_linear(o);
    }
    return vec4<f32>(o, 1.0);
}
