// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The brush engine: a stroke drawn as stamps ("dabs") of a tip along its
//! path, GIMP style. A stroke keeps only its points and these parameters;
//! the dabs are generated from them (deterministically, from a seed), so the
//! stroke stays vector: sharp at any zoom, small in files.
//!
//! Saved form (`BrushParams::encode`): version u8 (1), then the fields in
//! declaration order, f32 / u8 / u32 little-endian. Readers ignore trailing
//! bytes, so later versions only append.

use crate::scene::Point;

/// The shape stamped along the stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Tip {
    #[default]
    Round = 0,
    Square = 1,
    /// Rough, broken edge with dry texture.
    Chalk = 2,
    /// Parallel bristle streaks.
    Bristle = 3,
    Star = 4,
    Leaf = 5,
    /// A blob with droplets.
    Splat = 6,
    /// Soft wash, darker at the rim (watercolor).
    Wash = 7,
    /// Bright core, soft halo.
    Glow = 8,
    /// An oval outline (chain links when the tip follows the stroke).
    Ring = 9,
    /// A short dash across... along the stroke (stitches).
    Stitch = 10,
    /// A repeating pattern inside a round tip (see `Pattern`).
    Pattern = 11,
    Heart = 12,
}

impl Tip {
    pub const ALL: [Tip; 13] = [
        Tip::Round,
        Tip::Square,
        Tip::Chalk,
        Tip::Bristle,
        Tip::Star,
        Tip::Leaf,
        Tip::Splat,
        Tip::Wash,
        Tip::Glow,
        Tip::Ring,
        Tip::Stitch,
        Tip::Pattern,
        Tip::Heart,
    ];

    pub fn from_u8(v: u8) -> Self {
        Tip::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Tip::Round => "Round",
            Tip::Square => "Square",
            Tip::Chalk => "Chalk",
            Tip::Bristle => "Bristle",
            Tip::Star => "Star",
            Tip::Leaf => "Leaf",
            Tip::Splat => "Splat",
            Tip::Wash => "Wash",
            Tip::Glow => "Glow",
            Tip::Ring => "Ring",
            Tip::Stitch => "Stitch",
            Tip::Pattern => "Pattern",
            Tip::Heart => "Heart",
        }
    }
}

/// Patterns for the Pattern tip (texture painting), in canvas space so
/// overlapping dabs line up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Pattern {
    #[default]
    Dots = 0,
    Hatch = 1,
    CrossHatch = 2,
    Weave = 3,
    Grain = 4,
    Checker = 5,
    Stripes = 6,
    Scales = 7,
}

impl Pattern {
    pub const ALL: [Pattern; 8] = [
        Pattern::Dots,
        Pattern::Hatch,
        Pattern::CrossHatch,
        Pattern::Weave,
        Pattern::Grain,
        Pattern::Checker,
        Pattern::Stripes,
        Pattern::Scales,
    ];

    pub fn from_u8(v: u8) -> Self {
        Pattern::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Pattern::Dots => "Dots",
            Pattern::Hatch => "Hatch",
            Pattern::CrossHatch => "Cross-hatch",
            Pattern::Weave => "Weave",
            Pattern::Grain => "Grain",
            Pattern::Checker => "Checker",
            Pattern::Stripes => "Stripes",
            Pattern::Scales => "Scales",
        }
    }
}

/// Everything about how a dab stroke looks. Lengths are in stroke widths,
/// amounts 0..1 unless noted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushParams {
    /// The look it started from (for the UI; see `LOOKS`).
    pub look: u8,
    pub tip: Tip,
    /// Edge: 0 soft (feathered) .. 1 hard.
    pub hardness: f32,
    /// Distance between dabs, as a fraction of the dab size.
    pub spacing: f32,
    /// Tip angle (radians) and its minor / major axis ratio (1 = round).
    pub angle: f32,
    pub aspect: f32,
    /// The tip turns with the stroke.
    pub follow: bool,
    /// Pressure drives size and opacity (0 none .. 1 fully).
    pub p_size: f32,
    pub p_opacity: f32,
    /// Speed: positive makes fast strokes thinner / fainter.
    pub s_size: f32,
    pub s_opacity: f32,
    /// Taper at the start and end, in widths (0 = none).
    pub taper_in: f32,
    pub taper_out: f32,
    /// Fade out over this length, in widths (0 = none).
    pub fade: f32,
    /// Dabs scatter off the line by up to this many widths.
    pub jitter: f32,
    /// Random size and angle per dab.
    pub size_jitter: f32,
    pub angle_jitter: f32,
    /// Dabs per step (spray).
    pub count: u8,
    /// Opacity of each dab (they build up where they overlap).
    pub flow: f32,
    /// Random hue shift per dab (0..1 of the color wheel).
    pub hue_jitter: f32,
    /// The color at the end of the stroke (gradient); 0 = none.
    pub color2: u32,
    /// Paper grain showing through.
    pub grain: f32,
    /// A sketchy wobble off the path, in widths, and how many passes.
    pub wobble: f32,
    pub passes: u8,
    /// Pattern tip: which one and its size (in widths).
    pub pattern: Pattern,
    pub pattern_scale: f32,
    /// Fixed per stroke: makes the randomness repeatable.
    pub seed: u32,
    /// Dab size as a fraction of the stroke width (spray dots, sketch lines).
    pub dab_size: f32,
    /// Hue turns per 100 widths along the stroke (rainbow); 0 = none.
    pub hue_cycle: f32,
}

impl Default for BrushParams {
    fn default() -> Self {
        BrushParams {
            look: 0,
            tip: Tip::Round,
            hardness: 1.0,
            spacing: 0.1,
            angle: 0.0,
            aspect: 1.0,
            follow: false,
            p_size: 0.75,
            p_opacity: 0.0,
            s_size: 0.0,
            s_opacity: 0.0,
            taper_in: 0.0,
            taper_out: 0.0,
            fade: 0.0,
            jitter: 0.0,
            size_jitter: 0.0,
            angle_jitter: 0.0,
            count: 1,
            flow: 1.0,
            hue_jitter: 0.0,
            color2: 0,
            grain: 0.0,
            wobble: 0.0,
            passes: 1,
            pattern: Pattern::Dots,
            pattern_scale: 0.5,
            seed: 1,
            dab_size: 1.0,
            hue_cycle: 0.0,
        }
    }
}

/// A named starting point in the brush and texture pickers.
pub struct Look {
    pub name: &'static str,
    pub params: BrushParams,
    /// A texture-tool look (else a brush look).
    pub texture: bool,
}

fn look(name: &'static str, texture: bool, f: impl FnOnce(&mut BrushParams)) -> Look {
    let mut p = BrushParams::default();
    f(&mut p);
    Look {
        name,
        params: p,
        texture,
    }
}

/// The looks: brush looks first, then texture looks.
pub fn looks() -> Vec<Look> {
    let mut v = vec![
        look("Ink", false, |_| {}),
        look("Pencil", false, |p| {
            p.spacing = 0.08;
            p.p_size = 0.3;
            p.p_opacity = 0.7;
            p.grain = 0.7;
            p.hardness = 0.9;
            p.flow = 0.9;
        }),
        look("Charcoal", false, |p| {
            p.tip = Tip::Chalk;
            p.spacing = 0.06;
            p.grain = 0.6;
            p.p_opacity = 0.5;
            p.angle_jitter = 1.0;
            p.flow = 0.8;
        }),
        look("Bristle brush", false, |p| {
            p.tip = Tip::Bristle;
            p.follow = true;
            p.spacing = 0.04;
            p.p_size = 0.5;
            p.taper_out = 2.0;
            p.flow = 0.9;
        }),
        look("Watercolor", false, |p| {
            p.tip = Tip::Wash;
            p.hardness = 0.3;
            p.spacing = 0.15;
            p.flow = 0.3;
            p.size_jitter = 0.15;
            p.hue_jitter = 0.02;
            p.p_size = 0.4;
        }),
        look("Airbrush", false, |p| {
            p.hardness = 0.0;
            p.spacing = 0.1;
            p.flow = 0.08;
            p.p_size = 0.2;
            p.p_opacity = 0.8;
        }),
        look("Spray", false, |p| {
            p.count = 12;
            p.jitter = 0.6;
            p.spacing = 2.0;
            p.dab_size = 0.08;
            p.size_jitter = 0.6;
            p.p_size = 0.0;
            p.p_opacity = 0.6;
            p.hardness = 1.0;
        }),
        look("Calligraphy", false, |p| {
            p.aspect = 0.22;
            p.angle = std::f32::consts::FRAC_PI_4;
            p.spacing = 0.03;
            p.p_size = 0.4;
            p.hardness = 0.95;
        }),
        look("Sketchy", false, |p| {
            p.wobble = 0.5;
            p.passes = 3;
            p.dab_size = 0.25;
            p.spacing = 0.3;
            p.p_size = 0.2;
            p.flow = 0.8;
            p.taper_in = 3.0;
            p.taper_out = 3.0;
        }),
        look("Jagged", false, |p| {
            p.tip = Tip::Square;
            p.angle_jitter = 1.0;
            p.size_jitter = 0.5;
            p.jitter = 0.15;
            p.spacing = 0.12;
        }),
        look("Neon", false, |p| {
            p.tip = Tip::Glow;
            p.hardness = 0.5;
            p.spacing = 0.05;
            p.p_size = 0.2;
        }),
        look("Ribbon (chain)", false, |p| {
            p.tip = Tip::Ring;
            p.follow = true;
            p.aspect = 0.5;
            p.spacing = 0.9;
            p.p_size = 0.0;
        }),
        look("Stitches", false, |p| {
            p.tip = Tip::Stitch;
            p.follow = true;
            p.spacing = 1.6;
            p.p_size = 0.0;
        }),
        look("Felt tip", false, |p| {
            p.hardness = 0.7;
            p.spacing = 0.05;
            p.p_size = 0.0;
            p.flow = 0.55;
        }),
        look("Rainbow", false, |p| {
            p.spacing = 0.05;
            p.hue_cycle = 4.0;
            p.p_size = 0.3;
        }),
    ];
    v.extend([
        look("Splotches", true, |p| {
            p.tip = Tip::Splat;
            p.spacing = 0.9;
            p.jitter = 0.6;
            p.size_jitter = 0.7;
            p.angle_jitter = 1.0;
            p.p_size = 0.3;
        }),
        look("Spatter", true, |p| {
            p.count = 6;
            p.jitter = 1.2;
            p.spacing = 2.0;
            p.dab_size = 0.3;
            p.size_jitter = 0.9;
            p.tip = Tip::Splat;
            p.angle_jitter = 1.0;
            p.p_size = 0.0;
        }),
        look("Leaves", true, |p| {
            p.tip = Tip::Leaf;
            p.count = 2;
            p.jitter = 0.8;
            p.spacing = 0.7;
            p.size_jitter = 0.5;
            p.angle_jitter = 1.0;
            p.hue_jitter = 0.04;
            p.p_size = 0.0;
        }),
        look("Stars", true, |p| {
            p.tip = Tip::Star;
            p.jitter = 0.8;
            p.spacing = 1.2;
            p.size_jitter = 0.6;
            p.angle_jitter = 1.0;
            p.p_size = 0.0;
        }),
        look("Hearts", true, |p| {
            p.tip = Tip::Heart;
            p.jitter = 0.5;
            p.spacing = 1.3;
            p.size_jitter = 0.4;
            p.angle_jitter = 0.3;
            p.p_size = 0.0;
        }),
        look("Pattern", true, |p| {
            p.tip = Tip::Pattern;
            p.spacing = 0.15;
            p.hardness = 0.8;
            p.p_size = 0.0;
        }),
        look("Paper grain", true, |p| {
            p.tip = Tip::Pattern;
            p.pattern = Pattern::Grain;
            p.spacing = 0.15;
            p.hardness = 0.3;
            p.flow = 0.5;
            p.p_size = 0.0;
        }),
        look("Sponge", true, |p| {
            p.tip = Tip::Chalk;
            p.spacing = 0.5;
            p.jitter = 0.3;
            p.size_jitter = 0.3;
            p.angle_jitter = 1.0;
            p.grain = 0.9;
            p.flow = 0.6;
            p.p_size = 0.0;
        }),
    ]);
    for (i, l) in v.iter_mut().enumerate() {
        l.params.look = i as u8;
    }
    v
}

impl BrushParams {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![1u8, self.look, self.tip as u8];
        for v in [self.hardness, self.spacing, self.angle, self.aspect] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(self.follow as u8);
        for v in [
            self.p_size,
            self.p_opacity,
            self.s_size,
            self.s_opacity,
            self.taper_in,
            self.taper_out,
            self.fade,
            self.jitter,
            self.size_jitter,
            self.angle_jitter,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(self.count);
        for v in [self.flow, self.hue_jitter] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&self.color2.to_le_bytes());
        for v in [self.grain, self.wobble] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(self.passes);
        b.push(self.pattern as u8);
        b.extend_from_slice(&self.pattern_scale.to_le_bytes());
        b.extend_from_slice(&self.seed.to_le_bytes());
        b.extend_from_slice(&self.dab_size.to_le_bytes());
        b.extend_from_slice(&self.hue_cycle.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let mut r = Bytes { b, at: 0 };
        if r.u8()? != 1 {
            return None;
        }
        let look = r.u8()?;
        let tip = Tip::from_u8(r.u8()?);
        let (hardness, spacing, angle, aspect) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let follow = r.u8()? != 0;
        let (p_size, p_opacity, s_size, s_opacity) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let (taper_in, taper_out, fade) = (r.f32()?, r.f32()?, r.f32()?);
        let (jitter, size_jitter, angle_jitter) = (r.f32()?, r.f32()?, r.f32()?);
        let count = r.u8()?;
        let (flow, hue_jitter) = (r.f32()?, r.f32()?);
        let color2 = r.u32()?;
        let (grain, wobble) = (r.f32()?, r.f32()?);
        let passes = r.u8()?;
        let pattern = Pattern::from_u8(r.u8()?);
        let pattern_scale = r.f32()?;
        let seed = r.u32()?;
        let dab_size = r.f32()?;
        let hue_cycle = r.f32()?;
        Some(BrushParams {
            dab_size: dab_size.clamp(0.02, 4.0),
            hue_cycle: hue_cycle.clamp(-50.0, 50.0),
            look,
            tip,
            hardness: hardness.clamp(0.0, 1.0),
            spacing: spacing.clamp(0.01, 10.0),
            angle,
            aspect: aspect.clamp(0.02, 1.0),
            follow,
            p_size: p_size.clamp(0.0, 1.0),
            p_opacity: p_opacity.clamp(0.0, 1.0),
            s_size: s_size.clamp(-1.0, 1.0),
            s_opacity: s_opacity.clamp(-1.0, 1.0),
            taper_in: taper_in.clamp(0.0, 100.0),
            taper_out: taper_out.clamp(0.0, 100.0),
            fade: fade.clamp(0.0, 1000.0),
            jitter: jitter.clamp(0.0, 10.0),
            size_jitter: size_jitter.clamp(0.0, 1.0),
            angle_jitter: angle_jitter.clamp(0.0, 1.0),
            count: count.clamp(1, 64),
            flow: flow.clamp(0.0, 1.0),
            hue_jitter: hue_jitter.clamp(0.0, 1.0),
            color2,
            grain: grain.clamp(0.0, 1.0),
            wobble: wobble.clamp(0.0, 10.0),
            passes: passes.clamp(1, 8),
            pattern,
            pattern_scale: pattern_scale.clamp(0.05, 20.0),
            seed,
        })
    }
}

/// A little-endian reader.
struct Bytes<'a> {
    b: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let v = self.b.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(v)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.take()?);
        Some(if v.is_finite() { v } else { 0.0 })
    }
}

/// A tip's outline (unit size, centred, before aspect and rotation), for
/// previews and exports; soft and patterned tips give their round mask.
pub fn tip_outline(tip: Tip, rand: f32) -> Vec<[f32; 2]> {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let n = 24;
    let ring = |f: &dyn Fn(f32) -> f32| -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * TAU;
                let r = f(a) * 0.5;
                [a.cos() * r, a.sin() * r]
            })
            .collect()
    };
    match tip {
        Tip::Square => vec![[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]],
        Tip::Star => (0..10)
            .map(|i| {
                let a = i as f32 * PI / 5.0 - FRAC_PI_2;
                let r = if i % 2 == 0 { 0.5 } else { 0.21 };
                [a.cos() * r, a.sin() * r]
            })
            .collect(),
        Tip::Leaf => (0..n)
            .map(|i| {
                let t = i as f32 / n as f32 * TAU;
                let x = t.cos();
                [x * 0.5, t.sin() * 0.55 * (1.0 - x * x) * 0.5]
            })
            .collect(),
        Tip::Heart => (0..n)
            .map(|i| {
                let t = i as f32 / n as f32 * TAU;
                [
                    16.0 * t.sin().powi(3) / 34.0,
                    -(13.0 * t.cos()
                        - 5.0 * (2.0 * t).cos()
                        - 2.0 * (3.0 * t).cos()
                        - (4.0 * t).cos())
                        / 34.0,
                ]
            })
            .collect(),
        Tip::Splat => ring(&|a: f32| {
            0.72 + 0.18 * (a * 5.0 + rand * 9.0).sin() * (a * 3.0 + rand * 4.0).cos()
                + 0.1 * (a * 11.0 + rand * 2.0).sin()
        }),
        Tip::Chalk => ring(&|a: f32| 0.85 + 0.15 * (a * 9.0 + rand * 20.0).sin()),
        Tip::Stitch => vec![[-0.5, -0.08], [0.5, -0.08], [0.5, 0.08], [-0.5, 0.08]],
        _ => ring(&|_| 1.0),
    }
}

/// One stamp, in the stroke's own units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    /// Diameter (major axis).
    pub size: f32,
    pub angle: f32,
    /// RGBA8 (alpha = this dab's opacity).
    pub color: u32,
    /// Per-dab random number in 0..1 (varies the tip's details).
    pub rand: f32,
}

/// Most dabs one stroke makes (spacing grows past this).
pub const MAX_DABS: usize = 30_000;

/// Repeatable randomness: a hash of (seed, index) as 0..1.
fn rnd(seed: u32, i: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ i.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth 1D noise in -1..1 (for the sketchy wobble).
fn noise1(seed: u32, t: f32) -> f32 {
    let i = t.floor();
    let f = t - i;
    let a = rnd(seed, i as i32 as u32) * 2.0 - 1.0;
    let b = rnd(seed, (i as i32 + 1) as u32) * 2.0 - 1.0;
    let s = f * f * (3.0 - 2.0 * f);
    a + (b - a) * s
}

fn rgba(c: u32) -> [f32; 4] {
    let b = c.to_le_bytes();
    b.map(|v| v as f32 / 255.0)
}

fn pack(c: [f32; 4]) -> u32 {
    u32::from_le_bytes(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
}

/// Shift a color's hue by `h` turns.
/// The rainbow: the color's hue turned by `t` turns, made vivid enough to
/// show (a black ink still gives a rainbow).
fn rainbow(c: [f32; 4], t: f32) -> [f32; 4] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = if d < 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    h = (h + t).rem_euclid(1.0);
    let s = if max > 0.0 { d / max } else { 0.0 }.max(0.8);
    let v = max.max(0.8);
    let k = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [k(5.0), k(3.0), k(1.0), c[3]]
}

fn hue_shift(c: [f32; 4], h: f32) -> [f32; 4] {
    if h == 0.0 {
        return c;
    }
    // Rotate in YIQ space: cheap and good enough for jitter.
    let (r, g, b) = (c[0], c[1], c[2]);
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let i = 0.596 * r - 0.274 * g - 0.322 * b;
    let q = 0.211 * r - 0.523 * g + 0.312 * b;
    let (s, co) = (h * std::f32::consts::TAU).sin_cos();
    let (i2, q2) = (i * co - q * s, i * s + q * co);
    [
        y + 0.956 * i2 + 0.621 * q2,
        y - 0.272 * i2 - 0.647 * q2,
        y - 1.106 * i2 + 1.703 * q2,
        c[3],
    ]
}

/// The dabs of a stroke: `pts` as stored (x, y, pressure, distance along),
/// `width` and `color` the stroke's. Repeatable: the same input gives the
/// same dabs.
pub fn dabs(pts: &[Point], width: f32, color: u32, p: &BrushParams) -> Vec<Dab> {
    let mut out = Vec::new();
    if pts.is_empty() || !(width > 0.0) {
        return out;
    }
    let total = pts.last().map_or(0.0, |q| q[3]).max(0.0);
    let base = rgba(color);
    let end = (p.color2 != 0).then(|| rgba(p.color2));
    let w = width;
    // Average point gap: the speed proxy (input events come at a steady rate).
    let mean_gap = if pts.len() > 1 {
        total / (pts.len() - 1) as f32
    } else {
        0.0
    };
    let mut spacing = p.spacing.max(0.01);
    let est = (total / (w * spacing)).ceil() as usize
        * p.count.max(1) as usize
        * p.passes.max(1) as usize;
    if est > MAX_DABS {
        spacing *= est as f32 / MAX_DABS as f32;
    }
    let mut n: u32 = 0;
    for pass in 0..p.passes.max(1) {
        let pass_seed = p.seed.wrapping_add(pass as u32 * 7919);
        let mut seg = 0usize;
        let mut d = 0.0f32;
        loop {
            // The point at distance d.
            while seg + 1 < pts.len() && pts[seg + 1][3] < d {
                seg += 1;
            }
            let (a, b) = if seg + 1 < pts.len() {
                (pts[seg], pts[seg + 1])
            } else {
                (pts[seg], pts[seg])
            };
            let len = (b[3] - a[3]).max(1e-9);
            let t = ((d - a[3]) / len).clamp(0.0, 1.0);
            let x = a[0] + (b[0] - a[0]) * t;
            let y = a[1] + (b[1] - a[1]) * t;
            let pr = (a[2] + (b[2] - a[2]) * t).clamp(0.0, 1.0);
            let (dx, dy) = if seg + 1 < pts.len() {
                (b[0] - a[0], b[1] - a[1])
            } else if seg > 0 {
                (a[0] - pts[seg - 1][0], a[1] - pts[seg - 1][1])
            } else {
                (1.0, 0.0)
            };
            let dir = dy.atan2(dx);
            // Speed: this gap against the average one (1 = average).
            let speed = if mean_gap > 0.0 {
                (len / mean_gap).clamp(0.0, 3.0)
            } else {
                1.0
            };
            let mut size = w * (1.0 - p.p_size + p.p_size * pr);
            size *= 1.0 - p.s_size.clamp(-1.0, 1.0) * (speed - 1.0).clamp(-1.0, 1.0) * 0.5;
            if p.taper_in > 0.0 {
                size *= (d / (p.taper_in * w)).clamp(0.05, 1.0);
            }
            if p.taper_out > 0.0 {
                size *= ((total - d) / (p.taper_out * w)).clamp(0.05, 1.0);
            }
            let mut alpha = p.flow * (1.0 - p.p_opacity + p.p_opacity * pr);
            alpha *= 1.0 - p.s_opacity.clamp(-1.0, 1.0) * (speed - 1.0).clamp(-1.0, 1.0) * 0.5;
            if p.fade > 0.0 {
                alpha *= (1.0 - d / (p.fade * w)).clamp(0.0, 1.0);
            }
            let col = match end {
                Some(e) if total > 0.0 => {
                    let k = d / total;
                    let mut c = [0.0; 4];
                    for j in 0..4 {
                        c[j] = base[j] + (e[j] - base[j]) * k;
                    }
                    c
                }
                _ => base,
            };
            // The sketchy wobble: a smooth offset across the line.
            let (nx, ny) = (-dir.sin(), dir.cos());
            let wob = if p.wobble > 0.0 {
                noise1(pass_seed, d / (w * 6.0)) * p.wobble * w
            } else {
                0.0
            };
            for _ in 0..p.count.max(1) {
                n = n.wrapping_add(1);
                let r = |j: u32| rnd(pass_seed, n.wrapping_mul(8).wrapping_add(j));
                let (mut px, mut py) = (x + nx * wob, y + ny * wob);
                if p.jitter > 0.0 {
                    // Uniform in a disc of radius jitter * width.
                    let a = r(0) * std::f32::consts::TAU;
                    let rr = r(1).sqrt() * p.jitter * w;
                    px += a.cos() * rr;
                    py += a.sin() * rr;
                }
                let s = size * p.dab_size * (1.0 - p.size_jitter * r(2));
                let ang = p.angle
                    + if p.follow { dir } else { 0.0 }
                    + (r(3) - 0.5) * p.angle_jitter * std::f32::consts::TAU;
                let mut c = col;
                let mut h = 0.0;
                if p.hue_jitter > 0.0 {
                    h += (r(4) - 0.5) * 2.0 * p.hue_jitter;
                }
                c = hue_shift(c, h);
                if p.hue_cycle != 0.0 {
                    c = rainbow(c, d / (w * 100.0) * p.hue_cycle);
                }
                c[3] *= alpha.clamp(0.0, 1.0);
                if s > 0.0 && c[3] > 0.0 {
                    out.push(Dab {
                        x: px,
                        y: py,
                        size: s,
                        angle: ang,
                        color: pack(c),
                        rand: r(5),
                    });
                }
            }
            if d >= total {
                break;
            }
            d = (d + ((size * p.dab_size).max(w * 0.05) * spacing)).min(total);
            if out.len() >= MAX_DABS * 2 {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize, len: f32) -> Vec<Point> {
        (0..n)
            .map(|i| {
                let x = i as f32 / (n - 1) as f32 * len;
                [x, 0.0, 1.0, x]
            })
            .collect()
    }

    #[test]
    fn params_round_trip() {
        for l in looks() {
            let b = l.params.encode();
            assert_eq!(BrushParams::decode(&b), Some(l.params), "{}", l.name);
        }
        assert_eq!(BrushParams::decode(&[2]), None);
        assert_eq!(BrushParams::decode(&[1, 0]), None);
    }

    #[test]
    fn dabs_follow_spacing_and_repeat() {
        let p = BrushParams {
            spacing: 0.25,
            p_size: 0.0,
            ..Default::default()
        };
        let d = dabs(&line(11, 10.0), 1.0, 0xff00_00ff, &p);
        // 10 units, a dab every 0.25: 41 dabs.
        assert_eq!(d.len(), 41);
        assert!(d.iter().all(|q| q.y == 0.0 && (q.size - 1.0).abs() < 1e-6));
        assert_eq!(dabs(&line(11, 10.0), 1.0, 0xff00_00ff, &p), d);
        // Spray: several per step, scattered within the jitter.
        let s = BrushParams {
            count: 5,
            jitter: 1.0,
            spacing: 1.0,
            ..Default::default()
        };
        let d = dabs(&line(11, 10.0), 1.0, 0xffff_ffff, &s);
        assert_eq!(d.len(), 55);
        assert!(d.iter().all(|q| q.y.abs() <= 1.0));
    }

    #[test]
    fn huge_strokes_are_capped() {
        let p = BrushParams {
            spacing: 0.01,
            ..Default::default()
        };
        let d = dabs(&line(2, 1.0e6), 1.0, 0xffff_ffff, &p);
        assert!(d.len() <= MAX_DABS * 2);
    }
}
