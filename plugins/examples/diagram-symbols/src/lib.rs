// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Diagram symbols: an OG Paper plugin (see docs/PLUGINS.md) whose tools
//! place the symbols people reach for most, like draw.io's: a process box
//! and a decision for flowcharts, a resistor, capacitor, diode, ground,
//! switch and battery for circuits, a gear and a spring for machines. Each
//! is a tool in the tool fan: pick it, tap the canvas, and the symbol is
//! there, sized to the view. The app draws them (`{"add": "symbol"}`), with
//! pins that wires join in diagram mode; Menu › Symbols has all of them.
//! Build with `cargo build --release --target wasm32-unknown-unknown`.

/// The tools: button id, label, symbol.
const TOOLS: &[(i32, &str, &str)] = &[
    (1, "Process", "process"),
    (2, "Decision", "decision"),
    (3, "Resistor", "resistor"),
    (4, "Capacitor", "capacitor"),
    (5, "Diode", "diode"),
    (6, "Ground", "ground"),
    (7, "Switch", "switch"),
    (8, "Battery", "battery"),
    (9, "Gear", "gear"),
    (10, "Spring", "spring"),
];

/// Give the app room for the input (it writes it there).
#[no_mangle]
pub extern "C" fn og_alloc(len: i32) -> i32 {
    let mut v = Vec::<u8>::with_capacity(len.max(0) as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p as i32
}

/// Hand a string to the app: (ptr << 32) | len.
fn give(s: String) -> i64 {
    let b = s.into_bytes().into_boxed_slice();
    let len = b.len() as i64;
    let p = Box::into_raw(b) as *mut u8 as i64;
    (p << 32) | len
}

#[no_mangle]
pub extern "C" fn og_manifest() -> i64 {
    let buttons: Vec<String> = TOOLS
        .iter()
        .map(|(id, label, sym)| {
            format!(r#"{{"id": {id}, "label": "{label}", "icon": "symbol:{sym}", "place": true}}"#)
        })
        .collect();
    give(format!(
        r#"{{"name": "Diagram symbols", "version": "1.0",
            "description": "Tools that place flowchart, electrical and mechanical symbols with pins wires join (all of them: Menu › Symbols)",
            "buttons": [{}]}}"#,
        buttons.join(", ")
    ))
}

/// The number after `"key":` in `s` (enough for the app's flat input).
fn num(s: &str, key: &str) -> f64 {
    let k = format!("\"{key}\":");
    s.find(&k)
        .map(|i| &s[i + k.len()..])
        .map(|r| {
            r.trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit() || "-+.eE".contains(*c))
                .collect::<String>()
        })
        .and_then(|n| n.parse().ok())
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn og_run(button: i32, ptr: i32, len: i32) -> i64 {
    // SAFETY: the app wrote `len` bytes at `ptr`, which og_alloc gave it.
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len.max(0) as usize) };
    let input = std::str::from_utf8(input).unwrap_or("{}");
    let Some(&(_, _, sym)) = TOOLS.iter().find(|t| t.0 == button) else {
        return give("[]".into());
    };
    let view = input.find("\"view\"").map_or(input, |i| &input[i..]);
    let (x, y, w, h) = (num(view, "x"), num(view, "y"), num(view, "w"), num(view, "h"));
    let (cx, cy) = match input.find("\"at\"").map(|i| &input[i..]) {
        Some(at) => (num(at, "x"), num(at, "y")),
        None => (x + w / 2.0, y + h / 2.0),
    };
    // A grid unit a seventieth of the screen's smaller side.
    let size = w.min(h) / 70.0;
    give(format!(
        r#"[{{"add": "symbol", "name": "{sym}", "cx": {cx}, "cy": {cy}, "size": {size}}}]"#
    ))
}
