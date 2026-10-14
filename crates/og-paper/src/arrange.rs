// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Arranging the selection: align its things to each other (left, centre,
//! right, top, middle, bottom), spread them evenly, and snap a selection
//! being dragged to the edges and centres of what is around it, with
//! guides showing what it lined up with.

use std::collections::HashMap;

use crate::objects::{self, ObjRef, Op};
use crate::App;

/// How near (px) an edge or centre must come to snap.
const SNAP_PX: f64 = 6.0;

/// Where to line things up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

pub const ALIGNS: [(Align, &str); 6] = [
    (Align::Left, "Align left"),
    (Align::Center, "Align centres"),
    (Align::Right, "Align right"),
    (Align::Top, "Align tops"),
    (Align::Middle, "Align middles"),
    (Align::Bottom, "Align bottoms"),
];

/// A dragged selection's snapping: what it can snap to (screen px), and
/// where it snapped (for the guides).
#[derive(Clone, Debug, Default)]
pub struct Snap {
    /// Edges and centres of what is around, across and down.
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
    /// The selection's box at the start (px): low and high corners.
    pub lo: [f64; 2],
    pub hi: [f64; 2],
}

impl App {
    /// The box around a thing in camera units: low and high corners.
    pub(crate) fn obj_rect(&self, r: &ObjRef) -> Option<([f64; 2], [f64; 2])> {
        let pts: Vec<[f64; 2]> = match *r {
            ObjRef::Group(g) => {
                let grp = self.objs.groups.get(g as usize)?;
                objects::to_cam(&grp.cell, &grp.data, &self.cam).extent()
            }
            ObjRef::Ink(id) => {
                let (o, side) = objects::frame(self.scene.stroke_cell(id), &self.cam);
                let w = self.scene.strokes[id as usize].width as f64 * side * 0.5;
                let mut v: Vec<[f64; 2]> = Vec::new();
                for p in self.scene.stroke_points(id) {
                    let q = [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side];
                    v.push([q[0] - w, q[1] - w]);
                    v.push([q[0] + w, q[1] + w]);
                }
                v
            }
        };
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &pts {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        (lo[0] <= hi[0] && lo.iter().chain(hi.iter()).all(|v| v.is_finite())).then_some((lo, hi))
    }

    /// Move each thing in `moves` by its own amount (camera units), as one
    /// undo step (diagram lines linked to them follow).
    fn move_each(&mut self, moves: HashMap<ObjRef, [f64; 2]>) {
        if moves.values().all(|d| d[0] == 0.0 && d[1] == 0.0) {
            return;
        }
        self.edit.selection = moves.keys().copied().collect();
        // A group's move is each member's.
        let mut moves = moves;
        for (g, d) in moves.clone() {
            if let (ObjRef::Group(i), true) = (g, self.objs.is_bundle(&g)) {
                for m in self.objs.members(i) {
                    moves.insert(m, d);
                }
            }
        }
        self.replace_selection(|app, r| {
            let d = *moves.get(&r)?;
            if d == [0.0, 0.0] {
                return None;
            }
            let op = Op::Move(d);
            Some(match r {
                ObjRef::Ink(id) => app.ink_copy(id, Some(&op), None, None),
                ObjRef::Group(g) => app.group_copy(g, |data| data.edited(&op), None),
            })
        });
    }

    /// Line the selected things up with each other.
    pub(crate) fn align(&mut self, how: Align) {
        self.sel_alive();
        let rects: Vec<(ObjRef, ([f64; 2], [f64; 2]))> = self
            .edit
            .selection
            .iter()
            .filter_map(|r| Some((*r, self.obj_rect(r)?)))
            .collect();
        if rects.len() < 2 {
            self.say("Select two or more things to line up");
            return;
        }
        let lo = rects.iter().fold([f64::MAX; 2], |a, (_, (l, _))| {
            [a[0].min(l[0]), a[1].min(l[1])]
        });
        let hi = rects.iter().fold([f64::MIN; 2], |a, (_, (_, h))| {
            [a[0].max(h[0]), a[1].max(h[1])]
        });
        let mid = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let moves = rects
            .iter()
            .map(|(r, (l, h))| {
                let c = [(l[0] + h[0]) * 0.5, (l[1] + h[1]) * 0.5];
                let d = match how {
                    Align::Left => [lo[0] - l[0], 0.0],
                    Align::Center => [mid[0] - c[0], 0.0],
                    Align::Right => [hi[0] - h[0], 0.0],
                    Align::Top => [0.0, lo[1] - l[1]],
                    Align::Middle => [0.0, mid[1] - c[1]],
                    Align::Bottom => [0.0, hi[1] - h[1]],
                };
                (*r, d)
            })
            .collect();
        self.move_each(moves);
        self.redraw();
    }

    /// Spread the selected things out evenly, across or down: the first and
    /// last stay, the rest go so the gaps between them are equal.
    pub(crate) fn distribute(&mut self, across: bool) {
        self.sel_alive();
        let a = if across { 0 } else { 1 };
        let mut rects: Vec<(ObjRef, ([f64; 2], [f64; 2]))> = self
            .edit
            .selection
            .iter()
            .filter_map(|r| Some((*r, self.obj_rect(r)?)))
            .collect();
        if rects.len() < 3 {
            self.say("Select three or more things to spread out");
            return;
        }
        rects.sort_by(|x, y| (x.1 .0[a] + x.1 .1[a]).total_cmp(&(y.1 .0[a] + y.1 .1[a])));
        let start = rects
            .iter()
            .map(|(_, (l, _))| l[a])
            .fold(f64::MAX, f64::min);
        let end = rects
            .iter()
            .map(|(_, (_, h))| h[a])
            .fold(f64::MIN, f64::max);
        let sizes: f64 = rects.iter().map(|(_, (l, h))| h[a] - l[a]).sum();
        let gap = (end - start - sizes) / (rects.len() - 1) as f64;
        let mut at = start;
        let mut moves = HashMap::new();
        for (r, (l, h)) in &rects {
            let mut d = [0.0, 0.0];
            d[a] = at - l[a];
            moves.insert(*r, d);
            at += h[a] - l[a] + gap;
        }
        self.move_each(moves);
        self.redraw();
    }

    /// What a selection being dragged can snap to: the edges and centres
    /// of the other things on screen (px).
    pub(crate) fn snap_targets(&self) -> Snap {
        let sel = &self.edit.selection;
        let mut seen = std::collections::HashSet::new();
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for inst in &self.draw.strokes {
            let r = self.objs.obj_of(inst.stroke);
            if sel.contains(&r) || !seen.insert(r) || seen.len() > 400 {
                continue;
            }
            if let Some((l, h)) = self.obj_rect(&r) {
                let (l, h) = (self.cam_to_px(l), self.cam_to_px(h));
                xs.extend([l[0], (l[0] + h[0]) * 0.5, h[0]]);
                ys.extend([l[1], (l[1] + h[1]) * 0.5, h[1]]);
            }
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for r in sel {
            if let Some((l, h)) = self.obj_rect(r) {
                let (l, h) = (self.cam_to_px(l), self.cam_to_px(h));
                lo = [lo[0].min(l[0]), lo[1].min(l[1])];
                hi = [hi[0].max(h[0]), hi[1].max(h[1])];
            }
        }
        Snap { xs, ys, lo, hi }
    }

    /// A move `v` (px) snapped: the selection's edges or centre onto the
    /// nearest target within reach, each way. With where it snapped.
    pub(crate) fn snap_move(&self, s: &Snap, v: [f64; 2]) -> ([f64; 2], [Option<f64>; 2]) {
        if self.mods.alt_key() || s.lo[0] > s.hi[0] {
            return (v, [None, None]);
        }
        let reach = SNAP_PX * self.ppp();
        let mut out = v;
        let mut guides = [None, None];
        for a in 0..2 {
            let targets = if a == 0 { &s.xs } else { &s.ys };
            let mine = [
                s.lo[a] + v[a],
                (s.lo[a] + s.hi[a]) * 0.5 + v[a],
                s.hi[a] + v[a],
            ];
            let best = mine
                .iter()
                .flat_map(|&m| targets.iter().map(move |&t| (t - m, t)))
                .filter(|(d, _)| d.abs() <= reach)
                .min_by(|x, y| x.0.abs().total_cmp(&y.0.abs()));
            if let Some((d, t)) = best {
                out[a] += d;
                guides[a] = Some(t);
            }
        }
        (out, guides)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn boxes(app: &mut App) -> Vec<(f64, f64, f64, f64)> {
        let r = app.run_commands(&json!([{"get": "objects", "kinds": ["shape"]}]));
        r[0]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let f = |k: &str| o[k].as_f64().unwrap();
                (f("x"), f("y"), f("w"), f("h"))
            })
            .collect()
    }

    fn three(app: &mut App) {
        let r = app.run_commands(&json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 50, "h": 20},
            {"add": "shape", "kind": "rect", "x": 70, "y": 40, "w": 30, "h": 40},
            {"add": "shape", "kind": "rect", "x": 300, "y": 10, "w": 100, "h": 30},
        ]));
        assert!(r.as_array().unwrap().iter().all(|x| x["ok"] == true));
        app.edit.selection = (0..3)
            .map(|g| ObjRef::Group(app.objs.groups.len() as u32 - 3 + g))
            .collect();
    }

    // Shapes have a few points of outline width around their box.
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 3.5
    }

    #[test]
    fn align_lines_things_up() {
        let mut app = App::new(None);
        three(&mut app);
        app.cam = crate::home_camera();
        app.align(Align::Left);
        let b = boxes(&mut app);
        assert!(b.iter().all(|x| near(x.0, b[0].0)), "{b:?}");
        // The selection is the lined-up copies (and what didn't move).
        assert_eq!(app.edit.selection.len(), 3);
        app.align(Align::Bottom);
        let b = boxes(&mut app);
        assert!(b.iter().all(|x| near(x.1 + x.3, b[0].1 + b[0].3)), "{b:?}");
        // One undo step each.
        app.undo_redo(false);
        app.undo_redo(false);
        let b = boxes(&mut app);
        assert!(near(b[2].0, 300.0) && near(b[1].1, 40.0), "{b:?}");
    }

    #[test]
    fn distribute_makes_gaps_equal() {
        let mut app = App::new(None);
        three(&mut app);
        app.cam = crate::home_camera();
        app.distribute(true);
        let mut b = boxes(&mut app);
        b.sort_by(|x, y| x.0.total_cmp(&y.0));
        let g1 = b[1].0 - (b[0].0 + b[0].2);
        let g2 = b[2].0 - (b[1].0 + b[1].2);
        assert!((g1 - g2).abs() < 0.5, "{g1} {g2}");
        // The ends stay put.
        assert!(near(b[0].0, 0.0) && near(b[2].0 + b[2].2, 400.0));
    }

    #[test]
    fn snapping_pulls_edges_onto_targets() {
        let app = App::new(None);
        let s = Snap {
            xs: vec![100.0],
            ys: vec![50.0],
            lo: [0.0, 0.0],
            hi: [40.0, 20.0],
        };
        // The right edge (40 + 57 = 97) is 3 px from 100: it snaps.
        let (v, g) = app.snap_move(&s, [57.0, 0.0]);
        assert_eq!(v[0], 60.0);
        assert_eq!(g[0], Some(100.0));
        // Too far: free.
        let (v, g) = app.snap_move(&s, [20.0, 0.0]);
        assert_eq!((v[0], g[0]), (20.0, None));
        // The middle (10 + 38 = 48) snaps to 50 down.
        let (v, _) = app.snap_move(&s, [0.0, 38.0]);
        assert_eq!(v[1], 40.0);
    }
}
