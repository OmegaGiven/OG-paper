// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! The `.ogp` file: one SQLite database per canvas.
//!
//! Every object is one row, written the moment it is created (WAL mode, so a
//! crash loses at most the stroke in progress). Deleting only sets a flag, so
//! undo keeps working after reopening and nothing is ever silently lost. The
//! `meta` table carries a plain-English README describing the format.

use std::path::{Path, PathBuf};

use num_bigint::BigInt;
use ogpaper_core::{Brush, Camera, CellAddr, Point, Scene};
use rusqlite::{params, Connection, OptionalExtension};

pub use rusqlite::Error;

pub const FORMAT_VERSION: &str = "0.1";

const README: &str = "This is an OG Paper canvas (https://github.com/OmegaGiven/OG-paper). \
It is a SQLite database. Table `objects` holds one row per drawn object. Each object is \
anchored to a cell of an infinite quadtree: a cell at `level` L has side 2^-L world units \
and integer coordinates (`ix`, `iy`, stored as decimal text because they can be arbitrarily \
large). For kind = 'stroke', `points` is little-endian f32 triples (x, y, pressure) in the \
cell's local space, where [0,1]x[0,1] is the cell; `width` is in the same units; `color` is \
RGBA8 packed as R | G<<8 | B<<16 | A<<24; `brush` is 0 = pen, 1 = marker, 2 = highlighter. \
Rows with deleted = 1 are hidden (kept for undo). Draw objects in rowid order.";

pub struct OgpFile {
    conn: Connection,
    path: PathBuf,
}

/// The last view, stored so a canvas reopens where you left it.
pub struct SavedView {
    pub cell: CellAddr,
    pub off: [f64; 2],
    pub scale: f64,
}

impl OgpFile {
    /// Create a new, empty canvas file (replacing any file at `path`).
    pub fn create(path: &Path) -> Result<Self, Error> {
        let _ = std::fs::remove_file(path);
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE objects (
                 id BLOB PRIMARY KEY,
                 level INTEGER NOT NULL,
                 ix TEXT NOT NULL,
                 iy TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 brush INTEGER NOT NULL DEFAULT 0,
                 color INTEGER NOT NULL DEFAULT 0,
                 width REAL NOT NULL DEFAULT 0,
                 points BLOB,
                 deleted INTEGER NOT NULL DEFAULT 0,
                 created INTEGER NOT NULL
             );
             CREATE INDEX objects_cell ON objects (level, ix, iy);",
        )?;
        let f = Self {
            conn,
            path: path.to_path_buf(),
        };
        f.set_meta("format", "ogp")?;
        f.set_meta("format_version", FORMAT_VERSION)?;
        f.set_meta("README", README)?;
        f.set_meta("created", &now_ms().to_string())?;
        f.set_meta("app", concat!("og-paper ", env!("CARGO_PKG_VERSION")))?;
        Ok(f)
    }

    /// Open an existing canvas and load it into a scene.
    pub fn open(path: &Path) -> Result<(Self, Scene, Option<SavedView>), Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        let f = Self {
            conn,
            path: path.to_path_buf(),
        };
        let mut scene = Scene::new();
        {
            let mut q = f.conn.prepare(
                "SELECT id, level, ix, iy, brush, color, width, points, deleted
                 FROM objects WHERE kind = 'stroke' ORDER BY rowid",
            )?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                let id: Vec<u8> = r.get(0)?;
                let cell = CellAddr {
                    level: r.get(1)?,
                    x: parse_big(&r.get::<_, String>(2)?),
                    y: parse_big(&r.get::<_, String>(3)?),
                };
                let brush = Brush::from_u8(r.get::<_, i64>(4)? as u8);
                let color = r.get::<_, i64>(5)? as u32;
                let width = r.get::<_, f64>(6)? as f32;
                let pts = decode_points(&r.get::<_, Vec<u8>>(7)?);
                let deleted: i64 = r.get(8)?;
                if pts.is_empty() {
                    continue;
                }
                let sid = scene.add_stroke_styled(&cell, &pts, width, color, brush, uid_from(&id));
                if deleted != 0 {
                    scene.delete(sid);
                }
            }
        }
        let view = f.get_meta("view")?.and_then(|v| parse_view(&v));
        Ok((f, scene, view))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one stroke (no-op if it is already in the file).
    pub fn put_stroke(&self, scene: &Scene, id: u32) -> Result<(), Error> {
        let s = &scene.strokes[id as usize];
        let cell = scene.stroke_cell(id);
        self.conn.execute(
            "INSERT OR IGNORE INTO objects (id, level, ix, iy, kind, brush, color, width, points, deleted, created)
             VALUES (?1, ?2, ?3, ?4, 'stroke', ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                s.uid.to_be_bytes().to_vec(),
                cell.level,
                cell.x.to_string(),
                cell.y.to_string(),
                s.brush as u8 as i64,
                s.color as i64,
                s.width as f64,
                encode_points(scene.stroke_points(id)),
                s.deleted as i64,
                now_ms(),
            ],
        )?;
        Ok(())
    }

    /// Mirror a stroke's deleted flag (after erase / undo / redo).
    pub fn put_deleted(&self, scene: &Scene, id: u32) -> Result<(), Error> {
        let s = &scene.strokes[id as usize];
        self.conn.execute(
            "UPDATE objects SET deleted = ?1 WHERE id = ?2",
            params![s.deleted as i64, s.uid.to_be_bytes().to_vec()],
        )?;
        Ok(())
    }

    /// Write every stroke of `scene` (Save As into a fresh file).
    pub fn put_all(&self, scene: &Scene) -> Result<(), Error> {
        self.conn.execute_batch("BEGIN")?;
        for id in 0..scene.strokes.len() as u32 {
            if scene.strokes[id as usize].uid != 0 {
                self.put_stroke(scene, id)?;
            }
        }
        self.conn.execute_batch("COMMIT")
    }

    pub fn put_view(&self, cam: &Camera) -> Result<(), Error> {
        let v = format!(
            "{}|{}|{}|{}|{}|{}",
            cam.cell.level, cam.cell.x, cam.cell.y, cam.off[0], cam.off[1], cam.scale
        );
        self.set_meta("view", &v)
    }

    fn set_meta(&self, k: &str, v: &str) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![k, v],
        )?;
        Ok(())
    }

    fn get_meta(&self, k: &str) -> Result<Option<String>, Error> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [k], |r| r.get(0))
            .optional()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn parse_big(s: &str) -> BigInt {
    s.parse().unwrap_or_default()
}

fn uid_from(b: &[u8]) -> u128 {
    let mut a = [0u8; 16];
    let n = b.len().min(16);
    a[16 - n..].copy_from_slice(&b[b.len() - n..]);
    u128::from_be_bytes(a)
}

fn encode_points(pts: &[Point]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pts.len() * 12);
    for p in pts {
        for v in &p[..3] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn decode_points(b: &[u8]) -> Vec<Point> {
    b.chunks_exact(12)
        .map(|c| {
            let f = |i: usize| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
            [f(0), f(4), f(8), 0.0]
        })
        .collect()
}

fn parse_view(v: &str) -> Option<SavedView> {
    let p: Vec<&str> = v.split('|').collect();
    if p.len() != 6 {
        return None;
    }
    Some(SavedView {
        cell: CellAddr {
            level: p[0].parse().ok()?,
            x: p[1].parse().ok()?,
            y: p[2].parse().ok()?,
        },
        off: [p[3].parse().ok()?, p[4].parse().ok()?],
        scale: p[5].parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_deep_cells_and_tombstones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.ogp");
        let mut scene = Scene::new();
        let deep = CellAddr::new(300, BigInt::from(1) << 299u32, -(BigInt::from(7) << 250u32));
        let a = scene.add_stroke_styled(
            &deep,
            &[[0.1, 0.2, 0.5, 0.0], [0.3, 0.4, 1.0, 0.0]],
            0.02,
            0xff0000ff,
            Brush::Highlighter,
            42,
        );
        let b = scene.add_stroke_styled(
            &CellAddr::new(-3, 5, 5),
            &[[0.5, 0.5, 1.0, 0.0]; 3],
            0.1,
            7,
            Brush::Pen,
            43,
        );
        let f = OgpFile::create(&path).unwrap();
        f.put_stroke(&scene, a).unwrap();
        f.put_stroke(&scene, b).unwrap();
        scene.delete(b);
        f.put_deleted(&scene, b).unwrap();
        let cam = Camera::new(deep.clone(), [0.25, 0.75], 700.0);
        f.put_view(&cam).unwrap();
        drop(f);

        let (_f, s2, view) = OgpFile::open(&path).unwrap();
        assert_eq!(s2.strokes.len(), 2);
        assert_eq!(*s2.stroke_cell(0), deep);
        assert_eq!(s2.strokes[0].brush, Brush::Highlighter);
        assert_eq!(s2.strokes[0].uid, 42);
        assert_eq!(s2.stroke_points(0)[0], [0.1, 0.2, 0.5, 0.0]);
        assert!(s2.strokes[1].deleted);
        let v = view.unwrap();
        assert_eq!(v.cell, cam.cell);
        assert_eq!(v.off, cam.off);
    }
}
