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
    dash: u32,
    width: f32,
    color: u32,
};

const BRUSH_FILL: u32 = 3u;
// Fill polygons are tested point by point; outlines longer than this are cut
// (font.rs keeps letters within it).
const FILL_MAX_PTS: u32 = 1024u;

fn texel(i: u32) -> vec2<i32> {
    return vec2<i32>(i32(i % g.tex_w), i32(i / g.tex_w));
}

fn stroke_at(i: u32) -> StrokeRec {
    let v = textureLoad(strokes_tex, texel(i), 0);
    let kind = v.y >> 24u;
    return StrokeRec(v.x, v.y & 0xFFFFFFu, kind & 15u, (kind >> 4u) & 3u, bitcast<f32>(v.z), v.w);
}

// x, y (cell-local), pressure, distance along the stroke (cell-local).
fn point_at(i: u32) -> vec4<f32> {
    return textureLoad(points_tex, texel(i), 0);
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
    // Previous segment's start (px) and radius there, and whether it exists.
    @location(5) @interpolate(flat) prev: vec4<f32>,
    // Distance along the stroke at a and b (px), unit width (px), dash.
    @location(6) @interpolate(flat) along: vec4<f32>,
    // Fill: stroke id, kind (0 line, 1 fill); origin and scale.
    @location(7) @interpolate(flat) ids: vec2<u32>,
    @location(8) @interpolate(flat) xform: vec4<f32>,
};

fn collapsed() -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    out.px = vec2<f32>(0.0);
    out.a = vec2<f32>(0.0);
    out.b = vec2<f32>(0.0);
    out.r = vec2<f32>(0.0);
    out.color = vec4<f32>(0.0);
    out.prev = vec4<f32>(0.0);
    out.along = vec4<f32>(0.0);
    out.ids = vec2<u32>(0u);
    out.xform = vec4<f32>(0.0);
    return out;
}

// One instance covers a whole fill polygon: a quad over its bounding box.
fn vs_fill(vi: u32, sid: u32, s: StrokeRec, o: vec2<f32>, scale: f32) -> VsOut {
    if (vi >= 6u || s.len < 3u) {
        return collapsed();
    }
    var lo = vec2<f32>(1e30);
    var hi = vec2<f32>(-1e30);
    let n = min(s.len, FILL_MAX_PTS);
    for (var i = 0u; i < n; i = i + 1u) {
        let p = o + point_at(s.start + i).xy * scale;
        lo = min(lo, p);
        hi = max(hi, p);
    }
    lo = max(lo - vec2<f32>(1.0), vec2<f32>(-1.0));
    hi = min(hi + vec2<f32>(1.0), g.viewport + vec2<f32>(1.0));
    if (hi.x <= lo.x || hi.y <= lo.y) {
        return collapsed();
    }
    // When no edge of the outline crosses the view, the view is wholly
    // inside the polygon (paint it solid, no per-pixel test) or wholly
    // outside (nothing to draw). Deep in a picture, big fills around you
    // (walls, screens, glows) cover the whole view: without this every
    // pixel walks all their edges, and it slows the deeper you go.
    var kind = 1u;
    let vlo = vec2<f32>(-2.0);
    let vhi = g.viewport + vec2<f32>(2.0);
    var crosses = false;
    var prev = o + point_at(s.start + n - 1u).xy * scale;
    for (var i = 0u; i < n; i = i + 1u) {
        let cur = o + point_at(s.start + i).xy * scale;
        if (seg_hits_rect(prev, cur, vlo, vhi)) {
            crosses = true;
            break;
        }
        prev = cur;
    }
    if (!crosses) {
        if (winding_at(0.5 * (vlo + vhi), s, o, scale, n) == 0) {
            return collapsed();
        }
        kind = 2u;
    }
    // The biggest circle round the outline's middle that no edge enters:
    // pixels in it are inside without walking the outline (most of a big
    // disc, like a glow). Exact for any polygon whose middle is inside.
    var mid = vec2<f32>(0.0);
    var r_in = 0.0;
    if (kind == 1u) {
        var sum = vec2<f32>(0.0);
        for (var i = 0u; i < n; i = i + 1u) {
            sum = sum + o + point_at(s.start + i).xy * scale;
        }
        mid = sum / f32(n);
        if (winding_at(mid, s, o, scale, n) != 0) {
            r_in = 1e30;
            var e0 = o + point_at(s.start + n - 1u).xy * scale;
            for (var i = 0u; i < n; i = i + 1u) {
                let e1 = o + point_at(s.start + i).xy * scale;
                r_in = min(r_in, seg_dist(mid, e0, e1).x);
                e0 = e1;
            }
        }
    }
    var cx = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var cy = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    let p = vec2<f32>(mix(lo.x, hi.x, cx[vi]), mix(lo.y, hi.y, cy[vi]));
    var out = collapsed();
    out.pos = to_clip(p);
    out.px = p;
    out.color = unpack4x8unorm(s.color);
    out.ids = vec2<u32>(sid, kind);
    out.xform = vec4<f32>(o, scale, 0.0);
    out.a = mid;
    out.r = vec2<f32>(r_in, 0.0);
    return out;
}

// Whether segment a-b touches the rectangle lo-hi (Liang-Barsky clipping).
fn seg_hits_rect(a: vec2<f32>, b: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>) -> bool {
    if (all(a >= lo) && all(a <= hi)) {
        return true;
    }
    if (all(b >= lo) && all(b <= hi)) {
        return true;
    }
    let d = b - a;
    var t0 = 0.0;
    var t1 = 1.0;
    let p = array<f32, 4>(-d.x, d.x, -d.y, d.y);
    let q = array<f32, 4>(a.x - lo.x, hi.x - a.x, a.y - lo.y, hi.y - a.y);
    for (var k = 0u; k < 4u; k = k + 1u) {
        if (p[k] == 0.0) {
            if (q[k] < 0.0) {
                return false;
            }
        } else {
            let r = q[k] / p[k];
            if (p[k] < 0.0) {
                t0 = max(t0, r);
            } else {
                t1 = min(t1, r);
            }
            if (t0 > t1) {
                return false;
            }
        }
    }
    return true;
}

// The non-zero winding number of a fill's outline around point `px`.
fn winding_at(px: vec2<f32>, s: StrokeRec, o: vec2<f32>, scale: f32, n: u32) -> i32 {
    var wind = 0;
    var prev = o + point_at(s.start + n - 1u).xy * scale;
    for (var i = 0u; i < n; i = i + 1u) {
        let cur = o + point_at(s.start + i).xy * scale;
        if ((prev.y <= px.y) != (cur.y <= px.y)) {
            let x = prev.x + (px.y - prev.y) / (cur.y - prev.y) * (cur.x - prev.x);
            if (x > px.x) {
                if (cur.y > prev.y) {
                    wind = wind + 1;
                } else {
                    wind = wind - 1;
                }
            }
        }
        prev = cur;
    }
    return wind;
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
    if (s.brush == BRUSH_FILL) {
        return vs_fill(vi, sid, s, o, scale);
    }
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

    var out = collapsed();
    out.pos = to_clip(p);
    out.px = p;
    out.a = a;
    out.b = b;
    out.r = vec2<f32>(ra, rb);
    var c = unpack4x8unorm(s.color);
    // Hairlines fade rather than vanish.
    c.a = c.a * clamp(max(ra_true, rb_true) * 2.0, 0.2, 1.0);
    out.color = c;
    if (seg > 0u) {
        let pp = point_at(s.start + seg - 1u);
        let rp = max(hw * pressure_factor(s.brush, pp.z), 0.5);
        out.prev = vec4<f32>(o + pp.xy * scale, rp, 1.0);
    }
    out.along = vec4<f32>(pa.w * scale, pb.w * scale, max(s.width * scale, 1.5), f32(s.dash));
    return out;
}

// Distance from p to segment ab, and where along it (0..1) the closest point is.
fn seg_dist(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return vec2<f32>(length(pa - ba * h), h);
}

fn coverage(in: VsOut) -> f32 {
    let dh = seg_dist(in.px, in.a, in.b);
    var dist = dh.x;
    let h = dh.y;
    let rad = mix(in.r.x, in.r.y, h);
    // Dashes and dots: round-ended pieces along the stroke.
    let dash = u32(in.along.w);
    if (dash != 0u) {
        let t = mix(in.along.x, in.along.y, h);
        let u = in.along.z;
        var period = u * 2.4;
        var on = 0.0;
        if (dash == 1u) {
            period = u * 6.0;
            on = u * 3.0;
        }
        let m = t - floor(t / period) * period;
        var gap = 0.0;
        if (m > on) {
            gap = min(m - on, period - m);
        }
        dist = length(vec2<f32>(dist, gap));
    }
    var cov = clamp(rad + 0.5 - dist, 0.0, 1.0);
    // Translucent ink: where the previous segment of this stroke already
    // covers the pixel, leave it, so joints do not darken.
    if (in.prev.w > 0.5 && in.color.a < 0.999 && dash == 0u) {
        let dp = seg_dist(in.px, in.prev.xy, in.a);
        let rp = mix(in.prev.z, in.r.x, dp.y);
        if (clamp(rp + 0.5 - dp.x, 0.0, 1.0) >= cov) {
            cov = 0.0;
        }
    }
    return cov;
}

// Fill: inside test (non-zero winding) over the outline, with 1 px AA from
// the distance to the nearest edge. A point with pressure < 0 marks a join
// between two contours (a letter's hole or second part): its edge counts for
// the winding (the joins cancel out) but is not an edge you can see.
fn fill_coverage(in: VsOut) -> f32 {
    // Well inside the outline (see vs_fill): no need to walk it.
    if (distance(in.px, in.a) < in.r.x - 1.0) {
        return 1.0;
    }
    let s = stroke_at(in.ids.x);
    let o = in.xform.xy;
    let scale = in.xform.z;
    let n = min(s.len, FILL_MAX_PTS);
    var wind = 0;
    var dmin = 1e30;
    var prev = o + point_at(s.start + n - 1u).xy * scale;
    for (var i = 0u; i < n; i = i + 1u) {
        let q = point_at(s.start + i);
        let cur = o + q.xy * scale;
        if (q.z >= 0.0) {
            dmin = min(dmin, seg_dist(in.px, prev, cur).x);
        }
        let crosses = (prev.y <= in.px.y) != (cur.y <= in.px.y);
        if (crosses) {
            let x = prev.x + (in.px.y - prev.y) / (cur.y - prev.y) * (cur.x - prev.x);
            if (x > in.px.x) {
                if (cur.y > prev.y) {
                    wind = wind + 1;
                } else {
                    wind = wind - 1;
                }
            }
        }
        prev = cur;
    }
    if (wind != 0) {
        return clamp(0.5 + dmin, 0.0, 1.0);
    }
    return clamp(0.5 - dmin, 0.0, 1.0);
}

// Normal ink and fills: premultiplied alpha over.
@fragment
fn fs_stroke(in: VsOut) -> @location(0) vec4<f32> {
    var cov = 0.0;
    if (in.ids.y == 2u) {
        // A fill that covers the whole view (see vs_fill).
        cov = 1.0;
    } else if (in.ids.y == 1u) {
        cov = fill_coverage(in);
    } else {
        cov = coverage(in);
    }
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
