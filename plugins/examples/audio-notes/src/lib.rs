// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Audio notes: an OG Paper plugin (see docs/PLUGINS.md) that records a
//! short voice note and leaves it on the canvas as an audio clip, in the
//! middle of what is on screen. It asks for the "microphone" permission;
//! the app does the recording (with a bar to stop it) only once the person
//! allows that in Plugins. Build with
//! `cargo build --release --target wasm32-unknown-unknown`.

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
    give(
        r#"{"name": "Audio notes", "version": "1.0",
            "description": "Record a voice note and leave it on the canvas as a clip anyone on the page can play",
            "permissions": ["microphone"],
            "buttons": [{"id": 1, "label": "Record a note (up to 30 s)"},
                        {"id": 2, "label": "Record a long note (up to 2 min)"}]}"#
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
    // A player a quarter of the screen's smaller side wide, in the middle.
    let pw = w.min(h) * 0.45;
    let ph = pw * 0.24;
    let max = if button == 2 { 120 } else { 30 };
    give(format!(
        r#"[{{"add": "audio", "record": true, "x": {}, "y": {}, "w": {pw}, "h": {ph}, "max_seconds": {max}}}]"#,
        x + (w - pw) / 2.0,
        y + (h - ph) / 2.0
    ))
}
