// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Compatibility, so older and newer apps and servers keep working
//! together: a new kind of message gets a new tag (receivers ignore tags
//! they do not know), and new fields only ever go at the end of a message
//! (receivers read the fields they know and ignore the rest). Hello and
//! Welcome carry the sender's protocol version ([`PROTOCOL`]; 0 when
//! absent, from apps before it was added).
//!
//! Messages between OG Paper apps sharing a canvas live (see `net`), one
//! per WebSocket (or data channel) frame. Little endian: a tag byte, then
//! the fields in order. Strings are a u32 byte count + UTF-8; a version is
//! its text form (`Version::to_text`); an addr is level i64 + x and y as
//! (u32 byte count, signed LE bytes); bytes are a u32 count + the bytes.
//!
//! - 1 Hello (guest to host): canvas u128, peer u64, version, key, name
//! - 2 Welcome (host to guest): canvas u128, host peer u64, version,
//!   policy u8, can edit u8, canvas name
//! - 3 Changes: a changes-only copy (`.ogpt`, see `share`), as bytes
//! - 4 Presence: peer u64, name, color u32, addr, off f64 x2, scale f64
//! - 5 Wet (a stroke being drawn): peer u64, color u32, width f32 (in its
//!   cell's units), addr, point count u32, points (f32 x, y each)
//! - 6 Gone: peer u64 (left)
//! - 7 Error: text (the sender closes after it)
//! - 8 Need pictures: count u32, ids u64 (pictures a copy lacks)
//! - 9 Pictures: count u32; each id u64, file as bytes
//! - 10 List pages (to a server)
//! - 11 Pages: count u32; each canvas u128, name, edit key, view key,
//!   changed ms u64 (an empty edit key: this link only views)
//! - 12 New page: name · 13 Rename page: canvas u128, name · 14 Delete
//!   page: canvas u128

use num_bigint::BigInt;

/// This app's protocol version (see the compatibility rules above).
pub const PROTOCOL: u16 = 1;
use ogpaper_core::sync::Version;
use ogpaper_core::CellAddr;

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Hello {
        canvas: u128,
        peer: u64,
        version: Version,
        key: String,
        name: String,
        proto: u16,
    },
    Welcome {
        canvas: u128,
        host: u64,
        version: Version,
        policy: u8,
        edit: bool,
        name: String,
        proto: u16,
    },
    Changes(Vec<u8>),
    Presence {
        peer: u64,
        name: String,
        color: u32,
        cell: CellAddr,
        off: [f64; 2],
        scale: f64,
    },
    Wet {
        peer: u64,
        color: u32,
        width: f32,
        cell: CellAddr,
        pts: Vec<[f32; 2]>,
    },
    Gone(u64),
    Error(String),
    /// Pictures this side lacks (live changes leave the files out).
    NeedImages(Vec<u64>),
    /// Picture files: id (content hash) and bytes.
    Images(Vec<(u64, Vec<u8>)>),
    ListPages,
    Pages(Vec<PageInfo>),
    NewPage(String),
    RenamePage(u128, String),
    DeletePage(u128),
}

/// One page a server hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageInfo {
    pub canvas: u128,
    pub name: String,
    /// The page's edit key (empty when asked with a view link).
    pub edit: String,
    pub view: String,
    /// When it last changed (ms since the Unix epoch).
    pub changed: u64,
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u128(&mut self, v: u128) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn addr(&mut self, a: &CellAddr) {
        self.0.extend_from_slice(&a.level.to_le_bytes());
        self.bytes(&a.x.to_signed_bytes_le());
        self.bytes(&a.y.to_signed_bytes_le());
    }
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.b.len() - self.at < n {
            return Err("short message".into());
        }
        let s = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    /// A field added later: 0 when the sender was older.
    fn u16_or_0(&mut self) -> u16 {
        self.take(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .unwrap_or(0)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn u128(&mut self) -> Result<u128, String> {
        Ok(u128::from_le_bytes(self.take(16)?.try_into().expect("16")))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?).map_err(|_| "bad text".into())
    }
    fn addr(&mut self) -> Result<CellAddr, String> {
        let level = i64::from_le_bytes(self.take(8)?.try_into().expect("8"));
        let x = BigInt::from_signed_bytes_le(&self.bytes()?);
        let y = BigInt::from_signed_bytes_le(&self.bytes()?);
        Ok(CellAddr { level, x, y })
    }
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        match self {
            Msg::Hello {
                canvas,
                peer,
                version,
                key,
                name,
                proto,
            } => {
                w.u8(1);
                w.u128(*canvas);
                w.u64(*peer);
                w.str(&version.to_text());
                w.str(key);
                w.str(name);
                w.0.extend_from_slice(&proto.to_le_bytes());
            }
            Msg::Welcome {
                canvas,
                host,
                version,
                policy,
                edit,
                name,
                proto,
            } => {
                w.u8(2);
                w.u128(*canvas);
                w.u64(*host);
                w.str(&version.to_text());
                w.u8(*policy);
                w.u8(*edit as u8);
                w.str(name);
                w.0.extend_from_slice(&proto.to_le_bytes());
            }
            Msg::Changes(b) => {
                w.u8(3);
                w.bytes(b);
            }
            Msg::Presence {
                peer,
                name,
                color,
                cell,
                off,
                scale,
            } => {
                w.u8(4);
                w.u64(*peer);
                w.str(name);
                w.u32(*color);
                w.addr(cell);
                w.f64(off[0]);
                w.f64(off[1]);
                w.f64(*scale);
            }
            Msg::Wet {
                peer,
                color,
                width,
                cell,
                pts,
            } => {
                w.u8(5);
                w.u64(*peer);
                w.u32(*color);
                w.f32(*width);
                w.addr(cell);
                w.u32(pts.len() as u32);
                for p in pts {
                    w.f32(p[0]);
                    w.f32(p[1]);
                }
            }
            Msg::Gone(p) => {
                w.u8(6);
                w.u64(*p);
            }
            Msg::Error(t) => {
                w.u8(7);
                w.str(t);
            }
            Msg::NeedImages(ids) => {
                w.u8(8);
                w.u32(ids.len() as u32);
                for id in ids {
                    w.u64(*id);
                }
            }
            Msg::Images(all) => {
                w.u8(9);
                w.u32(all.len() as u32);
                for (id, b) in all {
                    w.u64(*id);
                    w.bytes(b);
                }
            }
            Msg::ListPages => w.u8(10),
            Msg::Pages(all) => {
                w.u8(11);
                w.u32(all.len() as u32);
                for p in all {
                    w.u128(p.canvas);
                    w.str(&p.name);
                    w.str(&p.edit);
                    w.str(&p.view);
                    w.u64(p.changed);
                }
            }
            Msg::NewPage(name) => {
                w.u8(12);
                w.str(name);
            }
            Msg::RenamePage(c, name) => {
                w.u8(13);
                w.u128(*c);
                w.str(name);
            }
            Msg::DeletePage(c) => {
                w.u8(14);
                w.u128(*c);
            }
        }
        w.0
    }

    pub fn decode(b: &[u8]) -> Result<Msg, String> {
        let mut r = R { b, at: 0 };
        Ok(match r.u8()? {
            1 => Msg::Hello {
                canvas: r.u128()?,
                peer: r.u64()?,
                version: Version::parse(&r.str()?),
                key: r.str()?,
                name: r.str()?,
                proto: r.u16_or_0(),
            },
            2 => Msg::Welcome {
                canvas: r.u128()?,
                host: r.u64()?,
                version: Version::parse(&r.str()?),
                policy: r.u8()?,
                edit: r.u8()? != 0,
                name: r.str()?,
                proto: r.u16_or_0(),
            },
            3 => Msg::Changes(r.bytes()?),
            4 => Msg::Presence {
                peer: r.u64()?,
                name: r.str()?,
                color: r.u32()?,
                cell: r.addr()?,
                off: [r.f64()?, r.f64()?],
                scale: r.f64()?,
            },
            5 => {
                let peer = r.u64()?;
                let color = r.u32()?;
                let width = r.f32()?;
                let cell = r.addr()?;
                let n = r.u32()? as usize;
                if n > (b.len() - r.at) / 8 {
                    return Err("short message".into());
                }
                let mut pts = Vec::with_capacity(n);
                for _ in 0..n {
                    pts.push([r.f32()?, r.f32()?]);
                }
                Msg::Wet {
                    peer,
                    color,
                    width,
                    cell,
                    pts,
                }
            }
            6 => Msg::Gone(r.u64()?),
            7 => Msg::Error(r.str()?),
            8 => {
                let n = r.u32()? as usize;
                if n > (b.len() - r.at) / 8 {
                    return Err("short message".into());
                }
                let mut ids = Vec::with_capacity(n);
                for _ in 0..n {
                    ids.push(r.u64()?);
                }
                Msg::NeedImages(ids)
            }
            9 => {
                let n = r.u32()? as usize;
                if n > (b.len() - r.at) / 12 {
                    return Err("short message".into());
                }
                let mut all = Vec::with_capacity(n);
                for _ in 0..n {
                    all.push((r.u64()?, r.bytes()?));
                }
                Msg::Images(all)
            }
            10 => Msg::ListPages,
            11 => {
                let n = r.u32()? as usize;
                if n > (b.len() - r.at) / 36 {
                    return Err("short message".into());
                }
                let mut all = Vec::with_capacity(n);
                for _ in 0..n {
                    all.push(PageInfo {
                        canvas: r.u128()?,
                        name: r.str()?,
                        edit: r.str()?,
                        view: r.str()?,
                        changed: r.u64()?,
                    });
                }
                Msg::Pages(all)
            }
            12 => Msg::NewPage(r.str()?),
            13 => Msg::RenamePage(r.u128()?, r.str()?),
            14 => Msg::DeletePage(r.u128()?),
            t => return Err(format!("unknown message {t}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogpaper_core::sync::Hlc;

    #[test]
    fn every_message_round_trips() {
        let mut v = Version::default();
        v.0.insert(
            3,
            Hlc {
                ms: 9,
                n: 1,
                peer: 3,
            },
        );
        let deep = CellAddr::new(200, BigInt::from(-5) << 190u32, 7);
        let all = vec![
            Msg::Hello {
                canvas: 1 << 120,
                peer: u64::MAX,
                version: v.clone(),
                key: "k3y".into(),
                name: "Sam ✏".into(),
                proto: PROTOCOL,
            },
            Msg::Welcome {
                canvas: 5,
                host: 6,
                version: v,
                policy: 2,
                edit: true,
                name: "Plan".into(),
                proto: 7,
            },
            Msg::Changes(vec![1, 2, 3]),
            Msg::Presence {
                peer: 4,
                name: "Ana".into(),
                color: 0xff00ffff,
                cell: deep.clone(),
                off: [0.25, 0.5],
                scale: 1.5,
            },
            Msg::Wet {
                peer: 4,
                color: 7,
                width: 0.01,
                cell: deep,
                pts: vec![[0.1, 0.2], [0.3, 0.4]],
            },
            Msg::Gone(4),
            Msg::Error("no".into()),
            Msg::NeedImages(vec![1, u64::MAX]),
            Msg::Images(vec![(7, vec![1, 2, 3]), (8, vec![])]),
            Msg::ListPages,
            Msg::Pages(vec![PageInfo {
                canvas: 9 << 100,
                name: "Plan ✏".into(),
                edit: "eAB".into(),
                view: "vCD".into(),
                changed: 123,
            }]),
            Msg::NewPage("New".into()),
            Msg::RenamePage(5, "R".into()),
            Msg::DeletePage(6),
        ];
        for m in all {
            assert_eq!(Msg::decode(&m.encode()).unwrap(), m);
        }
        assert!(Msg::decode(&[3, 9, 0, 0, 0]).is_err());
        // An older sender (no protocol field) and a newer one (fields
        // added after it) both still read.
        let mut old = Msg::Gone(9).encode();
        old.extend_from_slice(b"future fields");
        assert_eq!(Msg::decode(&old).unwrap(), Msg::Gone(9));
        let hello = Msg::Hello {
            canvas: 1,
            peer: 2,
            version: Version::default(),
            key: "k".into(),
            name: "n".into(),
            proto: 3,
        };
        let b = hello.encode();
        match Msg::decode(&b[..b.len() - 2]).unwrap() {
            Msg::Hello { proto, .. } => assert_eq!(proto, 0),
            _ => panic!(),
        }
        assert!(Msg::decode(&[99]).is_err());
    }
}
