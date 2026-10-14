// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Frames (artboards): Shapes › Frame drags out a named white page that
//! sits under what is drawn on its layer. Its name shows over its top left
//! corner; Menu › Frames lists them to fly to; selected, its name can be
//! changed, and Export with "only the selection" exports everything inside
//! it, cut to it.

use crate::objects::{ObjData, ObjRef};
use crate::App;

/// A frame as Menu › Frames lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRow {
    pub group: u32,
    pub name: String,
}

impl App {
    /// The frames on the page (shown ones), bottom to top.
    pub(crate) fn frames(&self) -> Vec<(ObjRef, String)> {
        let mut v: Vec<(f64, ObjRef, String)> = self
            .objs
            .groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| match &g.data {
                ObjData::Frame { name, .. } => {
                    let s = *g.strokes.first()?;
                    self.scene.shown(s).then(|| {
                        (
                            self.scene.strokes[s as usize].z,
                            ObjRef::Group(i as u32),
                            name.clone(),
                        )
                    })
                }
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, r, n)| (r, n)).collect()
    }

    pub(crate) fn frame_rows(&self) -> Vec<FrameRow> {
        self.frames()
            .into_iter()
            .filter_map(|(r, name)| match r {
                ObjRef::Group(group) => Some(FrameRow { group, name }),
                _ => None,
            })
            .collect()
    }

    /// A name for a new frame.
    pub(crate) fn frame_name_new(&self) -> String {
        let names: Vec<String> = self.frames().into_iter().map(|f| f.1).collect();
        (1..)
            .map(|n| format!("Frame {n}"))
            .find(|n| !names.contains(n))
            .expect("a free name")
    }

    /// Fly to frame `g`, filling the screen.
    pub(crate) fn frame_go(&mut self, g: u32) {
        let ids = self.objs.strokes(&ObjRef::Group(g)).to_vec();
        if let Some(cam) = self.frame_strokes(&ids) {
            let px = self.view_px();
            self.fly_to_pub(cam, px);
        }
    }

    /// Rename the selected frame.
    pub(crate) fn frame_rename(&mut self, name: String) {
        let name: String = name.trim().chars().take(200).collect();
        if name.is_empty() {
            return;
        }
        self.replace_selection(|app, r| {
            let ObjRef::Group(g) = r else { return None };
            match &app.objs.groups[g as usize].data {
                ObjData::Frame { name: old, .. } if *old != name => {
                    let n = name.clone();
                    Some(app.group_copy(
                        g,
                        move |d| match d {
                            ObjData::Frame { geom, .. } => ObjData::Frame { geom, name: n },
                            d => d,
                        },
                        None,
                    ))
                }
                _ => None,
            }
        });
    }

    /// Frame names over their top left corners.
    pub(crate) fn frame_overlay(&self, ov: &mut crate::ui::Overlay) {
        let k = self.uipp();
        let [w, h] = self.size();
        for (r, name) in self.frames() {
            let Some((lo, hi)) = self.obj_rect(&r) else {
                continue;
            };
            let (a, b) = (self.cam_to_px(lo), self.cam_to_px(hi));
            // On screen and big enough to read a name over.
            if b[0] < 0.0 || b[1] < 0.0 || a[0] > w || a[1] > h || (b[0] - a[0]) < 40.0 * self.ppp()
            {
                continue;
            }
            ov.notes
                .push((egui::pos2((a[0] / k) as f32, (a[1] / k) as f32), name));
        }
    }

    /// Selected frames only: the box around them (screen px), for Export.
    pub(crate) fn selected_frames_px(&self) -> Option<([f64; 2], [f64; 2])> {
        let sel = &self.edit.selection;
        if sel.is_empty()
            || !sel.iter().all(|r| match r {
                ObjRef::Group(g) => {
                    matches!(
                        self.objs.groups.get(*g as usize).map(|x| &x.data),
                        Some(ObjData::Frame { .. })
                    )
                }
                _ => false,
            })
        {
            return None;
        }
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for r in sel {
            let (a, b) = self.obj_rect(r)?;
            let (a, b) = (self.cam_to_px(a), self.cam_to_px(b));
            lo = [lo[0].min(a[0]), lo[1].min(a[1])];
            hi = [hi[0].max(b[0]), hi[1].max(b[1])];
        }
        Some((lo, hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_named_saved_and_exported_whole() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([800.0, 600.0]);
        app.ui.tool = crate::ui::Tool::Shapes;
        app.ui.shape.kind = crate::shapes::ShapeKind::Frame;
        app.shape_begin([200.0, 150.0]);
        app.shape_move([600.0, 450.0]);
        app.shape_end(false);
        let f = app.frames();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].1, "Frame 1");
        assert_eq!(app.frame_name_new(), "Frame 2");
        // Drawn on after: the frame stays under it.
        app.run_commands(&serde_json::json!([
            {"add": "shape", "kind": "ellipse", "x": -50, "y": -50, "w": 100, "h": 100}
        ]));
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list[0]["kind"], "frame", "frames go under: {list}");
        assert_eq!(list[0]["name"], "Frame 1");
        // Renamed, and kept in a save.
        app.edit.selection = vec![f[0].0];
        app.frame_rename("Cover".into());
        assert_eq!(app.frames()[0].1, "Cover");
        let bytes = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let snap = crate::snapshot::decode(&bytes, 800.0).unwrap();
        assert!(snap
            .objs
            .groups
            .iter()
            .any(|g| matches!(&g.data, ObjData::Frame { name, .. } if name == "Cover")));
        // Export of the selected frame: the frame's box, with the ellipse.
        ogpaper_core::query(
            &app.scene,
            &app.cam,
            800.0,
            600.0,
            crate::VIEW,
            &mut app.draw,
        );
        let page = app
            .export_page(&crate::export::Opts {
                selection: true,
                background: false,
                scale: 1.0,
            })
            .unwrap();
        let svg = page.svg(&Default::default());
        assert!(svg.contains("width=\"400\""), "{}", &svg[..200]);
        assert!(
            svg.contains("#ffffff") && svg.contains("#1c1c24"),
            "the frame and the ellipse: {svg}"
        );
    }
}
