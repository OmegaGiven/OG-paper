// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The library (sticker book): drawings saved from a selection, to place
//! copies of on any canvas, at any zoom.
//!
//! A sticker is self-contained: its ink and objects scaled into a unit box
//! (longest side 1, centred on 0), its pictures' files, and the size it had
//! on screen when saved, so a copy lands looking the same. Stickers are
//! `.ogps` files in `~/OG Paper/library` on desktop; the web page keeps
//! them in the browser's storage.
//!
//! `.ogps` (all little-endian): magic `OGPS`, version u8 (2; 1 is still
//! read), size on screen f64 (points), item count u32; each item: kind u8,
//! then for ink (0): width f32, color u32, brush u8, dash u8, (v2) brush
//! engine parameters (u16 byte count + bytes), point count u32, points (f32 x,
//! y, pressure); for an object (1): data byte count u32, then the data as in
//! `.ogp` groups. Then picture count u32; each: id u64, byte count u32, file.

use ogpaper_core::{Brush, Dash, Point, Scene, Style};

use crate::images::Asset;
use crate::objects::{to_cam, ObjData, ObjRef};
use crate::App;

#[derive(Clone, Debug)]
pub enum Item {
    /// Ink: style (width in unit-box units) and points (x, y, pressure).
    Ink {
        style: Style,
        pts: Vec<[f32; 3]>,
    },
    Obj(ObjData),
}

#[derive(Clone)]
pub struct Sticker {
    /// Longest side on screen when saved (points).
    pub size_pt: f64,
    pub items: Vec<Item>,
    pub images: Vec<(u64, Asset)>,
}

impl Sticker {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = b"OGPS".to_vec();
        b.push(2);
        b.extend_from_slice(&self.size_pt.to_le_bytes());
        b.extend_from_slice(&(self.items.len() as u32).to_le_bytes());
        for it in &self.items {
            match it {
                Item::Ink { style, pts } => {
                    b.push(0);
                    b.extend_from_slice(&style.width.to_le_bytes());
                    b.extend_from_slice(&style.color.to_le_bytes());
                    b.push(style.brush as u8);
                    b.push(style.dash as u8);
                    let ext = style.ext.map(|p| p.encode()).unwrap_or_default();
                    b.extend_from_slice(&(ext.len() as u16).to_le_bytes());
                    b.extend_from_slice(&ext);
                    b.extend_from_slice(&(pts.len() as u32).to_le_bytes());
                    for p in pts {
                        for v in p {
                            b.extend_from_slice(&v.to_le_bytes());
                        }
                    }
                }
                Item::Obj(d) => {
                    b.push(1);
                    let data = crate::snapshot::data_bytes(d);
                    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    b.extend_from_slice(&data);
                }
            }
        }
        b.extend_from_slice(&(self.images.len() as u32).to_le_bytes());
        for (id, a) in &self.images {
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&(a.bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(&a.bytes);
        }
        b
    }

    pub fn decode(b: &[u8]) -> Result<Sticker, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let s = b.get(at..at + n).ok_or("not a whole sticker")?;
            at += n;
            Ok(s)
        };
        if take(4)? != b"OGPS" {
            return Err("not a sticker".into());
        }
        let version = take(1)?[0];
        if !(1..=2).contains(&version) {
            return Err("a sticker from a newer version of OG Paper".into());
        }
        let f64_ = |s: &[u8]| f64::from_le_bytes(s.try_into().expect("8"));
        let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().expect("4"));
        let size_pt = f64_(take(8)?);
        let n = u32_(take(4)?) as usize;
        let mut items = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            match take(1)?[0] {
                0 => {
                    let width = f32::from_le_bytes(take(4)?.try_into().expect("4"));
                    let color = u32_(take(4)?);
                    let brush = Brush::from_u8(take(1)?[0]);
                    let dash = Dash::from_u8(take(1)?[0]);
                    let ext = if version >= 2 {
                        let n = u16::from_le_bytes(take(2)?.try_into().expect("2")) as usize;
                        ogpaper_core::BrushParams::decode(take(n)?)
                    } else {
                        None
                    };
                    let brush = if brush == Brush::Dabs && ext.is_none() {
                        Brush::Marker
                    } else {
                        brush
                    };
                    let np = u32_(take(4)?) as usize;
                    let raw = take(np.checked_mul(12).ok_or("bad sticker")?)?;
                    let pts = raw
                        .chunks_exact(12)
                        .map(|c| {
                            std::array::from_fn(|k| {
                                f32::from_le_bytes(c[4 * k..4 * k + 4].try_into().expect("4"))
                            })
                        })
                        .collect();
                    items.push(Item::Ink {
                        style: Style {
                            width,
                            color,
                            brush,
                            dash,
                            ext,
                        },
                        pts,
                    });
                }
                1 => {
                    let len = u32_(take(4)?) as usize;
                    items.push(Item::Obj(crate::snapshot::get_data(take(len)?)?));
                }
                k => return Err(format!("unknown sticker item {k}")),
            }
        }
        let ni = u32_(take(4)?) as usize;
        let mut images = Vec::new();
        for _ in 0..ni {
            let id = u64::from_le_bytes(take(8)?.try_into().expect("8"));
            let len = u32_(take(4)?) as usize;
            images.push((id, crate::images::load(take(len)?.to_vec())?));
        }
        Ok(Sticker {
            size_pt,
            items,
            images,
        })
    }
}

impl App {
    /// The selection as a sticker. None when nothing is selected.
    pub(crate) fn sticker_from_selection(&self) -> Option<Sticker> {
        let sel = self.sel_sorted_pub();
        if sel.is_empty() {
            return None;
        }
        // Everything in camera units first.
        enum Cam {
            Ink(Style, Vec<[f64; 3]>),
            Obj(ObjData),
        }
        let mut cam = Vec::new();
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        let mut grow = |q: [f64; 2], r: f64| {
            lo = [lo[0].min(q[0] - r), lo[1].min(q[1] - r)];
            hi = [hi[0].max(q[0] + r), hi[1].max(q[1] + r)];
        };
        let mut images = Vec::new();
        for r in &sel {
            match *r {
                ObjRef::Ink(id) => {
                    if self.scene.strokes[id as usize].deleted {
                        continue;
                    }
                    let (o, side) = crate::objects::frame(self.scene.stroke_cell(id), &self.cam);
                    let mut st = self.scene.stroke_style(id);
                    let w = st.width as f64 * side;
                    let pts: Vec<[f64; 3]> = self
                        .scene
                        .stroke_points(id)
                        .iter()
                        .map(|p| {
                            [
                                o[0] + p[0] as f64 * side,
                                o[1] + p[1] as f64 * side,
                                p[2] as f64,
                            ]
                        })
                        .collect();
                    pts.iter().for_each(|p| grow([p[0], p[1]], w * 0.5));
                    st.width = w as f32;
                    cam.push(Cam::Ink(st, pts));
                }
                ObjRef::Group(g) => {
                    let grp = &self.objs.groups[g as usize];
                    if !self.objs.alive(&self.scene, r) {
                        continue;
                    }
                    let d = to_cam(&grp.cell, &grp.data, &self.cam);
                    d.extent().iter().for_each(|&q| grow(q, 0.0));
                    if let Some(id) = d.blob_id() {
                        if let Some(a) = self.objs.images.get(&id) {
                            if !images.iter().any(|(i, _)| *i == id) {
                                images.push((id, a.clone()));
                            }
                        }
                    }
                    cam.push(Cam::Obj(d));
                }
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let l = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-300);
        let norm = |p: [f64; 2]| [(p[0] - c[0]) / l, (p[1] - c[1]) / l];
        let items = cam
            .into_iter()
            .map(|it| match it {
                Cam::Ink(mut st, pts) => {
                    st.width = (st.width as f64 / l) as f32;
                    Item::Ink {
                        style: st,
                        pts: pts
                            .iter()
                            .map(|p| {
                                let q = norm([p[0], p[1]]);
                                [q[0] as f32, q[1] as f32, p[2] as f32]
                            })
                            .collect(),
                    }
                }
                Cam::Obj(d) => Item::Obj(d.map(norm, 1.0 / l)),
            })
            .collect();
        Some(Sticker {
            size_pt: l * self.cam.ppc() / self.ppp(),
            items,
            images,
        })
    }

    /// Place a copy of `s` centred on screen point `at` (px; the middle if
    /// None), as big as it was when saved, and select it.
    pub(crate) fn place_sticker(&mut self, s: &Sticker, at: Option<[f64; 2]>) {
        let at = self.drop_point(at);
        let c = self.px_to_cam(at);
        // As big as when saved, but never more than 80% of the screen.
        let [w, h] = self.size();
        let px = (s.size_pt * self.ppp()).min(w.min(h) * 0.8);
        let l = px / self.cam.ppc();
        let map = |p: [f64; 2]| [c[0] + p[0] * l, c[1] + p[1] * l];
        for (id, a) in &s.images {
            self.objs.images.entry(*id).or_insert_with(|| a.clone());
        }
        let mut added = Vec::new();
        let mut sel = Vec::new();
        for it in &s.items {
            let z = self.scene.z_top + 1.0;
            match it {
                Item::Ink { style, pts } => {
                    let cam_pts: Vec<[f64; 2]> = pts
                        .iter()
                        .map(|p| map([p[0] as f64, p[1] as f64]))
                        .collect();
                    let width_cam = style.width as f64 * l;
                    let (anchor, local, side) =
                        Scene::anchor_for_min(&self.cam.cell, &cam_pts, width_cam.max(1e-12));
                    let pts: Vec<Point> = local
                        .iter()
                        .zip(pts)
                        .map(|(q, p)| [q[0], q[1], p[2], 0.0])
                        .collect();
                    let st = Style {
                        width: (width_cam / side) as f32,
                        ..*style
                    };
                    let id = self
                        .scene
                        .add_stroke_at(&anchor, &pts, st, crate::uid::new(), z);
                    added.push(id);
                    sel.push(ObjRef::Ink(id));
                }
                Item::Obj(d) => {
                    let d = d.map(map, l);
                    let n = d.pieces().len().max(1) as f64;
                    let (g, ids) = self.add_group(&d, Some((z, z + n)));
                    added.extend(ids);
                    sel.push(ObjRef::Group(g));
                }
            }
        }
        self.record_edit(vec![], added);
        self.edit.selection = sel;
        if !self.ui.tool.selects() {
            self.ui.tool = crate::ui::Tool::Select;
        }
        self.redraw();
    }
}

/// Stickers saved on this device (desktop): `~/OG Paper/library/*.ogps`.
#[cfg(not(target_arch = "wasm32"))]
pub mod store {
    use std::path::PathBuf;

    pub fn dir() -> Option<PathBuf> {
        Some(crate::fonts_dir()?.parent()?.join("library"))
    }

    /// (file, name) newest first.
    pub fn list() -> Vec<(PathBuf, String)> {
        let Some(d) = dir() else { return vec![] };
        let mut v: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "ogps"))
            .map(|p| {
                let t = p
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (p, t)
            })
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter()
            .map(|(p, _)| {
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (p, name)
            })
            .collect()
    }

    /// Save under a free file name based on `name`.
    pub fn save(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        let d = dir().ok_or("no home folder")?;
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let clean: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || " -_".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let clean = clean.trim();
        let base = if clean.is_empty() { "Sticker" } else { clean };
        let mut p = d.join(format!("{base}.ogps"));
        let mut k = 2;
        while p.exists() {
            p = d.join(format!("{base} {k}.ogps"));
            k += 1;
        }
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::{Geom, ShapeStyle};

    #[test]
    fn stickers_round_trip() {
        let s = Sticker {
            size_pt: 120.0,
            items: vec![
                Item::Ink {
                    style: Style {
                        width: 0.01,
                        color: 0xff00_00ff,
                        brush: Brush::Dabs,
                        dash: Dash::Dotted,
                        ext: Some(ogpaper_core::BrushParams {
                            tip: ogpaper_core::Tip::Star,
                            ..Default::default()
                        }),
                    },
                    pts: vec![[-0.5, 0.0, 0.3], [0.5, 0.1, 0.9]],
                },
                Item::Obj(ObjData::Shape {
                    style: ShapeStyle::default(),
                    geom: Geom {
                        center: [0.1, 0.2],
                        half: [0.3, 0.2],
                        rot: 0.0,
                        pts: vec![],
                    },
                    width: 0.01,
                    seed: 4,
                }),
            ],
            images: vec![],
        };
        let b = s.encode();
        let t = Sticker::decode(&b).unwrap();
        assert_eq!(t.size_pt, 120.0);
        assert_eq!(t.items.len(), 2);
        match &t.items[0] {
            Item::Ink { style, pts } => {
                assert_eq!(style.dash, Dash::Dotted);
                assert_eq!(style.ext.map(|p| p.tip), Some(ogpaper_core::Tip::Star));
                assert_eq!(pts[1], [0.5, 0.1, 0.9]);
            }
            _ => panic!(),
        }
        assert!(matches!(
            &t.items[1],
            Item::Obj(ObjData::Shape { seed: 4, .. })
        ));
        assert!(Sticker::decode(&b[..b.len() - 1]).is_err());
        assert!(Sticker::decode(b"nope").is_err());
    }
}
