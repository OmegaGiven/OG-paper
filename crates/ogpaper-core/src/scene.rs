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

/// How a stroke is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Brush {
    /// Round nib; width follows pressure.
    #[default]
    Pen = 0,
    /// Round nib, constant width.
    Marker = 1,
    /// Wide translucent ink that darkens but never hides what is under it.
    Highlighter = 2,
    /// A filled polygon: the points are its outline (closed implicitly).
    Fill = 3,
}

impl Brush {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Brush::Marker,
            2 => Brush::Highlighter,
            3 => Brush::Fill,
            _ => Brush::Pen,
        }
    }
}

/// Line pattern along a stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Dash {
    #[default]
    Solid = 0,
    Dashed = 1,
    Dotted = 2,
}

impl Dash {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Dash::Dashed,
            2 => Dash::Dotted,
            _ => Dash::Solid,
        }
    }
}

/// How a new stroke looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Width in the anchor cell's local units.
    pub width: f32,
    /// RGBA8, straight alpha (alpha is the stroke's opacity).
    pub color: u32,
    pub brush: Brush,
    pub dash: Dash,
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
    pub brush: Brush,
    pub dash: Dash,
    /// Draw order: higher draws on top. New strokes go on top; "send to back"
    /// gives strokes a z below everything else.
    pub z: f64,
    /// Deleted strokes stay in the arrays (undo, file tombstones) but never draw.
    pub deleted: bool,
    /// Permanent id (UUIDv7 in files); 0 for synthetic content.
    pub uid: u128,
}

/// A point: x, y in the anchor cell's local space, pressure in [0, 1], and
/// the distance along the stroke from its first point (local units; filled
/// in by the scene, used for dashes).
pub type Point = [f32; 4];

#[derive(Default)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub index: HashMap<CellAddr, u32>,
    pub roots: Vec<u32>,
    pub roots_level: Level,
    pub strokes: Vec<Stroke>,
    pub points: Vec<Point>,
    /// Highest and lowest z in use (new strokes go above `z_top`).
    pub z_top: f64,
    pub z_bottom: f64,
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

    /// Add a pen stroke at full pressure (synthetic content, tests).
    pub fn add_stroke(&mut self, addr: &CellAddr, pts: &[[f32; 2]], width: f32, color: u32) -> u32 {
        let pts: Vec<Point> = pts.iter().map(|p| [p[0], p[1], 1.0, 0.0]).collect();
        self.add_stroke_styled(addr, &pts, width, color, Brush::Pen, 0)
    }

    /// Add a stroke anchored at `addr` with points in that cell's local space.
    pub fn add_stroke_styled(
        &mut self,
        addr: &CellAddr,
        pts: &[Point],
        width: f32,
        color: u32,
        brush: Brush,
        uid: u128,
    ) -> u32 {
        let style = Style {
            width,
            color,
            brush,
            dash: Dash::Solid,
        };
        self.add_stroke_with(addr, pts, style, uid)
    }

    /// Add a stroke on top of everything else.
    pub fn add_stroke_with(
        &mut self,
        addr: &CellAddr,
        pts: &[Point],
        style: Style,
        uid: u128,
    ) -> u32 {
        let z = self.z_top + 1.0;
        self.add_stroke_at(addr, pts, style, uid, z)
    }

    /// Add a stroke at draw order `z`.
    pub fn add_stroke_at(
        &mut self,
        addr: &CellAddr,
        pts: &[Point],
        style: Style,
        uid: u128,
        z: f64,
    ) -> u32 {
        let node = self.ensure(addr);
        let id = self.strokes.len() as u32;
        let start = self.points.len() as u32;
        let mut along = 0.0f32;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                let q = pts[i - 1];
                along += (p[0] - q[0]).hypot(p[1] - q[1]);
            }
            self.points.push([p[0], p[1], p[2], along]);
        }
        self.note_z(z);
        self.strokes.push(Stroke {
            node,
            start,
            len: pts.len() as u32,
            width: style.width,
            color: style.color,
            brush: style.brush,
            dash: style.dash,
            z,
            deleted: false,
            uid,
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

    fn note_z(&mut self, z: f64) {
        if self.strokes.is_empty() {
            self.z_top = z;
            self.z_bottom = z;
        } else {
            self.z_top = self.z_top.max(z);
            self.z_bottom = self.z_bottom.min(z);
        }
    }

    /// The style a stroke was drawn with.
    pub fn stroke_style(&self, id: u32) -> Style {
        let s = &self.strokes[id as usize];
        Style {
            width: s.width,
            color: s.color,
            brush: s.brush,
            dash: s.dash,
        }
    }

    /// Hide a stroke (undoable). Returns false if it was already deleted.
    pub fn delete(&mut self, id: u32) -> bool {
        self.set_deleted(id, true)
    }

    /// Bring back a deleted stroke. Returns false if it was not deleted.
    pub fn restore(&mut self, id: u32) -> bool {
        self.set_deleted(id, false)
    }

    fn set_deleted(&mut self, id: u32, deleted: bool) -> bool {
        let s = &mut self.strokes[id as usize];
        if s.deleted == deleted {
            return false;
        }
        s.deleted = deleted;
        // Keep subtree counts live so empty regions are skipped. (Occupancy
        // masks may keep a stale bit until the next rebuild; harmless.)
        let mut n = s.node;
        while n != NONE {
            let node = &mut self.nodes[n as usize];
            if deleted {
                node.subtree -= 1;
            } else {
                node.subtree += 1;
            }
            n = node.parent;
        }
        true
    }

    /// Points of one stroke.
    pub fn stroke_points(&self, id: u32) -> &[Point] {
        let s = &self.strokes[id as usize];
        &self.points[s.start as usize..(s.start + s.len) as usize]
    }

    /// Anchor cell of one stroke.
    pub fn stroke_cell(&self, id: u32) -> &CellAddr {
        &self.nodes[self.strokes[id as usize].node as usize].addr
    }

    /// Pick an anchor cell for a stroke whose points are given in camera-cell
    /// units relative to `cam_cell`'s origin, and return the points converted to
    /// that cell's local space. The anchor is the cell whose side is the
    /// smallest power of two >= the stroke's extent, containing its min corner.
    pub fn anchor_for(cam_cell: &CellAddr, pts: &[[f64; 2]]) -> (CellAddr, Vec<[f32; 2]>, f64) {
        Self::anchor_for_min(cam_cell, pts, 1e-12)
    }

    /// Like [`Scene::anchor_for`], treating the stroke as at least `min_extent`
    /// camera cells across (its ink width), so dots and short dashes anchor to a
    /// cell as big as what is actually drawn.
    pub fn anchor_for_min(
        cam_cell: &CellAddr,
        pts: &[[f64; 2]],
        min_extent: f64,
    ) -> (CellAddr, Vec<[f32; 2]>, f64) {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in pts {
            for a in 0..2 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        let extent = (hi[0] - lo[0])
            .max(hi[1] - lo[1])
            .max(min_extent)
            .max(1e-12);
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
