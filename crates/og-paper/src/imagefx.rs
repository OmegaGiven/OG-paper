// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Pictures' colors (Brightness, Contrast and Saturation in the selection
//! panel, applied as the picture is drawn: the file stays as it was) and
//! Remove background: the plain background around a subject, from the
//! picture's edges inwards, made see-through in a new copy of the file
//! (one undo step back to the old).

use image::RgbaImage;

use crate::objects::{ObjData, ObjRef};
use crate::App;

/// How far (0-255 per channel, as a distance) a color may be from the
/// background's and still count as background.
const TOLERANCE: f32 = 42.0;

fn dist(a: [u8; 4], b: [f32; 3]) -> f32 {
    let d = [a[0] as f32 - b[0], a[1] as f32 - b[1], a[2] as f32 - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The background's color: the commonest color around the edges (in
/// coarse bins, averaged within the winning bin).
fn edge_color(img: &RgbaImage) -> Option<[f32; 3]> {
    let (w, h) = img.dimensions();
    let mut bins: std::collections::HashMap<[u8; 3], (u32, [f32; 3])> = Default::default();
    let mut add = |x: u32, y: u32| {
        let p = img.get_pixel(x, y).0;
        if p[3] < 128 {
            return;
        }
        let e = bins.entry([p[0] / 24, p[1] / 24, p[2] / 24]).or_default();
        e.0 += 1;
        for k in 0..3 {
            e.1[k] += p[k] as f32;
        }
    };
    for x in 0..w {
        add(x, 0);
        add(x, h - 1);
    }
    for y in 0..h {
        add(0, y);
        add(w - 1, y);
    }
    let (n, sum) = bins.into_values().max_by_key(|b| b.0)?;
    Some(sum.map(|v| v / n as f32))
}

/// Make the background around the subject see-through: everything joined
/// to the edges whose color is near the edges' commonest color. Its rim
/// fades out over a little band, so the cut is soft. False when there is
/// no plain background to take.
pub fn knock_out(img: &mut RgbaImage) -> bool {
    let (w, h) = img.dimensions();
    if w < 3 || h < 3 {
        return false;
    }
    let Some(bg) = edge_color(img) else {
        return false;
    };
    let near = |p: [u8; 4]| p[3] > 0 && dist(p, bg) < TOLERANCE;
    let idx = |x: u32, y: u32| (y * w + x) as usize;
    let mut out = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let i = idx(x, y);
        if out[i] || !near(img.get_pixel(x, y).0) {
            continue;
        }
        out[i] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    let taken = out.iter().filter(|o| **o).count();
    if taken == 0 || taken == out.len() {
        return false;
    }
    for y in 0..h {
        for x in 0..w {
            let i = idx(x, y);
            let p = img.get_pixel_mut(x, y);
            if out[i] {
                p.0[3] = 0;
                continue;
            }
            // Next to the background: fade by how near its color is.
            let edge = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)]
                .iter()
                .any(|(dx, dy)| {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    nx >= 0
                        && ny >= 0
                        && nx < w as i32
                        && ny < h as i32
                        && out[idx(nx as u32, ny as u32)]
                });
            if edge {
                let d = dist(p.0, bg);
                let keep = ((d - TOLERANCE * 0.5) / TOLERANCE).clamp(0.35, 1.0);
                p.0[3] = (p.0[3] as f32 * keep) as u8;
            }
        }
    }
    true
}

impl App {
    /// Remove the plain backgrounds of the selected pictures.
    pub(crate) fn remove_background(&mut self) {
        self.sel_alive();
        let mut made = 0;
        let mut failed = false;
        self.replace_selection(|app, r| {
            let ObjRef::Group(g) = r else { return None };
            let ObjData::Image { id, .. } = app.objs.groups[g as usize].data else {
                return None;
            };
            let asset = app.objs.images.get(&id)?;
            let mut img = image::load_from_memory(&asset.bytes).ok()?.to_rgba8();
            if !knock_out(&mut img) {
                failed = true;
                return None;
            }
            let new = crate::images::from_rgba(img).ok()?;
            let nid = crate::images::id_of(&new.bytes);
            app.objs.images.entry(nid).or_insert(new);
            made += 1;
            Some(app.group_copy(
                g,
                move |d| match d {
                    ObjData::Image {
                        geom,
                        opacity,
                        crop,
                        adjust,
                        ..
                    } => ObjData::Image {
                        id: nid,
                        geom,
                        opacity,
                        crop,
                        adjust,
                    },
                    d => d,
                },
                None,
            ))
        });
        if made == 0 && failed {
            self.say("No plain background around the edges to take away");
        }
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_background_goes_and_the_subject_stays() {
        // White, with a red square in the middle (and white inside it that
        // isn't joined to the edges: it stays).
        let mut img = RgbaImage::from_pixel(40, 40, image::Rgba([250, 250, 248, 255]));
        for y in 10..30 {
            for x in 10..30 {
                img.put_pixel(x, y, image::Rgba([200, 30, 30, 255]));
            }
        }
        for y in 18..22 {
            for x in 18..22 {
                img.put_pixel(x, y, image::Rgba([250, 250, 248, 255]));
            }
        }
        assert!(knock_out(&mut img));
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
        assert_eq!(img.get_pixel(5, 20).0[3], 0);
        assert_eq!(img.get_pixel(15, 15).0[3], 255);
        assert_eq!(img.get_pixel(20, 20).0[3], 255, "enclosed white stays");
        // A picture of noise has no plain background.
        let mut noise = RgbaImage::from_fn(20, 20, |x, y| {
            image::Rgba([
                (x * 53 % 255) as u8,
                (y * 97 % 255) as u8,
                ((x * y * 31) % 255) as u8,
                255,
            ])
        });
        assert!(!knock_out(&mut noise) || noise.pixels().filter(|p| p.0[3] == 0).count() < 40);
    }
}
