// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Saved tools: named toolbars of nine slots (one shown along the bottom at a
//! time, switched with the toolbar button or `[` / `]`) and an inventory that
//! grows as it fills, each slot holding a tool with its settings (a red 2 px
//! pen, a dashed blue arrow, a font and size, ...). They belong to the person,
//! not the canvas: kept in `~/OG Paper/hotbar.txt` on desktop and in browser
//! storage on the web.
//!
//! Stored as text (`og-paper-hotbar 2`): `active N`; `b N name` names toolbar
//! N; `h N slot tool…` fills a toolbar slot; `i slot tool…` an inventory slot.
//! A tool is its key then its settings: for pens, color (RGBA hex), width,
//! pressure (0/1), dash and opacity; for shapes and text, the hex of the same
//! settings record a canvas file stores (see `snapshot.rs`). Version 1 files
//! (one bar: `h slot tool…`) still load.

use egui::Color32;
use ogpaper_core::Dash;

use crate::objects::{ObjData, TextStyle};
use crate::shapes::{self, Geom, ShapeKind, ShapeStyle};
use crate::ui::{InkSettings, Tool};

/// Slots per toolbar (keys 1–9).
pub const BAR: usize = 9;
/// The inventory starts with this many slots and grows a row at a time.
pub const INVENTORY: usize = 27;
/// Width of one inventory row: there is always a free row at the end.
pub const INV_ROW: usize = 9;

/// A named set of quick-bar slots.
#[derive(Clone, PartialEq, Debug)]
pub struct Toolbar {
    pub name: String,
    pub slots: Vec<Option<Preset>>,
}

/// Everything saved: the toolbars, which one is showing, and the inventory.
#[derive(Clone, PartialEq, Debug)]
pub struct Saved {
    pub bars: Vec<Toolbar>,
    pub active: usize,
    pub inv: Vec<Option<Preset>>,
}

/// Keep at least one empty row at the end of the inventory (and never less
/// than the starting size), so there is always room to save another tool.
pub fn grow_inventory(inv: &mut Vec<Option<Preset>>) {
    let used = inv.iter().rposition(|x| x.is_some()).map_or(0, |i| i + 1);
    let want = (used.div_ceil(INV_ROW) + 1) * INV_ROW;
    inv.resize(want.max(INVENTORY), None);
}

/// A tool and its settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Preset {
    pub tool: Tool,
    pub ink: Option<InkSettings>,
    /// Shape settings and outline width (screen px).
    pub shape: Option<(ShapeStyle, f32)>,
    /// Text settings and cap height (screen px).
    pub text: Option<(TextStyle, f32)>,
}

impl Preset {
    pub fn tool(tool: Tool) -> Self {
        Self {
            tool,
            ink: None,
            shape: None,
            text: None,
        }
    }

    pub fn ink(tool: Tool, color: Color32, width: f32, pressure: bool, dash: Dash) -> Self {
        Self {
            ink: Some(InkSettings {
                color,
                width,
                pressure,
                dash,
                opacity: 255,
            }),
            ..Self::tool(tool)
        }
    }
}

/// What a fresh install starts with: an everyday toolbar, a diagramming one,
/// and a few spares in the inventory.
pub fn defaults() -> Saved {
    let black = Color32::from_rgb(28, 28, 36);
    let red = Color32::from_rgb(210, 40, 60);
    let blue = Color32::from_rgb(30, 90, 200);
    let green = Color32::from_rgb(20, 140, 80);
    let shape = |kind, fill_style| {
        let st = ShapeStyle {
            kind,
            fill_style,
            ..Default::default()
        };
        Preset {
            shape: Some((st, 3.0)),
            ..Preset::tool(Tool::Shapes)
        }
    };
    let bar = vec![
        Some(Preset::ink(Tool::Pen, black, 3.0, true, Dash::Solid)),
        Some(Preset::ink(Tool::Marker, blue, 8.0, false, Dash::Solid)),
        Some(Preset::ink(
            Tool::Highlighter,
            Color32::from_rgb(255, 214, 0),
            22.0,
            false,
            Dash::Solid,
        )),
        Some(Preset::ink(Tool::Pen, red, 3.0, true, Dash::Solid)),
        Some(Preset::ink(Tool::Pen, blue, 2.0, true, Dash::Solid)),
        Some(shape(ShapeKind::Rect, shapes::FillStyle::None)),
        Some(shape(ShapeKind::Arrow, shapes::FillStyle::None)),
        Some(Preset {
            text: Some((TextStyle::default(), 24.0)),
            ..Preset::tool(Tool::Text)
        }),
        Some(Preset::tool(Tool::Eraser)),
    ];
    let mut inv = vec![
        Some(Preset::ink(Tool::Marker, green, 8.0, false, Dash::Solid)),
        Some(Preset::ink(Tool::Marker, red, 14.0, false, Dash::Solid)),
        Some(Preset::ink(
            Tool::Pen,
            Color32::from_rgb(90, 90, 100),
            2.0,
            false,
            Dash::Dashed,
        )),
        Some(Preset::ink(
            Tool::Highlighter,
            Color32::from_rgb(120, 230, 90),
            22.0,
            false,
            Dash::Solid,
        )),
        Some(Preset::ink(
            Tool::Highlighter,
            Color32::from_rgb(255, 120, 200),
            22.0,
            false,
            Dash::Solid,
        )),
        Some(shape(ShapeKind::Ellipse, shapes::FillStyle::Hachure)),
        Some(shape(ShapeKind::Diamond, shapes::FillStyle::Solid)),
        Some(Preset::tool(Tool::Select)),
        Some(Preset::tool(Tool::Hand)),
    ];
    grow_inventory(&mut inv);
    let text = Preset {
        text: Some((TextStyle::default(), 24.0)),
        ..Preset::tool(Tool::Text)
    };
    let diagram = vec![
        Some(Preset::tool(Tool::Select)),
        Some(shape(ShapeKind::Rect, shapes::FillStyle::None)),
        Some(shape(ShapeKind::Ellipse, shapes::FillStyle::None)),
        Some(shape(ShapeKind::Diamond, shapes::FillStyle::None)),
        Some(shape(ShapeKind::Arrow, shapes::FillStyle::None)),
        Some(shape(ShapeKind::Line, shapes::FillStyle::None)),
        Some(text),
        Some(Preset::ink(Tool::Pen, black, 2.0, true, Dash::Solid)),
        Some(Preset::tool(Tool::Eraser)),
    ];
    Saved {
        bars: vec![
            Toolbar {
                name: "Everyday".into(),
                slots: bar,
            },
            Toolbar {
                name: "Diagram".into(),
                slots: diagram,
            },
        ],
        active: 0,
        inv,
    }
}

/// A new, empty toolbar.
pub fn empty_bar(name: impl Into<String>) -> Toolbar {
    Toolbar {
        name: name.into(),
        slots: vec![None; BAR],
    }
}

fn tool_key(t: Tool) -> &'static str {
    match t {
        Tool::Pen => "pen",
        Tool::Marker => "marker",
        Tool::Highlighter => "highlighter",
        Tool::Eraser => "eraser",
        Tool::Hand => "hand",
        Tool::Picker => "picker",
        Tool::Shapes => "shapes",
        Tool::Text => "text",
        Tool::Select => "select",
        Tool::Lasso => "lasso",
        Tool::Bucket => "bucket",
    }
}

fn tool_from(k: &str) -> Option<Tool> {
    Some(match k {
        "pen" => Tool::Pen,
        "marker" => Tool::Marker,
        "highlighter" => Tool::Highlighter,
        "eraser" => Tool::Eraser,
        "hand" => Tool::Hand,
        "picker" => Tool::Picker,
        "shapes" => Tool::Shapes,
        "text" => Tool::Text,
        "select" => Tool::Select,
        "lasso" => Tool::Lasso,
        "bucket" => Tool::Bucket,
        _ => return None,
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn put(p: &Preset) -> String {
    let mut out = tool_key(p.tool).to_string();
    if let Some(i) = p.ink {
        let [r, g, b, a] = i.color.to_array();
        out += &format!(
            " ink {:08x} {} {} {} {}",
            u32::from_be_bytes([r, g, b, a]),
            i.width,
            i.pressure as u8,
            i.dash as u8,
            i.opacity
        );
    }
    if let Some((style, w)) = p.shape {
        let d = ObjData::Shape {
            style,
            geom: Geom::default(),
            width: w as f64,
            seed: 0,
        };
        out += &format!(" shape {}", hex(&crate::snapshot::data_bytes(&d)));
    }
    if let Some((style, size)) = p.text {
        let d = ObjData::Text {
            text: String::new(),
            style,
            geom: Geom::default(),
            size: size as f64,
            seed: 0,
        };
        out += &format!(" text {}", hex(&crate::snapshot::data_bytes(&d)));
    }
    out
}

fn get(s: &str) -> Option<Preset> {
    let mut w = s.split_whitespace();
    let mut p = Preset::tool(tool_from(w.next()?)?);
    while let Some(k) = w.next() {
        match k {
            "ink" => {
                let [r, g, b, a] = u32::from_str_radix(w.next()?, 16).ok()?.to_be_bytes();
                p.ink = Some(InkSettings {
                    color: Color32::from_rgba_unmultiplied(r, g, b, a),
                    width: w.next()?.parse().ok()?,
                    pressure: w.next()? == "1",
                    dash: Dash::from_u8(w.next()?.parse().ok()?),
                    opacity: w.next()?.parse().ok()?,
                });
            }
            "shape" => {
                if let ObjData::Shape { style, width, .. } =
                    crate::snapshot::get_data(&unhex(w.next()?)?).ok()?
                {
                    p.shape = Some((style, width as f32));
                }
            }
            "text" => {
                if let ObjData::Text { style, size, .. } =
                    crate::snapshot::get_data(&unhex(w.next()?)?).ok()?
                {
                    p.text = Some((style, size as f32));
                }
            }
            _ => return None,
        }
    }
    Some(p)
}

/// Everything as text.
pub fn encode(saved: &Saved) -> String {
    let mut out = format!("og-paper-hotbar 2\nactive {}\n", saved.active);
    for (b, bar) in saved.bars.iter().enumerate() {
        // Names are one line; anything after the number is the name.
        out += &format!("b {b} {}\n", bar.name.replace(['\n', '\r'], " "));
        for (i, s) in bar.slots.iter().enumerate() {
            if let Some(p) = s {
                out += &format!("h {b} {i} {}\n", put(p));
            }
        }
    }
    for (i, s) in saved.inv.iter().enumerate() {
        if let Some(p) = s {
            out += &format!("i {i} {}\n", put(p));
        }
    }
    out
}

/// Read what [`encode`] wrote (or a version 1 file); slots that do not parse
/// stay empty.
pub fn decode(text: &str) -> Option<Saved> {
    let mut lines = text.lines();
    let version = match lines.next()?.trim() {
        "og-paper-hotbar 1" => 1,
        "og-paper-hotbar 2" => 2,
        _ => return None,
    };
    let mut bars: Vec<Toolbar> = Vec::new();
    let mut inv: Vec<Option<Preset>> = Vec::new();
    let mut active = 0;
    let bar = |bars: &mut Vec<Toolbar>, b: usize| -> Option<()> {
        if b > 255 {
            return None;
        }
        while bars.len() <= b {
            let n = bars.len() + 1;
            bars.push(empty_bar(format!("Toolbar {n}")));
        }
        Some(())
    };
    for l in lines {
        let mut w = l.splitn(2, ' ');
        let (Some(tag), Some(rest)) = (w.next(), w.next()) else {
            continue;
        };
        match (version, tag) {
            (2, "active") => active = rest.trim().parse().unwrap_or(0),
            (2, "b") => {
                let mut w = rest.splitn(2, ' ');
                if let (Some(Ok(b)), name) = (w.next().map(str::parse::<usize>), w.next()) {
                    if bar(&mut bars, b).is_some() {
                        bars[b].name = name.unwrap_or("").trim().to_string();
                    }
                }
            }
            (_, "h") => {
                // v1: `h slot tool…` is toolbar 0; v2: `h bar slot tool…`.
                let (b, rest) = if version == 1 {
                    (0, rest)
                } else {
                    let mut w = rest.splitn(2, ' ');
                    let (Some(Ok(b)), Some(r)) = (w.next().map(str::parse::<usize>), w.next())
                    else {
                        continue;
                    };
                    (b, r)
                };
                let mut w = rest.splitn(2, ' ');
                let (Some(Ok(i)), Some(tool)) = (w.next().map(str::parse::<usize>), w.next())
                else {
                    continue;
                };
                if bar(&mut bars, b).is_some() {
                    if let Some(slot) = bars[b].slots.get_mut(i) {
                        *slot = get(tool);
                    }
                }
            }
            (_, "i") => {
                let mut w = rest.splitn(2, ' ');
                let (Some(Ok(i)), Some(tool)) = (w.next().map(str::parse::<usize>), w.next())
                else {
                    continue;
                };
                if i < 10_000 {
                    if inv.len() <= i {
                        inv.resize(i + 1, None);
                    }
                    inv[i] = get(tool);
                }
            }
            _ => {}
        }
    }
    if bars.is_empty() {
        bars.push(empty_bar("Toolbar 1"));
    }
    grow_inventory(&mut inv);
    let active = active.min(bars.len() - 1);
    Some(Saved { bars, active, inv })
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    Some(crate::fonts_dir()?.parent()?.join("hotbar.txt"))
}

/// The saved toolbars and inventory, or the defaults.
pub fn load() -> Saved {
    #[cfg(not(target_arch = "wasm32"))]
    let text = path().and_then(|p| std::fs::read_to_string(p).ok());
    #[cfg(target_arch = "wasm32")]
    let text = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("og-hotbar").ok().flatten());
    text.and_then(|t| decode(&t)).unwrap_or_else(defaults)
}

pub fn save(saved: &Saved) {
    let text = encode(saved);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(p) = path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, text);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("og-hotbar", &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_round_trip() {
        let saved = defaults();
        assert!(saved.bars.len() >= 2 && saved.bars[0].slots.len() == BAR);
        let text = encode(&saved);
        assert_eq!(decode(&text).unwrap(), saved);
        assert!(decode("nonsense").is_none());
        // A bad line leaves its slot empty.
        let s3 = decode("og-paper-hotbar 2\nb 0 A\nh 0 2 pen ink zz\nh 0 4 eraser\n").unwrap();
        assert!(
            s3.bars[0].slots[2].is_none()
                && s3.bars[0].slots[4] == Some(Preset::tool(Tool::Eraser))
        );
    }

    #[test]
    fn reads_version_1() {
        let s = decode("og-paper-hotbar 1\nh 3 eraser\ni 40 hand\n").unwrap();
        assert_eq!(s.bars.len(), 1);
        assert_eq!(s.bars[0].slots[3], Some(Preset::tool(Tool::Eraser)));
        // The inventory grew to hold slot 40 plus a free row.
        assert_eq!(s.inv[40], Some(Preset::tool(Tool::Hand)));
        assert!(s.inv.len() >= 41 + INV_ROW - 1 && s.inv.len() % INV_ROW == 0);
    }

    #[test]
    fn names_and_many_toolbars_survive() {
        let mut saved = defaults();
        saved.bars.push(Toolbar {
            name: "Ink & wash: blues".into(),
            ..empty_bar("")
        });
        saved.bars[2].slots[8] = Some(Preset::tool(Tool::Picker));
        saved.active = 2;
        let back = decode(&encode(&saved)).unwrap();
        assert_eq!(back.bars[2].name, "Ink & wash: blues");
        assert_eq!(back.active, 2);
        assert_eq!(back.bars[2].slots[8], Some(Preset::tool(Tool::Picker)));
    }

    #[test]
    fn inventory_keeps_a_free_row() {
        let mut inv = vec![None; INVENTORY];
        inv[26] = Some(Preset::tool(Tool::Hand));
        grow_inventory(&mut inv);
        assert_eq!(inv.len(), 36);
        assert!(inv[27..].iter().all(|x| x.is_none()));
    }
}
