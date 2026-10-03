// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Portals: windows onto another view of the canvas (see
//! `ObjData::Portal`). Each frame, every portal on screen gets its view
//! queried like the screen itself, fitted to the window's box (the saved
//! view's smaller side across the window's smaller side) and clipped to
//! the window by the renderer's stencil. Only the visible part of a window
//! is queried, so a portal you zoom into costs no more than the screen.
//! Portals seen through portals nest, up to `MAX_DEPTH` deep: a portal
//! that shows itself makes a tunnel.
//!
//! Zooming into a portal goes through it: once its window covers the
//! whole screen, the camera moves to its view, framed exactly as the
//! window showed it, so nothing on screen changes. A portal at the end of
//! a street that shows the street itself is an endless zoom.

use ogpaper_core::{query, DrawList};

use crate::objects::{self, Group, ObjData, ObjRef, PortalView};
use crate::render::{PortalLayer, Portals};
use crate::shapes::{self, FillStyle, Geom, ShapeStyle};
use crate::App;

/// Portals inside portals, at most this deep.
const MAX_DEPTH: u32 = 3;
/// Views drawn through portals in one frame, at most.
const MAX_VIEWS: usize = 32;
/// Windows smaller than this (px) show plain paper.
const MIN_PX: f32 = 12.0;

impl App {
    /// The views to draw through the portals on screen this frame.
    pub(crate) fn portal_layers(&self) -> Portals {
        if self.objs.portal_of.is_empty() {
            return Portals::default();
        }
        let [w, h] = self.size();
        self.portal_layers_in(&self.draw, w, h)
    }

    /// Go through a portal whose window covers the screen: the camera moves
    /// to its view, framed as the window shows it (nothing on screen
    /// changes). True when it did.
    pub(crate) fn portal_pass(&mut self) -> bool {
        if self.objs.portal_of.is_empty() || self.fly.is_some() {
            return false;
        }
        let [w, h] = self.size();
        let probe = [
            [0.0, 0.0],
            [w, 0.0],
            [w, h],
            [0.0, h],
            [w * 0.5, 0.0],
            [w, h * 0.5],
            [w * 0.5, h],
            [0.0, h * 0.5],
            [w * 0.5, h * 0.5],
        ];
        for i in &self.draw.strokes {
            let Some(&g) = self.objs.portal_of.get(&i.stroke) else {
                continue;
            };
            let Some(ObjData::Portal { view, .. }) =
                self.objs.groups.get(g as usize).map(|g| &g.data)
            else {
                continue;
            };
            if self.scene.strokes[i.stroke as usize].deleted {
                continue;
            }
            let poly: Vec<[f64; 2]> = self
                .scene
                .stroke_points(i.stroke)
                .iter()
                .map(|p| {
                    [
                        i.ox as f64 + p[0] as f64 * i.scale as f64,
                        i.oy as f64 + p[1] as f64 * i.scale as f64,
                    ]
                })
                .collect();
            if poly.len() < 3 || !probe.iter().all(|&q| inside(&poly, q)) {
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for q in &poly {
                (x0, y0, x1, y1) = (x0.min(q[0]), y0.min(q[1]), x1.max(q[0]), y1.max(q[1]));
            }
            // As `portals_in` frames it, centred on the screen.
            let side = (x1 - x0).min(y1 - y0) as f32 as f64;
            let mut cam = view.cam.clone();
            cam.base_px = self.cam.base_px;
            cam.zoom_at(side / view.view_px.max(1.0), [0.0, 0.0]);
            cam.pan_px(-(w - x0 - x1) * 0.5, -(h - y0 - y1) * 0.5);
            self.cam = cam;
            self.portal_passes += 1;
            self.view_changed();
            return true;
        }
        false
    }

    /// The same for draw list `draw` of a `w` x `h` px screen.
    fn portal_layers_in(&self, draw: &DrawList, w: f64, h: f64) -> Portals {
        let mut out = Portals::default();
        let screen = [0.0, 0.0, w as f32, h as f32];
        self.portals_in(&mut out, 0, draw, [0.0, 0.0], screen, 0);
        out
    }

    /// The portals in layer `li` (its draw list moved by `off`), seen
    /// within `clip` (screen px), at depth `depth`.
    fn portals_in(
        &self,
        out: &mut Portals,
        li: usize,
        draw: &DrawList,
        off: [f32; 2],
        clip: [f32; 4],
        depth: u32,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }
        for (k, i) in draw.strokes.iter().enumerate() {
            if out.layers.len() >= MAX_VIEWS {
                return;
            }
            let Some(&g) = self.objs.portal_of.get(&i.stroke) else {
                continue;
            };
            let Some(ObjData::Portal { view, .. }) =
                self.objs.groups.get(g as usize).map(|g| &g.data)
            else {
                continue;
            };
            if self.scene.strokes[i.stroke as usize].deleted {
                continue;
            }
            // The window's box on screen.
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in self.scene.stroke_points(i.stroke) {
                let (x, y) = (
                    off[0] + i.ox + p[0] * i.scale,
                    off[1] + i.oy + p[1] * i.scale,
                );
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
            let side = (x1 - x0).min(y1 - y0);
            if side.is_nan() || side < MIN_PX {
                continue;
            }
            // Only the part of it that can be seen.
            let v = [
                x0.max(clip[0]),
                y0.max(clip[1]),
                x1.min(clip[2]),
                y1.min(clip[3]),
            ];
            if v[2] - v[0] < 1.0 || v[3] - v[1] < 1.0 {
                continue;
            }
            let mut cam = view.cam.clone();
            cam.base_px = self.cam.base_px;
            cam.zoom_at(side as f64 / view.view_px.max(1.0), [0.0, 0.0]);
            // Centred on the window; move to the centre of its visible part.
            let d = [
                (v[0] + v[2] - x0 - x1) as f64 * 0.5,
                (v[1] + v[3] - y0 - y1) as f64 * 0.5,
            ];
            cam.pan_px(-d[0], -d[1]);
            let mut dl = DrawList::default();
            query(
                &self.scene,
                &cam,
                (v[2] - v[0]) as f64,
                (v[3] - v[1]) as f64,
                crate::VIEW,
                &mut dl,
            );
            let idx = out.layers.len();
            out.child.insert((li, k), idx);
            out.layers.push(PortalLayer {
                draw: DrawList::default(),
                offset: [v[0], v[1]],
            });
            self.portals_in(out, idx + 1, &dl, [v[0], v[1]], v, depth + 1);
            out.layers[idx].draw = dl;
        }
    }
}

/// Whether `q` is inside polygon `poly` (non-zero winding).
fn inside(poly: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut wind = 0;
    let mut prev = poly[poly.len() - 1];
    for &cur in poly {
        if (prev[1] <= q[1]) != (cur[1] <= q[1]) {
            let x = prev[0] + (q[1] - prev[1]) / (cur[1] - prev[1]) * (cur[0] - prev[0]);
            if x > q[0] {
                wind += if cur[1] > prev[1] { 1 } else { -1 };
            }
        }
        prev = cur;
    }
    wind != 0
}

impl App {
    /// Portal tool: new portals show this view (`None`) or bookmark `i`.
    pub(crate) fn portal_pick(&mut self, bookmark: Option<usize>) {
        let view = match bookmark {
            None => PortalView {
                name: "this view".into(),
                cam: self.cam.clone(),
                view_px: self.view_px(),
            },
            Some(i) => {
                let Some(b) = self.bookmarks.get(i) else {
                    return;
                };
                PortalView {
                    name: b.name.clone(),
                    cam: b.cam.clone(),
                    view_px: b.view_px,
                }
            }
        };
        self.ui.portal_shows = view.name.clone();
        self.say(if bookmark.is_none() {
            "Portals will show this view: go where one goes and draw its window".to_string()
        } else {
            format!("Portals will show \"{}\": draw a window", view.name)
        });
        self.portal_view = Some(view);
    }

    /// The selected portal's view.
    fn selected_portal(&self) -> Option<&PortalView> {
        self.edit.selection.iter().find_map(|r| match r {
            ObjRef::Group(g) => match &self.objs.groups.get(*g as usize)?.data {
                ObjData::Portal { view, .. } => Some(view),
                _ => None,
            },
            _ => None,
        })
    }

    /// Fly into the selected portal's view.
    pub(crate) fn portal_go(&mut self) {
        if let Some(v) = self.selected_portal().cloned() {
            self.edit.selection.clear();
            self.fly_to(v.cam, v.view_px);
        }
    }

    /// The selected portals show the Portal tool's view instead.
    pub(crate) fn portal_retarget(&mut self) {
        let Some(view) = self.portal_view.clone() else {
            return;
        };
        self.replace_selection(|app, r| {
            let ObjRef::Group(g) = r else { return None };
            let grp = &app.objs.groups[g as usize];
            if !matches!(grp.data, ObjData::Portal { .. }) {
                return None;
            }
            let changed = grp.data.clone().into_portal(view.clone());
            let cell = grp.cell.clone();
            let z = app.z_range(&grp.strokes.clone());
            let ids = objects::emit(&mut app.scene, &cell, &changed, z);
            Some(ObjRef::Group(app.objs.add(Group {
                cell,
                data: changed,
                strokes: ids,
            })))
        });
    }

    pub(crate) fn portal_begin(&mut self, p: [f64; 2]) {
        if self.portal_view.is_none() {
            self.say("First choose what portals show: This view, or a bookmark, in the tool panel");
            self.gesture = crate::Gesture::None;
            return;
        }
        if self.ui.portal_free {
            self.portal_path = vec![p];
        } else {
            self.edit.shape_drag = Some((p, p));
        }
    }

    pub(crate) fn portal_move(&mut self, p: [f64; 2]) {
        if self.ui.portal_free {
            let far = self
                .portal_path
                .last()
                .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > 2.0 * self.ppp());
            if far {
                self.portal_path.push(p);
            }
        } else if let Some(d) = self.edit.shape_drag.as_mut() {
            d.1 = p;
        }
        self.redraw();
    }

    /// The portal's style: the shape tool's colors, the portal's outline.
    fn portal_style(&self) -> ShapeStyle {
        ShapeStyle {
            kind: self.ui.portal_kind,
            fill: objects::PORTAL_PAPER,
            fill_style: FillStyle::None,
            ..self.ui.shape
        }
    }

    /// The window being drawn, in screen px (None when too small).
    fn portal_geom_px(&self) -> Option<Geom> {
        let min = 8.0 * self.ppp();
        if self.ui.portal_free {
            let pts = &self.portal_path;
            if pts.len() < 3 {
                return None;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for q in pts {
                (x0, y0, x1, y1) = (x0.min(q[0]), y0.min(q[1]), x1.max(q[0]), y1.max(q[1]));
            }
            ((x1 - x0).min(y1 - y0) >= min).then(|| Geom {
                center: [(x0 + x1) * 0.5, (y0 + y1) * 0.5],
                half: [(x1 - x0) * 0.5, (y1 - y0) * 0.5],
                rot: 0.0,
                pts: pts.clone(),
            })
        } else {
            let (a, b) = self.edit.shape_drag?;
            ((b[0] - a[0]).abs().min((b[1] - a[1]).abs()) >= min)
                .then(|| shapes::box_from_drag(a, b, self.mods.shift_key(), self.mods.alt_key()))
        }
    }

    pub(crate) fn portal_end(&mut self, cancel: bool) {
        let g = self.portal_geom_px();
        self.portal_path.clear();
        self.edit.shape_drag = None;
        let (Some(g), Some(view), false) = (g, self.portal_view.clone(), cancel) else {
            self.redraw();
            return;
        };
        let ppc = self.cam.ppc();
        let to_cam = |p: [f64; 2]| self.px_to_cam(p);
        let data = ObjData::Portal {
            style: self.portal_style(),
            geom: Geom {
                center: to_cam(g.center),
                half: [g.half[0] / ppc, g.half[1] / ppc],
                rot: 0.0,
                pts: g.pts.iter().map(|&p| to_cam(p)).collect(),
            },
            width: self.ui.shape_width as f64 * self.ppp() / ppc,
            seed: crate::uid::new() as u32,
            view,
        };
        let (_, ids) = self.add_group(&data, None);
        self.record_edit(vec![], ids);
    }

    /// The window being drawn, for the overlay: polylines in screen px
    /// (points, width, color, filled).
    pub(crate) fn portal_preview(&self) -> Vec<(Vec<[f64; 2]>, f64, u32, bool)> {
        if self.ui.portal_free && self.portal_path.len() > 1 {
            let mut ring = self.portal_path.clone();
            ring.push(ring[0]);
            return vec![(
                ring,
                (self.ui.shape_width as f64).max(1.0) * self.ppp(),
                self.ui.shape.stroke,
                false,
            )];
        }
        let Some(g) = self.portal_geom_px() else {
            return vec![];
        };
        let w = self.ui.shape_width as f64 * self.ppp();
        let data = ObjData::Portal {
            style: self.portal_style(),
            geom: g,
            width: w,
            seed: 7,
            view: self.portal_view.clone().expect("a view while drawing"),
        };
        data.pieces()
            .into_iter()
            .map(|p| {
                let fill = p.brush == ogpaper_core::Brush::Fill;
                // The window shows faintly while drawing.
                let color = if fill { 0x40fa_e8de } else { p.color };
                (p.pts, p.width, color, fill)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::ShapeKind;

    /// A dot at the home view's centre, and a portal onto the home view
    /// drawn off to the side.
    fn app_with_portal() -> (App, u32) {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        let dot = app.scene.add_stroke(
            &ogpaper_core::CellAddr::new(0, 0, 0),
            &[[0.5, 0.5]],
            0.004,
            0xff00_0000,
        );
        let ppc = app.cam.ppc();
        // Screen px -> camera units, for a 1400 x 900 screen.
        let c = |x: f64, y: f64| {
            [
                app.cam.off[0] + (x - 700.0) / ppc,
                app.cam.off[1] + (y - 450.0) / ppc,
            ]
        };
        let data = ObjData::Portal {
            style: ShapeStyle {
                kind: ShapeKind::Ellipse,
                fill: objects::PORTAL_PAPER,
                ..Default::default()
            },
            geom: Geom {
                center: c(1090.0, 650.0),
                half: [210.0 / ppc, 170.0 / ppc],
                rot: 0.0,
                pts: vec![],
            },
            width: 3.0 / ppc,
            seed: 1,
            view: PortalView {
                name: "home".into(),
                cam: crate::home_camera(),
                view_px: 900.0,
            },
        };
        app.add_group(&data, None);
        (app, dot)
    }

    fn layers_at(app: &App) -> Portals {
        let mut draw = DrawList::default();
        query(&app.scene, &app.cam, 1400.0, 900.0, crate::VIEW, &mut draw);
        app.portal_layers_in(&draw, 1400.0, 900.0)
    }

    #[test]
    fn a_portal_shows_its_view_at_every_zoom() {
        let (mut app, dot) = app_with_portal();
        // The dot shows in the window (and the window in itself, nested).
        let p = layers_at(&app);
        assert!(!p.layers.is_empty());
        assert!(p.layers[0].draw.strokes.iter().any(|i| i.stroke == dot));
        // Zoomed far into the window's centre (where the dot shows), it
        // still shows there.
        for zoom in [10.0, 100.0, 1000.0] {
            app.cam = crate::home_camera();
            app.cam.zoom_at(zoom, [1090.0 - 700.0, 650.0 - 450.0]);
            let p = layers_at(&app);
            assert!(
                p.layers
                    .first()
                    .is_some_and(|l| l.draw.strokes.iter().any(|i| i.stroke == dot)),
                "at {zoom}x: {} layers",
                p.layers.len()
            );
        }
    }

    #[test]
    fn a_portal_that_shows_itself_nests_to_the_limit() {
        let (app, _) = app_with_portal();
        let p = layers_at(&app);
        assert_eq!(p.layers.len(), MAX_DEPTH as usize);
    }

    #[test]
    fn exports_clip_the_view_to_the_window() {
        let (app, dot) = app_with_portal();
        let mut draw = DrawList::default();
        query(&app.scene, &app.cam, 1400.0, 900.0, crate::VIEW, &mut draw);
        let p = app.portal_layers_in(&draw, 1400.0, 900.0);
        let page = app.export_test_items(&draw, &p);
        assert!(page.0 >= 1, "a clip per portal");
        assert!(
            page.1 > 1,
            "the dot shows on the canvas and through the portal"
        );
        let _ = dot;
    }

    #[test]
    fn portals_survive_saving_and_old_apps_see_a_shape() {
        let (app, _) = app_with_portal();
        let g = app.objs.groups.last().unwrap();
        // A lone object (files, the library): the shape, then the view.
        let bytes = crate::snapshot::data_bytes(&g.data);
        assert_eq!(crate::snapshot::get_data(&bytes).unwrap(), g.data);
        // What an app before portals reads from the same bytes: a shape.
        let mut shape = Vec::new();
        crate::snapshot::put_data(&mut shape, &g.data);
        assert!(bytes.starts_with(&shape));
        assert!(matches!(
            crate::snapshot::get_data(&shape).unwrap(),
            ObjData::Shape { .. }
        ));
        // A snapshot: the portal comes back, with its view.
        let snap = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let back = crate::snapshot::decode(&snap, app.cam.base_px).unwrap();
        let got = &back.objs.groups.last().unwrap().data;
        assert_eq!(got, &g.data);
        assert_eq!(back.objs.portal_of.len(), 1);
    }

    #[test]
    fn zooming_down_the_endless_street_goes_round() {
        let d = crate::demo::build();
        let mut app = App::new(None);
        let start = d.bookmarks().pop().expect("the street's bookmark").cam;
        app.load_scene(d.scene, start.clone());
        app.objs = d.objs;
        // Zoomed 4.4x into the far end: its window (1/4 of the street)
        // covers the screen, so the view goes through, back to the start
        // and just 1.1x in.
        app.cam.zoom_at(4.4, [0.0, 0.0]);
        let [w, h] = app.size();
        query(&app.scene, &app.cam, w, h, crate::VIEW, &mut app.draw);
        assert!(app.portal_pass());
        let dz = app.cam.log10_zoom() - start.log10_zoom();
        assert!((dz - 1.1f64.log10()).abs() < 1e-3, "{dz}");
        let p = app.cam.to_screen(&start.cell, start.off);
        assert!(p[0].hypot(p[1]) < 1e-3, "centred on the street: {p:?}");
    }
}
