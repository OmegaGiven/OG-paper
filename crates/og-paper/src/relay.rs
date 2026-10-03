// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! A relay: `og-paper --relay [--port N] [--data DIR]`. It holds the
//! changes of shared canvases for people who are not online at the same
//! time, and passes live frames between those who are. It cannot read any
//! of it: frames are sealed end to end (see `seal`) and filed under a room
//! name derived from the key, which says nothing about the key itself.
//!
//! Frames in (from an app): `ROOM` + 32-byte room (first), then `PUT ` +
//! sealed frame (kept and passed on), `EPH ` + sealed frame (only passed
//! on: views, strokes in progress) or `CKPT` + sealed frame (a full copy:
//! kept instead of everything before it). Out: every kept frame of the
//! room on joining, then `READY`, then others' frames as they come. Kept
//! frames are written to `DIR/<room>.frames` (u32 length + frame each).

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::net::native::Server;
use crate::net::{Ev, READY};

/// The most a room keeps before old frames are dropped (a checkpoint from
/// any editor brings it back down).
const ROOM_MAX: usize = 256 << 20;

#[derive(Default)]
struct Room {
    frames: Vec<Vec<u8>>,
    bytes: usize,
    members: HashSet<u64>,
}

fn room_path(dir: &Path, room: &[u8]) -> PathBuf {
    let hex: String = room.iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{hex}.frames"))
}

fn load(path: &Path) -> Vec<Vec<u8>> {
    let Ok(b) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= b.len() {
        let n = u32::from_le_bytes(b[at..at + 4].try_into().expect("4")) as usize;
        if at + 4 + n > b.len() {
            break;
        }
        out.push(b[at + 4..at + 4 + n].to_vec());
        at += 4 + n;
    }
    out
}

fn append(path: &Path, frame: &[u8]) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(&(frame.len() as u32).to_le_bytes());
        let _ = f.write_all(frame);
    }
}

fn rewrite(path: &Path, frames: &[Vec<u8>]) {
    let tmp = path.with_extension("part");
    if let Ok(mut f) = std::fs::File::create(&tmp) {
        for fr in frames {
            let _ = f.write_all(&(fr.len() as u32).to_le_bytes());
            let _ = f.write_all(fr);
        }
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Run a relay until stopped.
pub fn run(port: u16, dir: PathBuf) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot use {}: {e}", dir.display());
        std::process::exit(1);
    }
    let server = match Server::start(port) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "Relay on port {}, keeping frames in {} (it cannot read them)",
        server.port,
        dir.display()
    );
    println!(
        "Share links through it from Share live > Share through a relay: ws://<this host>:{}/relay",
        server.port
    );
    let mut rooms: HashMap<Vec<u8>, Room> = HashMap::new();
    let mut member: HashMap<u64, Vec<u8>> = HashMap::new();
    loop {
        for ev in server.poll() {
            match ev {
                Ev::Open(_) => {}
                Ev::Closed(id, _) => {
                    if let Some(r) = member.remove(&id) {
                        if let Some(room) = rooms.get_mut(&r) {
                            room.members.remove(&id);
                        }
                    }
                }
                Ev::Data(id, b) => {
                    if b.len() < 4 {
                        continue;
                    }
                    let (tag, body) = b.split_at(4);
                    match (tag, member.get(&id).cloned()) {
                        (b"ROOM", None) if body.len() == 32 => {
                            let room = rooms.entry(body.to_vec()).or_insert_with(|| {
                                let frames = load(&room_path(&dir, body));
                                let bytes = frames.iter().map(Vec::len).sum();
                                Room {
                                    frames,
                                    bytes,
                                    members: HashSet::new(),
                                }
                            });
                            for f in &room.frames {
                                server.send(id, f.clone());
                            }
                            server.send(id, READY.to_vec());
                            room.members.insert(id);
                            member.insert(id, body.to_vec());
                        }
                        (b"PUT " | b"EPH " | b"CKPT", Some(r)) => {
                            let Some(room) = rooms.get_mut(&r) else {
                                continue;
                            };
                            let path = room_path(&dir, &r);
                            match tag {
                                b"PUT " => {
                                    room.frames.push(body.to_vec());
                                    room.bytes += body.len();
                                    append(&path, body);
                                    if room.bytes > ROOM_MAX {
                                        while room.bytes > ROOM_MAX / 2 && room.frames.len() > 1 {
                                            room.bytes -= room.frames.remove(0).len();
                                        }
                                        rewrite(&path, &room.frames);
                                    }
                                }
                                b"CKPT" => {
                                    room.frames = vec![body.to_vec()];
                                    room.bytes = body.len();
                                    rewrite(&path, &room.frames);
                                }
                                _ => {}
                            }
                            for &m in &room.members {
                                if m != id {
                                    server.send(m, body.to_vec());
                                }
                            }
                        }
                        _ => server.close(id),
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
