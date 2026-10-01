// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A compact binary snapshot of a canvas: strokes (erased ones too), their
//! timeline, bookmarks and the view. The web app keeps it in browser storage
//! and downloads it as an offline copy (`.ogpt`). It is not the `.ogp` format
//! and holds no undo history.
//!
//! Layout (little endian): b"OGPT", version u8, camera, then
//! - strokes: count u32; each: addr, width f32, color u32, brush u8,
//!   deleted u8, uid u128, point count u32, points (f32 x4 each)
//! - events: count u32; each: time i64 (ms), stroke u32, alive u8
//! - bookmarks: count u32; each: name (u32 length + UTF-8), camera, view_px f64
//!
//! A camera is addr, off f64 x2, scale f64. An addr is level i64 then x and y
//! as (byte count u32, signed LE bytes).

use num_bigint::BigInt;
use ogpaper_core::{Brush, Camera, CellAddr, Scene};

use crate::timeline::{Bookmark, Event, Timeline};

const MAGIC: &[u8; 4] = b"OGPT";
const VERSION: u8 = 1;

pub struct Snapshot {
    pub scene: Scene,
    pub cam: Camera,
    pub timeline: Timeline,
    pub bookmarks: Vec<Bookmark>,
}

pub fn encode(scene: &Scene, cam: &Camera, timeline: &Timeline, bookmarks: &[Bookmark]) -> Vec<u8> {
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
    b
}

pub fn decode(bytes: &[u8], base_px: f64) -> Result<Snapshot, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err("not an OG Paper snapshot".into());
    }
    let v = r.u8()?;
    if v != VERSION {
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
        let id = scene.add_stroke_styled(&cell, &pts, width, color, brush, uid);
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
    Ok(Snapshot {
        scene,
        cam,
        timeline: Timeline::from_events(events),
        bookmarks,
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
        let bytes = encode(&s, &cam, &tl, &marks);
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
        assert_eq!(snap.bookmarks[0].cam.cell, cam.cell);
        assert_eq!(snap.bookmarks[0].view_px, 640.0);
        assert_eq!(snap.cam.cell, cam.cell);
        assert_eq!(snap.cam.off, cam.off);
        assert!((snap.cam.scale - cam.scale).abs() < 1e-12);
        assert!(decode(&bytes[..bytes.len() - 3], 800.0).is_err());
        assert!(decode(b"nope", 800.0).is_err());
    }
}
