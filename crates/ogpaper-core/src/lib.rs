// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! OG Paper core: exact infinite-zoom coordinates, a precision-safe camera,
//! a sparse cell tree and bounded per-frame visibility.

pub mod addr;
pub mod brush;
pub mod camera;
pub mod gen;
pub mod history;
pub mod hit;
pub mod scene;
pub mod sync;
pub mod visible;

pub use addr::{CellAddr, Level};
pub use brush::{BrushParams, Dab, Pattern, Tip};
pub use camera::Camera;
pub use history::{Change, History};
pub use scene::{Brush, Dash, Point, Scene, Stroke, Style};
pub use visible::{query, DotInst, DrawList, Params, StrokeInst, TileInst};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Fly the camera through the whole chain and back over a 1M-stroke scene;
    /// every frame's draw list must stay bounded and the chain cell must be visible.
    #[test]
    fn million_strokes_visible_set_stays_bounded() {
        let t = Instant::now();
        let demo = gen::build(1_000_000, 40, 7);
        eprintln!(
            "built {} strokes, {} nodes in {:?}",
            demo.scene.strokes.len(),
            demo.scene.nodes.len(),
            t.elapsed()
        );
        let (vw, vh) = (1920.0, 1080.0);
        let mut out = DrawList::default();
        let mut worst = (0u32, 0u32, 0u32);
        let t = Instant::now();
        let mut frames = 0;
        for target in &demo.chain {
            // Centre the camera on the chain cell at a zoom where it fills ~half the screen.
            let cam = Camera::new(target.parent().parent(), [0.5, 0.5], 512.0);
            let mut cam = cam;
            let o = target.origin_in(&cam.cell);
            let s = target.side_in(&cam.cell);
            cam.off = [o[0] + s * 0.5, o[1] + s * 0.5];
            cam.normalize();
            query(&demo.scene, &cam, vw, vh, Params::default(), &mut out);
            frames += 1;
            worst.0 = worst.0.max(out.stats.nodes_visited);
            worst.1 = worst.1.max(out.stats.strokes);
            worst.2 = worst.2.max(out.stats.dots);
            assert!(
                out.stats.strokes > 20,
                "chain content visible at level {}",
                target.level
            );
        }
        // Mass page, several zooms.
        for lvl in [-1i64, 0, 2, 4, 6, 8, 10, 12] {
            let mut cam = Camera::new(CellAddr::new(lvl, 0, 0), [0.0, 0.0], 512.0);
            let page = CellAddr::new(0, 1, 0);
            let o = page.origin_in(&cam.cell);
            let s = page.side_in(&cam.cell);
            cam.off = [o[0] + s * 0.37, o[1] + s * 0.41];
            cam.normalize();
            query(&demo.scene, &cam, vw, vh, Params::default(), &mut out);
            frames += 1;
            eprintln!("mass lvl {lvl}: {:?}", out.stats);
            worst.0 = worst.0.max(out.stats.nodes_visited);
            worst.1 = worst.1.max(out.stats.strokes);
            worst.2 = worst.2.max(out.stats.dots);
        }
        let per = t.elapsed() / frames;
        eprintln!("worst nodes/strokes/dots = {worst:?}; avg query {per:?}");
        // Bounded by screen area, not by the 1M strokes in the scene: dots are
        // cells between min/2 and min px, so at most (vw*vh)/(min/2)^2 of them.
        let max_dots = (vw * vh / 16.0) as u32;
        assert!(worst.1 < 150_000 && worst.2 < max_dots, "{worst:?}");
        // Tiles replace dots on the dense page: traversal stays small.
        assert!(worst.0 < 60_000, "nodes visited {worst:?}");
    }
}
