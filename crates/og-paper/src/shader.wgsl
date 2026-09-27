// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Strokes: one instance = one window of up to MAX_SEG segments of a stroke,
// placed on screen by its cell transform (screen = o + local * scale). Each
// segment is a quad around a tapered capsule; the fragment shader computes the
// exact distance, giving round caps/joins, pressure-varying width and 1px AA.
//
// Stroke records and points live in data textures (WebGL2-compatible):
// element i is at texel (i % tex_w, i / tex_w).

const MAX_SEG: u32 = 15u;

struct Globals {
    viewport: vec2<f32>,
    tex_w: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var strokes_tex: texture_2d<u32>;
@group(0) @binding(2) var points_tex: texture_2d<f32>;

struct StrokeRec {
    start: u32,
    len: u32,
    brush: u32,
    width: f32,
    color: u32,
};

fn texel(i: u32) -> vec2<i32> {
    return vec2<i32>(i32(i % g.tex_w), i32(i / g.tex_w));
}

fn stroke_at(i: u32) -> StrokeRec {
    let v = textureLoad(strokes_tex, texel(i), 0);
    return StrokeRec(v.x, v.y & 0xFFFFFFu, v.y >> 24u, bitcast<f32>(v.z), v.w);
}

// x, y (cell-local), pressure.
fn point_at(i: u32) -> vec3<f32> {
    return textureLoad(points_tex, texel(i), 0).xyz;
}

// Width factor by brush: pen follows pressure, marker/highlighter do not.
fn pressure_factor(brush: u32, p: f32) -> f32 {
    if (brush == 0u) {
        return 0.25 + 0.75 * clamp(p, 0.0, 1.0);
    }
    return 1.0;
}

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x / g.viewport.x * 2.0 - 1.0, 1.0 - p.y / g.viewport.y * 2.0, 0.0, 1.0);
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    @location(3) @interpolate(flat) r: vec2<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
};

fn collapsed() -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    out.px = vec2<f32>(0.0);
    out.a = vec2<f32>(0.0);
    out.b = vec2<f32>(0.0);
    out.r = vec2<f32>(0.0);
    out.color = vec4<f32>(0.0);
    return out;
}

@vertex
fn vs_stroke(
    @builtin(vertex_index) vi: u32,
    @location(0) sid: u32,
    @location(1) first: u32,
    @location(2) o: vec2<f32>,
    @location(3) scale: f32,
) -> VsOut {
    let s = stroke_at(sid);
    let seg = first + vi / 6u;
    let corner = vi % 6u;
    if (seg + 1u >= s.len && !(s.len == 1u && seg == 0u)) {
        return collapsed();
    }
    // A single-point stroke is a dot: a zero-length segment.
    let ib = min(seg + 1u, s.len - 1u);
    let pa = point_at(s.start + seg);
    let pb = point_at(s.start + ib);
    let a = o + pa.xy * scale;
    let b = o + pb.xy * scale;
    let hw = s.width * scale * 0.5;
    let ra_true = hw * pressure_factor(s.brush, pa.z);
    let rb_true = hw * pressure_factor(s.brush, pb.z);
    let ra = max(ra_true, 0.5);
    let rb = max(rb_true, 0.5);
    let r = max(ra, rb) + 1.0;
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
    out.r = vec2<f32>(ra, rb);
    var c = unpack4x8unorm(s.color);
    // Hairlines fade rather than vanish.
    c.a = c.a * clamp(max(ra_true, rb_true) * 2.0, 0.2, 1.0);
    out.color = c;
    return out;
}

fn coverage(in: VsOut) -> f32 {
    let pa = in.px - in.a;
    let ba = in.b - in.a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    let dist = length(pa - ba * h);
    let rad = mix(in.r.x, in.r.y, h);
    return clamp(rad + 0.5 - dist, 0.0, 1.0);
}

// Normal ink: premultiplied alpha over.
@fragment
fn fs_stroke(in: VsOut) -> @location(0) vec4<f32> {
    let cov = coverage(in);
    if (cov <= 0.0) {
        discard;
    }
    let a = in.color.a * cov;
    return vec4<f32>(in.color.rgb * a, a);
}

// Highlighter: blended with Min, so overlapping segments never double up and
// dark ink underneath stays readable, like a real highlighter.
@fragment
fn fs_highlight(in: VsOut) -> @location(0) vec4<f32> {
    let cov = coverage(in);
    if (cov <= 0.0) {
        discard;
    }
    let tint = mix(vec3<f32>(1.0), in.color.rgb, 0.55 * cov);
    return vec4<f32>(tint, 1.0);
}

// Occupancy tiles: a 16x16 grid of marks for a region whose content is too
// small to read.
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
    let gg = in.uv * 16.0;
    let b = vec2<u32>(clamp(floor(gg), vec2<f32>(0.0), vec2<f32>(15.0)));
    let idx = b.y * 16u + b.x;
    var m = array<u32, 8>(in.m0.x, in.m0.y, in.m0.z, in.m0.w, in.m1.x, in.m1.y, in.m1.z, in.m1.w);
    if (((m[idx / 32u] >> (idx % 32u)) & 1u) == 0u) {
        discard;
    }
    let bin_px = in.size / 16.0;
    let f = (fract(gg) - vec2<f32>(0.5)) * bin_px;
    let r = bin_px * 0.42;
    let cov = clamp(r - length(f) + 0.5, 0.0, 1.0);
    let a = in.alpha * cov;
    return vec4<f32>(vec3<f32>(0.22, 0.22, 0.3) * a, a);
}
