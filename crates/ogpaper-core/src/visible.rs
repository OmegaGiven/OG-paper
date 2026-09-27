// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Per-frame visibility: which strokes to draw, and where.
//!
//! Cost is bounded by what is on screen, not by canvas size:
//! 1. Find the few cells around the camera at a level where one cell is at
//!    least twice the viewport (exact big-integer math, done once per frame).
//! 2. Descend through existing children only, culling off-screen cells, all in
//!    small camera-relative f64 numbers.
//! 3. The first cell under `tile_px` draws its content 4-6 levels down as one
//!    occupancy tile; content that would be under a pixel draws nothing.
//! 4. Also draw the strokes of a few ancestor levels (big objects covering the view).

use crate::addr::CellAddr;
use crate::camera::Camera;
use crate::scene::{Scene, NONE};

/// One stroke to draw: screen = (ox, oy) + local * scale, in pixels from the
/// top-left of the viewport.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct StrokeInst {
    pub stroke: u32,
    pub ox: f32,
    pub oy: f32,
    pub scale: f32,
}

/// A placeholder dot for a subtree too small to draw.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct DotInst {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: f32,
}

/// A region whose deep content is too small to read, drawn as one 16x16
/// occupancy tile. (x, y) is the top-left corner in pixels.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct TileInst {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: f32,
    pub mask: [u32; 8],
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub nodes_visited: u32,
    pub strokes: u32,
    pub dots: u32,
    pub tiles: u32,
}

#[derive(Default)]
pub struct DrawList {
    pub strokes: Vec<StrokeInst>,
    pub dots: Vec<DotInst>,
    pub tiles: Vec<TileInst>,
    pub stats: Stats,
}

#[derive(Clone, Copy)]
pub struct Params {
    /// Cells smaller than this on screen collapse to a dot.
    pub min_cell_px: f64,
    /// The first cell below this size draws its content 4+ levels down as one
    /// tile (16 bins of <= this/16 px); only 3 more levels are traversed.
    pub tile_px: f64,
    /// How many levels above the query top to scan for big objects.
    pub ancestor_levels: i64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            min_cell_px: 8.0,
            tile_px: 128.0,
            ancestor_levels: 8,
        }
    }
}

struct Ctx<'a> {
    scene: &'a Scene,
    ppc: f64,
    off: [f64; 2],
    half: [f64; 2],
    /// View rect in camera-cell units.
    view: [f64; 4],
    params: Params,
}

impl Ctx<'_> {
    fn overlaps(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        x1 >= self.view[0] && x0 <= self.view[2] && y1 >= self.view[1] && y0 <= self.view[3]
    }

    fn inst(&self, stroke: u32, origin: [f64; 2], side: f64) -> StrokeInst {
        StrokeInst {
            stroke,
            ox: ((origin[0] - self.off[0]) * self.ppc + self.half[0]) as f32,
            oy: ((origin[1] - self.off[1]) * self.ppc + self.half[1]) as f32,
            scale: (side * self.ppc) as f32,
        }
    }

    /// `tile_depth`: levels below the nearest tile-emitting ancestor, if any.
    fn descend(
        &self,
        id: u32,
        origin: [f64; 2],
        side: f64,
        tile_depth: Option<u8>,
        out: &mut DrawList,
    ) {
        let n = self.scene.node(id);
        out.stats.nodes_visited += 1;
        if n.subtree == 0 {
            return;
        }
        // Strokes may overflow their cell by one side, so cull on the expanded rect.
        if !self.overlaps(
            origin[0] - side,
            origin[1] - side,
            origin[0] + 2.0 * side,
            origin[1] + 2.0 * side,
        ) {
            return;
        }
        let side_px = side * self.ppc;
        let px = |p: [f64; 2]| {
            [
                ((p[0] - self.off[0]) * self.ppc + self.half[0]) as f32,
                ((p[1] - self.off[1]) * self.ppc + self.half[1]) as f32,
            ]
        };
        let inside = self.overlaps(origin[0], origin[1], origin[0] + side, origin[1] + side);
        if side_px < self.params.min_cell_px {
            // Only reached when zoomed far out past the tile level. Draw this
            // cell's own strokes while they are still >= ~1 px (they span 1/4 to
            // 1 of the cell); anything smaller draws nothing.
            if inside && side_px >= 4.0 {
                for &s in &n.strokes {
                    if !self.scene.strokes[s as usize].deleted {
                        out.strokes.push(self.inst(s, origin, side));
                    }
                }
            }
            return;
        }
        for &s in &n.strokes {
            if !self.scene.strokes[s as usize].deleted {
                out.strokes.push(self.inst(s, origin, side));
            }
        }
        let child_depth = match tile_depth {
            Some(d) => d + 1,
            None if side_px < self.params.tile_px => {
                // The mask holds content 4-6 levels down; at >= 64 px that content
                // is >= 1 px. Smaller tile nodes skip it (sub-pixel draws nothing).
                if inside && side_px >= self.params.tile_px * 0.5 && n.mask.iter().any(|&w| w != 0)
                {
                    let p = px(origin);
                    out.tiles.push(TileInst {
                        x: p[0],
                        y: p[1],
                        size: side_px as f32,
                        alpha: 0.7,
                        mask: n.mask,
                    });
                }
                1
            }
            None => 0,
        };
        // Children at tile depth 4+ are already in the tile.
        let next = if tile_depth.is_some() || child_depth == 1 {
            Some(child_depth)
        } else {
            None
        };
        if child_depth >= 4 {
            return;
        }
        let h = side * 0.5;
        for q in 0..4u8 {
            let c = n.children[q as usize];
            // Under a tile, only visit children holding strokes within the
            // remaining (not tiled) levels.
            if c != NONE
                && (next.is_none()
                    || self.scene.node(c).near & ((1u8 << (4 - child_depth)) - 1) != 0)
            {
                let o = [
                    origin[0] + (q & 1) as f64 * h,
                    origin[1] + (q >> 1) as f64 * h,
                ];
                self.descend(c, o, h, next, out);
            }
        }
    }
}

/// Build the draw list for a `vw` x `vh` pixel viewport.
pub fn query(scene: &Scene, cam: &Camera, vw: f64, vh: f64, params: Params, out: &mut DrawList) {
    out.strokes.clear();
    out.dots.clear();
    out.tiles.clear();
    out.stats = Stats::default();
    if scene.nodes.is_empty() {
        return;
    }
    let ppc = cam.ppc();
    let hw = vw * 0.5 / ppc;
    let hh = vh * 0.5 / ppc;
    let ctx = Ctx {
        scene,
        ppc,
        off: cam.off,
        half: [vw * 0.5, vh * 0.5],
        view: [
            cam.off[0] - hw,
            cam.off[1] - hh,
            cam.off[0] + hw,
            cam.off[1] + hh,
        ],
        params,
    };
    // Top level: one cell there is >= 2x the viewport.
    let k = ((vw.max(vh) / ppc).log2().ceil() as i64 + 1).max(0);
    let top = cam.level() - k;

    if top <= scene.roots_level {
        // Zoomed out beyond the tree's top: every root is small; cull each.
        for &r in &scene.roots {
            let a = &scene.node(r).addr;
            ctx.descend(r, a.origin_in(&cam.cell), a.side_in(&cam.cell), None, out);
        }
        out.stats.strokes = out.strokes.len() as u32;
        out.stats.dots = out.dots.len() as u32;
        out.stats.tiles = out.tiles.len() as u32;
        return;
    }

    let anc = cam.cell.ancestor(top);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let a = anc.offset(dx, dy);
            if let Some(id) = scene.lookup(&a) {
                ctx.descend(id, a.origin_in(&cam.cell), a.side_in(&cam.cell), None, out);
            }
        }
    }

    // Ancestor levels: draw only their own strokes (their children are covered above).
    let lowest = (top - params.ancestor_levels).max(scene.roots_level);
    let mut lvl = top - 1;
    while lvl >= lowest {
        let a0: CellAddr = cam.cell.ancestor(lvl);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let a = a0.offset(dx, dy);
                if let Some(id) = scene.lookup(&a) {
                    let n = scene.node(id);
                    if n.strokes.is_empty() {
                        continue;
                    }
                    let o = a.origin_in(&cam.cell);
                    let s = a.side_in(&cam.cell);
                    if ctx.overlaps(o[0] - s, o[1] - s, o[0] + 2.0 * s, o[1] + 2.0 * s) {
                        for &st in &n.strokes {
                            if !scene.strokes[st as usize].deleted {
                                out.strokes.push(ctx.inst(st, o, s));
                            }
                        }
                    }
                }
            }
        }
        lvl -= 1;
    }
    out.stats.strokes = out.strokes.len() as u32;
    out.stats.dots = out.dots.len() as u32;
    out.stats.tiles = out.tiles.len() as u32;
}
