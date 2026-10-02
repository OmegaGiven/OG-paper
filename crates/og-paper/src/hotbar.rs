// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Saved tools: a quick bar of nine slots along the bottom and a bigger
//! inventory, each slot holding a tool with its settings (a red 2 px pen, a
//! dashed blue arrow, a font and size, ...). They belong to the person, not
//! the canvas: kept in `~/OG Paper/hotbar.txt` on desktop and in browser
//! storage on the web.
//!
//! Stored as text, one slot per line: `h` (quick bar) or `i` (inventory),
//! the slot number, the tool, then its settings: for pens, color (RGBA hex),
//! width, pressure (0/1), dash and opacity; for shapes and text, the hex of
//! the same settings record a canvas file stores (see `snapshot.rs`).

use egui::Color32;
use ogpaper_core::Dash;

use crate::objects::{ObjData, TextStyle};
use crate::shapes::{self, Geom, ShapeKind, ShapeStyle};
use crate::ui::{InkSettings, Tool};

pub const BAR: usize = 9;
pub const INVENTORY: usize = 27;

/// The quick bar's slots and the inventory's.
pub type Slots = (Vec<Option<Preset>>, Vec<Option<Preset>>);

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

/// What a fresh install starts with.
pub fn defaults() -> Slots {
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
    inv.resize(INVENTORY, None);
    (bar, inv)
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

/// Both sets of slots as text.
pub fn encode(bar: &[Option<Preset>], inv: &[Option<Preset>]) -> String {
    let mut out = String::from("og-paper-hotbar 1\n");
    for (tag, slots) in [("h", bar), ("i", inv)] {
        for (i, s) in slots.iter().enumerate() {
            if let Some(p) = s {
                out += &format!("{tag} {i} {}\n", put(p));
            }
        }
    }
    out
}

/// Read what [`encode`] wrote; slots that do not parse stay empty.
pub fn decode(text: &str) -> Option<Slots> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "og-paper-hotbar 1" {
        return None;
    }
    let mut bar = vec![None; BAR];
    let mut inv = vec![None; INVENTORY];
    for l in lines {
        let mut w = l.splitn(3, ' ');
        let (Some(tag), Some(i), Some(rest)) = (w.next(), w.next(), w.next()) else {
            continue;
        };
        let Ok(i) = i.parse::<usize>() else {
            continue;
        };
        let slots = match tag {
            "h" => &mut bar,
            "i" => &mut inv,
            _ => continue,
        };
        if let Some(slot) = slots.get_mut(i) {
            *slot = get(rest);
        }
    }
    Some((bar, inv))
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    Some(crate::fonts_dir()?.parent()?.join("hotbar.txt"))
}

/// The saved slots, or the defaults.
pub fn load() -> Slots {
    #[cfg(not(target_arch = "wasm32"))]
    let text = path().and_then(|p| std::fs::read_to_string(p).ok());
    #[cfg(target_arch = "wasm32")]
    let text = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("og-hotbar").ok().flatten());
    text.and_then(|t| decode(&t)).unwrap_or_else(defaults)
}

pub fn save(bar: &[Option<Preset>], inv: &[Option<Preset>]) {
    let text = encode(bar, inv);
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
        let (bar, inv) = defaults();
        assert_eq!(bar.len(), BAR);
        let text = encode(&bar, &inv);
        let (b2, i2) = decode(&text).unwrap();
        assert_eq!(b2, bar);
        assert_eq!(i2, inv);
        assert!(decode("nonsense").is_none());
        // A bad line leaves its slot empty.
        let (b3, _) = decode("og-paper-hotbar 1\nh 2 pen ink zz\nh 4 eraser\n").unwrap();
        assert!(b3[2].is_none() && b3[4] == Some(Preset::tool(Tool::Eraser)));
    }
}
