// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A compact binary snapshot of a canvas: strokes (erased ones too), their
//! timeline, bookmarks, shapes and texts, and the view. The web app keeps it
//! in browser storage and downloads it as an offline copy (`.ogpt`). It is
//! not the `.ogp` format and holds no undo history.
//!
//! Layout (little endian): b"OGPT", version u8 (3; 1 and 2 are still read), camera,
//! then
//! - strokes: count u32; each: addr, width f32, color u32, brush u8,
//!   deleted u8, uid u128, point count u32, points (f32 x4 each),
//!   and in v2: dash u8, z f64
//! - events: count u32; each: time i64 (ms), stroke u32, alive u8
//! - bookmarks: count u32; each: name (u32 length + UTF-8), camera, view_px f64
//! - v2 groups (shapes and texts): count u32; each: addr, stroke count u32,
//!   stroke indexes (u32 each), data (see [`put_data`])
//! - v3 pictures: count u32; each: id u64, byte count u32, the file (PNG,
//!   JPEG, GIF or WebP)
//!
//! A camera is addr, off f64 x2, scale f64. An addr is level i64 then x and y
//! as (byte count u32, signed LE bytes).

use num_bigint::BigInt;
use ogpaper_core::{Brush, Camera, CellAddr, Dash, Scene, Style};

use crate::font::{self, Align};
use crate::objects::{Group, ObjData, Objects, TextStyle};
use crate::shapes::{ArrowType, FillStyle, Geom, Head, ShapeKind, ShapeStyle, Sloppiness};
use crate::timeline::{Bookmark, Event, Timeline};

const MAGIC: &[u8; 4] = b"OGPT";
const VERSION: u8 = 3;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct Snapshot {
    pub scene: Scene,
    pub cam: Camera,
    pub timeline: Timeline,
    pub bookmarks: Vec<Bookmark>,
    pub objs: Objects,
}

/// A shape's or text's settings: kind u8 (0 shape, 1 text), then
/// - shape: kind, stroke u32, fill u32, fill style, dash, sloppiness, round,
///   sides, start head, end head, arrow type, opacity (u8 unless noted),
///   geometry, width f64, seed u32
/// - text (kind 2): text (u32 length + UTF-8), font name (u32 length +
///   UTF-8), align u8, color u32, opacity u8, geometry, size f64, seed u32.
///   Kind 1 (older) had a font number u8 (0 single-line, 1 single-line
///   hand, 2 single-line mono) instead of the name.
/// - picture (kind 3): picture id u64, opacity u8, geometry
/// - table (kind 4): row count u32; each row: cell count u32, cells (u32
///   length + UTF-8); then font name, align, color, opacity, geometry, size,
///   seed as for text
///
/// Geometry: centre f64 x2, half size f64 x2, rotation f64, point count u32,
/// points f64 x2. All lengths in the group cell's units.
pub fn put_data(b: &mut Vec<u8>, d: &ObjData) {
    let geom = |b: &mut Vec<u8>, g: &Geom| {
        for v in [g.center[0], g.center[1], g.half[0], g.half[1], g.rot] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&(g.pts.len() as u32).to_le_bytes());
        for p in &g.pts {
            b.extend_from_slice(&p[0].to_le_bytes());
            b.extend_from_slice(&p[1].to_le_bytes());
        }
    };
    match d {
        ObjData::Shape {
            style,
            geom: g,
            width,
            seed,
        } => {
            b.push(0);
            b.push(style.kind as u8);
            b.extend_from_slice(&style.stroke.to_le_bytes());
            b.extend_from_slice(&style.fill.to_le_bytes());
            b.extend_from_slice(&[
                style.fill_style as u8,
                style.dash as u8,
                style.sloppiness as u8,
                style.round as u8,
                style.sides,
                style.start as u8,
                style.end as u8,
                style.arrow as u8,
                style.opacity,
            ]);
            geom(b, g);
            b.extend_from_slice(&width.to_le_bytes());
            b.extend_from_slice(&seed.to_le_bytes());
        }
        ObjData::Text {
            text,
            style,
            geom: g,
            size,
            seed,
        } => {
            b.push(2);
            b.extend_from_slice(&(text.len() as u32).to_le_bytes());
            b.extend_from_slice(text.as_bytes());
            let name = font::name_of(style.font);
            b.extend_from_slice(&(name.len() as u32).to_le_bytes());
            b.extend_from_slice(name.as_bytes());
            b.push(style.align as u8);
            b.extend_from_slice(&style.color.to_le_bytes());
            b.push(style.opacity);
            geom(b, g);
            b.extend_from_slice(&size.to_le_bytes());
            b.extend_from_slice(&seed.to_le_bytes());
        }
        ObjData::Image {
            id,
            geom: g,
            opacity,
        } => {
            b.push(3);
            b.extend_from_slice(&id.to_le_bytes());
            b.push(*opacity);
            geom(b, g);
        }
        ObjData::Table {
            cells,
            style,
            geom: g,
            size,
            seed,
        } => {
            b.push(4);
            let str = |b: &mut Vec<u8>, t: &str| {
                b.extend_from_slice(&(t.len() as u32).to_le_bytes());
                b.extend_from_slice(t.as_bytes());
            };
            b.extend_from_slice(&(cells.len() as u32).to_le_bytes());
            for row in cells {
                b.extend_from_slice(&(row.len() as u32).to_le_bytes());
                for c in row {
                    str(b, c);
                }
            }
            str(b, &font::name_of(style.font));
            b.push(style.align as u8);
            b.extend_from_slice(&style.color.to_le_bytes());
            b.push(style.opacity);
            geom(b, g);
            b.extend_from_slice(&size.to_le_bytes());
            b.extend_from_slice(&seed.to_le_bytes());
        }
    }
}

/// Read what [`put_data`] wrote.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn get_data(bytes: &[u8]) -> Result<ObjData, String> {
    Reader { b: bytes, at: 0 }.data()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn data_bytes(d: &ObjData) -> Vec<u8> {
    let mut b = Vec::new();
    put_data(&mut b, d);
    b
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn encode(
    scene: &Scene,
    cam: &Camera,
    timeline: &Timeline,
    bookmarks: &[Bookmark],
    objs: &Objects,
) -> Vec<u8> {
    let mut b = Vec::with_capacity(64 + scene.points.len() * 16 + timeline.events.len() * 13);
    b.extend_from_slice(MAGIC);
    b.push(VERSION);
    put_cam(&mut b, cam);
    b.extend_from_slice(&(scene.strokes.len() as u32).to_le_bytes());
    for (id, s) in scene.strokes.iter().enumerate() {
        let id = id as u32;
        put_addr(&mut b, scene.stroke_cell(id));
        b.extend_from_slice(&s.width.to_le_bytes());
        b.extend_from_slice(&s.color.to_le_bytes());
        b.push(s.brush as u8);
        b.push(s.deleted as u8);
        b.extend_from_slice(&s.uid.to_le_bytes());
        let pts = scene.stroke_points(id);
        b.extend_from_slice(&(pts.len() as u32).to_le_bytes());
        for p in pts {
            for v in p {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        b.push(s.dash as u8);
        b.extend_from_slice(&s.z.to_le_bytes());
    }
    b.extend_from_slice(&(timeline.events.len() as u32).to_le_bytes());
    for e in &timeline.events {
        b.extend_from_slice(&e.t.to_le_bytes());
        b.extend_from_slice(&e.id.to_le_bytes());
        b.push(e.alive as u8);
    }
    b.extend_from_slice(&(bookmarks.len() as u32).to_le_bytes());
    for m in bookmarks {
        b.extend_from_slice(&(m.name.len() as u32).to_le_bytes());
        b.extend_from_slice(m.name.as_bytes());
        put_cam(&mut b, &m.cam);
        b.extend_from_slice(&m.view_px.to_le_bytes());
    }
    b.extend_from_slice(&(objs.groups.len() as u32).to_le_bytes());
    for g in &objs.groups {
        put_addr(&mut b, &g.cell);
        b.extend_from_slice(&(g.strokes.len() as u32).to_le_bytes());
        for s in &g.strokes {
            b.extend_from_slice(&s.to_le_bytes());
        }
        put_data(&mut b, &g.data);
    }
    b.extend_from_slice(&(objs.images.len() as u32).to_le_bytes());
    for (id, a) in &objs.images {
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&(a.bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(&a.bytes);
    }
    b
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn decode(bytes: &[u8], base_px: f64) -> Result<Snapshot, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err("not an OG Paper snapshot".into());
    }
    let v = r.u8()?;
    if !(1..=VERSION).contains(&v) {
        return Err(format!("unsupported snapshot version {v}"));
    }
    let cam = r.cam(base_px)?;
    let n = r.u32()?;
    let mut scene = Scene::new();
    for _ in 0..n {
        let cell = r.addr()?;
        let width = r.f32()?;
        let color = r.u32()?;
        let brush = Brush::from_u8(r.u8()?);
        let deleted = r.u8()? != 0;
        let uid = u128::from_le_bytes(r.take(16)?.try_into().expect("16 bytes"));
        let len = r.u32()? as usize;
        if len > (bytes.len() - r.at) / 16 {
            return Err("truncated snapshot".into());
        }
        let mut pts = Vec::with_capacity(len);
        for _ in 0..len {
            pts.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
        }
        let (dash, z) = if v >= 2 {
            (Dash::from_u8(r.u8()?), r.f64()?)
        } else {
            (Dash::Solid, scene.z_top + 1.0)
        };
        let style = Style {
            width,
            color,
            brush,
            dash,
        };
        let id = scene.add_stroke_at(&cell, &pts, style, uid, z);
        if deleted {
            scene.delete(id);
        }
    }
    let n = r.u32()?;
    let mut events = Vec::with_capacity((n as usize).min(bytes.len() / 13));
    for _ in 0..n {
        let t = i64::from_le_bytes(r.take(8)?.try_into().expect("8 bytes"));
        let id = r.u32()?;
        let alive = r.u8()? != 0;
        if (id as usize) < scene.strokes.len() {
            events.push(Event { t, id, alive });
        }
    }
    let n = r.u32()?;
    let mut bookmarks = Vec::new();
    for _ in 0..n {
        let len = r.u32()? as usize;
        let name = String::from_utf8_lossy(r.take(len)?).into_owned();
        let cam = r.cam(base_px)?;
        let view_px = r.f64()?;
        bookmarks.push(Bookmark { name, cam, view_px });
    }
    let mut objs = Objects::default();
    if v >= 2 {
        let n = r.u32()?;
        for _ in 0..n {
            let cell = r.addr()?;
            let k = r.u32()? as usize;
            if k > (bytes.len() - r.at) / 4 {
                return Err("truncated snapshot".into());
            }
            let mut strokes = Vec::with_capacity(k);
            for _ in 0..k {
                let s = r.u32()?;
                if (s as usize) < scene.strokes.len() {
                    strokes.push(s);
                }
            }
            let data = r.data()?;
            objs.add(Group {
                cell,
                data,
                strokes,
            });
        }
    }
    if v >= 3 {
        let n = r.u32()?;
        for _ in 0..n {
            let id = u64::from_le_bytes(r.take(8)?.try_into().expect("8 bytes"));
            let len = r.u32()? as usize;
            let bytes = r.take(len)?.to_vec();
            // A picture that no longer decodes is skipped, not fatal.
            if let Ok(a) = crate::images::load(bytes) {
                objs.images.insert(id, a);
            }
        }
    }
    Ok(Snapshot {
        scene,
        cam,
        timeline: Timeline::from_events(events),
        bookmarks,
        objs,
    })
}

fn put_cam(b: &mut Vec<u8>, cam: &Camera) {
    put_addr(b, &cam.cell);
    b.extend_from_slice(&cam.off[0].to_le_bytes());
    b.extend_from_slice(&cam.off[1].to_le_bytes());
    b.extend_from_slice(&cam.scale.to_le_bytes());
}

fn put_addr(b: &mut Vec<u8>, a: &CellAddr) {
    b.extend_from_slice(&a.level.to_le_bytes());
    for v in [&a.x, &a.y] {
        let bytes = v.to_signed_bytes_le();
        b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(&bytes);
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len());
        let end = end.ok_or("truncated snapshot")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
    fn cam(&mut self, base_px: f64) -> Result<Camera, String> {
        let cell = self.addr()?;
        let off = [self.f64()?, self.f64()?];
        let scale = self.f64()?;
        let mut cam = Camera::new(cell, off, base_px);
        if scale.is_finite() && scale > 0.0 {
            cam.scale = scale;
            cam.normalize();
        }
        Ok(cam)
    }
    fn data(&mut self) -> Result<ObjData, String> {
        let kind = self.u8()?;
        let geom = |r: &mut Reader| -> Result<Geom, String> {
            let center = [r.f64()?, r.f64()?];
            let half = [r.f64()?, r.f64()?];
            let rot = r.f64()?;
            let n = r.u32()? as usize;
            if n > (r.b.len() - r.at) / 16 {
                return Err("truncated snapshot".into());
            }
            let mut pts = Vec::with_capacity(n);
            for _ in 0..n {
                pts.push([r.f64()?, r.f64()?]);
            }
            Ok(Geom {
                center,
                half,
                rot,
                pts,
            })
        };
        match kind {
            0 => {
                let k = ShapeKind::from_u8(self.u8()?);
                let stroke = self.u32()?;
                let fill = self.u32()?;
                let f = self.take(9)?;
                let style = ShapeStyle {
                    kind: k,
                    stroke,
                    fill,
                    fill_style: FillStyle::from_u8(f[0]),
                    dash: Dash::from_u8(f[1]),
                    sloppiness: Sloppiness::from_u8(f[2]),
                    round: f[3] != 0,
                    sides: f[4],
                    start: Head::from_u8(f[5]),
                    end: Head::from_u8(f[6]),
                    arrow: ArrowType::from_u8(f[7]),
                    opacity: f[8],
                };
                let geom = geom(self)?;
                let width = self.f64()?;
                let seed = self.u32()?;
                Ok(ObjData::Shape {
                    style,
                    geom,
                    width,
                    seed,
                })
            }
            1 | 2 => {
                let n = self.u32()? as usize;
                let text = String::from_utf8_lossy(self.take(n)?).into_owned();
                let font = if kind == 1 {
                    // Ids 0-2 are the single-line fonts in every registry.
                    (self.u8()?).min(2) as font::FontId
                } else {
                    let n = self.u32()? as usize;
                    font::id_of(&String::from_utf8_lossy(self.take(n)?))
                };
                let align = Align::from_u8(self.u8()?);
                let color = self.u32()?;
                let opacity = self.u8()?;
                let geom = geom(self)?;
                let size = self.f64()?;
                let seed = self.u32()?;
                Ok(ObjData::Text {
                    text,
                    style: TextStyle {
                        font,
                        align,
                        color,
                        opacity,
                    },
                    geom,
                    size,
                    seed,
                })
            }
            3 => {
                let id = u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes"));
                let opacity = self.u8()?;
                let geom = geom(self)?;
                Ok(ObjData::Image { id, geom, opacity })
            }
            4 => {
                let rows = self.u32()? as usize;
                if rows > self.b.len() - self.at {
                    return Err("truncated snapshot".into());
                }
                let mut cells = Vec::with_capacity(rows);
                for _ in 0..rows {
                    let n = self.u32()? as usize;
                    if n > self.b.len() - self.at {
                        return Err("truncated snapshot".into());
                    }
                    let mut row = Vec::with_capacity(n);
                    for _ in 0..n {
                        row.push(self.string()?);
                    }
                    cells.push(row);
                }
                let font = font::id_of(&self.string()?);
                let align = Align::from_u8(self.u8()?);
                let color = self.u32()?;
                let opacity = self.u8()?;
                let geom = geom(self)?;
                let size = self.f64()?;
                let seed = self.u32()?;
                Ok(ObjData::Table {
                    cells,
                    style: TextStyle {
                        font,
                        align,
                        color,
                        opacity,
                    },
                    geom,
                    size,
                    seed,
                })
            }
            k => Err(format!("unknown object kind {k}")),
        }
    }

    fn addr(&mut self) -> Result<CellAddr, String> {
        let level = i64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes"));
        let mut big = || -> Result<BigInt, String> {
            let n = self.u32()? as usize;
            Ok(BigInt::from_signed_bytes_le(self.take(n)?))
        };
        let x = big()?;
        let y = big()?;
        Ok(CellAddr { level, x, y })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_strokes_timeline_bookmarks_and_view() {
        let mut s = Scene::new();
        let deep = CellAddr::new(150, BigInt::from(-7) << 140u32, BigInt::from(3) << 149u32);
        let a = s.add_stroke(&deep, &[[0.1, 0.2], [0.3, 0.4]], 0.01, 0x11223344);
        let b = s.add_stroke(&CellAddr::new(-3, -1, 2), &[[0.5, 0.5]], 0.2, 7);
        s.delete(b);
        let tl = Timeline::from_events(vec![
            Event {
                t: 10,
                id: a,
                alive: true,
            },
            Event {
                t: 20,
                id: b,
                alive: true,
            },
            Event {
                t: 30,
                id: b,
                alive: false,
            },
        ]);
        let mut cam = Camera::new(deep.clone(), [0.25, 0.75], 800.0);
        cam.zoom_at(1.5, [0.0, 0.0]);
        let marks = vec![Bookmark {
            name: "deep ✨".into(),
            cam: cam.clone(),
            view_px: 640.0,
        }];
        let mut objs = Objects::default();
        objs.add(Group {
            cell: deep.clone(),
            data: ObjData::Text {
                text: "Hi ✨".into(),
                style: TextStyle {
                    font: font::id_of("Lora"),
                    ..TextStyle::default()
                },
                geom: Geom {
                    center: [0.5, 0.5],
                    half: [0.2, 0.1],
                    rot: 0.3,
                    pts: vec![],
                },
                size: 0.1,
                seed: 9,
            },
            strokes: vec![a],
        });
        objs.add(Group {
            cell: deep.clone(),
            data: ObjData::Table {
                cells: vec![vec!["a".into(), "b ✨".into()], vec![]],
                style: TextStyle::default(),
                geom: Geom::default(),
                size: 0.1,
                seed: 2,
            },
            strokes: vec![a],
        });
        let png = {
            let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        objs.images
            .insert(77, crate::images::prepare(png.clone()).unwrap());
        objs.add(Group {
            cell: deep.clone(),
            data: ObjData::Image {
                id: 77,
                geom: Geom::default(),
                opacity: 128,
            },
            strokes: vec![a],
        });
        let bytes = encode(&s, &cam, &tl, &marks, &objs);
        let snap = decode(&bytes, 800.0).unwrap();
        let s2 = &snap.scene;
        assert_eq!(s2.strokes.len(), 2);
        assert!(!s2.strokes[0].deleted && s2.strokes[1].deleted);
        let live: u32 = s2.roots.iter().map(|&r| s2.node(r).subtree).sum();
        assert_eq!(live, 1);
        assert_eq!(s2.stroke_cell(0), s.stroke_cell(a));
        assert_eq!(s2.stroke_points(0), s.stroke_points(a));
        assert_eq!(s2.strokes[0].color, 0x11223344);
        assert_eq!(snap.timeline.events, tl.events);
        assert_eq!(snap.bookmarks[0].name, "deep ✨");
        assert_eq!(snap.objs.groups.len(), 3);
        for i in 0..3 {
            assert_eq!(snap.objs.groups[i].data, objs.groups[i].data);
        }
        assert_eq!(snap.objs.images[&77].w, 3);
        assert_eq!(snap.objs.image_of.get(&a), Some(&(77, 128)));
        assert_eq!(snap.scene.strokes[0].z, s.strokes[0].z);
        assert_eq!(snap.bookmarks[0].cam.cell, cam.cell);
        assert_eq!(snap.bookmarks[0].view_px, 640.0);
        assert_eq!(snap.cam.cell, cam.cell);
        assert_eq!(snap.cam.off, cam.off);
        assert!((snap.cam.scale - cam.scale).abs() < 1e-12);
        assert!(decode(&bytes[..bytes.len() - 3], 800.0).is_err());
        assert!(decode(b"nope", 800.0).is_err());
    }
}
