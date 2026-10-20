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
//! diamond, triangle, star, polygon, line, arrow, and path (through
//! "points", with "smooth" and "closed").
//!
//! Things on the page have ids (`add` answers with the new thing's; `get:
//! objects` lists them): hex, the low 64 bits of the thing's first stroke's
//! permanent id, so they hold across reloads and devices. Editing a thing
//! makes a new one (see `objects`), so `move`, `set` and `order` answer
//! with the new ids. Each edit command is its own undo step; the adds of a
//! run are one.
//!
//! ```json
//! [{"get": "objects", "x": -500, "y": -300, "w": 1000, "h": 600},
//!  {"get": "picture", "px": 1024},
//!  {"move": ["0a1b…"], "dx": 40, "dy": 0},
//!  {"set": "0a1b…", "text": "New words", "color": "#1e5ac8", "w": 300},
//!  {"order": ["0a1b…"], "to": "back"},
//!  {"delete": ["0a1b…"]}]
//! ```

use std::collections::HashSet;

use ogpaper_core::{Brush, Camera, Dash, Point, Scene, Style};
use serde_json::{json, Value};

use crate::font::{self, Align, FontId};
use crate::objects::{self, ObjData, ObjRef, Op, TextStyle};
use crate::shapes::{FillStyle, Geom, ShapeKind, ShapeStyle, Sloppiness};
use crate::App;

/// Most commands one run takes.
const MAX_COMMANDS: usize = 10_000;
/// Most points one stroke takes.
const MAX_POINTS: usize = 100_000;
/// How many things `get: objects` lists, unless asked for more (up to
/// `LIST_MAX`).
const LIST_DEFAULT: usize = 500;
const LIST_MAX: usize = 5000;
/// Longest side (px) of a `get: picture`, unless asked (up to
/// `PICTURE_MAX`).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const PICTURE_PX: f64 = 1024.0;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const PICTURE_MAX: f64 = 2048.0;
/// Commands that change things already on the page.
const EDITS: [&str; 9] = [
    "component",
    "move",
    "set",
    "delete",
    "order",
    "combine",
    "group",
    "ungroup",
    "remove_background",
];

/// `#rrggbb` or `#rrggbbaa` as the scene's packed RGBA.
pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    let (r, g, b) = (byte(0)?, byte(2)?, byte(4)?);
    let a = if h.len() >= 8 { byte(6)? } else { 255 };
    (h.len() == 6 || h.len() == 8).then(|| u32::from_le_bytes([r, g, b, a]))
}

/// Packed RGBA as `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn color_hex(c: u32) -> String {
    let [r, g, b, a] = c.to_le_bytes();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// Whether a command changes the page (a view-only key may not send it).
#[cfg_attr(not(desktop), allow(dead_code))]
pub fn changes_page(c: &Value) -> bool {
    c.get("add").is_some() || EDITS.iter().any(|k| c.get(*k).is_some())
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
        "path" | "pen" | "curve" => ShapeKind::Path,
        _ => return None,
    })
}

fn kind_name(k: ShapeKind) -> &'static str {
    match k {
        ShapeKind::Rect => "rect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Diamond => "diamond",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Star => "star",
        ShapeKind::Polygon => "polygon",
        ShapeKind::Line => "line",
        ShapeKind::Arrow => "arrow",
        ShapeKind::Path => "path",
        ShapeKind::Frame => "frame",
        ShapeKind::Pin => "pin",
    }
}

fn dash_of(s: &str) -> Option<Dash> {
    Some(match s {
        "solid" => Dash::Solid,
        "dashed" => Dash::Dashed,
        "dotted" => Dash::Dotted,
        _ => return None,
    })
}

fn dash_name(d: Dash) -> &'static str {
    match d {
        Dash::Solid => "solid",
        Dash::Dashed => "dashed",
        Dash::Dotted => "dotted",
    }
}

fn align_of(s: &str) -> Option<Align> {
    Some(match s {
        "left" => Align::Left,
        "center" | "centre" => Align::Center,
        "right" => Align::Right,
        _ => return None,
    })
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn brush_name(b: Brush) -> &'static str {
    match b {
        Brush::Pen => "pen",
        Brush::Marker => "marker",
        Brush::Highlighter => "highlighter",
        Brush::Fill => "fill",
        Brush::Dabs => "brush",
    }
}

/// A font on this device, by name (any case).
fn font_of(name: &str) -> Result<FontId, String> {
    font::list()
        .into_iter()
        .find(|f| f.available && f.name.eq_ignore_ascii_case(name.trim()))
        .map(|f| f.id)
        .ok_or(format!(
            "no font called {name} here (get: fonts lists them)"
        ))
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64().filter(|x| x.is_finite())
}

fn color_at(c: &Value, k: &str) -> Result<Option<u32>, String> {
    c.get(k)
        .and_then(Value::as_str)
        .map(|s| parse_color(s).ok_or(format!("bad {k} (use #rrggbb)")))
        .transpose()
}

/// `x`, `y`, `w`, `h` of a command, when it gives all four.
fn rect_of(c: &Value) -> Option<[f64; 4]> {
    let r = [num(c, "x")?, num(c, "y")?, num(c, "w")?, num(c, "h")?];
    (r[2] > 0.0 && r[3] > 0.0).then_some(r)
}

/// The ids a command names: one, or a list.
fn ids_of(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// The box around both (x, y, w, h).
fn union(a: Option<[f64; 4]>, b: [f64; 4]) -> [f64; 4] {
    let Some(a) = a else { return b };
    let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
    let (x1, y1) = (
        (a[0] + a[2]).max(b[0] + b[2]),
        (a[1] + a[3]).max(b[1] + b[3]),
    );
    [x0, y0, x1 - x0, y1 - y0]
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[0] + b[2] && b[0] <= a[0] + a[2] && a[1] <= b[1] + b[3] && b[1] <= a[1] + a[3]
}

/// Five significant digits: enough to place things, short to read.
fn tidy(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return 0.0;
    }
    let m = 10f64.powi(4 - v.abs().log10().floor() as i32);
    if m.is_finite() && m > 0.0 {
        (v * m).round() / m
    } else {
        v
    }
}

fn rect_json(r: [f64; 4]) -> Value {
    json!({"x": tidy(r[0]), "y": tidy(r[1]), "w": tidy(r[2]), "h": tidy(r[3])})
}

/// Camera units as home-view points.
fn to_home(cam: &Camera, q: [f64; 2]) -> [f64; 2] {
    let ppc = cam.ppc();
    [(q[0] - cam.off[0]) * ppc, (q[1] - cam.off[1]) * ppc]
}

/// What a `set` changes besides where a thing is, read up front so a bad
/// value fails before anything changes. Sizes in camera units.
#[derive(Default)]
struct Patch {
    color: Option<u32>,
    /// `Some(None)`: no fill.
    fill: Option<Option<u32>>,
    width: Option<f64>,
    text: Option<String>,
    size: Option<f64>,
    font: Option<FontId>,
    align: Option<Align>,
    dash: Option<Dash>,
    round: Option<bool>,
    opacity: Option<u8>,
    kind: Option<ShapeKind>,
    cells: Option<Vec<Vec<String>>>,
    /// A gradient fill: end color, angle (degrees), radial.
    gradient: Option<(u32, f32, bool)>,
    /// A frame's name.
    name: Option<String>,
    letter: Option<f32>,
    line: Option<f32>,
    /// A picture's colors.
    brightness: Option<f32>,
    contrast: Option<f32>,
    saturation: Option<f32>,
}

/// `"gradient": {"to": "#rrggbb", "angle": 90, "radial": false}`.
fn gradient_of(c: &Value) -> Result<Option<(u32, f32, bool)>, String> {
    let Some(g) = c.get("gradient").filter(|g| !g.is_null()) else {
        return Ok(None);
    };
    let to = g
        .get("to")
        .and_then(Value::as_str)
        .and_then(parse_color)
        .ok_or("gradient needs \"to\": \"#rrggbb\"")?;
    let angle = g.get("angle").and_then(Value::as_f64).unwrap_or(90.0) as f32;
    let radial = g.get("radial").and_then(Value::as_bool).unwrap_or(false);
    Ok(Some((to, angle, radial)))
}

fn put_gradient(st: &mut ShapeStyle, (to, angle, radial): (u32, f32, bool)) {
    st.fill2 = to;
    st.grad = angle.rem_euclid(360.0);
    st.fill_style = if radial {
        FillStyle::Radial
    } else {
        FillStyle::Linear
    };
}

impl Patch {
    fn parse(c: &Value, ppc: f64) -> Result<Self, String> {
        let s = |k: &str| c.get(k).and_then(Value::as_str);
        Ok(Patch {
            color: color_at(c, "color")?,
            fill: match s("fill") {
                Some("none") => Some(None),
                Some(f) => Some(Some(
                    parse_color(f).ok_or("bad fill (use #rrggbb or none)")?,
                )),
                None => None,
            },
            width: num(c, "width").map(|w| w.clamp(1e-200, 1e200) / ppc),
            text: s("text").map(|t| t.chars().take(10_000).collect()),
            size: num(c, "size").map(|v| v.clamp(1e-200, 1e200) / ppc),
            font: s("font").map(font_of).transpose()?,
            align: s("align")
                .map(|a| align_of(a).ok_or(format!("unknown align: {a}")))
                .transpose()?,
            dash: s("dash")
                .map(|d| dash_of(d).ok_or(format!("unknown dash: {d}")))
                .transpose()?,
            round: c.get("round").and_then(Value::as_bool),
            opacity: num(c, "opacity").map(|o| (o.clamp(0.0, 1.0) * 255.0).round() as u8),
            kind: s("kind")
                .map(|k| kind_of(k).ok_or(format!("unknown shape kind: {k}")))
                .transpose()?,
            cells: c.get("cells").map(cells_of).transpose()?,
            gradient: gradient_of(c)?,
            name: s("name").map(|t| t.chars().take(200).collect()),
            letter: num(c, "letter_spacing").map(|v| v.clamp(-0.5, 5.0) as f32),
            line: num(c, "line_height").map(|v| v.clamp(0.3, 5.0) as f32),
            brightness: num(c, "brightness").map(|v| v.clamp(-1.0, 1.0) as f32),
            contrast: num(c, "contrast").map(|v| v.clamp(0.0, 3.0) as f32),
            saturation: num(c, "saturation").map(|v| v.clamp(0.0, 3.0) as f32),
        })
    }

    fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.fill.is_none()
            && self.width.is_none()
            && self.text.is_none()
            && self.size.is_none()
            && self.font.is_none()
            && self.align.is_none()
            && self.dash.is_none()
            && self.round.is_none()
            && self.opacity.is_none()
            && self.kind.is_none()
            && self.cells.is_none()
            && self.gradient.is_none()
            && self.name.is_none()
            && self.letter.is_none()
            && self.line.is_none()
            && self.brightness.is_none()
            && self.contrast.is_none()
            && self.saturation.is_none()
    }

    /// `d` (camera units) with the patch applied. Texts and tables keep
    /// their top left corner when their size changes.
    fn apply(&self, d: ObjData) -> ObjData {
        let regrow = |geom: &mut Geom, b: [f64; 2]| {
            let tl = [geom.center[0] - geom.half[0], geom.center[1] - geom.half[1]];
            geom.half = [b[0] * 0.5, b[1] * 0.5];
            geom.center = [tl[0] + geom.half[0], tl[1] + geom.half[1]];
        };
        let restyle_text = |st: &mut TextStyle| {
            st.color = self.color.unwrap_or(st.color);
            st.font = self.font.unwrap_or(st.font);
            st.align = self.align.unwrap_or(st.align);
            st.opacity = self.opacity.unwrap_or(st.opacity);
            st.letter = self.letter.unwrap_or(st.letter);
            st.line = self.line.unwrap_or(st.line);
        };
        let restyle_shape = |st: &mut ShapeStyle| {
            st.stroke = self.color.unwrap_or(st.stroke);
            match self.fill {
                Some(Some(f)) => {
                    st.fill = f;
                    st.fill_style = FillStyle::Solid;
                }
                Some(None) => st.fill_style = FillStyle::None,
                None => {}
            }
            st.dash = self.dash.unwrap_or(st.dash);
            st.round = self.round.unwrap_or(st.round);
            st.opacity = self.opacity.unwrap_or(st.opacity);
            if let Some(g) = self.gradient {
                put_gradient(st, g);
            }
        };
        match d {
            ObjData::Text {
                mut text,
                mut style,
                mut geom,
                mut size,
                seed,
            } => {
                text = self.text.clone().unwrap_or(text);
                size = self.size.unwrap_or(size);
                restyle_text(&mut style);
                if geom.pts.len() < 2 {
                    regrow(&mut geom, objects::text_box(&text, &style, size));
                }
                ObjData::Text {
                    text,
                    style,
                    geom,
                    size,
                    seed,
                }
            }
            ObjData::Table {
                mut cells,
                mut style,
                mut geom,
                mut size,
                seed,
            } => {
                cells = self.cells.clone().unwrap_or(cells);
                size = self.size.unwrap_or(size);
                restyle_text(&mut style);
                regrow(
                    &mut geom,
                    objects::table_layout(&cells, &style, size).size(),
                );
                ObjData::Table {
                    cells,
                    style,
                    geom,
                    size,
                    seed,
                }
            }
            ObjData::Shape {
                mut style,
                geom,
                width,
                seed,
            } => {
                restyle_shape(&mut style);
                // Lines and arrows keep their points: no switching to or
                // from a box shape.
                if let Some(k) = self.kind.filter(|k| k.same_family(style.kind)) {
                    style.kind = k;
                }
                ObjData::Shape {
                    style,
                    geom,
                    width: self.width.unwrap_or(width),
                    seed,
                }
            }
            ObjData::Portal {
                mut style,
                geom,
                width,
                seed,
                view,
            } => {
                restyle_shape(&mut style);
                ObjData::Portal {
                    style,
                    geom,
                    width: self.width.unwrap_or(width),
                    seed,
                    view,
                }
            }
            ObjData::Image {
                id,
                geom,
                opacity,
                crop,
                adjust,
            } => ObjData::Image {
                id,
                geom,
                opacity: self.opacity.unwrap_or(opacity),
                crop,
                adjust: [
                    self.brightness.unwrap_or(adjust[0]),
                    self.contrast.unwrap_or(adjust[1]),
                    self.saturation.unwrap_or(adjust[2]),
                ],
            },
            d @ ObjData::LayerLook { .. } => d,
            ObjData::Frame { geom, name } => ObjData::Frame {
                geom,
                name: self.name.clone().unwrap_or(name),
            },
            d @ (ObjData::Audio { .. } | ObjData::Bundle { .. }) => d,
        }
    }
}

/// A table's rows: `[["a", "b"], ["1", "2"]]` (numbers are written out).
fn cells_of(v: &Value) -> Result<Vec<Vec<String>>, String> {
    let rows = v
        .as_array()
        .ok_or("cells needs rows: [[\"a\", \"b\"], …]")?;
    let cells: Vec<Vec<String>> = rows
        .iter()
        .take(500)
        .map(|r| {
            r.as_array()
                .map(|r| {
                    r.iter()
                        .take(100)
                        .map(|c| match c {
                            Value::String(s) => s.chars().take(1000).collect(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    if cells.iter().all(Vec::is_empty) {
        return Err("a table needs at least one cell".into());
    }
    Ok(cells)
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
        // Everything is placed relative to the home view. Edits work on
        // the selection, so the person's own is put back after.
        let cam = home();
        let saved = std::mem::replace(&mut self.cam, cam.clone());
        let saved_sel = std::mem::take(&mut self.edit.selection);
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
        self.edit.selection = saved_sel;
        self.sel_alive();
        Value::Array(results)
    }

    fn command(&mut self, c: &Value, cam: &Camera, added: &mut Vec<u32>) -> Result<Value, String> {
        if let Some(msg) = c.get("say").and_then(Value::as_str) {
            self.say(msg.chars().take(200).collect::<String>());
            return Ok(Value::Null);
        }
        if let Some(what) = c.get("get").and_then(Value::as_str) {
            return match what {
                "texts" => Ok(json!({ "texts": self.texts_at(cam) })),
                "objects" => Ok(self.list_objects(c, cam)),
                "fonts" => Ok(json!({ "fonts": font::list()
                    .into_iter()
                    .filter(|f| f.available)
                    .map(|f| json!({"name": f.name, "category": f.category}))
                    .collect::<Vec<_>>() })),
                "picture" => self.picture(c, cam),
                _ => Err(format!("unknown get: {what}")),
            };
        }
        if let Some(verb) = EDITS.into_iter().find(|v| c.get(*v).is_some()) {
            if self.view_only {
                return Err("this copy can only view".into());
            }
            // What this run added so far is stored first, so it can be
            // edited like anything else.
            if !added.is_empty() {
                self.record_edit(vec![], std::mem::take(added));
            }
            return self.edit_command(verb, c, cam);
        }
        let what = c.get("add").and_then(Value::as_str).ok_or(
            "a command needs \"add\", \"get\", \"move\", \"set\", \"order\", \"delete\" or \"say\"",
        )?;
        if self.view_only {
            return Err("this copy can only view".into());
        }
        let color = color_at(c, "color")?;
        let ppc = cam.ppc();
        let z = self.scene.top() + 1.0;
        let s = |k: &str| c.get(k).and_then(Value::as_str);
        let text_style = |color: Option<u32>| -> Result<TextStyle, String> {
            let d = TextStyle::default();
            Ok(TextStyle {
                color: color.unwrap_or(d.color),
                font: s("font").map(font_of).transpose()?.unwrap_or(d.font),
                align: s("align")
                    .map(|a| align_of(a).ok_or(format!("unknown align: {a}")))
                    .transpose()?
                    .unwrap_or(d.align),
                ..d
            })
        };
        let made = match what {
            "text" => {
                let text: String = c
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("text needs \"text\"")?
                    .chars()
                    .take(10_000)
                    .collect();
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                let size = num(c, "size").unwrap_or(24.0).clamp(1e-200, 1e200) / ppc;
                let mut style = text_style(color)?;
                if let Some(v) = num(c, "letter_spacing") {
                    style.letter = v.clamp(-0.5, 5.0) as f32;
                }
                if let Some(v) = num(c, "line_height") {
                    style.line = v.clamp(0.3, 5.0) as f32;
                }
                let b = objects::text_box(&text, &style, size);
                let half = [b[0] * 0.5, b[1] * 0.5];
                let at = to_cam(cam, x, y);
                // Along "path": [[x, y], …] (straight between the points,
                // or "smooth").
                let path: Vec<[f64; 2]> = c
                    .get("path")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .take(MAX_POINTS)
                            .filter_map(|p| {
                                let a = p.as_array()?;
                                Some(to_cam(cam, a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let geom = if path.len() >= 2 {
                    let smooth = c.get("smooth").and_then(Value::as_bool).unwrap_or(false);
                    let pts = if smooth {
                        let p = crate::shapes::path_through(&path, true, false);
                        crate::shapes::subpaths(&p)
                            .into_iter()
                            .next()
                            .map(|q| q.0)
                            .unwrap_or(path)
                    } else {
                        path
                    };
                    let (center, half) = crate::shapes::bounds_of(&pts);
                    Geom {
                        center,
                        half,
                        rot: 0.0,
                        pts,
                    }
                } else {
                    Geom {
                        center: [at[0] + half[0], at[1] + half[1]],
                        half,
                        rot: 0.0,
                        pts: vec![],
                    }
                };
                let data = ObjData::Text {
                    text,
                    style,
                    geom,
                    size,
                    seed: crate::uid::new() as u32,
                };
                let (g, ids) = self.add_group(&data, None);
                added.extend(ids);
                Some(ObjRef::Group(g))
            }
            "table" => {
                let cells = cells_of(c.get("cells").ok_or("table needs \"cells\"")?)?;
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                let size = num(c, "size").unwrap_or(18.0).clamp(1e-200, 1e200) / ppc;
                let style = text_style(color)?;
                let b = objects::table_layout(&cells, &style, size).size();
                let half = [b[0] * 0.5, b[1] * 0.5];
                let at = to_cam(cam, x, y);
                let data = ObjData::Table {
                    cells,
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
                let (g, ids) = self.add_group(&data, None);
                added.extend(ids);
                Some(ObjRef::Group(g))
            }
            "symbol" => {
                let name = s("name").ok_or("symbol needs \"name\" (see the symbols list)")?;
                let sym = crate::symbols::find(name).ok_or_else(|| {
                    format!(
                        "no symbol {name}: try {}",
                        crate::symbols::SYMBOLS
                            .iter()
                            .map(|s| s.key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                // "size": points per grid unit (10 by default).
                let unit = num(c, "size").unwrap_or(10.0).clamp(1e-200, 1e200);
                let (lo, hi) = crate::symbols::bounds(&crate::symbols::parse(sym.spec));
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                // Placed by its top left corner, or centred on "cx", "cy".
                let center = match (num(c, "cx"), num(c, "cy")) {
                    (Some(cx), Some(cy)) => to_cam(cam, cx, cy),
                    _ => to_cam(
                        cam,
                        x + (hi[0] - lo[0]) * unit * 0.5,
                        y + (hi[1] - lo[1]) * unit * 0.5,
                    ),
                };
                let (b, ids) = self
                    .add_symbol(sym, center, unit / ppc, color.unwrap_or(0xff24_1c1c))
                    .ok_or("could not draw that symbol")?;
                added.extend(ids);
                Some(b)
            }
            "shape" => {
                let kind = s("kind")
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
                if let Some(f) = s("fill").filter(|f| *f != "none") {
                    style.fill = parse_color(f).ok_or("bad fill (use #rrggbb)")?;
                    style.fill_style = FillStyle::Solid;
                }
                if let Some(d) = s("dash") {
                    style.dash = dash_of(d).ok_or(format!("unknown dash: {d}"))?;
                }
                if let Some(g) = gradient_of(c)? {
                    put_gradient(&mut style, g);
                }
                style.round = c.get("round").and_then(Value::as_bool).unwrap_or(false);
                if let Some(o) = num(c, "opacity") {
                    style.opacity = (o.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                style.sloppiness = if c.get("neat").and_then(Value::as_bool).unwrap_or(true) {
                    Sloppiness::Architect
                } else {
                    Sloppiness::Artist
                };
                let linear = matches!(kind, ShapeKind::Line | ShapeKind::Arrow);
                let mut geom = Geom {
                    center: [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5],
                    half: [((b[0] - a[0]) * 0.5).abs(), ((b[1] - a[1]) * 0.5).abs()],
                    rot: 0.0,
                    pts: if linear { vec![a, b] } else { vec![] },
                };
                // A line or arrow through "points" (bends at each), not x..x+w.
                if let (true, Some(list)) = (linear, c.get("points").and_then(Value::as_array)) {
                    let pts: Vec<[f64; 2]> = list
                        .iter()
                        .take(MAX_POINTS)
                        .filter_map(|p| {
                            let a = p.as_array()?;
                            Some(to_cam(cam, a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                        })
                        .collect();
                    if pts.len() >= 2 {
                        (geom.center, geom.half) = crate::shapes::bounds_of(&pts);
                        geom.pts = pts;
                    }
                }
                if kind == ShapeKind::Path {
                    // Through "points", straight or "smooth", maybe "closed".
                    let anchors: Vec<[f64; 2]> = c
                        .get("points")
                        .and_then(Value::as_array)
                        .ok_or("a path needs \"points\": [[x, y], …]")?
                        .iter()
                        .take(MAX_POINTS)
                        .filter_map(|p| {
                            let a = p.as_array()?;
                            Some(to_cam(cam, a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                        })
                        .collect();
                    if anchors.len() < 2 {
                        return Err("a path needs two or more points".into());
                    }
                    let flag = |k: &str| c.get(k).and_then(Value::as_bool).unwrap_or(false);
                    geom.pts =
                        crate::shapes::path_through(&anchors, flag("smooth"), flag("closed"));
                    (geom.center, geom.half) = crate::shapes::bounds_of(&geom.pts);
                }
                let data = ObjData::Shape {
                    style,
                    geom,
                    width: num(c, "width").unwrap_or(3.0).clamp(1e-200, 1e200) / ppc,
                    seed: crate::uid::new() as u32,
                };
                let (g, ids) = self.add_group(&data, None);
                added.extend(ids);
                Some(ObjRef::Group(g))
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
                let width = num(c, "width").unwrap_or(3.0).clamp(1e-200, 1e200) / ppc;
                let (anchor, local, side) = Scene::anchor_for_min(&cam.cell, &pts, width);
                let pts: Vec<Point> = local.iter().map(|q| [q[0], q[1], 1.0, 0.0]).collect();
                let style = Style {
                    width: (width / side) as f32,
                    color: color.unwrap_or(0xff24_1c1c),
                    brush: Brush::Pen,
                    dash: s("dash").and_then(dash_of).unwrap_or(Dash::Solid),
                    ext: None,
                };
                let id = self
                    .scene
                    .add_stroke_at(&anchor, &pts, style, crate::uid::new(), z);
                added.push(id);
                Some(ObjRef::Ink(id))
            }
            "audio" => {
                let (x, y) = (num(c, "x").unwrap_or(0.0), num(c, "y").unwrap_or(0.0));
                let w = num(c, "w").unwrap_or(220.0).clamp(8.0, 100_000.0);
                let h = num(c, "h").unwrap_or(w * 0.24).clamp(4.0, 100_000.0);
                let rec = crate::audio::RecRequest {
                    x,
                    y,
                    w,
                    h,
                    max_ms: (num(c, "max_seconds").unwrap_or(30.0).clamp(1.0, 120.0) * 1000.0)
                        as u32,
                };
                if c.get("record").and_then(Value::as_bool) == Some(true) {
                    // The microphone: only for a plugin the person allowed it.
                    if !self.cmd_perms.iter().any(|p| p == "microphone") {
                        return Err(
                            "recording needs the microphone: allow it for this plugin in Plugins"
                                .into(),
                        );
                    }
                    self.audio_record_start(rec)?;
                    return Ok(json!({ "recording": true }));
                }
                let data = c
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or("audio needs \"data\" (a sound file, base64) or \"record\": true")?;
                if data.len() > crate::audio::MAX_BYTES * 4 / 3 + 8 {
                    return Err("that sound is too big (2 MB at most)".into());
                }
                let bytes = crate::audio::base64_decode(data)?;
                let dur = num(c, "duration_ms").unwrap_or(0.0).clamp(0.0, 3_600_000.0) as u32;
                let before = added.len();
                self.audio_add_now(bytes, dur, &rec, added)?;
                added.get(before).map(|&s| self.objs.obj_of(s))
            }
            other => return Err(format!("unknown add: {other}")),
        };
        // The new thing's id and box, so a layout can be built on it.
        Ok(made.map_or(Value::Null, |r| {
            let mut o = self.obj_box(&r, cam).map_or(json!({}), rect_json);
            o["id"] = self.obj_id(&r).into();
            o
        }))
    }

    /// A thing's id for commands (see the module docs).
    pub(crate) fn obj_id(&self, r: &ObjRef) -> String {
        let uid = self
            .objs
            .strokes(r)
            .first()
            .map_or(0, |&s| self.scene.strokes[s as usize].uid);
        format!("{:016x}", uid as u64)
    }

    /// The thing on the page with id `id`, if it is still there.
    pub(crate) fn find_obj(&self, id: &str) -> Option<ObjRef> {
        let want = u64::from_str_radix(id.trim(), 16).ok()?;
        let i = self
            .scene
            .strokes
            .iter()
            .position(|s| !s.deleted && s.uid != 0 && s.uid as u64 == want)?;
        Some(self.objs.obj_of(i as u32))
    }

    #[cfg(test)]
    pub(crate) fn find_all_pub(&self, ids: &[String]) -> Result<Vec<ObjRef>, String> {
        self.find_all(ids)
    }

    fn find_all(&self, ids: &[String]) -> Result<Vec<ObjRef>, String> {
        if ids.is_empty() {
            return Err("name the things to change by id".into());
        }
        ids.iter()
            .map(|id| {
                self.find_obj(id).ok_or(format!(
                    "nothing with id {id} (editing a thing gives it a new id; list the page again)"
                ))
            })
            .collect()
    }

    /// Everything on the page, bottom to top.
    fn all_objects(&self) -> Vec<ObjRef> {
        let mut ids: Vec<u32> = (0..self.scene.strokes.len() as u32)
            .filter(|&i| {
                !self.scene.strokes[i as usize].deleted && !self.objs.look_strokes.contains(&i)
            })
            .collect();
        ids.sort_by(|a, b| {
            let (za, zb) = (
                self.scene.strokes[*a as usize].z,
                self.scene.strokes[*b as usize].z,
            );
            za.total_cmp(&zb).then(a.cmp(b))
        });
        let mut seen = HashSet::new();
        ids.into_iter()
            .map(|i| self.objs.obj_of(i))
            .filter(|r| seen.insert(*r))
            .collect()
    }

    /// The box around a thing, in home-view points: x, y, w, h.
    pub(crate) fn obj_box(&self, r: &ObjRef, cam: &Camera) -> Option<[f64; 4]> {
        let (pts, pad): (Vec<[f64; 2]>, f64) = match *r {
            ObjRef::Group(g) => {
                let grp = self.objs.groups.get(g as usize)?;
                let d = objects::to_cam(&grp.cell, &grp.data, cam);
                (
                    d.extent().into_iter().map(|q| to_home(cam, q)).collect(),
                    0.0,
                )
            }
            ObjRef::Ink(id) => {
                let cell = self.scene.stroke_cell(id);
                let (o, side) = objects::frame(cell, cam);
                let pts = self
                    .scene
                    .stroke_points(id)
                    .iter()
                    .map(|p| to_home(cam, [o[0] + p[0] as f64 * side, o[1] + p[1] as f64 * side]))
                    .collect();
                let w = self.scene.strokes[id as usize].width as f64 * side * cam.ppc();
                (pts, w * 0.5)
            }
        };
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &pts {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        (lo[0] <= hi[0]).then(|| {
            [
                lo[0] - pad,
                lo[1] - pad,
                hi[0] - lo[0] + 2.0 * pad,
                hi[1] - lo[1] + 2.0 * pad,
            ]
        })
    }

    /// What a thing is, for `get: objects`.
    fn describe_obj(&self, r: &ObjRef, b: [f64; 4], cam: &Camera) -> Value {
        let ppc = cam.ppc();
        let mut o = rect_json(b);
        let m = o.as_object_mut().expect("object");
        m.insert("id".into(), self.obj_id(r).into());
        let mut put = |k: &str, v: Value| {
            m.insert(k.into(), v);
        };
        match *r {
            ObjRef::Ink(id) => {
                let s = &self.scene.strokes[id as usize];
                let side = self.scene.stroke_cell(id).side_in(&cam.cell);
                put(
                    "kind",
                    if s.brush == Brush::Fill {
                        "fill"
                    } else {
                        "ink"
                    }
                    .into(),
                );
                put("color", color_hex(s.color).into());
                if s.brush != Brush::Fill {
                    put("width", tidy(s.width as f64 * side * ppc).into());
                    put("brush", brush_name(s.brush).into());
                    if s.dash != Dash::Solid {
                        put("dash", dash_name(s.dash).into());
                    }
                }
                put("points", self.scene.stroke_points(id).len().into());
            }
            ObjRef::Group(g) => {
                let grp = &self.objs.groups[g as usize];
                let rot = grp.data.geom().rot;
                if rot != 0.0 {
                    put("rotate", tidy(rot.to_degrees()).into());
                }
                match objects::to_cam(&grp.cell, &grp.data, cam) {
                    ObjData::Shape {
                        style, geom, width, ..
                    }
                    | ObjData::Portal {
                        style, geom, width, ..
                    } => {
                        if let ObjData::Portal { view, .. } = &grp.data {
                            put("kind", "portal".into());
                            put("view", view.name.clone().into());
                        } else {
                            put("kind", "shape".into());
                            put("shape", kind_name(style.kind).into());
                        }
                        put("color", color_hex(style.stroke).into());
                        if style.fill_style != FillStyle::None {
                            put("fill", color_hex(style.fill).into());
                        }
                        if style.fill_style.gradient() {
                            put(
                                "gradient",
                                json!({
                                    "to": color_hex(style.fill2),
                                    "angle": style.grad,
                                    "radial": style.fill_style == FillStyle::Radial,
                                }),
                            );
                        }
                        put("width", tidy(width * ppc).into());
                        if style.dash != Dash::Solid {
                            put("dash", dash_name(style.dash).into());
                        }
                        if style.round {
                            put("round", true.into());
                        }
                        if let [a, b] = geom.pts[..] {
                            let (a, b) = (to_home(cam, a), to_home(cam, b));
                            put("from", json!([tidy(a[0]), tidy(a[1])]));
                            put("to", json!([tidy(b[0]), tidy(b[1])]));
                        }
                    }
                    ObjData::Text {
                        text, style, size, ..
                    } => {
                        put("kind", "text".into());
                        put("text", text.into());
                        put("size", tidy(size * ppc).into());
                        put("color", color_hex(style.color).into());
                        put("font", font::name_of(style.font).into());
                        if style.align != Align::Left {
                            put("align", align_name(style.align).into());
                        }
                    }
                    ObjData::Table {
                        cells, style, size, ..
                    } => {
                        put("kind", "table".into());
                        put("cells", json!(cells));
                        put("size", tidy(size * ppc).into());
                        put("color", color_hex(style.color).into());
                    }
                    ObjData::Image {
                        opacity, adjust, ..
                    } => {
                        put("kind", "image".into());
                        if opacity != 255 {
                            put("opacity", tidy(opacity as f64 / 255.0).into());
                        }
                        if adjust != objects::NO_ADJUST {
                            put("brightness", tidy(adjust[0] as f64).into());
                            put("contrast", tidy(adjust[1] as f64).into());
                            put("saturation", tidy(adjust[2] as f64).into());
                        }
                    }
                    ObjData::LayerLook { ref name, .. } => {
                        put("kind", "layer".into());
                        put("name", name.clone().into());
                    }
                    ObjData::Frame { ref name, .. } => {
                        put("kind", "frame".into());
                        put("name", name.clone().into());
                    }
                    ObjData::Bundle { ref comp, .. } => {
                        put("kind", "group".into());
                        put("members", self.objs.members(g).len().into());
                        if let Some(c) = comp {
                            put("component", c.name.clone().into());
                            put("main", c.main.into());
                        }
                    }
                    ObjData::Audio { dur_ms, .. } => {
                        put("kind", "audio".into());
                        put("seconds", tidy(dur_ms as f64 / 1000.0).into());
                    }
                }
            }
        }
        o
    }

    /// `get: objects`: what is on the page (in a region, of some kinds),
    /// bottom to top, and the box around everything.
    fn list_objects(&self, c: &Value, cam: &Camera) -> Value {
        let region = rect_of(c);
        let kinds: Vec<String> = ids_of(c.get("kinds"));
        let limit = c
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(LIST_DEFAULT, |n| (n as usize).min(LIST_MAX));
        let (mut bounds, mut total, mut list) = (None, 0, Vec::new());
        for r in self.all_objects() {
            let Some(b) = self.obj_box(&r, cam) else {
                continue;
            };
            bounds = Some(union(bounds, b));
            if region.is_some_and(|reg| !overlaps(&reg, &b)) {
                continue;
            }
            let d = self.describe_obj(&r, b, cam);
            let kind = d.get("kind").and_then(Value::as_str).unwrap_or("");
            if !kinds.is_empty() && !kinds.iter().any(|k| k == kind) {
                continue;
            }
            total += 1;
            if list.len() < limit {
                list.push(d);
            }
        }
        json!({
            "objects": list,
            "total": total,
            "bounds": bounds.map_or(Value::Null, rect_json),
        })
    }

    /// `move`, `set`, `order` or `delete`.
    fn edit_command(&mut self, verb: &str, c: &Value, cam: &Camera) -> Result<Value, String> {
        let refs = self.find_all(&ids_of(c.get(verb)))?;
        let ppc = cam.ppc();
        let ids = |app: &App, refs: &[ObjRef]| -> Value {
            refs.iter()
                .map(|r| app.obj_id(r))
                .collect::<Vec<_>>()
                .into()
        };
        match verb {
            "delete" => {
                self.edit.selection = refs;
                self.delete_selection();
                Ok(Value::Null)
            }
            "order" => {
                let front = match c.get("to").and_then(Value::as_str).unwrap_or("front") {
                    "front" => true,
                    "back" => false,
                    other => return Err(format!("order to front or back, not {other}")),
                };
                self.edit.selection = refs;
                self.reorder(front);
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            "combine" => {
                use crate::booleans::BoolOp;
                let op = match c.get("op").and_then(Value::as_str).unwrap_or("union") {
                    "union" => BoolOp::Union,
                    "subtract" => BoolOp::Subtract,
                    "intersect" => BoolOp::Intersect,
                    "exclude" => BoolOp::Exclude,
                    other => {
                        return Err(format!(
                            "combine with op union, subtract, intersect or exclude, not {other}"
                        ))
                    }
                };
                self.edit.selection = refs;
                self.boolean(op);
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            "component" => {
                self.edit.selection = refs;
                self.make_component();
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            "remove_background" => {
                self.edit.selection = refs;
                self.remove_background();
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            "group" | "ungroup" => {
                self.edit.selection = refs;
                // (No "Grouped" note for a script: it says nothing to the person.)
                let note = self.ui.message.clone();
                if verb == "group" {
                    self.group_selection();
                } else {
                    self.ungroup_selection();
                }
                self.ui.message = note;
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            "move" => {
                let d = [num(c, "dx").unwrap_or(0.0), num(c, "dy").unwrap_or(0.0)];
                self.edit.selection = refs;
                self.apply_op(&Op::Move([d[0] / ppc, d[1] / ppc]));
                let sel = std::mem::take(&mut self.edit.selection);
                Ok(json!({ "ids": ids(self, &sel) }))
            }
            _ => {
                let patch = Patch::parse(c, ppc)?;
                let mut out = Vec::new();
                for mut r in refs {
                    // Size (from the top left corner), turn, then place.
                    let (w, h) = (num(c, "w"), num(c, "h"));
                    if w.is_some() || h.is_some() {
                        let b = self.obj_box(&r, cam).ok_or("that has no size")?;
                        let k = |want: Option<f64>, had: f64| {
                            want.map_or(1.0, |v| v.max(0.01) / had.max(1e-9))
                        };
                        let op = Op::Scale {
                            pivot: to_cam(cam, b[0], b[1]),
                            sx: k(w, b[2]),
                            sy: k(h, b[3]),
                            rot: 0.0,
                        };
                        r = self.edit_one(r, &op);
                    }
                    if let Some(deg) = num(c, "rotate") {
                        let b = self.obj_box(&r, cam).ok_or("that has no size")?;
                        let op = Op::Rotate {
                            pivot: to_cam(cam, b[0] + b[2] * 0.5, b[1] + b[3] * 0.5),
                            angle: deg.to_radians(),
                        };
                        r = self.edit_one(r, &op);
                    }
                    if !patch.is_empty() {
                        r = self.restyle_one(r, &patch);
                    }
                    let (x, y) = (num(c, "x"), num(c, "y"));
                    if x.is_some() || y.is_some() {
                        let b = self.obj_box(&r, cam).ok_or("that has no size")?;
                        let d = [x.map_or(0.0, |x| x - b[0]), y.map_or(0.0, |y| y - b[1])];
                        r = self.edit_one(r, &Op::Move([d[0] / ppc, d[1] / ppc]));
                    }
                    out.push(r);
                }
                self.edit.selection.clear();
                Ok(json!({ "ids": ids(self, &out) }))
            }
        }
    }

    /// Apply `op` (camera units) to one thing; the thing it became.
    fn edit_one(&mut self, r: ObjRef, op: &Op) -> ObjRef {
        self.edit.selection = vec![r];
        self.apply_op(op);
        self.edit.selection.first().copied().unwrap_or(r)
    }

    /// One thing restyled; the thing it became.
    fn restyle_one(&mut self, r: ObjRef, p: &Patch) -> ObjRef {
        self.edit.selection = vec![r];
        self.replace_selection(|app, r| match r {
            ObjRef::Group(g) => Some(app.group_copy(g, |d| p.apply(d), None)),
            ObjRef::Ink(id) => {
                if p.color.is_none() && p.width.is_none() && p.dash.is_none() {
                    return None;
                }
                let mut st = app.scene.stroke_style(id);
                st.color = p.color.unwrap_or(st.color);
                st.dash = p.dash.unwrap_or(st.dash);
                if let Some(w) = p.width {
                    let side = app.scene.stroke_cell(id).side_in(&app.cam.cell);
                    st.width = (w / side) as f32;
                }
                Some(app.ink_copy(id, None, Some(st), None))
            }
        });
        self.edit.selection.first().copied().unwrap_or(r)
    }

    /// `get: picture`: a PNG of part of the page (`x`, `y`, `w`, `h`), of
    /// some things (`ids`), or of everything, `px` on its longer side.
    #[cfg(target_arch = "wasm32")]
    fn picture(&mut self, _c: &Value, _cam: &Camera) -> Result<Value, String> {
        Err("pictures come from the page server and the desktop app".into())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn picture(&mut self, c: &Value, cam: &Camera) -> Result<Value, String> {
        use crate::export::{Format, Opts};
        let px = num(c, "px").unwrap_or(PICTURE_PX).clamp(64.0, PICTURE_MAX);
        let ids = ids_of(c.get("ids"));
        let refs = if ids.is_empty() {
            vec![]
        } else {
            self.find_all(&ids)?
        };
        let frame = match rect_of(c) {
            Some(r) => r,
            None => {
                let around = if refs.is_empty() {
                    self.all_objects()
                } else {
                    refs.clone()
                };
                let b = around
                    .iter()
                    .filter_map(|r| self.obj_box(r, cam))
                    .fold(None, |acc, b| Some(union(acc, b)));
                match b {
                    Some(b) => {
                        let pad = (b[2].max(b[3]) * 0.05).max(1e-30);
                        [b[0] - pad, b[1] - pad, b[2] + 2.0 * pad, b[3] + 2.0 * pad]
                    }
                    // An empty page: the home view.
                    None => [-600.0, -400.0, 1200.0, 800.0],
                }
            }
        };
        let k = px / frame[2].max(frame[3]);
        let (w, h) = (
            (frame[2] * k).round().max(16.0),
            (frame[3] * k).round().max(16.0),
        );
        let mut view = cam.clone();
        view.pan_px(-(frame[0] + frame[2] * 0.5), -(frame[1] + frame[3] * 0.5));
        view.zoom_at(k, [0.0, 0.0]);
        let saved_cam = std::mem::replace(&mut self.cam, view);
        let saved_size = self.size_override.replace([w, h]);
        let only = !refs.is_empty();
        // Layer looks as the canvas keeps them (a page server draws no frames).
        self.layers_refresh();
        self.edit.selection = refs;
        ogpaper_core::query(&self.scene, &self.cam, w, h, crate::VIEW, &mut self.draw);
        if self.portal_pass() {
            ogpaper_core::query(&self.scene, &self.cam, w, h, crate::VIEW, &mut self.draw);
        }
        let png = self.export(
            Format::Png,
            &Opts {
                selection: only,
                background: true,
                scale: 1.0,
            },
        );
        self.cam = saved_cam;
        self.size_override = saved_size;
        self.edit.selection.clear();
        Ok(json!({
            "png": crate::export::base64(&png?),
            "frame": rect_json(frame),
            "width": w,
            "height": h,
        }))
    }

    /// Every text on the page, with where it is (home-view points).
    pub(crate) fn texts_at(&self, cam: &Camera) -> Vec<Value> {
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
        assert_eq!(color_hex(parse_color("#1e5ac8").unwrap()), "#1e5ac8");
        assert_eq!(color_hex(parse_color("#1e5ac880").unwrap()), "#1e5ac880");
    }

    fn run(app: &mut App, cmds: Value) -> Vec<Value> {
        app.run_commands(&cmds).as_array().unwrap().clone()
    }

    fn objects(app: &mut App) -> Vec<Value> {
        run(app, json!([{"get": "objects"}]))[0]["objects"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn near(a: &Value, b: f64) -> bool {
        (a.as_f64().unwrap() - b).abs() < 0.5
    }

    #[test]
    fn things_added_are_listed_with_ids_and_boxes() {
        let mut app = App::new(None);
        let r = run(
            &mut app,
            json!([
                {"add": "shape", "kind": "ellipse", "x": -100, "y": 50, "w": 200, "h": 80,
                 "fill": "#ffd166", "color": "#333333", "dash": "dashed"},
                {"add": "text", "x": 10, "y": 20, "text": "Hello", "size": 30, "align": "center"},
                {"add": "table", "x": 300, "y": 0, "cells": [["a", "b"], [1, 2]]},
                {"add": "stroke", "points": [[0, 0], [100, 40]], "width": 4, "color": "#c82850"},
            ]),
        );
        assert!(r.iter().all(|r| r["ok"] == true), "{r:?}");
        let ids: Vec<&str> = r.iter().map(|r| r["id"].as_str().unwrap()).collect();
        let list = objects(&mut app);
        assert_eq!(list.len(), 4);
        // Bottom to top, ids as add gave them.
        let got: Vec<&str> = list.iter().map(|o| o["id"].as_str().unwrap()).collect();
        assert_eq!(got, ids);
        let (e, t, tb, ink) = (&list[0], &list[1], &list[2], &list[3]);
        assert_eq!(e["kind"], "shape");
        assert_eq!(e["shape"], "ellipse");
        assert_eq!(e["fill"], "#ffd166");
        assert_eq!(e["dash"], "dashed");
        assert!(near(&e["x"], -100.0) && near(&e["y"], 50.0) && near(&e["w"], 200.0));
        assert_eq!(t["text"], "Hello");
        assert_eq!(t["align"], "center");
        assert!(near(&t["x"], 10.0) && near(&t["size"], 30.0));
        assert_eq!(tb["cells"], json!([["a", "b"], ["1", "2"]]));
        assert_eq!(ink["kind"], "ink");
        assert_eq!(ink["color"], "#c82850");
        assert!(near(&ink["width"], 4.0));
        // Region and kind filters.
        let only = run(
            &mut app,
            json!([{"get": "objects", "x": 250, "y": -10, "w": 100, "h": 100},
                   {"get": "objects", "kinds": ["text"]}]),
        );
        assert_eq!(only[0]["total"], 1);
        assert_eq!(only[0]["objects"][0]["kind"], "table");
        assert_eq!(only[1]["objects"][0]["text"], "Hello");
        // A view-only key may not change things.
        assert!(changes_page(&json!({"set": "1"})) && !changes_page(&json!({"get": "objects"})));
    }

    #[test]
    fn edits_move_restyle_reorder_and_delete_by_id() {
        let mut app = App::new(None);
        let r = run(
            &mut app,
            json!([{"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 100, "h": 50},
                   {"add": "text", "x": 0, "y": 100, "text": "Hi", "size": 20}]),
        );
        let (rect, text) = (
            r[0]["id"].as_str().unwrap().to_string(),
            r[1]["id"].as_str().unwrap().to_string(),
        );
        let r = run(
            &mut app,
            json!([{"move": [rect], "dx": 50, "dy": -10},
                   {"set": text, "text": "Hello there", "color": "#1e5ac8", "x": 200, "y": 300}]),
        );
        assert!(r.iter().all(|r| r["ok"] == true), "{r:?}");
        let rect2 = r[0]["ids"][0].as_str().unwrap().to_string();
        let text2 = r[1]["ids"][0].as_str().unwrap().to_string();
        assert!(rect2 != rect && text2 != text);
        let list = objects(&mut app);
        let by = |id: &str| list.iter().find(|o| o["id"] == id).unwrap().clone();
        let (a, b) = (by(&rect2), by(&text2));
        assert!(near(&a["x"], 50.0) && near(&a["y"], -10.0) && near(&a["w"], 100.0));
        assert_eq!(b["text"], "Hello there");
        assert_eq!(b["color"], "#1e5ac8");
        assert!(near(&b["x"], 200.0) && near(&b["y"], 300.0));
        // Resize and fill; the old id is gone.
        let r = run(
            &mut app,
            json!([{"set": rect2, "w": 300, "h": 120, "fill": "#ffd166", "round": true},
                   {"move": [rect], "dx": 1}]),
        );
        assert_eq!(r[1]["ok"], false);
        let rect3 = r[0]["ids"][0].as_str().unwrap().to_string();
        let list = objects(&mut app);
        let a = list.iter().find(|o| o["id"] == rect3).unwrap();
        assert!(near(&a["w"], 300.0) && near(&a["h"], 120.0) && near(&a["x"], 50.0));
        assert_eq!(a["fill"], "#ffd166");
        assert_eq!(a["round"], true);
        // The text is on top; send it back.
        assert_eq!(list.last().unwrap()["id"], text2);
        let r = run(&mut app, json!([{"order": [text2], "to": "back"}]));
        let text3 = r[0]["ids"][0].as_str().unwrap().to_string();
        assert_eq!(objects(&mut app)[0]["id"], text3);
        // Turning a text keeps it turned through a restyle.
        let r = run(
            &mut app,
            json!([{"set": text3, "rotate": 30, "color": "#c82850"}]),
        );
        let text3 = r[0]["ids"][0].as_str().unwrap().to_string();
        let g = match app.find_obj(&text3).unwrap() {
            ObjRef::Group(g) => g,
            _ => panic!(),
        };
        assert!((app.objs.groups[g as usize].data.geom().rot - 30f64.to_radians()).abs() < 1e-6);
        let list = objects(&mut app);
        let t = list.iter().find(|o| o["id"] == text3).unwrap();
        assert!(near(&t["rotate"], 30.0));
        // Delete both.
        let r = run(&mut app, json!([{"delete": [text3, rect3]}]));
        assert_eq!(r[0]["ok"], true);
        assert!(objects(&mut app).is_empty());
        // Things added in the same run can be edited in it.
        let r = run(&mut app, json!([{"add": "text", "text": "x"}]));
        let id = r[0]["id"].as_str().unwrap().to_string();
        let r = run(
            &mut app,
            json!([{"add": "text", "text": "y"}, {"delete": [id]}]),
        );
        assert_eq!(r[1]["ok"], true);
        assert_eq!(objects(&mut app).len(), 1);
    }

    #[test]
    fn a_picture_shows_the_page() {
        let mut app = App::new(None);
        // An empty page still gives a picture (the home view).
        let r = run(&mut app, json!([{"get": "picture", "px": 200}]));
        assert_eq!(r[0]["ok"], true, "{r:?}");
        run(
            &mut app,
            json!([{"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 400, "h": 100,
                    "fill": "#c82850"}]),
        );
        let r = run(&mut app, json!([{"get": "picture", "px": 400}]));
        assert_eq!(r[0]["ok"], true, "{r:?}");
        // Framed around the rectangle, at 400 px wide.
        assert_eq!(r[0]["width"], 400.0);
        assert!(r[0]["height"].as_f64().unwrap() < 200.0);
        let png = crate::audio::base64_decode(r[0]["png"].as_str().unwrap()).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        // The middle is the fill, a corner the paper.
        let mid = img.get_pixel(img.width() / 2, img.height() / 2).0;
        assert!(mid[0] > 150 && mid[1] < 90, "{mid:?}");
        let corner = img.get_pixel(1, 1).0;
        assert!(corner[0] > 230 && corner[1] > 230, "{corner:?}");
        // The camera is put back.
        assert_eq!(app.cam.off, crate::home_camera().off);
    }

    #[test]
    fn gradients_combine_and_groups_by_id() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        let r = run(
            &mut app,
            json!([
                {"add": "shape", "kind": "rect", "x": 0, "y": 0, "w": 100, "h": 100,
                 "fill": "#ff0000", "gradient": {"to": "#0000ff", "angle": 0}},
                {"add": "shape", "kind": "ellipse", "x": 50, "y": 50, "w": 100, "h": 100, "fill": "#00ff00"},
                {"add": "shape", "kind": "path", "points": [[200, 0], [260, 40], [220, 90]], "closed": true, "smooth": true},
            ]),
        );
        let ids: Vec<String> = r
            .iter()
            .map(|x| x["id"].as_str().unwrap().to_string())
            .collect();
        let list = run(&mut app, json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list[0]["gradient"]["to"], "#0000ff", "{list}");
        assert_eq!(list[2]["shape"], "path");
        let r = run(
            &mut app,
            json!([{"combine": [ids[0], ids[1]], "op": "union"}]),
        );
        assert_eq!(r[0]["ok"], true, "{r:?}");
        let u = r[0]["ids"][0].as_str().unwrap().to_string();
        let r = run(&mut app, json!([{"group": [u, ids[2]]}]));
        assert_eq!(r[0]["ok"], true, "{r:?}");
        let list = run(&mut app, json!([{"get": "objects"}]))[0]["objects"].clone();
        assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
        assert_eq!(list[0]["kind"], "group");
    }
}
