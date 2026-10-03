// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Commands from outside the app: automations (the server's JSON API, see
//! `hub`) and plugins (see `plugin`) both hand over the same JSON commands,
//! run here as one undo step that syncs like any other edit.
//!
//! Coordinates are points at the home view (Settings > Bookmarks > Home),
//! measured from its centre: x to the right, y down. Sizes are in the same
//! points. Colors are `#rrggbb` or `#rrggbbaa`.
//!
//! ```json
//! [{"add": "text", "x": 0, "y": 0, "text": "Hello", "size": 24, "color": "#c82850"},
//!  {"add": "stroke", "points": [[0, 40], [80, 60], [160, 40]], "width": 3},
//!  {"add": "shape", "kind": "rect", "x": -100, "y": 100, "w": 200, "h": 80,
//!   "fill": "#ffd166", "neat": true},
//!  {"get": "texts"},
//!  {"say": "Done"}]
//! ```
//!
//! Each command gets a result in order: `{"ok": true}` (and `"texts"` for
//! `get`), or `{"ok": false, "error": "…"}`. Shape kinds: rect, ellipse,
//! diamond, triangle, star, polygon, line, arrow.

use ogpaper_core::{Brush, Camera, Dash, Point, Scene, Style};
use serde_json::{json, Value};

use crate::objects::{self, ObjData, TextStyle};
use crate::shapes::{FillStyle, Geom, ShapeKind, ShapeStyle, Sloppiness};
use crate::App;

/// Most commands one run takes.
const MAX_COMMANDS: usize = 10_000;
/// Most points one stroke takes.
const MAX_POINTS: usize = 100_000;

/// `#rrggbb` or `#rrggbbaa` as the scene's packed RGBA.
pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let (r, g, b) = (byte(0)?, byte(2)?, byte(4)?);
    let a = if h.len() >= 8 { byte(6)? } else { 255 };
    (h.len() == 6 || h.len() == 8).then(|| u32::from_le_bytes([r, g, b, a]))
}

fn kind_of(s: &str) -> Option<ShapeKind> {
    Some(match s {
        "rect" | "rectangle" => ShapeKind::Rect,
        "ellipse" | "circle" => ShapeKind::Ellipse,
        "diamond" => ShapeKind::Diamond,
        "triangle" => ShapeKind::Triangle,
        "star" => ShapeKind::Star,
        "polygon" => ShapeKind::Polygon,
        "line" => ShapeKind::Line,
        "arrow" => ShapeKind::Arrow,
        _ => return None,
    })
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64().filter(|x| x.is_finite())
}

/// The home view: the frame commands are placed in.
fn home() -> Camera {
    crate::home_camera()
}

/// A point in home-view points, in the home camera's cell units.
fn to_cam(cam: &Camera, x: f64, y: f64) -> [f64; 2] {
    let ppc = cam.ppc();
    [cam.off[0] + x / ppc, cam.off[1] + y / ppc]
}

impl App {
    /// Run commands (a JSON array, or one command); the results, in order.
    pub(crate) fn run_commands(&mut self, cmds: &Value) -> Value {
        let list: Vec<Value> = match cmds {
            Value::Array(a) => a.iter().take(MAX_COMMANDS).cloned().collect(),
            v => vec![v.clone()],
        };
        // Everything is placed relative to the home view.
        let cam = home();
        let saved = std::mem::replace(&mut self.cam, cam.clone());
        let mut added = Vec::new();
        let results: Vec<Value> = list
            .iter()
            .map(|c| match self.command(c, &cam, &mut added) {
                Ok(extra) => {
                    let mut r = json!({"ok": true});
                    if let (Some(o), Value::Object(e)) = (r.as_object_mut(), extra) {
                        o.extend(e);
                    }
                    r
                }
                Err(e) => json!({"ok": false, "error": e}),
            })
            .collect();
        self.cam = saved;
        if !added.is_empty() {
            self.record_edit(vec![], added);
        }
        Value::Array(results)
    }

    fn command(&mut self, c: &Value, cam: &Camera, added: &mut Vec<u32>) -> Result<Value, String> {
        if let Some(msg) = c.get("say").and_then(Value::as_str) {
            self.say(msg.chars().take(200).collect::<String>());
            return Ok(Value::Null);
        }
        if let Some(what) = c.get("get").and_then(Value::as_str) {
            return match what {
                "texts" => Ok(json!({ "texts": self.texts(cam) })),
                _ => Err(format!("unknown get: {what}")),
            };
        }
        let what = c
            .get("add")
            .and_then(Value::as_str)
            .ok_or("a command needs \"add\", \"get\" or \"say\"")?;
        if self.view_only {
            return Err("this copy can only view".into());
        }
        let color = c
            .get("color")
            .and_then(Value::as_str)
            .map(|s| parse_color(s).ok_or("bad color (use #rrggbb)"))
            .transpose()?;
        let ppc = cam.ppc();
        let z = self.scene.z_top + 1.0;
        match what {
            "text" => {
                let text: String = c
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("text needs \"text\"")?
                    .chars()
                    .take(10_000)
                    .collect();
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                let size = num(c, "size").unwrap_or(24.0).clamp(1.0, 10_000.0) / ppc;
                let style = TextStyle {
                    color: color.unwrap_or(TextStyle::default().color),
                    ..TextStyle::default()
                };
                let b = objects::text_box(&text, &style, size);
                let half = [b[0] * 0.5, b[1] * 0.5];
                let at = to_cam(cam, x, y);
                let data = ObjData::Text {
                    text,
                    style,
                    geom: Geom {
                        center: [at[0] + half[0], at[1] + half[1]],
                        half,
                        rot: 0.0,
                        pts: vec![],
                    },
                    size,
                    seed: crate::uid::new() as u32,
                };
                let (_, ids) = self.add_group(&data, None);
                added.extend(ids);
            }
            "shape" => {
                let kind = c
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(|k| kind_of(k).ok_or(format!("unknown shape kind: {k}")))
                    .transpose()?
                    .unwrap_or(ShapeKind::Rect);
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                let (w, h) = (num(c, "w").unwrap_or(100.0), num(c, "h").unwrap_or(100.0));
                let a = to_cam(cam, x, y);
                let b = to_cam(cam, x + w, y + h);
                let mut style = ShapeStyle {
                    kind,
                    ..ShapeStyle::default()
                };
                if let Some(col) = color {
                    style.stroke = col;
                }
                if let Some(f) = c.get("fill").and_then(Value::as_str) {
                    style.fill = parse_color(f).ok_or("bad fill (use #rrggbb)")?;
                    style.fill_style = FillStyle::Solid;
                }
                style.sloppiness = if c.get("neat").and_then(Value::as_bool).unwrap_or(true) {
                    Sloppiness::Architect
                } else {
                    Sloppiness::Artist
                };
                let linear = matches!(kind, ShapeKind::Line | ShapeKind::Arrow);
                let geom = Geom {
                    center: [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5],
                    half: [((b[0] - a[0]) * 0.5).abs(), ((b[1] - a[1]) * 0.5).abs()],
                    rot: 0.0,
                    pts: if linear { vec![a, b] } else { vec![] },
                };
                let data = ObjData::Shape {
                    style,
                    geom,
                    width: num(c, "width").unwrap_or(3.0).clamp(0.1, 1000.0) / ppc,
                    seed: crate::uid::new() as u32,
                };
                let (_, ids) = self.add_group(&data, None);
                added.extend(ids);
            }
            "stroke" => {
                let pts: Vec<[f64; 2]> = c
                    .get("points")
                    .and_then(Value::as_array)
                    .ok_or("stroke needs \"points\": [[x, y], …]")?
                    .iter()
                    .take(MAX_POINTS)
                    .filter_map(|p| {
                        let a = p.as_array()?;
                        Some(to_cam(cam, a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                    })
                    .collect();
                if pts.is_empty() {
                    return Err("stroke needs at least one point".into());
                }
                let width = num(c, "width").unwrap_or(3.0).clamp(0.1, 1000.0) / ppc;
                let (anchor, local, side) = Scene::anchor_for_min(&cam.cell, &pts, width);
                let pts: Vec<Point> = local.iter().map(|q| [q[0], q[1], 1.0, 0.0]).collect();
                let style = Style {
                    width: (width / side) as f32,
                    color: color.unwrap_or(0xff24_1c1c),
                    brush: Brush::Pen,
                    dash: Dash::Solid,
                    ext: None,
                };
                let id = self
                    .scene
                    .add_stroke_at(&anchor, &pts, style, crate::uid::new(), z);
                added.push(id);
            }
            other => return Err(format!("unknown add: {other}")),
        }
        Ok(Value::Null)
    }

    /// Every text on the page, with where it is (home-view points).
    fn texts(&self, cam: &Camera) -> Vec<Value> {
        let ppc = cam.ppc();
        self.objs
            .groups
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                self.objs
                    .alive(&self.scene, &objects::ObjRef::Group(*i as u32))
            })
            .filter_map(|(_, g)| {
                let ObjData::Text {
                    text, geom, size, ..
                } = objects::to_cam(&g.cell, &g.data, cam)
                else {
                    return None;
                };
                let x = (geom.center[0] - geom.half[0] - cam.off[0]) * ppc;
                let y = (geom.center[1] - geom.half[1] - cam.off[1]) * ppc;
                Some(json!({"text": text, "x": x, "y": y, "size": size * ppc}))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse() {
        assert_eq!(
            parse_color("#ff0000"),
            Some(u32::from_le_bytes([255, 0, 0, 255]))
        );
        assert_eq!(
            parse_color("00ff0080"),
            Some(u32::from_le_bytes([0, 255, 0, 128]))
        );
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
    }
}
