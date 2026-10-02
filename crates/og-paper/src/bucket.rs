// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Bucket fill: tap inside a closed outline and it fills with color.
//!
//! The ink on screen is drawn into a coarse mask (one cell per point), the
//! tapped area is flood-filled (small gaps in the outline can be bridged),
//! grown a little under the lines so no seam shows, and traced back into
//! filled polygons. The result is real vector ink, placed just under the
//! lines around it, so it stays sharp when zoomed and can be selected,
//! recolored, moved or erased like anything else. The outline is traced in
//! tiles, so even a fill covering the screen stays cheap to draw.

use ogpaper_core::{Brush, Dash, Scene, Style};

use crate::objects::ObjData;
use crate::App;

/// Most mask cells (keeps big desktop screens quick).
const MAX_CELLS: f64 = 3.0e6;
/// How far (cells) the fill tucks under the lines around it.
const UNDER: usize = 2;
/// Tile side (cells): each tile's part of the fill is its own polygon.
const TILE: usize = 96;
/// The fill shader reads at most this many points per polygon.
const MAX_PTS: usize = 1000;

/// A grid over the screen: `scale` cells per screen pixel.
struct Mask {
    w: usize,
    h: usize,
    barrier: Vec<bool>,
}

impl Mask {
    fn new(w: usize, h: usize) -> Self {
        Mask {
            w,
            h,
            barrier: vec![false; w * h],
        }
    }

    /// A round-capped line from `a` to `b` (cells), radius `r`.
    fn capsule(&mut self, a: [f64; 2], b: [f64; 2], r: f64) {
        let r = r.max(0.5);
        let x0 = (a[0].min(b[0]) - r).floor().max(0.0) as usize;
        let y0 = (a[1].min(b[1]) - r).floor().max(0.0) as usize;
        let x1 = ((a[0].max(b[0]) + r).ceil().max(0.0) as usize).min(self.w);
        let y1 = ((a[1].max(b[1]) + r).ceil().max(0.0) as usize).min(self.h);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = (dx * dx + dy * dy).max(1e-12);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = [x as f64 + 0.5, y as f64 + 0.5];
                let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
                let (ex, ey) = (p[0] - a[0] - t * dx, p[1] - a[1] - t * dy);
                if ex * ex + ey * ey <= r * r {
                    self.barrier[y * self.w + x] = true;
                }
            }
        }
    }

    /// A filled polygon (non-zero winding), e.g. a letter.
    fn polygon(&mut self, pts: &[[f64; 2]]) {
        if pts.len() < 3 {
            return;
        }
        let y0 = pts
            .iter()
            .map(|p| p[1])
            .fold(f64::MAX, f64::min)
            .floor()
            .max(0.0) as usize;
        let y1 = (pts
            .iter()
            .map(|p| p[1])
            .fold(f64::MIN, f64::max)
            .ceil()
            .max(0.0) as usize)
            .min(self.h);
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for y in y0..y1 {
            let cy = y as f64 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a[1] <= cy) != (b[1] <= cy) {
                    let x = a[0] + (cy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    xs.push((x, if b[1] > a[1] { 1 } else { -1 }));
                }
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for k in 0..xs.len() {
                let before = wind;
                wind += xs[k].1;
                if before == 0 && wind != 0 {
                    let from = xs[k].0;
                    let to = xs[k + 1..]
                        .iter()
                        .scan(wind, |w, e| {
                            *w += e.1;
                            Some((e.0, *w))
                        })
                        .find(|e| e.1 == 0)
                        .map_or(from, |e| e.0);
                    let a = (from - 0.5).ceil().max(0.0) as usize;
                    let b = ((to - 0.5).floor() + 1.0).max(0.0) as usize;
                    for x in a..b.min(self.w) {
                        self.barrier[y * self.w + x] = true;
                    }
                }
            }
        }
    }
}

/// Grow `mask` by `n` cells (8-neighbour), only where `allow` says. Works
/// from the edge outwards, so it costs the edge length, not the screen.
fn grow(mask: &mut [bool], w: usize, h: usize, n: usize, allow: impl Fn(usize) -> bool) {
    let mut front: Vec<usize> = (0..mask.len()).filter(|&i| mask[i]).collect();
    for _ in 0..n {
        let mut next = Vec::new();
        for &i in &front {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !mask[j] && allow(j) {
                        mask[j] = true;
                        next.push(j);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        front = next;
    }
}

/// Flood the free cells reachable from `seed`. None if it reaches the edge
/// of the screen (the area isn't closed).
fn flood(free: &[bool], w: usize, h: usize, seed: usize) -> Option<Vec<bool>> {
    let mut region = vec![false; w * h];
    let mut stack = vec![seed];
    region[seed] = true;
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            return None;
        }
        for j in [i - 1, i + 1, i - w, i + w] {
            if free[j] && !region[j] {
                region[j] = true;
                stack.push(j);
            }
        }
    }
    Some(region)
}

/// A traced outline: corner points (cells) and, per point, whether the
/// edge coming into it is hidden (a cut between tiles).
type Loop = Vec<([f64; 2], bool)>;

/// The outlines of `region` inside the tile [x0, x1) x [y0, y1), along cell
/// edges: outer ones clockwise, holes anticlockwise (y down). Edges on the
/// tile's border with fill on the far side are cuts.
fn trace_tile(
    region: &[bool],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> Vec<Loop> {
    use std::collections::HashMap;
    let inside = |x: i64, y: i64| {
        x >= x0 as i64
            && y >= y0 as i64
            && x < x1 as i64
            && y < y1 as i64
            && region[y as usize * w + x as usize]
    };
    let global = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && region[y as usize * w + x as usize]
    };
    // Directed edges: from -> (to, cut).
    let mut out: HashMap<(i64, i64), Vec<((i64, i64), bool)>> = HashMap::new();
    let mut n = 0;
    for y in y0 as i64..y1 as i64 {
        for x in x0 as i64..x1 as i64 {
            if !inside(x, y) {
                continue;
            }
            // (neighbour, edge from, edge to): clockwise around the cell.
            for (nx, ny, a, b) in [
                (x, y - 1, (x, y), (x + 1, y)),
                (x + 1, y, (x + 1, y), (x + 1, y + 1)),
                (x, y + 1, (x + 1, y + 1), (x, y + 1)),
                (x - 1, y, (x, y + 1), (x, y)),
            ] {
                if !inside(nx, ny) {
                    out.entry(a).or_default().push((b, global(nx, ny)));
                    n += 1;
                }
            }
        }
    }
    let mut loops = Vec::new();
    while let Some((&start, _)) = out.iter().find(|(_, v)| !v.is_empty()) {
        let mut lp: Loop = Vec::new();
        let mut at = start;
        let mut dir = (0i64, 0i64);
        loop {
            let Some(v) = out.get_mut(&at) else { break };
            if v.is_empty() {
                break;
            }
            // Where two outlines touch at a corner, turn right (keeps
            // diagonal neighbours apart).
            let k = if v.len() > 1 {
                let right = (-dir.1, dir.0);
                v.iter()
                    .position(|e| (e.0 .0 - at.0, e.0 .1 - at.1) == right)
                    .unwrap_or(0)
            } else {
                0
            };
            let (to, cut) = v.swap_remove(k);
            dir = (to.0 - at.0, to.1 - at.1);
            lp.push(([to.0 as f64, to.1 as f64], cut));
            at = to;
            n -= 1;
            if at == start {
                break;
            }
        }
        if lp.len() >= 4 {
            loops.push(lp);
        }
        if n == 0 {
            break;
        }
    }
    loops
}

/// Drop points along straight runs, and simplify visible runs within `eps`
/// (cuts stay exact, so neighbouring tiles meet without a seam).
fn simplify(lp: &Loop, eps: f64) -> Loop {
    let n = lp.len();
    // Keep points where the edge kind changes or the direction turns on a cut.
    let keep_hard = |i: usize| {
        let (p, c) = lp[i];
        let (q, cq) = lp[(i + 1) % n];
        let (o, _) = lp[(i + n - 1) % n];
        c != cq
            || (c && ((p[0] - o[0]) * (q[1] - p[1]) - (p[1] - o[1]) * (q[0] - p[0])).abs() > 1e-9)
    };
    let hard: Vec<usize> = (0..n).filter(|&i| keep_hard(i)).collect();
    let mut keep = vec![false; n];
    if hard.is_empty() {
        // One visible loop: split it at its two farthest-apart points.
        let far = (1..n).max_by(|&a, &b| {
            crate::dist(lp[0].0, lp[a].0).total_cmp(&crate::dist(lp[0].0, lp[b].0))
        });
        keep[0] = true;
        if let Some(f) = far {
            keep[f] = true;
            rdp(lp, 0, f, eps, &mut keep);
            rdp(lp, f, n, eps, &mut keep);
        }
    } else {
        for (k, &a) in hard.iter().enumerate() {
            keep[a] = true;
            let b = if k + 1 < hard.len() {
                hard[k + 1]
            } else {
                hard[0] + n
            };
            // The run's edges are all of one kind: cuts are exact.
            let cut = lp[(a + 1) % n].1;
            rdp(lp, a, b, if cut { 1e-6 } else { eps }, &mut keep);
        }
    }
    (0..n).filter(|&i| keep[i]).map(|i| lp[i]).collect()
}

/// Ramer–Douglas–Peucker over lp[a..=b] (indexes wrap).
fn rdp(lp: &Loop, a: usize, b: usize, eps: f64, keep: &mut [bool]) {
    let n = lp.len();
    if b <= a + 1 {
        return;
    }
    let (p, q) = (lp[a % n].0, lp[b % n].0);
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let len = dx.hypot(dy);
    let mut best = (0.0, a);
    for i in a + 1..b {
        let r = lp[i % n].0;
        let d = if len < 1e-12 {
            crate::dist(p, r)
        } else {
            ((r[0] - p[0]) * dy - (r[1] - p[1]) * dx).abs() / len
        };
        if d > best.0 {
            best = (d, i);
        }
    }
    if best.0 > eps {
        keep[best.1 % n] = true;
        rdp(lp, a, best.1, eps, keep);
        rdp(lp, best.1, b, eps, keep);
    }
}

/// The loops of one tile as one fill polygon: each loop closed, joined by
/// hidden bridges walked back at the end. Points and their "hidden edge in"
/// flags.
fn join(loops: &[Loop]) -> Vec<([f64; 2], bool)> {
    let mut pts: Vec<([f64; 2], bool)> = Vec::new();
    for (k, lp) in loops.iter().enumerate() {
        // Start at the loop's last point, so its closing edge is its own.
        let last = *lp.last().expect("non-empty");
        pts.push((last.0, k > 0 || pts.is_empty()));
        pts.extend_from_slice(lp);
    }
    for k in (0..loops.len().saturating_sub(1)).rev() {
        pts.push((loops[k].last().expect("non-empty").0, true));
    }
    pts
}

impl App {
    /// Fill the closed area around screen point `p` (px) with the bucket's
    /// color.
    pub(crate) fn bucket_fill(&mut self, p: [f64; 2]) {
        let [sw, sh] = self.size();
        let ppp = self.ppp();
        // One cell per point, coarser on very big screens.
        let mut scale = 1.0 / ppp;
        let cells = sw * sh * scale * scale;
        if cells > MAX_CELLS {
            scale *= (MAX_CELLS / cells).sqrt();
        }
        let (w, h) = (
            (sw * scale).ceil() as usize + 2,
            (sh * scale).ceil() as usize + 2,
        );
        let mut m = Mask::new(w, h);
        let to = |q: [f64; 2]| [q[0] * scale + 1.0, q[1] * scale + 1.0];
        // What the fill must stay inside: lines, and letters. Highlighter,
        // pictures and other fills don't count.
        let mut zs: Vec<(u32, f64, [f64; 4])> = Vec::new();
        for inst in &self.draw.strokes {
            let s = &self.scene.strokes[inst.stroke as usize];
            if s.deleted || s.brush == Brush::Highlighter || s.color.to_le_bytes()[3] == 0 {
                continue;
            }
            let lettering = matches!(
                self.objs
                    .of_stroke
                    .get(&inst.stroke)
                    .map(|&g| &self.objs.groups[g as usize].data),
                Some(ObjData::Text { .. } | ObjData::Table { .. })
            );
            if s.brush == Brush::Fill && !lettering {
                continue;
            }
            let pts: Vec<[f64; 3]> = self
                .scene
                .stroke_points(inst.stroke)
                .iter()
                .map(|q| {
                    let c = to([
                        inst.ox as f64 + q[0] as f64 * inst.scale as f64,
                        inst.oy as f64 + q[1] as f64 * inst.scale as f64,
                    ]);
                    [c[0], c[1], q[2] as f64]
                })
                .collect();
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for q in &pts {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
            if hi[0] < 0.0 || hi[1] < 0.0 || lo[0] > w as f64 || lo[1] > h as f64 {
                continue;
            }
            zs.push((inst.stroke, s.z, [lo[0], lo[1], hi[0], hi[1]]));
            if s.brush == Brush::Fill {
                m.polygon(&pts.iter().map(|q| [q[0], q[1]]).collect::<Vec<_>>());
                continue;
            }
            let r = (s.width * inst.scale) as f64 * scale * 0.5;
            let pf = |pr: f64| {
                if s.brush == Brush::Pen {
                    0.25 + 0.75 * pr.clamp(0.0, 1.0)
                } else {
                    1.0
                }
            };
            if pts.len() == 1 {
                m.capsule(
                    [pts[0][0], pts[0][1]],
                    [pts[0][0], pts[0][1]],
                    r * pf(pts[0][2]),
                );
            }
            for s2 in pts.windows(2) {
                let rr = r * pf(s2[0][2]).max(pf(s2[1][2]));
                m.capsule([s2[0][0], s2[0][1]], [s2[1][0], s2[1][1]], rr);
            }
        }
        // Close small gaps: grow the lines first, then flood what is left.
        let gap = self.ui.fill_gap as usize;
        let mut wall = m.barrier.clone();
        if gap > 0 {
            grow(&mut wall, w, h, gap, |_| true);
        }
        let free: Vec<bool> = wall.iter().map(|b| !b).collect();
        let seed = to(p);
        let (sx, sy) = (seed[0] as usize, seed[1] as usize);
        let mut start = None;
        'find: for r in 0..=4i64 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (sx as i64 + dx, sy as i64 + dy);
                    if x > 0
                        && y > 0
                        && (x as usize) < w - 1
                        && (y as usize) < h - 1
                        && free[y as usize * w + x as usize]
                    {
                        start = Some(y as usize * w + x as usize);
                        break 'find;
                    }
                }
            }
        }
        let Some(start) = start else {
            return self.say("Tap inside an outline to fill it");
        };
        let Some(mut region) = flood(&free, w, h, start) else {
            return self.say(if gap > 0 {
                "That area isn't closed — close the outline, zoom out, or use bigger gap closing"
            } else {
                "That area isn't closed — close the outline, zoom out, or turn on gap closing"
            });
        };
        // Win back the margin the gap closing took (free cells only, so it
        // can't cross a line), then tuck under the lines.
        let line = &m.barrier;
        if gap > 0 {
            grow(&mut region, w, h, gap, |i| wall[i] && !line[i]);
        }
        grow(&mut region, w, h, UNDER, |i| line[i]);
        // Bounds, and the draw order: just under the lowest line around it.
        let (mut lo, mut hi) = ([usize::MAX; 2], [0usize; 2]);
        for (i, &r) in region.iter().enumerate() {
            if r {
                let (x, y) = (i % w, i / w);
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x + 1), hi[1].max(y + 1)];
            }
        }
        let near = |b: &[f64; 4]| {
            b[2] >= lo[0] as f64 - 2.0
                && b[3] >= lo[1] as f64 - 2.0
                && b[0] <= hi[0] as f64 + 2.0
                && b[1] <= hi[1] as f64 + 2.0
        };
        let z = zs
            .iter()
            .filter(|e| near(&e.2))
            .map(|e| e.1)
            .fold(f64::INFINITY, f64::min);
        let z = if z.is_finite() {
            z - 0.5
        } else {
            self.scene.z_top + 1.0
        };
        // Trace tile by tile into polygons, back in camera units.
        let ink = self.ui.fill;
        let [r, g, b, a] = ink.rgba();
        let style = Style {
            width: 0.0,
            color: u32::from_le_bytes([r, g, b, a]),
            brush: Brush::Fill,
            dash: Dash::Solid,
        };
        let mut ids = Vec::new();
        let mut ty = lo[1] - lo[1] % TILE;
        while ty < hi[1] {
            let mut tx = lo[0] - lo[0] % TILE;
            while tx < hi[0] {
                let loops = trace_tile(
                    &region,
                    w,
                    h,
                    tx,
                    ty,
                    (tx + TILE).min(w),
                    (ty + TILE).min(h),
                );
                if !loops.is_empty() {
                    let mut eps = 0.75;
                    let poly = loop {
                        let simple: Vec<Loop> = loops
                            .iter()
                            .map(|l| simplify(l, eps))
                            .filter(|l| l.len() >= 3)
                            .collect();
                        let poly = join(&simple);
                        if poly.len() <= MAX_PTS || eps > 50.0 {
                            break poly;
                        }
                        eps *= 1.6;
                    };
                    if poly.len() >= 3 {
                        let cam: Vec<[f64; 2]> = poly
                            .iter()
                            .map(|(q, _)| {
                                self.px_to_cam([(q[0] - 1.0) / scale, (q[1] - 1.0) / scale])
                            })
                            .collect();
                        let (anchor, local, _) = Scene::anchor_for_min(&self.cam.cell, &cam, 1e-12);
                        let pts: Vec<[f32; 4]> = local
                            .iter()
                            .zip(&poly)
                            .map(|(l, (_, hidden))| {
                                [l[0], l[1], if *hidden { -1.0 } else { 1.0 }, 0.0]
                            })
                            .collect();
                        ids.push(self.scene.add_stroke_at(
                            &anchor,
                            &pts,
                            style,
                            crate::uid::new(),
                            z,
                        ));
                    }
                }
                tx += TILE;
            }
            ty += TILE;
        }
        if ids.is_empty() {
            return;
        }
        self.record_edit(vec![], ids);
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(w: usize, h: usize) -> Mask {
        let mut m = Mask::new(w, h);
        // A closed square outline, 2 cells thick, from 10 to 40.
        for (a, b) in [
            ([10.0, 10.0], [40.0, 10.0]),
            ([40.0, 10.0], [40.0, 40.0]),
            ([40.0, 40.0], [10.0, 40.0]),
            ([10.0, 40.0], [10.0, 10.0]),
        ] {
            m.capsule(a, b, 1.0);
        }
        m
    }

    #[test]
    fn floods_inside_and_refuses_outside() {
        let m = ring(60, 60);
        let free: Vec<bool> = m.barrier.iter().map(|b| !b).collect();
        let r = flood(&free, 60, 60, 25 * 60 + 25).expect("closed");
        let n = r.iter().filter(|&&v| v).count();
        assert!(n > 700 && n < 900, "{n}");
        assert!(flood(&free, 60, 60, 5 * 60 + 5).is_none());
    }

    #[test]
    fn gap_closing_bridges_a_small_gap() {
        let mut m = Mask::new(60, 60);
        for (a, b) in [
            ([10.0, 10.0], [40.0, 10.0]),
            ([40.0, 10.0], [40.0, 40.0]),
            ([40.0, 40.0], [10.0, 40.0]),
            ([10.0, 40.0], [10.0, 27.0]),
        ] {
            m.capsule(a, b, 1.0);
        }
        m.capsule([10.0, 23.0], [10.0, 10.0], 1.0); // a 4-cell gap
        let free: Vec<bool> = m.barrier.iter().map(|b| !b).collect();
        assert!(flood(&free, 60, 60, 25 * 60 + 25).is_none());
        let mut wall = m.barrier.clone();
        grow(&mut wall, 60, 60, 2, |_| true);
        let free: Vec<bool> = wall.iter().map(|b| !b).collect();
        assert!(flood(&free, 60, 60, 25 * 60 + 25).is_some());
    }

    /// Signed area (positive = clockwise with y down).
    fn area(l: &Loop) -> f64 {
        let n = l.len();
        (0..n)
            .map(|i| {
                let (a, b) = (l[i].0, l[(i + 1) % n].0);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            * 0.5
    }

    #[test]
    fn traces_a_square_with_a_hole_and_cuts_tiles() {
        let w = 20;
        let mut region = vec![false; w * w];
        for y in 2..18 {
            for x in 2..18 {
                region[y * w + x] = !(8..12).contains(&x) || !(8..12).contains(&y);
            }
        }
        let loops = trace_tile(&region, w, w, 0, 0, w, w);
        assert_eq!(loops.len(), 2);
        let mut areas: Vec<f64> = loops.iter().map(|l| area(&simplify(l, 0.5))).collect();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(areas, vec![-16.0, 256.0]);
        // Cut down the middle: each half has cut edges on x = 10.
        let left = trace_tile(&region, w, w, 0, 0, 10, w);
        assert!(left.iter().flatten().any(|(p, cut)| *cut && p[0] == 10.0));
        let total: f64 = left.iter().map(|l| area(&simplify(l, 0.5))).sum::<f64>()
            + trace_tile(&region, w, w, 10, 0, w, w)
                .iter()
                .map(|l| area(&simplify(l, 0.5)))
                .sum::<f64>();
        assert_eq!(total, 240.0);
    }

    #[test]
    fn letters_rasterise_as_filled() {
        let mut m = Mask::new(20, 20);
        m.polygon(&[[2.0, 2.0], [12.0, 2.0], [12.0, 12.0], [2.0, 12.0]]);
        assert_eq!(m.barrier.iter().filter(|&&b| b).count(), 100);
    }
}
