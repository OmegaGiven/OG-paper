// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Strokes are drawn as one capsule per segment. Each instance is one stroke
// placed on screen by its cell transform (screen = o + local * scale); the
// vertex shader fetches points from storage and emits MAX_SEG quads, collapsing
// unused or off-screen segments. The fragment shader computes the exact
// distance to the segment, giving round caps/joins and 1px anti-aliasing.

struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

struct StrokeGpu {
    start: u32,
    len: u32,
    width: f32,
    color: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> strokes: array<StrokeGpu>;
@group(0) @binding(2) var<storage, read> points: array<vec2<f32>>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    @location(3) @interpolate(flat) hw: f32,
    @location(4) @interpolate(flat) color: vec4<f32>,
};

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x / g.viewport.x * 2.0 - 1.0, 1.0 - p.y / g.viewport.y * 2.0, 0.0, 1.0);
}

fn collapsed() -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    out.px = vec2<f32>(0.0);
    out.a = vec2<f32>(0.0);
    out.b = vec2<f32>(0.0);
    out.hw = 0.0;
    out.color = vec4<f32>(0.0);
    return out;
}

@vertex
fn vs_stroke(
    @builtin(vertex_index) vi: u32,
    @location(0) sid: u32,
    @location(1) o: vec2<f32>,
    @location(2) scale: f32,
) -> VsOut {
    let s = strokes[sid];
    let seg = vi / 6u;
    let corner = vi % 6u;
    if (seg + 1u >= s.len) {
        return collapsed();
    }
    let a = o + points[s.start + seg] * scale;
    let b = o + points[s.start + seg + 1u] * scale;
    let hw_true = s.width * scale * 0.5;
    let hw = max(hw_true, 0.5);
    let r = hw + 1.0;
    // Skip segments entirely off screen.
    let lo = min(a, b) - vec2<f32>(r);
    let hi = max(a, b) + vec2<f32>(r);
    if (hi.x < 0.0 || hi.y < 0.0 || lo.x > g.viewport.x || lo.y > g.viewport.y) {
        return collapsed();
    }
    let d = b - a;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-6) {
        dir = d / len;
    }
    let nrm = vec2<f32>(-dir.y, dir.x);
    var along = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var side = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    var base = a;
    if (along[corner] > 0.0) {
        base = b;
    }
    let p = base + dir * along[corner] * r + nrm * side[corner] * r;

    var out: VsOut;
    out.pos = to_clip(p);
    out.px = p;
    out.a = a;
    out.b = b;
    out.hw = hw;
    var c = unpack4x8unorm(s.color);
    // Hairlines fade rather than vanish.
    c.a = c.a * clamp(hw_true * 2.0, 0.2, 1.0);
    out.color = c;
    return out;
}

@fragment
fn fs_stroke(in: VsOut) -> @location(0) vec4<f32> {
    let pa = in.px - in.a;
    let ba = in.b - in.a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    let dist = length(pa - ba * h);
    let cov = clamp(in.hw + 0.5 - dist, 0.0, 1.0);
    if (cov <= 0.0) {
        discard;
    }
    let a = in.color.a * cov;
    return vec4<f32>(in.color.rgb * a, a);
}

struct DotOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) size: f32,
    @location(2) @interpolate(flat) alpha: f32,
};

@vertex
fn vs_dot(
    @builtin(vertex_index) vi: u32,
    @location(0) xy: vec2<f32>,
    @location(1) size: f32,
    @location(2) alpha: f32,
) -> DotOut {
    var cx = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var cy = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let uv = vec2<f32>(cx[vi], cy[vi]);
    var out: DotOut;
    out.pos = to_clip(xy + uv * (size * 0.5 + 1.0));
    out.uv = uv * (size * 0.5 + 1.0) / max(size * 0.5, 0.5);
    out.size = size;
    out.alpha = alpha;
    return out;
}

@fragment
fn fs_dot(in: DotOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    let cov = clamp((1.0 - d) * in.size * 0.5 + 0.5, 0.0, 1.0);
    let a = in.alpha * cov;
    return vec4<f32>(vec3<f32>(0.25, 0.25, 0.32) * a, a);
}

// Occupancy tiles: a 16x16 grid of "something is written here" marks for a
// region whose content is too small to read.
struct TileOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) size: f32,
    @location(2) @interpolate(flat) alpha: f32,
    @location(3) @interpolate(flat) m0: vec4<u32>,
    @location(4) @interpolate(flat) m1: vec4<u32>,
};

@vertex
fn vs_tile(
    @builtin(vertex_index) vi: u32,
    @location(0) xysa: vec4<f32>,
    @location(1) m0: vec4<u32>,
    @location(2) m1: vec4<u32>,
) -> TileOut {
    var cx = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var cy = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    let uv = vec2<f32>(cx[vi], cy[vi]);
    var out: TileOut;
    out.pos = to_clip(xysa.xy + uv * xysa.z);
    out.uv = uv;
    out.size = xysa.z;
    out.alpha = xysa.w;
    out.m0 = m0;
    out.m1 = m1;
    return out;
}

@fragment
fn fs_tile(in: TileOut) -> @location(0) vec4<f32> {
    let g = in.uv * 16.0;
    let b = vec2<u32>(clamp(floor(g), vec2<f32>(0.0), vec2<f32>(15.0)));
    let idx = b.y * 16u + b.x;
    var m = array<u32, 8>(in.m0.x, in.m0.y, in.m0.z, in.m0.w, in.m1.x, in.m1.y, in.m1.z, in.m1.w);
    if (((m[idx / 32u] >> (idx % 32u)) & 1u) == 0u) {
        discard;
    }
    // A soft mark per occupied bin, sized to the bin.
    let bin_px = in.size / 16.0;
    let f = (fract(g) - vec2<f32>(0.5)) * bin_px;
    let r = bin_px * 0.42;
    let cov = clamp(r - length(f) + 0.5, 0.0, 1.0);
    let a = in.alpha * cov;
    return vec4<f32>(vec3<f32>(0.22, 0.22, 0.3) * a, a);
}
