// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Groups (Group / Ungroup in the selection panel, Ctrl+G / Ctrl+Shift+G):
//! several things picked and changed as one. A group (`ObjData::Bundle`)
//! draws nothing of its own: it lists its members' strokes, and each
//! member keeps its own object, so a group's freehand ink keeps its
//! pressure and brush. Editing a group edits each member as usual and
//! makes a new group of the results (every member copied, so undo and
//! sync see one consistent step).

use std::collections::HashMap;

use crate::objects::{self, Group, ObjData, ObjRef};
use crate::shapes::Geom;
use crate::App;

impl App {
    /// `sel` with its groups opened up: every member, flat, and for each
    /// thing in `sel` its members when it is a group.
    pub(crate) fn open_groups(&self, sel: &[ObjRef]) -> (Vec<ObjRef>, Vec<Option<Vec<ObjRef>>>) {
        let mut flat: Vec<ObjRef> = Vec::new();
        let mut each = Vec::with_capacity(sel.len());
        for r in sel {
            match *r {
                ObjRef::Group(g) if self.objs.is_bundle(r) => {
                    let ms: Vec<ObjRef> = self
                        .objs
                        .members(g)
                        .into_iter()
                        .filter(|m| self.objs.alive(&self.scene, m))
                        .collect();
                    for m in &ms {
                        if !flat.contains(m) {
                            flat.push(*m);
                        }
                    }
                    each.push(Some(ms));
                }
                _ => {
                    if !flat.contains(r) {
                        flat.push(*r);
                    }
                    each.push(None);
                }
            }
        }
        (flat, each)
    }

    /// A new group of `members` (camera frame): its box around them.
    pub(crate) fn make_bundle(&mut self, members: &[ObjRef]) -> Option<ObjRef> {
        self.make_bundle_as(members, None)
    }

    /// `make_bundle`, a component's main or copy if `comp`.
    pub(crate) fn make_bundle_as(
        &mut self,
        members: &[ObjRef],
        comp: Option<objects::Comp>,
    ) -> Option<ObjRef> {
        let strokes: Vec<u32> = members
            .iter()
            .flat_map(|m| self.objs.strokes(m).to_vec())
            .collect();
        if strokes.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for m in members {
            if let Some((l, h)) = self.obj_rect(m) {
                lo = [lo[0].min(l[0]), lo[1].min(l[1])];
                hi = [hi[0].max(h[0]), hi[1].max(h[1])];
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        // A symbol's pins (members that are pins): where wires join it.
        let pins: Vec<[f64; 2]> = members
            .iter()
            .filter_map(|m| match *m {
                ObjRef::Group(g) => {
                    let grp = self.objs.groups.get(g as usize)?;
                    match objects::to_cam(&grp.cell, &grp.data, &self.cam) {
                        ObjData::Shape { style, geom, .. }
                            if style.kind == crate::shapes::ShapeKind::Pin =>
                        {
                            Some(geom.center)
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        let data = ObjData::Bundle {
            geom: Geom {
                center: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5],
                half: [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5],
                rot: 0.0,
                pts: pins,
            },
            comp,
        };
        let (cell, data) = objects::home(&data, &self.cam);
        Some(ObjRef::Group(self.objs.add(Group {
            cell,
            data,
            strokes,
        })))
    }

    /// After members were replaced (`map`: old -> new), the things of the
    /// selection again: each group a new group of its members' copies.
    pub(crate) fn regroup(
        &mut self,
        sel: &[ObjRef],
        each: &[Option<Vec<ObjRef>>],
        map: &HashMap<ObjRef, ObjRef>,
        dup: bool,
    ) -> Vec<ObjRef> {
        self.comp_remade.clear();
        self.bundle_remade.clear();
        let mut out = Vec::with_capacity(sel.len());
        for (r, ms) in sel.iter().zip(each) {
            match ms {
                Some(ms) => {
                    let new: Vec<ObjRef> = ms.iter().map(|m| *map.get(m).unwrap_or(m)).collect();
                    match self.make_bundle(&new) {
                        Some(b) => {
                            if let (ObjRef::Group(o), ObjRef::Group(n)) = (*r, b) {
                                self.bundle_remade.push((o, n));
                            }
                            // A component stays one (a duplicate is a copy).
                            self.carry_comp(r, &b, dup);
                            out.push(b)
                        }
                        None => out.extend(new),
                    }
                }
                None => out.push(*map.get(r).unwrap_or(r)),
            }
        }
        out
    }

    /// `old`, a member of a group, was replaced by `new`: the group made
    /// again around it (its other members copied, so it is all new
    /// strokes, as any edit of a group is). The group, and the strokes
    /// taken away and added besides `old`'s and `new`'s.
    pub(crate) fn rebundle(
        &mut self,
        old: ObjRef,
        new: ObjRef,
    ) -> Option<(ObjRef, Vec<u32>, Vec<u32>)> {
        let s = *self.objs.strokes(&old).first()?;
        let b = *self.objs.bundle_of.get(&s)?;
        let comp = self.comp_of(&ObjRef::Group(b));
        let (mut removed, mut added, mut out) = (Vec::new(), Vec::new(), Vec::new());
        for m in self.objs.members(b) {
            if m == old {
                out.push(new);
                continue;
            }
            if !self.objs.alive(&self.scene, &m) {
                continue;
            }
            let c = self.same_copy(m);
            removed.extend_from_slice(self.objs.strokes(&m));
            added.extend_from_slice(self.objs.strokes(&c));
            out.push(c);
        }
        let nb = self.make_bundle_as(&out, comp)?;
        Some((nb, removed, added))
    }

    /// A copy of `r` (new strokes, same everything): how a group's
    /// untouched members come along when it is edited.
    pub(crate) fn same_copy(&mut self, r: ObjRef) -> ObjRef {
        match r {
            ObjRef::Ink(id) => self.ink_copy(id, None, None, None),
            ObjRef::Group(g) => self.group_copy(g, |d| d, None),
        }
    }

    /// Group the selection (groups in it join as their members).
    pub(crate) fn group_selection(&mut self) {
        self.sel_alive();
        let (flat, _) = self.open_groups(&self.edit.selection.clone());
        if flat.len() < 2 {
            self.say("Select two or more things to group");
            return;
        }
        // Copies of every member (one undo step), then the group of them.
        self.edit.selection = flat;
        self.replace_selection(|app, r| Some(app.same_copy(r)));
        let members = std::mem::take(&mut self.edit.selection);
        if let Some(b) = self.make_bundle(&members) {
            self.edit.selection = vec![b];
            self.persist_groups();
        } else {
            self.edit.selection = members;
        }
        self.say("Grouped: picked and changed as one (Ungroup splits it)");
        self.redraw();
    }

    /// Split the groups in the selection back into their members.
    pub(crate) fn ungroup_selection(&mut self) {
        self.sel_alive();
        let sel = self.edit.selection.clone();
        if !sel.iter().any(|r| self.objs.is_bundle(r)) {
            self.say("Nothing grouped is selected");
            return;
        }
        let (flat, _) = self.open_groups(&sel);
        // Copies of the members, without the group (one undo step).
        self.edit.selection = flat;
        let bundle = self.ui.diagram;
        self.replace_selection(|app, r| Some(app.same_copy(r)));
        self.ui.diagram = bundle;
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_pick_move_save_and_ungroup_as_one() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        let r = app.run_commands(&json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 50, "h": 20},
            {"add": "text", "text": "label", "x": 0, "y": 40},
            {"add": "stroke", "points": [[100, 0], [150, 30]], "width": 4},
        ]));
        assert!(r.as_array().unwrap().iter().all(|x| x["ok"] == true));
        let all: Vec<ObjRef> = {
            let mut seen = std::collections::HashSet::new();
            (0..app.scene.strokes.len() as u32)
                .filter(|&s| !app.scene.strokes[s as usize].deleted)
                .map(|s| app.objs.obj_of(s))
                .filter(|r| seen.insert(*r))
                .collect()
        };
        assert_eq!(all.len(), 3);
        app.edit.selection = all;
        app.group_selection();
        let [b] = app.edit.selection[..] else {
            panic!("one group")
        };
        assert!(app.objs.is_bundle(&b));
        // Any member's stroke picks the group.
        for &s in app.objs.strokes(&b) {
            assert_eq!(app.objs.obj_of(s), b);
        }
        // It lists as a group of three, and moves as one.
        let list = app.run_commands(&json!([{"get": "objects"}]))[0]["objects"].clone();
        let g = list
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["kind"] == "group")
            .expect("a group");
        assert_eq!(g["members"], 3);
        let x0 = g["x"].as_f64().unwrap();
        app.apply_op(&crate::objects::Op::Move([10.0 / app.cam.ppc(), 0.0]));
        let [b2] = app.edit.selection[..] else {
            panic!("still one group")
        };
        assert!(app.objs.is_bundle(&b2) && b2 != b);
        let list = app.run_commands(&json!([{"get": "objects"}]))[0]["objects"].clone();
        let g = list
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["kind"] == "group")
            .unwrap();
        assert!((g["x"].as_f64().unwrap() - x0 - 10.0).abs() < 1.0, "{g}");
        assert_eq!(list.as_array().unwrap().len(), 1);
        // Saved and read back, it is still a group.
        let bytes = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let snap = crate::snapshot::decode(&bytes, 800.0).expect("decodes");
        let groups = snap
            .objs
            .groups
            .iter()
            .filter(|g| matches!(g.data, ObjData::Bundle { .. }))
            .count();
        assert!(groups >= 1);
        // A lone group's bytes come back a group too.
        let one = crate::snapshot::data_bytes(
            &app.objs.groups[match b2 {
                ObjRef::Group(g) => g as usize,
                _ => 0,
            }]
            .data,
        );
        assert!(matches!(
            crate::snapshot::get_data(&one).unwrap(),
            ObjData::Bundle { .. }
        ));
        // Ungroup: three things again, where they were.
        app.ungroup_selection();
        assert_eq!(app.edit.selection.len(), 3);
        assert!(app.edit.selection.iter().all(|r| !app.objs.is_bundle(r)));
        // Undo puts the group back.
        app.undo_redo(false);
        let s = app.scene.strokes.iter().position(|s| !s.deleted).unwrap() as u32;
        assert!(app.objs.is_bundle(&app.objs.obj_of(s)));
    }

    #[test]
    fn a_group_lines_up_as_one() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 50, "h": 20},
            {"add": "shape", "kind": "rect", "x": 80, "y": 0, "w": 50, "h": 20},
        ]));
        let refs = |app: &App| -> Vec<ObjRef> {
            let mut seen = std::collections::HashSet::new();
            (0..app.scene.strokes.len() as u32)
                .filter(|&s| !app.scene.strokes[s as usize].deleted)
                .map(|s| app.objs.obj_of(s))
                .filter(|r| seen.insert(*r))
                .collect()
        };
        app.edit.selection = refs(&app);
        app.group_selection();
        app.run_commands(
            &json!([{"add": "shape", "kind": "ellipse", "x": -100, "y": 100, "w": 30, "h": 30}]),
        );
        app.edit.selection = refs(&app);
        assert_eq!(app.edit.selection.len(), 2);
        app.align(crate::arrange::Align::Left);
        let list = app.run_commands(&json!([{"get": "objects"}]))[0]["objects"].clone();
        let xs: Vec<f64> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["x"].as_f64().unwrap())
            .collect();
        assert_eq!(xs.len(), 2, "{list}");
        assert!(
            (xs[0] - xs[1]).abs() < 1.0 && (xs[0] + 100.0).abs() < 2.0,
            "{xs:?}"
        );
        assert!(list
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["kind"] == "group"));
    }
}
