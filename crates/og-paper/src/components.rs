// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Components (selection panel › Make component, with a group or several
//! things selected): the selection becomes a component's main. Duplicating
//! it (or a copy of it) makes copies; change the main and every copy is
//! made again from it, in the same undo step, each keeping its own place
//! and size. Detach makes a copy an ordinary group again.

use crate::objects::{Comp, ObjData, ObjRef, Op};
use crate::App;

/// `d` (the main's member, remade) with what the copy's member changed
/// from the main's as it was: its text, and its colors.
fn keep_own(d: ObjData, own: Option<(Option<ObjData>, Option<ObjData>)>) -> ObjData {
    let Some((Some(copy), Some(was))) = own else {
        return d;
    };
    match (d, copy, was) {
        (
            ObjData::Text {
                mut text,
                mut style,
                geom,
                size,
                seed,
            },
            ObjData::Text {
                text: ct,
                style: cs,
                ..
            },
            ObjData::Text {
                text: wt,
                style: ws,
                ..
            },
        ) => {
            if ct != wt {
                text = ct;
            }
            if cs.color != ws.color {
                style.color = cs.color;
            }
            // The box fits the words, about the same middle.
            let b = crate::objects::text_box(&text, &style, size);
            let geom = if geom.pts.len() >= 2 {
                geom
            } else {
                crate::shapes::Geom {
                    half: [b[0] * 0.5, b[1] * 0.5],
                    ..geom
                }
            };
            ObjData::Text {
                text,
                style,
                geom,
                size,
                seed,
            }
        }
        (
            ObjData::Shape {
                mut style,
                geom,
                width,
                seed,
            },
            ObjData::Shape { style: cs, .. },
            ObjData::Shape { style: ws, .. },
        ) => {
            if cs.stroke != ws.stroke {
                style.stroke = cs.stroke;
            }
            if cs.fill != ws.fill || cs.fill_style != ws.fill_style {
                style.fill = cs.fill;
                style.fill_style = cs.fill_style;
                style.fill2 = cs.fill2;
            }
            ObjData::Shape {
                style,
                geom,
                width,
                seed,
            }
        }
        (d, _, _) => d,
    }
}

impl App {
    /// An object's data (frame units; for comparing styles), if it has some.
    fn data_of(&self, r: &ObjRef) -> Option<ObjData> {
        match r {
            ObjRef::Group(g) => self.objs.groups.get(*g as usize).map(|x| x.data.clone()),
            _ => None,
        }
    }

    /// The component `r` is the main or a copy of, if any.
    pub(crate) fn comp_of(&self, r: &ObjRef) -> Option<Comp> {
        match r {
            ObjRef::Group(g) => match &self.objs.groups.get(*g as usize)?.data {
                ObjData::Bundle { comp, .. } => comp.clone(),
                _ => None,
            },
            _ => None,
        }
    }

    fn set_comp(&mut self, r: &ObjRef, c: Option<Comp>) {
        if let ObjRef::Group(g) = r {
            if let Some(ObjData::Bundle { comp, .. }) =
                self.objs.groups.get_mut(*g as usize).map(|x| &mut x.data)
            {
                *comp = c;
            }
        }
    }

    /// What a remade group `old` → `new` is: the same component (a copy's
    /// size kept up to date), a copy when duplicated.
    pub(crate) fn carry_comp(&mut self, old: &ObjRef, new: &ObjRef, dup: bool) {
        let Some(mut c) = self.comp_of(old) else {
            return;
        };
        if c.main && !dup {
            self.comp_remade.push((*old, *new));
        }
        if dup {
            if c.main {
                c.main = false;
                c.k = [1.0, 1.0];
            }
        } else if !c.main {
            if let (Some((a0, a1)), Some((b0, b1))) = (self.obj_rect(old), self.obj_rect(new)) {
                let ratio = |n: f64, o: f64| if o.abs() > 1e-300 { n / o } else { 1.0 };
                c.k = [
                    c.k[0] * ratio(b1[0] - b0[0], a1[0] - a0[0]),
                    c.k[1] * ratio(b1[1] - b0[1], a1[1] - a0[1]),
                ];
            }
        }
        self.set_comp(new, Some(c));
    }

    /// Mains just changed (`mains`): their copies made again. The strokes
    /// taken away, those added, and (old, new) pairs for sync.
    pub(crate) fn remake_copies(
        &mut self,
        mains: &[ObjRef],
    ) -> (Vec<u32>, Vec<u32>, Vec<(Vec<u32>, Vec<u32>)>) {
        let (mut removed, mut added, mut pairs) = (Vec::new(), Vec::new(), Vec::new());
        let before: std::collections::HashMap<ObjRef, ObjRef> =
            std::mem::take(&mut self.comp_remade)
                .into_iter()
                .map(|(o, n)| (n, o))
                .collect();
        for m in mains {
            let Some(mc) = self.comp_of(m).filter(|c| c.main) else {
                continue;
            };
            let ObjRef::Group(mg) = *m else { continue };
            let Some((ml, mh)) = self.obj_rect(m) else {
                continue;
            };
            let mcen = [(ml[0] + mh[0]) * 0.5, (ml[1] + mh[1]) * 0.5];
            let copies: Vec<ObjRef> = (0..self.objs.groups.len() as u32)
                .map(ObjRef::Group)
                .filter(|r| {
                    self.objs.alive(&self.scene, r)
                        && self.comp_of(r).is_some_and(|c| c.id == mc.id && !c.main)
                })
                .collect();
            let members = self.objs.members(mg);
            // The main as it was: a copy's member that differed from it
            // was changed on purpose, and keeps its text and colors.
            let old_members = match before.get(m) {
                Some(ObjRef::Group(og)) => self.objs.members(*og),
                _ => vec![],
            };
            let mz = self.z_range(self.objs.strokes(m));
            for cp in copies {
                let Some(cc) = self.comp_of(&cp) else {
                    continue;
                };
                let Some((cl, ch)) = self.obj_rect(&cp) else {
                    continue;
                };
                let ccen = [(cl[0] + ch[0]) * 0.5, (cl[1] + ch[1]) * 0.5];
                let old: Vec<u32> = self.objs.strokes(&cp).to_vec();
                let dz = self.z_range(&old).0 - mz.0;
                let op = Op::Fit {
                    from: mcen,
                    to: ccen,
                    k: cc.k,
                };
                let mut made = Vec::new();
                let copy_members = match cp {
                    ObjRef::Group(g) => self.objs.members(g),
                    _ => vec![],
                };
                let same_shape =
                    copy_members.len() == members.len() && old_members.len() == members.len();
                for (i, mem) in members.iter().enumerate() {
                    let z = self.z_range(self.objs.strokes(mem));
                    let own = same_shape.then(|| {
                        (
                            self.data_of(&copy_members[i]),
                            self.data_of(&old_members[i]),
                        )
                    });
                    made.push(match *mem {
                        ObjRef::Ink(id) => {
                            // Ink: its own color if it had one.
                            let st = own.as_ref().and_then(|_| {
                                let (ObjRef::Ink(c), ObjRef::Ink(o)) =
                                    (copy_members[i], old_members[i])
                                else {
                                    return None;
                                };
                                let (cs, os) =
                                    (self.scene.stroke_style(c), self.scene.stroke_style(o));
                                (cs.color != os.color).then(|| {
                                    let mut s = self.scene.stroke_style(id);
                                    s.color = cs.color;
                                    s
                                })
                            });
                            self.ink_copy(id, Some(&op), st, Some(z.0 + dz))
                        }
                        ObjRef::Group(g) => self.group_copy(
                            g,
                            |d| keep_own(d.edited(&op), own),
                            Some((z.0 + dz, z.1 + dz)),
                        ),
                    });
                }
                if let Some(b) = self.make_bundle_as(&made, Some(cc)) {
                    let new: Vec<u32> = self.objs.strokes(&b).to_vec();
                    added.extend_from_slice(&new);
                    removed.extend_from_slice(&old);
                    pairs.push((old, new));
                }
            }
        }
        (removed, added, pairs)
    }

    /// Make the selection a component's main (grouping it if it isn't one
    /// group already).
    pub(crate) fn make_component(&mut self) {
        self.sel_alive();
        if !(self.edit.selection.len() == 1 && self.objs.is_bundle(&self.edit.selection[0])) {
            self.group_selection();
        }
        let [b] = self.edit.selection[..] else { return };
        if !self.objs.is_bundle(&b) {
            return;
        }
        let n = (0..self.objs.groups.len() as u32)
            .filter(|&g| self.comp_of(&ObjRef::Group(g)).is_some_and(|c| c.main))
            .count();
        self.set_comp(
            &b,
            Some(Comp {
                id: crate::uid::new() as u64,
                main: true,
                name: format!("Component {}", n + 1),
                k: [1.0, 1.0],
            }),
        );
        self.persist_groups_pub();
        self.say("A component: duplicate it for copies that follow it");
        self.redraw();
    }

    /// Make the selected copies ordinary groups.
    pub(crate) fn detach_component(&mut self) {
        let sel = self.edit.selection.clone();
        self.replace_selection(|app, r| {
            let c = app.comp_of(&r)?;
            if c.main {
                return None;
            }
            let ObjRef::Group(g) = r else { return None };
            let members = app.objs.members(g);
            let copies: Vec<ObjRef> = members.iter().map(|m| app.same_copy(*m)).collect();
            app.make_bundle(&copies)
        });
        let _ = sel;
    }

    /// Select the main of the selected copy.
    pub(crate) fn select_main(&mut self) {
        let Some(c) = self.edit.selection.first().and_then(|r| self.comp_of(r)) else {
            return;
        };
        let main = (0..self.objs.groups.len() as u32)
            .map(ObjRef::Group)
            .find(|r| {
                self.objs.alive(&self.scene, r)
                    && self.comp_of(r).is_some_and(|x| x.id == c.id && x.main)
            });
        match main {
            Some(m) => {
                self.edit.selection = vec![m];
                if let Some(ids) = Some(self.objs.strokes(&m).to_vec()) {
                    if let Some(cam) = self.frame_strokes(&ids) {
                        let px = self.view_px();
                        self.fly_to_pub(cam, px * 0.5);
                    }
                }
            }
            None => self.say("Its main is gone (Detach makes it an ordinary group)"),
        }
        self.redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn copies_follow_their_main() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 60, "h": 40, "fill": "#2050d0"},
            {"add": "text", "text": "Btn", "x": 10, "y": 10},
        ]));
        let mut seen = std::collections::HashSet::new();
        app.edit.selection = (0..app.scene.strokes.len() as u32)
            .map(|s| app.objs.obj_of(s))
            .filter(|r| seen.insert(*r))
            .collect();
        app.make_component();
        let [main] = app.edit.selection[..] else {
            panic!()
        };
        assert!(app.comp_of(&main).unwrap().main);
        // A copy, moved away and made twice as wide.
        app.duplicate_selection_pub([300.0 / app.cam.ppc(), 0.0]);
        let [copy] = app.edit.selection[..] else {
            panic!()
        };
        assert!(!app.comp_of(&copy).unwrap().main);
        let (a, b) = app.obj_rect(&copy).unwrap();
        app.apply_op(&Op::Scale {
            pivot: a,
            sx: 2.0,
            sy: 1.0,
            rot: 0.0,
        });
        let [copy] = app.edit.selection[..] else {
            panic!()
        };
        let k = app.comp_of(&copy).unwrap().k;
        // (Its text grows evenly, so the box isn't exactly twice as wide.)
        assert!(k[0] > 1.5 && k[0] > k[1], "{k:?}");
        let w_copy = {
            let (a, b) = app.obj_rect(&copy).unwrap();
            b[0] - a[0]
        };
        let _ = b;
        // Restyle the main: the copy follows, keeping its place and size.
        let main = (0..app.objs.groups.len() as u32)
            .map(ObjRef::Group)
            .find(|r| app.objs.alive(&app.scene, r) && app.comp_of(r).is_some_and(|c| c.main))
            .unwrap();
        app.edit.selection = vec![main];
        app.apply_op(&Op::Move([0.0, 50.0 / app.cam.ppc()]));
        let r = app.run_commands(&json!([{"get": "objects"}]));
        let list = r[0]["objects"].as_array().unwrap().clone();
        assert_eq!(list.len(), 2, "{list:?}");
        let copy = (0..app.objs.groups.len() as u32)
            .map(ObjRef::Group)
            .find(|r| app.objs.alive(&app.scene, r) && app.comp_of(r).is_some_and(|c| !c.main))
            .expect("the copy, remade");
        let (a, b) = app.obj_rect(&copy).unwrap();
        assert!(
            ((b[0] - a[0]) - w_copy).abs() < w_copy * 0.15,
            "kept its size"
        );
        // Saved and read back, and alone (the library, the clipboard).
        let bytes = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let snap = crate::snapshot::decode(&bytes, 800.0).unwrap();
        let comps = snap
            .objs
            .groups
            .iter()
            .filter(|g| matches!(&g.data, ObjData::Bundle { comp: Some(_), .. }))
            .count();
        assert!(comps >= 2);
        let ObjRef::Group(cg) = copy else { panic!() };
        let d = &app.objs.groups[cg as usize].data;
        assert_eq!(
            &crate::snapshot::get_data(&crate::snapshot::data_bytes(d)).unwrap(),
            d
        );
        // One undo puts both back.
        app.undo_redo(false);
        // Detach: an ordinary group.
        app.edit.selection = vec![copy];
        app.detach_component();
    }

    #[test]
    fn a_copy_keeps_its_own_words_and_colors() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&json!([
            {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 80, "h": 40, "fill": "#2050d0"},
            {"add": "text", "text": "OK", "x": 10, "y": 10},
        ]));
        let mut seen = std::collections::HashSet::new();
        app.edit.selection = (0..app.scene.strokes.len() as u32)
            .map(|s| app.objs.obj_of(s))
            .filter(|r| seen.insert(*r))
            .collect();
        app.make_component();
        app.duplicate_selection_pub([200.0 / app.cam.ppc(), 0.0]);
        let copy = app.edit.selection[0];
        // The copy says something else.
        let id = app.run_commands(&json!([{"get": "objects"}]))[0]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["main"] == false)
            .unwrap()["id"]
            .clone();
        let _ = copy;
        let ObjRef::Group(cg) = app
            .find_all_pub(&[id.as_str().unwrap().to_string()])
            .unwrap()[0]
        else {
            panic!()
        };
        let text_member = app
            .objs
            .members(cg)
            .into_iter()
            .find(|m| matches!(app.data_of(m), Some(ObjData::Text { .. })))
            .unwrap();
        // (As typing does: the text remade, its group around it.)
        let ObjRef::Group(tg) = text_member else {
            panic!()
        };
        let new = app.group_copy(
            tg,
            |d| match d {
                ObjData::Text {
                    style,
                    geom,
                    size,
                    seed,
                    ..
                } => ObjData::Text {
                    text: "Cancel".into(),
                    style,
                    geom,
                    size,
                    seed,
                },
                d => d,
            },
            None,
        );
        let (b, mut removed, mut added) = app.rebundle(text_member, new).expect("in a group");
        removed.extend_from_slice(app.objs.strokes(&text_member));
        added.extend_from_slice(&app.objs.strokes(&new).to_vec());
        app.record_edit(removed, added);
        assert!(app.comp_of(&b).is_some_and(|c| !c.main), "still a copy");
        // The main turns red: the copy follows, still saying Cancel.
        let main = (0..app.objs.groups.len() as u32)
            .map(ObjRef::Group)
            .find(|r| app.objs.alive(&app.scene, r) && app.comp_of(r).is_some_and(|c| c.main))
            .unwrap();
        app.edit.selection = vec![main];
        app.apply_op(&Op::Move([0.0, 10.0 / app.cam.ppc()]));
        let texts: Vec<String> = app
            .objs
            .groups
            .iter()
            .filter(|g| {
                g.strokes
                    .first()
                    .is_some_and(|&s| !app.scene.strokes[s as usize].deleted)
            })
            .filter_map(|g| match &g.data {
                ObjData::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"OK".to_string()) && texts.contains(&"Cancel".to_string()),
            "{texts:?}"
        );
    }
}
