// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! An example OG Paper plugin (see docs/PLUGINS.md): three buttons that
//! answer with drawing commands. Plain Rust, no dependencies; build with
//! `cargo build --release --target wasm32-unknown-unknown` and install the
//! `.wasm` from target/wasm32-unknown-unknown/release in Settings > Plugins.

use std::fmt::Write;

/// Give the app room for the input (it writes it there).
#[no_mangle]
pub extern "C" fn og_alloc(len: i32) -> i32 {
    let mut v = Vec::<u8>::with_capacity(len.max(0) as usize);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p as i32
}

/// Hand a string to the app: (ptr << 32) | len. It stays alive: each run is
/// a fresh instance, so nothing builds up.
fn give(s: String) -> i64 {
    let b = s.into_bytes().into_boxed_slice();
    let len = b.len() as i64;
    let p = Box::into_raw(b) as *mut u8 as i64;
    (p << 32) | len
}

#[no_mangle]
pub extern "C" fn og_manifest() -> i64 {
    give(
        r#"{"name": "Starter kit", "version": "1.0",
            "description": "Example plugin: a dot grid, a spiral and a text count",
            "buttons": [{"id": 1, "label": "Dot grid here"},
                        {"id": 2, "label": "Spiral"},
                        {"id": 3, "label": "Count texts"}]}"#
            .into(),
    )
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
    let view = input.find("\"view\"").map_or(input, |i| &input[i..]);
    let (x, y, w, h) = (num(view, "x"), num(view, "y"), num(view, "w"), num(view, "h"));
    let mut out = String::from("[");
    match button {
        1 => {
            // A grid of small dots over the middle of the view.
            let step = (w.min(h) / 12.0).max(1e-9);
            let r = step * 0.06;
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let mut first = true;
            for i in -5..=5 {
                for j in -5..=5 {
                    let (px, py) = (cx + i as f64 * step, cy + j as f64 * step);
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    let _ = write!(
                        out,
                        r##"{{"add":"shape","kind":"ellipse","x":{},"y":{},"w":{},"h":{},"fill":"#5a6478","color":"#5a6478","width":{}}}"##,
                        px - r,
                        py - r,
                        2.0 * r,
                        2.0 * r,
                        r * 0.3
                    );
                }
            }
        }
        2 => {
            // An Archimedean spiral in the middle of the view.
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let size = w.min(h) * 0.35;
            out.push_str(r##"{"add":"stroke","color":"#c82850","width":"##);
            let _ = write!(out, "{},\"points\":[", size * 0.012);
            for k in 0..=400 {
                let t = k as f64 / 400.0 * 6.0 * std::f64::consts::PI;
                let rr = size * t / (6.0 * std::f64::consts::PI);
                if k > 0 {
                    out.push(',');
                }
                let _ = write!(out, "[{},{}]", cx + rr * t.cos(), cy + rr * t.sin());
            }
            out.push_str("]}");
        }
        3 => {
            let n = input.matches("\"text\":").count();
            let _ = write!(
                out,
                r##"{{"add":"text","x":{},"y":{},"size":{},"color":"#1e5ac8","text":"{} text{} on this page"}},{{"say":"Counted {}"}}"##,
                x + w * 0.05,
                y + h * 0.05,
                h * 0.04,
                n,
                if n == 1 { "" } else { "s" },
                n
            );
        }
        _ => out.push_str(r#"{"say":"Unknown button"}"#),
    }
    out.push(']');
    give(out)
}
