// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Personal preferences (grid, button layout, ...): `key=value` lines kept in
//! `~/OG Paper/prefs.txt` on desktop and in browser storage on the web. They
//! belong to the person, not to a canvas.

use std::collections::BTreeMap;

pub type Prefs = BTreeMap<String, String>;

pub fn decode(text: &str) -> Prefs {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

pub fn encode(p: &Prefs) -> String {
    p.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<std::path::PathBuf> {
    Some(crate::fonts_dir()?.parent()?.join("prefs.txt"))
}

pub fn load() -> Prefs {
    #[cfg(not(target_arch = "wasm32"))]
    let text = path().and_then(|p| std::fs::read_to_string(p).ok());
    #[cfg(target_arch = "wasm32")]
    let text = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("og-prefs").ok().flatten());
    text.map(|t| decode(&t)).unwrap_or_default()
}

pub fn save(p: &Prefs) {
    let text = encode(p);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = path() {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(path, text);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("og-prefs", &text);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let mut p = super::Prefs::new();
        p.insert("grid".into(), "dots".into());
        p.insert("layout".into(), "tool:br".into());
        assert_eq!(super::decode(&super::encode(&p)), p);
    }
}
