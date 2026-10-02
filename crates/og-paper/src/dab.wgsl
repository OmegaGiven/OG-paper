// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

// Brush-engine dabs: one quad per stamp. Tips, grain and patterns are
// procedural (no bitmaps), so they stay sharp at any zoom. Grain and
// patterns are anchored to the stroke (in stroke widths), so overlapping
// dabs line up and the texture holds still as the stroke grows.

struct Globals {
    viewport: vec2<f32>,
    tex_w: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    // Position in the dab: -1..1 across its width and height.
    @location(0) uv: vec2<f32>,
    // Texture position, in stroke widths.
    @location(1) tex: vec2<f32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    // aspect, hardness, grain, rand
    @location(3) @interpolate(flat) misc: vec4<f32>,
    // tip, pattern, size (px), pattern scale
    @location(4) @interpolate(flat) kind: vec4<f32>,
};

@vertex
fn vs_dab(
    @builtin(vertex_index) vi: u32,
    // x, y (px), size (px), angle
    @location(0) psa: vec4<f32>,
    // aspect, hardness, grain, rand
    @location(1) misc: vec4<f32>,
    // tex x, y (widths), widths per px, pattern scale (widths)
    @location(2) tex: vec4<f32>,
    // color, tip, pattern, flags
    @location(3) ids: vec4<u32>,
) -> VsOut {
    var corner = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    // A little margin for antialiasing.
    let size = max(psa.z, 0.5);
    let grow = 1.0 + 2.0 / size;
    let q = corner[vi] * grow;
    let half = vec2<f32>(size * 0.5, size * 0.5 * misc.x);
    let local = q * half;
    let c = cos(psa.w);
    let s = sin(psa.w);
    let off = vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    let p = psa.xy + off;
    var out: VsOut;
    out.pos = vec4<f32>(p.x / g.viewport.x * 2.0 - 1.0, 1.0 - p.y / g.viewport.y * 2.0, 0.0, 1.0);
    out.uv = q;
    out.tex = tex.xy + off * tex.z;
    out.color = unpack4x8unorm(ids.x);
    out.misc = misc;
    out.kind = vec4<f32>(f32(ids.y), f32(ids.z), size, tex.w);
    return out;
}

fn hash2(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}

// Value noise in 0..1.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Coverage of a shape whose edge is at d = 1 (d < 1 inside): soft by
// hardness, else a pixel-wide edge.
fn edge(d: f32, hardness: f32, aa: f32) -> f32 {
    let soft = max(1.0 - hardness, aa);
    return clamp((1.0 - d) / soft, 0.0, 1.0);
}

// 0..1 pattern value at p (in pattern cells), with a pixel-wide edge `aa`.
fn pattern(kind: u32, p: vec2<f32>, aa: f32) -> f32 {
    switch kind {
        case 0u: { // dots
            let d = length(fract(p) - 0.5);
            return clamp((0.28 - d) / aa + 0.5, 0.0, 1.0);
        }
        case 1u: { // hatch
            let d = abs(fract(p.x + p.y) - 0.5);
            return clamp((0.18 - d) / aa + 0.5, 0.0, 1.0);
        }
        case 2u: { // cross-hatch
            let a = abs(fract(p.x + p.y) - 0.5);
            let b = abs(fract(p.x - p.y) - 0.5);
            return clamp((0.14 - min(a, b)) / aa + 0.5, 0.0, 1.0);
        }
        case 3u: { // weave
            let cell = floor(p);
            let f = fract(p);
            let across = (i32(cell.x) + i32(cell.y)) % 2 == 0;
            var d = abs(f.y - 0.5);
            if (across) { d = abs(f.x - 0.5); }
            return clamp((0.36 - d) / aa + 0.5, 0.0, 1.0) * (0.75 + 0.25 * select(f.x, f.y, across));
        }
        case 4u: { // grain
            let n = vnoise(p * 3.0) * 0.6 + vnoise(p * 9.0) * 0.4;
            return smoothstep(0.45, 0.7, n);
        }
        case 5u: { // checker
            let c = floor(p);
            return select(0.0, 1.0, (i32(c.x) + i32(c.y)) % 2 == 0);
        }
        case 6u: { // stripes
            let d = abs(fract(p.y) - 0.5);
            return clamp((0.25 - d) / aa + 0.5, 0.0, 1.0);
        }
        default: { // scales
            let row = floor(p.y);
            let x = p.x + select(0.0, 0.5, i32(row) % 2 == 0);
            let f = vec2<f32>(fract(x) - 0.5, fract(p.y));
            let d = abs(length(f) - 0.5);
            return clamp((0.07 - d) / aa + 0.5, 0.0, 1.0);
        }
    }
}

// Signed distance to a heart, point at the origin, about 1.1 tall
// (Inigo Quilez).
fn sd_heart(p0: vec2<f32>) -> f32 {
    let p = vec2<f32>(abs(p0.x), p0.y);
    if (p.y + p.x > 1.0) {
        return length(p - vec2<f32>(0.25, 0.75)) - 0.35355339;
    }
    let a = p - vec2<f32>(0.0, 1.0);
    let b = p - 0.5 * max(p.x + p.y, 0.0);
    return sqrt(min(dot(a, a), dot(b, b))) * sign(p.x - p.y);
}

@fragment
fn fs_dab(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let r = length(uv);
    let hardness = in.misc.y;
    let grain = in.misc.z;
    let rnd = in.misc.w;
    let tip = u32(in.kind.x + 0.5);
    let size = in.kind.z;
    // One pixel, in dab units.
    let aa = 2.0 / max(size, 1.0);
    var a = 0.0;
    var col = in.color.rgb;
    switch tip {
        case 1u: { // square
            a = edge(max(abs(uv.x), abs(uv.y)), hardness, aa);
        }
        case 2u: { // chalk: broken, dry
            let wob = vnoise(uv * 2.5 + rnd * 17.0) - 0.5;
            a = edge(r * (1.0 + 0.35 * wob), hardness, aa);
            // Hard-edged, so overlapping dabs keep the same gaps.
            a = a * smoothstep(0.38, 0.46, vnoise(in.tex * 14.0));
        }
        case 3u: { // bristles: streaks along the stroke
            let streak = 0.5 + 0.5 * sin(uv.y * 23.0 + 3.0 * vnoise(vec2<f32>(uv.y * 5.0, 0.5)));
            a = edge(r, hardness, aa) * smoothstep(0.15, 0.65, streak);
        }
        case 4u: { // star
            let t = atan2(uv.y, uv.x);
            let rs = mix(0.42, 1.0, pow(abs(cos(t * 2.5)), 3.0));
            a = edge(r / rs, hardness, aa);
        }
        case 5u: { // leaf
            let w = 0.55 * (1.0 - uv.x * uv.x) + 0.001;
            a = edge(max(abs(uv.x), abs(uv.y) / w), hardness, aa);
            // A vein.
            a = a * (1.0 - 0.35 * clamp(1.0 - abs(uv.y) / (aa * 1.5 + 0.03), 0.0, 1.0));
        }
        case 6u: { // splat: a lumpy blob with droplets
            let t = atan2(uv.y, uv.x);
            let rs = 0.72 + 0.18 * sin(t * 5.0 + rnd * 9.0) * cos(t * 3.0 + rnd * 4.0) + 0.1 * sin(t * 11.0 + rnd * 2.0);
            a = edge(r / rs, hardness, aa);
            let dp = vec2<f32>(cos(rnd * 40.0), sin(rnd * 40.0)) * 0.85;
            a = max(a, edge(length(uv - dp) / 0.12, hardness, aa * 6.0));
        }
        case 7u: { // wash: soft, darker where it pools at the rim
            a = edge(r, hardness, aa) * (0.55 + 0.45 * smoothstep(0.45, 0.95, r));
        }
        case 8u: { // glow: bright core, soft halo
            let halo = exp(-r * r * 3.5) * 0.85;
            let core = edge(r / 0.35, 0.8, aa);
            a = max(halo, core);
            col = mix(col, vec3<f32>(1.0), core * 0.75);
        }
        case 9u: { // ring
            a = edge(abs(r - 0.8) / 0.2, hardness, aa * 4.0);
        }
        case 10u: { // stitch: a short dash
            a = edge(max(abs(uv.x), abs(uv.y) / 0.16), hardness, aa);
        }
        case 11u: { // pattern inside a round tip
            let cells = in.tex / max(in.kind.w, 0.01);
            let paa = fwidth(cells.x) + fwidth(cells.y);
            a = edge(r, hardness, aa) * pattern(u32(in.kind.y + 0.5), cells, max(paa, 0.001));
        }
        case 12u: { // heart (exact distance, point down)
            let q = vec2<f32>(uv.x, -uv.y) * 0.6 + vec2<f32>(0.0, 0.6);
            let d = sd_heart(q);
            a = clamp(-d / max(aa * 0.6, (1.0 - hardness) * 0.3) + 0.5, 0.0, 1.0);
        }
        default: { // round
            a = edge(r, hardness, aa);
        }
    }
    // Paper grain shows through.
    // Hard-edged gaps where the paper's tooth is, the same for every dab of
    // the stroke, so they stay open however much the dabs overlap.
    if (grain > 0.0) {
        let n = vnoise(in.tex * 9.0) * 0.6 + vnoise(in.tex * 27.0) * 0.4;
        let lo = grain * 0.62;
        a = a * smoothstep(lo, lo + 0.06, n);
    }
    a = a * in.color.a;
    if (a <= 0.002) {
        discard;
    }
    return vec4<f32>(col * a, a);
}
