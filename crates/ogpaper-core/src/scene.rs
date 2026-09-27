// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Sparse quadtree of cells holding strokes.
//!
//! Only cells that hold content (and their ancestors) exist. Nodes live in an
//! arena with child links, so per-frame traversal never touches big integers;
//! the address hash map is only used to find the handful of top cells around
//! the camera.

use std::collections::HashMap;

use crate::addr::{CellAddr, Level};

pub const NONE: u32 = u32::MAX;

#[derive(Debug)]
pub struct Node {
    pub addr: CellAddr,
    pub parent: u32,
    pub children: [u32; 4],
    pub strokes: Vec<u32>,
    /// Strokes in this node and all descendants.
    pub subtree: u32,
    /// Quadrant within the parent (cached so walks never touch big integers).
    pub quad: u8,
    /// 16x16 occupancy of content 4 to 6 levels below this node (bit = y*16 + x).
    /// Lets a whole region of too-small-to-read content draw as one tile. Content
    /// 7+ levels down is left out: when the tile is drawn (node >= 64 px) it is
    /// under a pixel, and sub-pixel content draws nothing.
    pub mask: [u32; 8],
    /// Bit d set if a stroke sits exactly d levels below (d = 0..=3).
    pub near: u8,
}

/// Points are in the anchor cell's local space: [0,1] is the cell, and a
/// stroke may overflow its cell by up to one cell side (so up to [-1, 2]).
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub node: u32,
    pub start: u32,
    pub len: u32,
    /// Width in local units.
    pub width: f32,
    /// RGBA8, straight alpha.
    pub color: u32,
}

#[derive(Default)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub index: HashMap<CellAddr, u32>,
    pub roots: Vec<u32>,
    pub roots_level: Level,
    pub strokes: Vec<Stroke>,
    pub points: Vec<[f32; 2]>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn lookup(&self, addr: &CellAddr) -> Option<u32> {
        self.index.get(addr).copied()
    }

    fn alloc(&mut self, addr: CellAddr, parent: u32) -> u32 {
        let id = self.nodes.len() as u32;
        self.index.insert(addr.clone(), id);
        let quad = addr.quadrant();
        self.nodes.push(Node {
            addr,
            parent,
            children: [NONE; 4],
            strokes: Vec::new(),
            subtree: 0,
            quad,
            mask: [0; 8],
            near: 0,
        });
        id
    }

    /// Get or create the node for `addr`, creating ancestors as needed so the
    /// tree stays connected from the root level down.
    pub fn ensure(&mut self, addr: &CellAddr) -> u32 {
        if let Some(id) = self.lookup(addr) {
            return id;
        }
        if self.nodes.is_empty() {
            self.roots_level = addr.level;
            let id = self.alloc(addr.clone(), NONE);
            self.roots.push(id);
            return id;
        }
        if addr.level < self.roots_level {
            self.lift_roots(addr.level);
            if let Some(id) = self.lookup(addr) {
                return id;
            }
        }
        if addr.level == self.roots_level {
            let id = self.alloc(addr.clone(), NONE);
            self.roots.push(id);
            return id;
        }
        let parent = self.ensure(&addr.parent());
        let id = self.alloc(addr.clone(), parent);
        let q = addr.quadrant() as usize;
        self.nodes[parent as usize].children[q] = id;
        id
    }

    /// Grow the tree upward so the root level becomes `level`.
    fn lift_roots(&mut self, level: Level) {
        while self.roots_level > level {
            let old = std::mem::take(&mut self.roots);
            let mut new_roots = Vec::new();
            for r in old {
                let pa = self.nodes[r as usize].addr.parent();
                let pid = match self.lookup(&pa) {
                    Some(p) => p,
                    None => {
                        let p = self.alloc(pa.clone(), NONE);
                        new_roots.push(p);
                        p
                    }
                };
                let q = self.nodes[r as usize].addr.quadrant() as usize;
                self.nodes[pid as usize].children[q] = r;
                self.nodes[r as usize].parent = pid;
                let sub = self.nodes[r as usize].subtree;
                self.nodes[pid as usize].subtree += sub;
            }
            self.roots = new_roots;
            self.roots_level -= 1;
        }
        self.rebuild_masks();
    }

    /// Recompute every occupancy mask (after the tree grows upward).
    pub fn rebuild_masks(&mut self) {
        for n in &mut self.nodes {
            n.mask = [0; 8];
            n.near = 0;
        }
        for i in 0..self.strokes.len() {
            let node = self.strokes[i].node;
            self.mark(node);
        }
    }

    /// Record one stroke in `node` in the masks of its ancestors >= 4 levels up.
    fn mark(&mut self, node: u32) {
        // Origin of `node` as a fraction of the ancestor being visited.
        let mut fo = [0.0f64; 2];
        let mut d = 0u32;
        let mut n = node;
        while n != NONE {
            if d <= 3 {
                self.nodes[n as usize].near |= 1 << d;
            }
            if (4..=6).contains(&d) {
                let bx = ((fo[0] * 16.0) as usize).min(15);
                let by = ((fo[1] * 16.0) as usize).min(15);
                let bit = by * 16 + bx;
                self.nodes[n as usize].mask[bit / 32] |= 1 << (bit % 32);
            }
            let q = self.nodes[n as usize].quad;
            fo = [
                (fo[0] + (q & 1) as f64) * 0.5,
                (fo[1] + (q >> 1) as f64) * 0.5,
            ];
            d += 1;
            n = self.nodes[n as usize].parent;
        }
    }

    /// Add a stroke anchored at `addr` with points in that cell's local space.
    pub fn add_stroke(&mut self, addr: &CellAddr, pts: &[[f32; 2]], width: f32, color: u32) -> u32 {
        let node = self.ensure(addr);
        let id = self.strokes.len() as u32;
        let start = self.points.len() as u32;
        self.points.extend_from_slice(pts);
        self.strokes.push(Stroke {
            node,
            start,
            len: pts.len() as u32,
            width,
            color,
        });
        self.nodes[node as usize].strokes.push(id);
        let mut n = node;
        while n != NONE {
            self.nodes[n as usize].subtree += 1;
            n = self.nodes[n as usize].parent;
        }
        self.mark(node);
        id
    }

    /// Pick an anchor cell for a stroke whose points are given in camera-cell
    /// units relative to `cam_cell`'s origin, and return the points converted to
    /// that cell's local space. The anchor is the cell whose side is the
    /// smallest power of two >= the stroke's extent, containing its min corner.
    pub fn anchor_for(cam_cell: &CellAddr, pts: &[[f64; 2]]) -> (CellAddr, Vec<[f32; 2]>, f64) {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in pts {
            for a in 0..2 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-12);
        // side = 2^n camera cells.
        let n = extent.log2().ceil() as i64;
        let level = cam_cell.level - n;
        // Cell at `level` containing lo: go down/up from the camera cell exactly.
        let ix = lo[0].floor() as i64;
        let iy = lo[1].floor() as i64;
        let at_cam = cam_cell.offset(ix, iy);
        let frac = [lo[0] - ix as f64, lo[1] - iy as f64];
        let anchor = if n >= 0 {
            at_cam.ancestor(level)
        } else {
            let m = (-n) as u32;
            let k = 2f64.powi(m as i32);
            let sx = (frac[0] * k).floor().min(k - 1.0) as i64;
            let sy = (frac[1] * k).floor().min(k - 1.0) as i64;
            CellAddr {
                level,
                x: (&at_cam.x << m) + sx,
                y: (&at_cam.y << m) + sy,
            }
        };
        let o = anchor.origin_in(cam_cell);
        let side = anchor.side_in(cam_cell);
        let local = pts
            .iter()
            .map(|p| [((p[0] - o[0]) / side) as f32, ((p[1] - o[1]) / side) as f32])
            .collect();
        (anchor, local, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_connects_across_levels() {
        let mut s = Scene::new();
        s.add_stroke(
            &CellAddr::new(10, 1000, 7),
            &[[0.1, 0.1], [0.2, 0.2]],
            0.01,
            0,
        );
        s.add_stroke(&CellAddr::new(3, -2, 5), &[[0.1, 0.1], [0.2, 0.2]], 0.01, 0);
        s.add_stroke(&CellAddr::new(20, 5, 5), &[[0.1, 0.1], [0.2, 0.2]], 0.01, 0);
        assert_eq!(s.roots_level, 3);
        let total: u32 = s.roots.iter().map(|&r| s.node(r).subtree).sum();
        assert_eq!(total, 3);
        // Every non-root node's parent links back to it.
        for (i, n) in s.nodes.iter().enumerate() {
            if n.parent != NONE {
                let p = s.node(n.parent);
                assert_eq!(p.children[n.addr.quadrant() as usize], i as u32);
                assert_eq!(p.addr, n.addr.parent());
            }
        }
    }

    #[test]
    fn anchoring_picks_fitting_cell() {
        let cam = CellAddr::new(50, 12345, -99);
        // A stroke 0.2 camera cells wide starting at (0.6, 0.3).
        let pts = [[0.6, 0.3], [0.8, 0.35]];
        let (a, local, side) = Scene::anchor_for(&cam, &pts);
        assert_eq!(side, 0.25);
        assert_eq!(a.level, 52);
        for (p, l) in pts.iter().zip(&local) {
            let o = a.origin_in(&cam);
            assert!((o[0] + l[0] as f64 * side - p[0]).abs() < 1e-6);
            assert!((o[1] + l[1] as f64 * side - p[1]).abs() < 1e-6);
        }
        assert!(local.iter().all(|l| l[0] >= 0.0 && l[0] <= 2.0));
    }
}
