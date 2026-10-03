// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Editing a line or arrow by its points, Miro style. With one line or
//! arrow selected it shows its points instead of a box: drag an end or a
//! joint to move just that point (in diagram mode ends snap onto outlines
//! and stay attached), drag a + between two points to add a joint there,
//! double-tap a joint to remove it. Dragging the line itself moves it whole.

use egui::{pos2, Color32, Pos2};
use web_time::{Duration, Instant};

use crate::diagram::is_connector;
use crate::objects::{to_cam, ObjData, ObjRef};
use crate::App;

/// How near (points) a press must be to a handle.
const GRAB_PT: f64 = 12.0;

pub struct JointDrag {
    pub group: u32,
    /// The point being moved, and the line's points before (camera units).
    pub index: usize,
    pub orig: Vec<[f64; 2]>,
    /// The line's points as dragged so far (camera units).
    pub pts: Vec<[f64; 2]>,
    /// Its strokes, hidden while the overlay draws it.
    pub hidden: Vec<u32>,
}

impl App {
    /// The single selected line or arrow: its group and data (camera units).
    pub(crate) fn sel_connector(&self) -> Option<(u32, ObjData)> {
        let [ObjRef::Group(g)] = self.edit.selection.as_slice() else {
            return None;
        };
        let grp = self.objs.groups.get(*g as usize)?;
        if !is_connector(&grp.data) || !self.objs.alive(&self.scene, &ObjRef::Group(*g)) {
            return None;
        }
        let d = to_cam(&grp.cell, &grp.data, &self.cam);
        (d.geom().pts.len() >= 2).then_some((*g, d))
    }

    /// Handles (px): each point, and the middle of each segment (to add a
    /// joint after point i).
    fn joint_handles(&self, pts: &[[f64; 2]]) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
        let px: Vec<[f64; 2]> = pts.iter().map(|&q| self.cam_to_px(q)).collect();
        let mids = px
            .windows(2)
            .map(|w| [(w[0][0] + w[1][0]) * 0.5, (w[0][1] + w[1][1]) * 0.5])
            .collect();
        (px, mids)
    }

    /// A press with a line or arrow selected: grab a point or a +. True when
    /// it was used.
    pub(crate) fn joint_begin(&mut self, p: [f64; 2]) -> bool {
        let Some((g, d)) = self.sel_connector() else {
            return false;
        };
        let pts = d.geom().pts.clone();
        let (px, mids) = self.joint_handles(&pts);
        let reach = GRAB_PT * self.ppp() * if self.ui.touch_ui { 1.5 } else { 1.0 };
        let near = |q: &[f64; 2]| crate::dist(*q, p) < reach;
        let (index, new_pts) = if let Some(i) = px.iter().position(near) {
            // Double-tap a joint (not an end): remove it.
            let double = self.edit.last_tap.is_some_and(|(t, q)| {
                t.elapsed() < Duration::from_millis(400) && crate::dist(q, p) < reach
            });
            self.edit.last_tap = Some((Instant::now(), p));
            if double && i > 0 && i + 1 < pts.len() {
                let mut v = pts.clone();
                v.remove(i);
                self.joint_commit(g, v);
                return true;
            }
            (i, pts.clone())
        } else if let Some(k) = mids.iter().position(near) {
            let mut v = pts.clone();
            v.insert(k + 1, self.px_to_cam(mids[k]));
            (k + 1, v)
        } else {
            return false;
        };
        let hidden = self.objs.groups[g as usize]
            .strokes
            .iter()
            .copied()
            .filter(|&s| !self.scene.strokes[s as usize].deleted)
            .collect::<Vec<_>>();
        for &s in &hidden {
            self.scene.strokes[s as usize].deleted = true;
        }
        self.edit.joint = Some(JointDrag {
            group: g,
            index,
            orig: pts,
            pts: new_pts,
            hidden,
        });
        self.redraw();
        true
    }

    pub(crate) fn joint_move(&mut self, p: [f64; 2]) {
        let Some(j) = self.edit.joint.as_ref() else {
            return;
        };
        let (i, n) = (j.index, j.pts.len());
        // Ends snap onto outlines (diagram mode), aimed at the next point.
        let at = if i == 0 || i + 1 == n {
            let next = if i == 0 { j.pts[1] } else { j.pts[n - 2] };
            self.snap_end(p, self.cam_to_px(next))
        } else {
            p
        };
        let q = self.px_to_cam(at);
        if let Some(j) = self.edit.joint.as_mut() {
            j.pts[i] = q;
        }
        self.redraw();
    }

    pub(crate) fn joint_end(&mut self, cancel: bool) {
        let Some(j) = self.edit.joint.take() else {
            return;
        };
        for &s in &j.hidden {
            self.scene.strokes[s as usize].deleted = false;
        }
        if !cancel && j.pts != j.orig {
            self.joint_commit(j.group, j.pts);
        }
        self.redraw();
    }

    /// The line with new points, as one undo step.
    fn joint_commit(&mut self, g: u32, pts: Vec<[f64; 2]>) {
        let data = {
            let grp = &self.objs.groups[g as usize];
            let mut d = to_cam(&grp.cell, &grp.data, &self.cam);
            if let ObjData::Shape { geom, .. } = &mut d {
                let (lo, hi) = pts
                    .iter()
                    .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                        (
                            [lo[0].min(p[0]), lo[1].min(p[1])],
                            [hi[0].max(p[0]), hi[1].max(p[1])],
                        )
                    });
                geom.center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
                geom.half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5];
                geom.pts = pts;
            }
            d
        };
        self.edit.selection = vec![ObjRef::Group(g)];
        self.replace_selection(|app, r| {
            let ObjRef::Group(gi) = r else { return None };
            let z = app.z_range(&app.objs.groups[gi as usize].strokes.clone());
            let (ng, _) = app.add_group(&data, Some(z));
            Some(ObjRef::Group(ng))
        });
    }

    /// What the overlay draws for a selected line or arrow: its points and
    /// the + between them, and the line as it is being dragged.
    pub(crate) fn joint_overlay(&self, ov: &mut crate::ui::Overlay) {
        let ppp = self.uipp();
        let pt = |q: [f64; 2]| pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32);
        let pts = match (&self.edit.joint, self.sel_connector()) {
            (Some(j), _) => {
                // The line as dragged.
                let grp = &self.objs.groups[j.group as usize];
                let mut d = to_cam(&grp.cell, &grp.data, &self.cam);
                if let ObjData::Shape { geom, .. } = &mut d {
                    geom.pts = j.pts.clone();
                }
                let px = d.map(|q| self.cam_to_px(q), self.cam.ppc());
                for pc in px.pieces() {
                    let [r, g, b, a] = pc.color.to_le_bytes();
                    let col = Color32::from_rgba_unmultiplied(r, g, b, a);
                    let fill = pc.brush == ogpaper_core::Brush::Fill;
                    ov.lines.push((
                        pc.pts.iter().map(|&q| pt(q)).collect(),
                        (pc.width / ppp) as f32,
                        col,
                        fill,
                    ));
                }
                j.pts.clone()
            }
            (None, Some((_, d))) => d.geom().pts.clone(),
            _ => return,
        };
        let (px, mids) = self.joint_handles(&pts);
        ov.joints = Some((
            px.iter().map(|&q| pt(q)).collect::<Vec<Pos2>>(),
            mids.iter().map(|&q| pt(q)).collect(),
        ));
    }
}
