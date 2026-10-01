// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A small single-stroke font, so text is ink: it stays sharp at any zoom,
//! erases, undoes and replays like any stroke. Glyphs sit on a 4 x 6 grid
//! (cap height 6, x-height 4, descenders to 8), y down.

use crate::shapes::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Font {
    /// Clean, proportional.
    #[default]
    Normal,
    /// Slanted and wobbly, like quick handwriting.
    Hand,
    /// Fixed width.
    Code,
}

pub const FONTS: [Font; 3] = [Font::Normal, Font::Hand, Font::Code];

impl Font {
    pub fn name(self) -> &'static str {
        match self {
            Font::Normal => "Normal",
            Font::Hand => "Hand-drawn",
            Font::Code => "Code",
        }
    }
    pub fn from_u8(v: u8) -> Self {
        FONTS.get(v as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

pub const ALIGNS: [Align; 3] = [Align::Left, Align::Center, Align::Right];

impl Align {
    pub fn from_u8(v: u8) -> Self {
        ALIGNS.get(v as usize).copied().unwrap_or_default()
    }
}

/// Grid units from one line's top to the next.
pub const LINE_UNITS: f64 = 11.0;

/// Horizontal extent of a glyph's ink (grid units).
fn ink_x(c: char) -> (f64, f64) {
    let g = glyph(c);
    let lo = g.iter().flatten().fold(f64::MAX, |m, p| m.min(p[0]));
    let hi = g.iter().flatten().fold(f64::MIN, |m, p| m.max(p[0]));
    if lo > hi {
        (0.0, 0.0)
    } else {
        (lo, hi)
    }
}

/// Horizontal advance of a glyph and how far to shift it left (grid units).
fn advance(c: char, font: Font) -> (f64, f64) {
    if font == Font::Code {
        return (6.0, -1.0);
    }
    if c == ' ' {
        return (3.6, 0.0);
    }
    let (lo, hi) = ink_x(c);
    ((hi - lo + 1.6).max(1.6), lo)
}

/// Lay out `text` (lines split on newlines) as strokes, in grid units, with
/// the box's top-left at the origin. Returns the strokes and the box size.
pub fn layout(text: &str, font: Font, align: Align, seed: u32) -> (Vec<Vec<[f64; 2]>>, [f64; 2]) {
    let lines: Vec<&str> = text.split('\n').collect();
    let widths: Vec<f64> = lines
        .iter()
        .map(|l| l.chars().map(|c| advance(c, font).0).sum::<f64>())
        .collect();
    let box_w = widths.iter().cloned().fold(0.0, f64::max).max(1.0);
    let box_h = LINE_UNITS * lines.len() as f64 - (LINE_UNITS - 8.0);
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let mut x = match align {
            Align::Left => 0.0,
            Align::Center => (box_w - widths[i]) * 0.5,
            Align::Right => box_w - widths[i],
        };
        let y = i as f64 * LINE_UNITS;
        for c in line.chars() {
            let (adv, shift) = advance(c, font);
            let dx = -shift;
            for stroke in glyph(c) {
                let pts = stroke
                    .iter()
                    .map(|p| {
                        let (mut px, mut py) = (x + dx + p[0], y + p[1]);
                        if font == Font::Hand {
                            // Slant and a small, repeatable wobble.
                            px += (6.0 - p[1]) * 0.16 + rng.f_pub() * 0.18;
                            py += rng.f_pub() * 0.18;
                        }
                        [px, py]
                    })
                    .collect();
                out.push(pts);
            }
            x += adv;
        }
    }
    (out, [box_w, box_h])
}

/// Single-stroke glyphs on a 4 x 6 grid (y down).
pub fn glyph(c: char) -> Vec<Vec<[f64; 2]>> {
    let spec: &str = match c {
        'A' => "0,6 0,2 2,0 4,2 4,6;0,3.5 4,3.5",
        'B' => "0,0 0,6 3,6 4,5 4,4 3,3 0,3;0,0 3,0 4,1 4,2 3,3",
        'C' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5",
        'D' => "0,0 0,6 2,6 4,4 4,2 2,0 0,0",
        'E' => "4,0 0,0 0,6 4,6;0,3 3,3",
        'F' => "4,0 0,0 0,6;0,3 3,3",
        'G' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5 4,3.5 2,3.5",
        'H' => "0,0 0,6;4,0 4,6;0,3 4,3",
        'I' => "1,0 3,0;2,0 2,6;1,6 3,6",
        'J' => "4,0 4,5 3,6 1,6 0,5",
        'K' => "0,0 0,6;4,0 0,4;1.3,3 4,6",
        'L' => "0,0 0,6 4,6",
        'M' => "0,6 0,0 2,3 4,0 4,6",
        'N' => "0,6 0,0 4,6 4,0",
        'O' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0",
        'P' => "0,6 0,0 3,0 4,1 4,2 3,3 0,3",
        'Q' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0;2.5,4.5 4,6",
        'R' => "0,6 0,0 3,0 4,1 4,2 3,3 0,3;2,3 4,6",
        'S' => "4,1 3,0 1,0 0,1 0,2 1,3 3,3 4,4 4,5 3,6 1,6 0,5",
        'T' => "0,0 4,0;2,0 2,6",
        'U' => "0,0 0,5 1,6 3,6 4,5 4,0",
        'V' => "0,0 2,6 4,0",
        'W' => "0,0 1,6 2,3 3,6 4,0",
        'X' => "0,0 4,6;4,0 0,6",
        'Y' => "0,0 2,3 4,0;2,3 2,6",
        'Z' => "0,0 4,0 0,6 4,6",
        '0' => "1,0 3,0 4,1 4,5 3,6 1,6 0,5 0,1 1,0;0.5,5 3.5,1",
        '1' => "1,1 2,0 2,6;1,6 3,6",
        '2' => "0,1 1,0 3,0 4,1 4,2 0,6 4,6",
        '3' => "0,1 1,0 3,0 4,1 4,2 3,3 4,4 4,5 3,6 1,6 0,5;1.5,3 3,3",
        '4' => "3,6 3,0 0,4 4,4",
        '5' => "4,0 0,0 0,3 3,3 4,4 4,5 3,6 0,6",
        '6' => "4,1 3,0 1,0 0,1 0,5 1,6 3,6 4,5 4,4 3,3 0,3",
        '7' => "0,0 4,0 1,6",
        '8' => "1,3 0,2 0,1 1,0 3,0 4,1 4,2 3,3 1,3 0,4 0,5 1,6 3,6 4,5 4,4 3,3",
        '9' => "4,3 1,3 0,2 0,1 1,0 3,0 4,1 4,5 3,6 1,6 0,5",
        '.' => "2,5.9 2,6",
        ',' => "2,5.5 2,6 1.4,7",
        '!' => "2,0 2,4;2,5.9 2,6",
        '?' => "0,1 1,0 3,0 4,1 4,2 2,3.5 2,4.2;2,5.9 2,6",
        '-' => "1,3 3,3",
        '\'' => "2,0 2,1.5",
        ':' => "2,1.9 2,2;2,4.9 2,5",
        '/' => "4,0 0,6",
        '>' => "0,0 4,3 0,6",
        '<' => "4,0 0,3 4,6",
        '(' => "3,0 2,1 1.5,2.5 1.5,3.5 2,5 3,6",
        ')' => "1,0 2,1 2.5,2.5 2.5,3.5 2,5 1,6",
        '+' => "2,1.5 2,4.5;0.5,3 3.5,3",
        '=' => "1,2 3,2;1,4 3,4",
        '×' => "1,2 3,4;3,2 1,4",
        'a' => "4,2 4,6;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'b' => "0,0 0,6;0,3 1,2 3,2 4,3 4,5 3,6 1,6 0,5",
        'c' => "4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'd' => "4,0 4,6;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'e' => "0,4 4,4 4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5.4",
        'f' => "3.6,0.4 3,0 2.2,0 1.4,0.8 1.4,6;0,2.4 3.2,2.4",
        'g' => "4,2 4,7 3,8 1,8 0.3,7.4;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'h' => "0,0 0,6;0,3 1,2 3,2 4,3 4,6",
        'i' => "2,2 2,6;2,0.3 2,0.6",
        'j' => "3,2 3,7 2.2,8 0.8,8;3,0.3 3,0.6",
        'k' => "0,0 0,6;3.8,2 0,4.6;1.3,3.7 4,6",
        'l' => "1.5,0 1.5,5.2 2.3,6 3.2,6",
        'm' => "0,6 0,2;0,3 0.7,2 1.4,2 2,3 2,6;2,3 2.6,2 3.3,2 4,3 4,6",
        'n' => "0,6 0,2;0,3 1,2 3,2 4,3 4,6",
        'o' => "1,2 3,2 4,3 4,5 3,6 1,6 0,5 0,3 1,2",
        'p' => "0,2 0,8;0,3 1,2 3,2 4,3 4,5 3,6 1,6 0,5",
        'q' => "4,2 4,8;4,3 3,2 1,2 0,3 0,5 1,6 3,6 4,5",
        'r' => "0,6 0,2;0,3.6 1.6,2 3.6,2",
        's' => "4,2.6 3,2 1,2 0,2.8 1,3.9 3,4.1 4,5 3,6 1,6 0,5.4",
        't' => "1.5,0.5 1.5,5.2 2.3,6 3.6,6;0,2 3.4,2",
        'u' => "0,2 0,5 1,6 3,6 4,5;4,2 4,6",
        'v' => "0,2 2,6 4,2",
        'w' => "0,2 1,6 2,3 3,6 4,2",
        'x' => "0,2 4,6;4,2 0,6",
        'y' => "0,2 2,6.2;4,2 1,8 0.2,8",
        'z' => "0,2 4,2 0,6 4,6",
        ';' => "2,1.9 2,2.1;2,4.9 2,5.5 1.4,6.6",
        '"' => "1.2,0 1.2,1.6;2.8,0 2.8,1.6",
        '#' => "1.4,0.8 0.8,6;3.2,0.8 2.6,6;0,2.4 4,2.4;0,4.4 4,4.4",
        '*' => "2,1.4 2,4.6;0.6,2.2 3.4,3.8;3.4,2.2 0.6,3.8",
        '&' => "4,6 1.2,2.6 1.2,1 2,0 3,0.6 3,1.8 0,4.4 0,5.4 1,6 2.6,6 4,3.6",
        '%' => "4,0 0,6;0.8,0.4 0.8,1.6;3.2,4.4 3.2,5.6",
        '_' => "0,6.8 4,6.8",
        '[' => "3,0 1.6,0 1.6,6.6 3,6.6",
        ']' => "1,0 2.4,0 2.4,6.6 1,6.6",
        '{' => "3,0 2.2,0.4 2,2.6 1,3.3 2,4 2.2,6.2 3,6.6",
        '}' => "1,0 1.8,0.4 2,2.6 3,3.3 2,4 1.8,6.2 1,6.6",
        '@' => "3,4 3,2.4 2,2 1.3,3 1.5,4.2 2.5,4.4 3,4 3.6,4.4 4,3.4 4,2 3,0.5 1,0.5 0,2 0,4.6 1,6 3.6,6",
        '$' => "4,1 3,0.4 1,0.4 0,1.2 0,2.2 1,3.1 3,3.1 4,4 4,5 3,5.8 1,5.8 0,5.2;2,-0.5 2,6.6",
        '|' => "2,0 2,7",
        '~' => "0,3.6 1,2.8 3,4.2 4,3.4",
        '`' => "1.4,0 2.6,1.2",
        '\\' => "0,0 4,6",
        '^' => "1,2 2,0 3,2",
        _ => "",
    };
    spec.split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.split_whitespace()
                .map(|p| {
                    let (x, y) = p.split_once(',').expect("glyph point");
                    [x.parse().expect("glyph x"), y.parse().expect("glyph y")]
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_ascii_has_glyphs() {
        for c in (33u8..127).map(char::from) {
            assert!(!glyph(c).is_empty(), "{c:?}");
        }
    }

    #[test]
    fn layout_measures_lines() {
        let (strokes, size) = layout("Hi\nthere", Font::Normal, Align::Center, 1);
        assert!(strokes.len() > 6);
        assert!(size[0] > 10.0 && size[1] > LINE_UNITS);
        let (_, mono) = layout("ab", Font::Code, Align::Left, 1);
        assert_eq!(mono[0], 12.0);
    }
}
