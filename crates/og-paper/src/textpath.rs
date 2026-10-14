// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Text on a path (selection panel, with a text and a line, arrow, pen
//! path, shape or ink stroke selected): the words run along it, and the
//! line itself goes (undo brings it back). Off path puts them straight.

use crate::objects::{self, ObjData, ObjRef};
use crate::shapes::{self, Geom, ShapeKind};
use crate::App;

impl App {
    /// The points of `r` (camera units) for words to run along.
    fn path_points(&self, r: &ObjRef) -> Option<Vec<[f64; 2]>> {
        match *r {
            ObjRef::Ink(id) => {
                let (o, side) = objects::frame(self.scene.stroke_cell(id), &self.cam);
                let pts: Vec<[f64; 2]> = self
                    .scene
                    .stroke_points(id)
                    .iter()
                    .map(|p| [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side])
                    .collect();
                (pts.len() >= 2).then_some(pts)
            }
            ObjRef::Group(g) => {
                let grp = self.objs.groups.get(g as usize)?;
                let d = objects::to_cam(&grp.cell, &grp.data, &self.cam);
                let ObjData::Shape { style, geom, .. } = &d else {
                    return None;
                };
                let pts = match style.kind {
                    ShapeKind::Line | ShapeKind::Arrow => shapes::line_path(style, geom),
                    ShapeKind::Path => {
                        let (mut p, closed) = shapes::subpaths(&geom.pts).into_iter().next()?;
                        if closed {
                            p.push(p[0]);
                        }
                        p
                    }
                    _ => {
                        let mut p = shapes::outline(style, geom);
                        p.push(*p.first()?);
                        p
                    }
                };
                (pts.len() >= 2).then_some(pts)
            }
        }
    }

    /// Run the selected text along the other selected thing.
    pub(crate) fn text_on_path(&mut self) {
        self.sel_alive();
        let sel = self.edit.selection.clone();
        let is_text = |app: &App, r: &ObjRef| match r {
            ObjRef::Group(g) => matches!(app.objs.groups[*g as usize].data, ObjData::Text { .. }),
            _ => false,
        };
        let (Some(t), Some(p)) = (
            sel.iter().find(|r| is_text(self, r)).copied(),
            sel.iter().find(|r| !is_text(self, r)).copied(),
        ) else {
            self.say("Select a text and a line, path or shape to run it along");
            return;
        };
        let Some(pts) = self.path_points(&p) else {
            self.say("That can't carry text");
            return;
        };
        let ObjRef::Group(tg) = t else { return };
        let grp = &self.objs.groups[tg as usize];
        let ObjData::Text {
            text,
            style,
            size,
            seed,
            ..
        } = objects::to_cam(&grp.cell, &grp.data, &self.cam)
        else {
            return;
        };
        let (center, half) = shapes::bounds_of(&pts);
        let data = ObjData::Text {
            text,
            style,
            geom: Geom {
                center,
                half,
                rot: 0.0,
                pts,
            },
            size,
            seed,
        };
        let removed: Vec<u32> = [t, p]
            .iter()
            .flat_map(|r| self.objs.strokes(r).to_vec())
            .collect();
        let z = self.z_range(self.objs.strokes(&t));
        let (g, ids) = self.add_group(&data, Some(z));
        self.edit.selection = vec![ObjRef::Group(g)];
        self.record_edit(removed, ids);
    }

    /// Put the selected texts on paths straight again (at their start).
    pub(crate) fn text_off_path(&mut self) {
        self.replace_selection(|app, r| {
            let ObjRef::Group(g) = r else { return None };
            let grp = &app.objs.groups[g as usize];
            let ObjData::Text { geom, .. } = &grp.data else {
                return None;
            };
            if geom.pts.len() < 2 {
                return None;
            }
            Some(app.group_copy(
                g,
                |d| match d {
                    ObjData::Text {
                        text,
                        style,
                        geom,
                        size,
                        seed,
                    } => {
                        let b = objects::text_box(&text, &style, size);
                        let p0 = geom.pts[0];
                        ObjData::Text {
                            geom: Geom {
                                center: [p0[0] + b[0] * 0.5, p0[1] - b[1] * 0.5],
                                half: [b[0] * 0.5, b[1] * 0.5],
                                rot: 0.0,
                                pts: vec![],
                            },
                            text,
                            style,
                            size,
                            seed,
                        }
                    }
                    d => d,
                },
                None,
            ))
        });
    }

    /// Whether a text on a path is among the selection.
    pub(crate) fn sel_on_path(&self) -> bool {
        self.edit.selection.iter().any(|r| match r {
            ObjRef::Group(g) => matches!(&self.objs.groups[*g as usize].data,
                ObjData::Text { geom, .. } if geom.pts.len() >= 2),
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_runs_along_a_path_and_back() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.run_commands(&serde_json::json!([
            {"add": "text", "text": "along the curve", "x": 0, "y": 0, "size": 20},
            {"add": "shape", "kind": "path", "points": [[0, 100], [150, 0], [300, 100]], "smooth": true},
        ]));
        let mut seen = std::collections::HashSet::new();
        app.edit.selection = (0..app.scene.strokes.len() as u32)
            .map(|s| app.objs.obj_of(s))
            .filter(|r| seen.insert(*r))
            .collect();
        app.text_on_path();
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list.as_array().unwrap().len(), 1, "the path went: {list}");
        assert_eq!(list[0]["kind"], "text");
        // The words follow the curve: their box spans it, and they keep
        // the path when restyled.
        assert!(list[0]["w"].as_f64().unwrap() > 200.0, "{list}");
        assert!(app.sel_on_path());
        let [r] = app.edit.selection[..] else {
            panic!()
        };
        app.edit.selection = vec![r];
        app.text_off_path();
        assert!(!app.sel_on_path());
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert!(list[0]["w"].as_f64().unwrap() < 260.0, "{list}");
    }

    #[test]
    fn letter_and_line_spacing_grow_the_box() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        let r = app.run_commands(&serde_json::json!([
            {"add": "text", "text": "spaced\nout", "x": 0, "y": 0, "size": 20},
            {"add": "text", "text": "spaced\nout", "x": 0, "y": 100, "size": 20, "letter_spacing": 0.5, "line_height": 2},
        ]));
        let (a, b) = (&r[0], &r[1]);
        assert!(
            b["w"].as_f64().unwrap() > a["w"].as_f64().unwrap() * 1.3,
            "{r}"
        );
        assert!(
            b["h"].as_f64().unwrap() > a["h"].as_f64().unwrap() * 1.3,
            "{r}"
        );
        // Kept in a save.
        let g = app.objs.groups.last().unwrap();
        let back = crate::snapshot::get_data(&crate::snapshot::data_bytes(&g.data)).unwrap();
        assert_eq!(back, g.data);
    }
}
