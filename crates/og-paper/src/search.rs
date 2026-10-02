// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Find text anywhere on the canvas (texts and table cells, at any zoom
//! depth) and fly to it, framed at a readable size, with a brief highlight.

use ogpaper_core::Camera;
use web_time::{Duration, Instant};

use crate::objects::{to_cam, ObjData, ObjRef};
use crate::App;

/// How long a found text stays outlined after flying to it.
pub const FLASH: Duration = Duration::from_millis(2200);
const MAX_HITS: usize = 200;

/// One match: the text group, a short excerpt around the match, and the
/// zoom depth (log10) it will be shown at.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub group: u32,
    pub snippet: String,
    pub zoom: f64,
}

/// The words of a text or table as one string to search.
fn searchable(data: &ObjData) -> Option<String> {
    match data {
        ObjData::Text { text, .. } => Some(text.clone()),
        ObjData::Table { cells, .. } => Some(
            cells
                .iter()
                .map(|r| r.join(" | "))
                .collect::<Vec<_>>()
                .join(" / "),
        ),
        _ => None,
    }
}

/// Up to ~70 characters around the match at char index `at` (len `n`).
fn excerpt(chars: &[char], at: usize, n: usize) -> String {
    let from = at.saturating_sub(24);
    let to = (at + n + 40).min(chars.len());
    let mut s: String = chars[from..to].iter().collect();
    s = s.replace(['\n', '\t'], " ");
    if from > 0 {
        s.insert(0, '…');
    }
    if to < chars.len() {
        s.push('…');
    }
    s
}

/// Case-insensitive position of `q` in `text`, as char indices.
fn find_ci(text: &str, q: &[char]) -> Option<(Vec<char>, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    if q.is_empty() || q.len() > lower.len() {
        return None;
    }
    let at = (0..=lower.len() - q.len()).find(|&i| lower[i..i + q.len()] == *q)?;
    Some((chars, at))
}

impl App {
    /// Every live text or table containing `query` (case-insensitive), oldest
    /// first.
    pub(crate) fn search(&self, query: &str) -> Vec<Hit> {
        let q: Vec<char> = query
            .trim()
            .chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        let mut hits = Vec::new();
        if q.is_empty() {
            return hits;
        }
        for (g, grp) in self.objs.groups.iter().enumerate() {
            let g = g as u32;
            let Some(text) = searchable(&grp.data) else {
                continue;
            };
            if !self.objs.alive(&self.scene, &ObjRef::Group(g)) {
                continue;
            }
            if let Some((chars, at)) = find_ci(&text, &q) {
                let zoom = self.search_target(g).map_or(0.0, |c| c.log10_zoom());
                hits.push(Hit {
                    group: g,
                    snippet: excerpt(&chars, at, q.len()),
                    zoom,
                });
                if hits.len() >= MAX_HITS {
                    break;
                }
            }
        }
        hits
    }

    /// A camera that frames group `g` at about half the screen.
    fn search_target(&self, g: u32) -> Option<Camera> {
        let grp = self.objs.groups.get(g as usize)?;
        let geom = grp.data.geom();
        let mut cam = Camera::new(grp.cell.clone(), geom.center, self.cam.base_px);
        let ppc = cam.ppc();
        let [w, h] = self.size();
        // A rotated box needs room for its diagonal.
        let (hx, hy) = if geom.rot.abs() > 1e-6 {
            let r = geom.half[0].hypot(geom.half[1]);
            (r, r)
        } else {
            (geom.half[0], geom.half[1])
        };
        let bw = (2.0 * hx * ppc).max(1e-12);
        let bh = (2.0 * hy * ppc).max(1e-12);
        let f = (0.5 * w / bw).min(0.5 * h / bh);
        cam.zoom_at(f, [0.0, 0.0]);
        Some(cam)
    }

    /// Fly to group `g` and outline it for a moment.
    pub(crate) fn search_go(&mut self, g: u32) {
        if let Some(t) = self.search_target(g) {
            self.fly = Some(t);
            self.fly_last = Instant::now();
            self.flash = Some((g, Instant::now()));
            self.redraw();
        }
    }

    /// The outline of the group being flashed, in screen px, if still on.
    pub(crate) fn flash_corners(&self) -> Option<[[f64; 2]; 4]> {
        let (g, t0) = self.flash?;
        if t0.elapsed() > FLASH {
            return None;
        }
        let grp = self.objs.groups.get(g as usize)?;
        let d = to_cam(&grp.cell, &grp.data, &self.cam);
        let geom = d.geom();
        let pad = 6.0 * self.ppp() / self.cam.ppc();
        let (hx, hy) = (geom.half[0] + pad, geom.half[1] + pad);
        let (s, c) = geom.rot.sin_cos();
        let corner = |x: f64, y: f64| {
            let q = [
                geom.center[0] + x * c - y * s,
                geom.center[1] + x * s + y * c,
            ];
            self.cam_to_px(q)
        };
        Some([
            corner(-hx, -hy),
            corner(hx, -hy),
            corner(hx, hy),
            corner(-hx, hy),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_case_insensitively_with_excerpt() {
        let q: Vec<char> = "WORLD".to_lowercase().chars().collect();
        let (chars, at) = find_ci("Hello, World of ideas", &q).unwrap();
        assert_eq!(at, 7);
        assert_eq!(excerpt(&chars, at, q.len()), "Hello, World of ideas");
        assert!(find_ci("nothing here", &q).is_none());
        // Non-ASCII stays on char boundaries.
        let q: Vec<char> = "café".chars().collect();
        assert!(find_ci("Le CAFÉ du coin", &q).is_some());
    }
}
