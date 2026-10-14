// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Shape booleans (selection panel, with two or more shapes selected):
//! join them (Union), cut the upper ones out of the lowest (Subtract), keep
//! where they all overlap (Intersect), or where an odd number do
//! (Exclude). The result is one pen path (`ShapeKind::Path`) styled like
//! the lowest shape, in its place, one undo step.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::objects::{self, ObjData, ObjRef};
use crate::shapes::{self, ShapeKind};
use crate::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
    Exclude,
}

pub const BOOL_OPS: [(BoolOp, &str, &str); 4] = [
    (BoolOp::Union, "Union", "Join them into one shape"),
    (
        BoolOp::Subtract,
        "Subtract",
        "Cut the upper shapes out of the lowest",
    ),
    (
        BoolOp::Intersect,
        "Intersect",
        "Keep only where they all overlap",
    ),
    (BoolOp::Exclude, "Exclude", "Keep where they don't overlap"),
];

type Contours = Vec<Vec<[f64; 2]>>;

/// `a` op `b`, both read even-odd (holes are holes).
fn combine(a: &Contours, b: &Contours, op: BoolOp) -> Contours {
    let rule = match op {
        BoolOp::Union => OverlayRule::Union,
        BoolOp::Subtract => OverlayRule::Difference,
        BoolOp::Intersect => OverlayRule::Intersect,
        BoolOp::Exclude => OverlayRule::Xor,
    };
    a.overlay(b, rule, FillRule::EvenOdd)
        .into_iter()
        .flatten()
        .filter(|c| c.len() >= 3)
        .collect()
}

/// Every operand in order, combined (Subtract: the first minus the rest).
pub fn combine_all(parts: &[Contours], op: BoolOp) -> Contours {
    let mut it = parts.iter();
    let Some(first) = it.next() else {
        return vec![];
    };
    it.fold(first.clone(), |acc, p| combine(&acc, p, op))
}

impl App {
    /// A shape's closed outlines in camera units (none for lines, open
    /// paths and everything not a shape).
    fn contours_of(&self, r: &ObjRef) -> Option<(Contours, ObjData)> {
        let ObjRef::Group(g) = *r else { return None };
        let grp = self.objs.groups.get(g as usize)?;
        let data = objects::to_cam(&grp.cell, &grp.data, &self.cam);
        let ObjData::Shape { style, geom, .. } = &data else {
            return None;
        };
        let c: Contours = match style.kind {
            ShapeKind::Line | ShapeKind::Arrow => return None,
            ShapeKind::Path => shapes::subpaths(&geom.pts)
                .into_iter()
                .filter(|p| p.1 && p.0.len() >= 3)
                .map(|p| p.0)
                .collect(),
            _ => vec![shapes::outline(style, geom)],
        };
        (!c.is_empty()).then_some((c, data))
    }

    pub(crate) fn boolean(&mut self, op: BoolOp) {
        self.sel_alive();
        let sel = self.sel_sorted_pub();
        let ops: Vec<(ObjRef, Contours, ObjData)> = sel
            .iter()
            .filter_map(|r| self.contours_of(r).map(|(c, d)| (*r, c, d)))
            .collect();
        if ops.len() < 2 {
            self.say("Select two or more closed shapes");
            return;
        }
        let parts: Vec<Contours> = ops.iter().map(|o| o.1.clone()).collect();
        let result = combine_all(&parts, op);
        let removed: Vec<u32> = ops
            .iter()
            .flat_map(|o| self.objs.strokes(&o.0).to_vec())
            .collect();
        let z = self.z_range(self.objs.strokes(&ops[0].0));
        if result.is_empty() {
            // Nothing left (say, shapes that don't overlap, intersected).
            self.edit.selection.clear();
            self.record_edit(removed, vec![]);
            self.say("Nothing was left");
            return;
        }
        let ObjData::Shape {
            mut style,
            width,
            seed,
            ..
        } = ops[0].2.clone()
        else {
            return;
        };
        style.kind = ShapeKind::Path;
        if style.fill_style == shapes::FillStyle::None {
            style.fill_style = shapes::FillStyle::Solid;
        }
        let pts = shapes::path_of_polygons(&result);
        let (center, half) = shapes::bounds_of(&pts);
        let data = ObjData::Shape {
            style,
            geom: shapes::Geom {
                center,
                half,
                rot: 0.0,
                pts,
            },
            width,
            seed,
        };
        let (g, ids) = self.add_group(&data, Some(z));
        self.edit.selection = vec![ObjRef::Group(g)];
        self.record_edit(removed, ids);
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Contours {
        vec![vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]]
    }

    fn area(c: &Contours) -> f64 {
        // Even-odd: holes come out with the other winding, so |sum| works
        // for one hole inside one outline.
        c.iter()
            .map(|p| {
                let n = p.len();
                (0..n)
                    .map(|i| {
                        let (a, b) = (p[i], p[(i + 1) % n]);
                        a[0] * b[1] - b[0] * a[1]
                    })
                    .sum::<f64>()
                    * 0.5
            })
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn booleans_of_two_squares() {
        let a = square(0.0, 0.0, 10.0);
        let b = square(5.0, 5.0, 10.0);
        let parts = [a.clone(), b.clone()];
        assert!((area(&combine_all(&parts, BoolOp::Union)) - 175.0).abs() < 1e-6);
        assert!((area(&combine_all(&parts, BoolOp::Intersect)) - 25.0).abs() < 1e-6);
        assert!((area(&combine_all(&parts, BoolOp::Subtract)) - 75.0).abs() < 1e-6);
        let x = combine_all(&parts, BoolOp::Exclude);
        assert!(!x.is_empty());
        // A hole: a small square cut from the middle of a big one.
        let hole = combine_all(
            &[square(0.0, 0.0, 10.0), square(3.0, 3.0, 4.0)],
            BoolOp::Subtract,
        );
        assert_eq!(hole.len(), 2, "outline and hole");
    }

    #[test]
    fn subtracting_on_the_page_makes_one_path() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&serde_json::json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 100, "h": 100, "fill": "#2050d0"},
            {"add": "shape", "kind": "ellipse", "x": 30, "y": 30, "w": 40, "h": 40},
        ]));
        let mut seen = std::collections::HashSet::new();
        app.edit.selection = (0..app.scene.strokes.len() as u32)
            .filter(|&s| !app.scene.strokes[s as usize].deleted)
            .map(|s| app.objs.obj_of(s))
            .filter(|r| seen.insert(*r))
            .collect();
        app.boolean(BoolOp::Subtract);
        let r = app.run_commands(&serde_json::json!([{"get": "objects"}]));
        let list = r[0]["objects"].as_array().unwrap().clone();
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0]["shape"], "path");
        assert_eq!(list[0]["fill"], "#2050d0");
        // A ring: an outline with a hole, the ellipse's middle is empty.
        let [g] = app.edit.selection[..] else {
            panic!()
        };
        let ObjData::Shape { geom, .. } = &app.objs.groups[match g {
            ObjRef::Group(i) => i as usize,
            _ => 0,
        }]
        .data
        else {
            panic!()
        };
        assert_eq!(shapes::subpaths(&geom.pts).len(), 2);
        app.undo_redo(false);
        let r = app.run_commands(&serde_json::json!([{"get": "objects"}]));
        assert_eq!(
            r[0]["objects"].as_array().unwrap().len(),
            2,
            "undo brings both back"
        );
    }
}
