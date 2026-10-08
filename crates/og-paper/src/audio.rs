// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Audio clips: a small player on the canvas (`ObjData::Audio`) whose
//! sound is kept with the pictures (`Objects::images`, by content hash), so
//! it is saved in the page, copied with it and synced like a picture.
//!
//! A clip comes from a command (`{"add": "audio", ...}`, see `script`):
//! with `"data"` (a sound file, base64) it is placed at once; with
//! `"record": true` the app records from the microphone and places it when
//! the person stops (only for a plugin they allowed the microphone, and
//! only where the app can record: the web app for now). Tapping a clip's
//! play button (Select tool) or Play in the selection panel plays it.

use crate::objects::{audio_layout, to_cam, ObjData, ObjRef};
use crate::shapes::Geom;
use crate::App;

/// Longest recording a command may ask for.
pub const MAX_RECORD_MS: u32 = 120_000;
/// Largest sound a clip may hold.
pub const MAX_BYTES: usize = 2 << 20;

/// Where a clip from data goes (home-view points, as commands use), and
/// how long a recording may run (a recording goes in the middle of the
/// screen when it stops).
#[derive(Clone, Copy, Debug)]
pub struct RecRequest {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub max_ms: u32,
}

impl App {
    /// Keep a sound (by its content hash) for clips to play.
    fn audio_keep(&mut self, bytes: Vec<u8>) -> Result<u64, String> {
        if bytes.len() > MAX_BYTES {
            return Err("that sound is too big (2 MB at most)".into());
        }
        if !crate::images::is_audio(&bytes) {
            return Err("not a sound file (WebM, Ogg, MP4/M4A, WAV or MP3)".into());
        }
        let id = crate::images::id_of(&bytes);
        if !self.objs.images.contains_key(&id) {
            let a = crate::images::load(bytes)?;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(f) = &self.file {
                let _ = f.put_image(id, &a.bytes);
            }
            self.objs.images.insert(id, a);
        }
        Ok(id)
    }

    /// A clip's object data at home-view points `x, y` (top left), `w`
    /// by `h`, in the frame of camera `cam` (the home camera).
    fn audio_data(
        &self,
        cam: &ogpaper_core::Camera,
        id: u64,
        dur_ms: u32,
        r: &RecRequest,
    ) -> ObjData {
        let ppc = cam.ppc();
        let (w, h) = (r.w.max(8.0), r.h.max(4.0));
        ObjData::Audio {
            id,
            geom: Geom {
                center: [
                    cam.off[0] + (r.x + w * 0.5) / ppc,
                    cam.off[1] + (r.y + h * 0.5) / ppc,
                ],
                half: [w * 0.5 / ppc, h * 0.5 / ppc],
                rot: 0.0,
                pts: vec![],
            },
            dur_ms,
            seed: crate::uid::new() as u32,
        }
    }

    /// Place a clip from a command (the camera is the home camera then);
    /// its strokes join the command run's undo step.
    pub(crate) fn audio_add_now(
        &mut self,
        bytes: Vec<u8>,
        dur_ms: u32,
        r: &RecRequest,
        added: &mut Vec<u32>,
    ) -> Result<(), String> {
        let id = self.audio_keep(bytes)?;
        let cam = self.cam.clone();
        let data = self.audio_data(&cam, id, dur_ms, r);
        let (_, ids) = self.add_group(&data, None);
        added.extend(ids);
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
        Ok(())
    }

    /// Start recording for a command (needs the microphone allowed).
    pub(crate) fn audio_record_start(&mut self, r: RecRequest) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.audio_rec.is_some() {
                return Err("already recording".into());
            }
            self.audio_rec = Some(r);
            crate::web::audio_record(r.max_ms);
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = r;
            Err("recording works in the web app for now".into())
        }
    }

    /// The recording stopped: place it in the middle of what is on screen
    /// now (a recording can take a while; the person may have moved), as
    /// one undo step, selected.
    pub(crate) fn audio_recorded(&mut self, bytes: Vec<u8>, dur_ms: u32) {
        if self.audio_rec.take().is_none() {
            return;
        }
        if bytes.is_empty() {
            self.say("Nothing was recorded");
            return;
        }
        let id = match self.audio_keep(bytes) {
            Ok(id) => id,
            Err(e) => {
                self.say(format!("Could not keep the recording: {e}"));
                return;
            }
        };
        let s = self.size();
        let w = s[0].min(s[1]) * 0.45;
        let ppc = self.cam.ppc();
        let center = self.px_to_cam([s[0] * 0.5, s[1] * 0.5]);
        self.insert(ObjData::Audio {
            id,
            geom: Geom {
                center,
                half: [w * 0.5 / ppc, w * 0.12 / ppc],
                rot: 0.0,
                pts: vec![],
            },
            dur_ms: dur_ms.min(MAX_RECORD_MS),
            seed: crate::uid::new() as u32,
        });
        #[cfg(target_arch = "wasm32")]
        crate::web::touch();
        self.say(format!("Recorded {}", clock(dur_ms)));
        self.redraw();
    }

    /// The recording was cancelled, or the microphone refused.
    pub(crate) fn audio_record_cancelled(&mut self, why: &str) {
        self.audio_rec = None;
        if !why.is_empty() {
            self.say(why.to_string());
        }
    }

    /// Play clip group `g` (again: stop it).
    pub(crate) fn audio_play(&mut self, g: u32) {
        let Some(ObjData::Audio { id, .. }) = self.objs.groups.get(g as usize).map(|g| &g.data)
        else {
            return;
        };
        let Some(a) = self.objs.images.get(id) else {
            self.say("This clip's sound hasn't arrived yet");
            return;
        };
        #[cfg(target_arch = "wasm32")]
        crate::web::audio_play(*id, a.bytes.to_vec());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = a;
            self.say("Playing clips works in the web app for now");
        }
    }

    /// Play the selected clip (the selection panel's Play).
    pub(crate) fn audio_play_selected(&mut self) {
        let g = self.edit.selection.iter().find_map(|r| match r {
            ObjRef::Group(g)
                if matches!(self.objs.groups[*g as usize].data, ObjData::Audio { .. }) =>
            {
                Some(*g)
            }
            _ => None,
        });
        if let Some(g) = g {
            self.audio_play(g);
        }
    }

    /// A tap at `p` (screen px) on a clip's play button: the clip.
    pub(crate) fn audio_button_at(&self, p: [f64; 2]) -> Option<u32> {
        let q = self.px_to_cam(p);
        self.objs.groups.iter().enumerate().find_map(|(i, grp)| {
            if !matches!(grp.data, ObjData::Audio { .. })
                || grp
                    .strokes
                    .iter()
                    .any(|&s| self.scene.strokes[s as usize].deleted)
            {
                return None;
            }
            let d = to_cam(&grp.cell, &grp.data, &self.cam);
            let g = d.geom();
            let (play, r, _, _) = audio_layout(g);
            let (s, c) = g.rot.sin_cos();
            let at = [
                g.center[0] + play[0] * c - play[1] * s,
                g.center[1] + play[0] * s + play[1] * c,
            ];
            (crate::dist(q, at) <= r * 1.15).then_some(i as u32)
        })
    }
}

/// A length as m:ss.
pub fn clock(ms: u32) -> String {
    let s = (ms + 500) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Decode standard base64 (padding optional, whitespace ignored).
pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = val(c).ok_or("bad base64")?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_clock() {
        assert_eq!(base64_decode("T2dnUw==").unwrap(), b"OggS");
        assert_eq!(base64_decode("T2dn\nUw").unwrap(), b"OggS");
        assert!(base64_decode("@@").is_err());
        assert_eq!(clock(7_400), "0:07");
        assert_eq!(clock(65_000), "1:05");
    }
}
