// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Who else is on a live canvas, and where: each app sends its view (a
//! few times a second while it moves) and the stroke it is drawing (as it
//! is drawn), so others see markers pointing to each person's view and
//! their ink appearing live. Tap a person in Share live to fly to their
//! view, or Follow to ride along with their camera until you move.

use egui::{pos2, vec2, Color32, Pos2};
use ogpaper_core::{Camera, CellAddr};
use web_time::{Duration, Instant};

use crate::wire::Msg;
use crate::App;

/// How often the view is sent while it moves.
const PRESENCE_EVERY: Duration = Duration::from_millis(150);
/// How often a stroke in progress is sent.
const WET_EVERY: Duration = Duration::from_millis(50);
/// A stroke in progress that stops updating is dropped after this.
const WET_STALE: Duration = Duration::from_secs(3);

pub struct Peer {
    pub peer: u64,
    pub name: String,
    pub color: u32,
    pub cam: Camera,
    /// A stroke being drawn: color, width (cell units), cell, points.
    pub wet: Option<(u32, f32, CellAddr, Vec<[f32; 2]>, Instant)>,
}

impl Peer {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn presence_msg(&self) -> Msg {
        Msg::Presence {
            peer: self.peer,
            name: self.name.clone(),
            color: self.color,
            cell: self.cam.cell.clone(),
            off: self.cam.off,
            scale: self.cam.scale,
        }
    }
}

/// What this device sent last, so only changes go out.
#[derive(Default)]
pub struct Sent {
    view: Option<(CellAddr, [f64; 2], f64)>,
    view_at: Option<Instant>,
    wet_len: usize,
    wet_at: Option<Instant>,
}

/// The name shown to others (set in Share live; kept in the prefs).
pub fn my_name() -> String {
    crate::prefs::load()
        .get("name")
        .cloned()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("Guest {:04x}", crate::share::peer_id() & 0xffff))
}

pub fn set_my_name(n: &str) {
    let mut p = crate::prefs::load();
    p.insert("name".into(), n.trim().chars().take(40).collect());
    crate::prefs::save(&p);
}

/// A steady color per person (RGBA8, opaque).
pub fn color_of(peer: u64) -> u32 {
    let hue = (peer.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
    let c = egui::ecolor::Hsva::new(hue, 0.75, 0.85, 1.0).to_srgba_unmultiplied();
    u32::from_le_bytes(c)
}

fn rgba(c: u32) -> Color32 {
    let [r, g, b, a] = c.to_le_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

impl App {
    pub(crate) fn my_presence(&self) -> Option<Msg> {
        self.net.as_ref()?;
        if self.headless {
            return None;
        }
        let peer = self.share.clock.peer();
        Some(Msg::Presence {
            peer,
            name: my_name(),
            color: color_of(peer),
            cell: self.cam.cell.clone(),
            off: self.cam.off,
            scale: self.cam.scale,
        })
    }

    /// Send the view when it moved, and the stroke being drawn.
    pub(crate) fn presence_tick(&mut self) {
        if self.net.is_none() {
            return;
        }
        let now = Instant::now();
        let view = (self.cam.cell.clone(), self.cam.off, self.cam.scale);
        let due = self.sent.view_at.is_none_or(|t| now - t >= PRESENCE_EVERY);
        if due && self.sent.view.as_ref() != Some(&view) {
            if let Some(m) = self.my_presence() {
                self.broadcast(&m, None);
            }
            self.sent.view = Some(view);
            self.sent.view_at = Some(now);
        }
        // The stroke in progress, in the camera cell's units.
        let n = self.wet.len();
        let wet_due = self.sent.wet_at.is_none_or(|t| now - t >= WET_EVERY);
        if n != self.sent.wet_len && (wet_due || n == 0) && !self.view_only {
            let ppc = self.cam.ppc();
            let pts: Vec<[f32; 2]> = self
                .wet
                .iter()
                .rev()
                .take(2000)
                .rev()
                .map(|p| {
                    let q = self.px_to_cam([p[0] as f64, p[1] as f64]);
                    [q[0] as f32, q[1] as f32]
                })
                .collect();
            let ink = self.ui.ink().copied();
            let (color, width) = ink
                .map(|i| {
                    (
                        u32::from_le_bytes(i.rgba()),
                        i.width as f64 * self.ppp() / ppc,
                    )
                })
                .unwrap_or((0xff00_0000, 2.0 / ppc));
            let m = Msg::Wet {
                peer: self.share.clock.peer(),
                color,
                width: width as f32,
                cell: self.cam.cell.clone(),
                pts,
            };
            self.broadcast(&m, None);
            self.sent.wet_len = n;
            self.sent.wet_at = Some(now);
        }
    }

    /// Someone's view or stroke in progress arrived.
    pub(crate) fn peer_msg(&mut self, m: &Msg) {
        match m {
            Msg::Presence {
                peer,
                name,
                color,
                cell,
                off,
                scale,
            } => {
                if *peer == self.share.clock.peer() {
                    return;
                }
                let mut cam = Camera::new(cell.clone(), *off, self.cam.base_px);
                cam.scale = *scale;
                cam.normalize();
                if self.follow == Some(*peer) {
                    self.fly = None;
                    self.cam = cam.clone();
                    self.view_changed();
                }
                let p = self.peers.entry(*peer).or_insert_with(|| Peer {
                    peer: *peer,
                    name: String::new(),
                    color: *color,
                    cam: cam.clone(),
                    wet: None,
                });
                p.name = name.clone();
                p.color = *color;
                p.cam = cam;
            }
            Msg::Wet {
                peer,
                color,
                width,
                cell,
                pts,
            } => {
                if let Some(p) = self.peers.get_mut(peer) {
                    p.wet = (!pts.is_empty())
                        .then(|| (*color, *width, cell.clone(), pts.clone(), Instant::now()));
                }
            }
            _ => {}
        }
        self.redraw();
    }

    /// Fly to someone's view, or ride along with it.
    pub(crate) fn go_to_peer(&mut self, peer: u64, follow: bool) {
        let Some(p) = self.peers.get(&peer) else {
            return;
        };
        let mut cam = p.cam.clone();
        cam.base_px = self.cam.base_px;
        self.fly = Some(cam);
        self.fly_last = Instant::now();
        self.follow = follow.then_some(peer);
        let name = crate::net::display_name(&p.name);
        self.say(if follow {
            format!("Following {name}: move the canvas to stop")
        } else {
            format!("Going to {name}'s view")
        });
    }

    /// Others' strokes in progress, and a marker for each person: at their
    /// view's centre when it is on screen, else at the screen edge pointing
    /// toward it.
    pub(crate) fn presence_overlay(&mut self, ov: &mut crate::ui::Overlay) {
        if self.peers.is_empty() {
            return;
        }
        let ppp = self.ppp();
        let [w, h] = self.size();
        let pt = |q: [f64; 2]| pos2((q[0] / ppp) as f32, (q[1] / ppp) as f32);
        for p in self.peers.values_mut() {
            if p.wet.as_ref().is_some_and(|w| w.4.elapsed() > WET_STALE) {
                p.wet = None;
            }
        }
        for p in self.peers.values() {
            if let Some((color, width, cell, pts, _)) = &p.wet {
                let (o, side) = crate::objects::frame(cell, &self.cam);
                let line: Vec<Pos2> = pts
                    .iter()
                    .map(|q| {
                        pt(self.cam_to_px([o[0] + q[0] as f64 * side, o[1] + q[1] as f64 * side]))
                    })
                    .collect();
                let wpx = (*width as f64 * side * self.cam.ppc() / ppp).max(1.0) as f32;
                ov.lines.push((line, wpx, rgba(*color), false));
            }
            // Their view centre, in our camera's units.
            let o = p.cam.cell.origin_in(&self.cam.cell);
            let side = p.cam.cell.side_in(&self.cam.cell);
            let c = self.cam_to_px([o[0] + p.cam.off[0] * side, o[1] + p.cam.off[1] * side]);
            let col = rgba(p.color);
            let name = crate::net::display_name(&p.name);
            let m = 18.0 * ppp;
            if c.iter().all(|v| v.is_finite())
                && c[0] > m
                && c[0] < w - m
                && c[1] > m
                && c[1] < h - m
            {
                ov.labels.push((pt(c), name, col, None));
            } else {
                // On the edge, toward them.
                let (cx, cy) = (w * 0.5, h * 0.5);
                let d = [c[0] - cx, c[1] - cy];
                let d = if d.iter().all(|v| v.is_finite()) && (d[0] != 0.0 || d[1] != 0.0) {
                    d
                } else {
                    [0.0, -1.0]
                };
                let k = ((cx - m) / d[0].abs().max(1e-9)).min((cy - m) / d[1].abs().max(1e-9));
                let at = [cx + d[0] * k, cy + d[1] * k];
                let dir = vec2(d[0] as f32, d[1] as f32).normalized();
                ov.labels.push((pt(at), name, col, Some(dir)));
            }
        }
    }
}
