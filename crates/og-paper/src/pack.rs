// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Packs: toolbars, tools (brushes with all their settings, shapes, text
//! styles) and stickers to share, with no code. A pack is a text file:
//!
//! ```text
//! og-paper-pack 1
//! name Watercolor set
//! b 0 Washes                      (toolbars and slots, as in the toolbar file)
//! h 0 0 pen …
//! i 3 pen …                       (inventory tools)
//! sticker Leaf <base64 .ogps>     (library stickers)
//! ```
//!
//! Installing adds its toolbars (named "<pack>: <toolbar>"), puts its tools
//! in free inventory slots and its stickers in the library; nothing of
//! yours is replaced.

use crate::hotbar;
use crate::App;

pub const HEADER: &str = "og-paper-pack 1";

/// A pack's name, its toolbar-file part and its stickers.
pub fn parse(text: &str) -> Result<(String, hotbar::Saved, Vec<(String, Vec<u8>)>), String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Err("not an OG Paper pack".into());
    }
    let mut name = "Pack".to_string();
    let mut bar_text = String::from("og-paper-hotbar 2\n");
    let mut stickers = Vec::new();
    for l in lines {
        let l = l.trim_end();
        if let Some(n) = l.strip_prefix("name ") {
            name = n.trim().chars().take(60).collect();
        } else if let Some(rest) = l.strip_prefix("sticker ") {
            if let Some((n, b)) = rest.rsplit_once(' ') {
                if let Some(bytes) = crate::seal::unb64(b) {
                    stickers.push((n.trim().to_string(), bytes));
                }
            }
        } else if l.starts_with("b ")
            || l.starts_with("h ")
            || l.starts_with("i ")
            || l.starts_with("s ")
        {
            bar_text += l;
            bar_text.push('\n');
        }
    }
    let saved = hotbar::decode(&bar_text).ok_or("the pack's tools did not read")?;
    Ok((name, saved, stickers))
}

impl App {
    /// The toolbars and inventory as a pack.
    pub(crate) fn pack_export(&self) -> String {
        let text = hotbar::encode(&self.ui.saved());
        let mut out = format!("{HEADER}\nname My toolbars\n");
        for l in text.lines().skip(1) {
            if l.starts_with("b ") || l.starts_with("h ") || l.starts_with("i ") {
                out += l;
                out.push('\n');
            }
        }
        out
    }

    /// Install a pack: its toolbars, tools and stickers join yours.
    pub(crate) fn pack_install(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let (name, saved, stickers) = match parse(&text) {
            Ok(p) => p,
            Err(e) => return self.say(format!("Could not install that pack: {e}")),
        };
        let mut bars = 0;
        for mut b in saved.bars {
            if b.slots.iter().all(Option::is_none) {
                continue;
            }
            b.name = format!("{name}: {}", b.name);
            b.shown = false;
            self.ui.toolbars.push(b);
            bars += 1;
        }
        let mut tools = 0;
        for t in saved.inv.into_iter().flatten() {
            match self.ui.inventory.iter().position(Option::is_none) {
                Some(k) => self.ui.inventory[k] = Some(t),
                None => self.ui.inventory.push(Some(t)),
            }
            tools += 1;
        }
        hotbar::grow_inventory(&mut self.ui.inventory);
        self.ui.presets_dirty = true;
        let n = stickers.len();
        #[cfg(not(target_arch = "wasm32"))]
        for (s, b) in &stickers {
            let _ = crate::library::store::save(s, b);
        }
        #[cfg(target_arch = "wasm32")]
        crate::web::pack_stickers(stickers);
        self.say(format!(
            "Installed {name}: {bars} toolbar{}, {tools} tool{}, {n} sticker{}",
            if bars == 1 { "" } else { "s" },
            if tools == 1 { "" } else { "s" },
            if n == 1 { "" } else { "s" }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_read_toolbars_tools_and_stickers() {
        let saved = hotbar::defaults();
        let tools = hotbar::encode(&saved);
        let mut pack = format!("{HEADER}\nname Test set\n");
        for l in tools
            .lines()
            .skip(1)
            .filter(|l| l.starts_with("b ") || l.starts_with("h "))
        {
            pack += l;
            pack.push('\n');
        }
        pack += &format!("sticker A leaf {}\n", crate::seal::b64(b"OGPS-bytes"));
        let (name, got, stickers) = parse(&pack).unwrap();
        assert_eq!(name, "Test set");
        assert_eq!(got.bars.len(), saved.bars.len());
        assert_eq!(got.bars[0].slots, saved.bars[0].slots);
        assert_eq!(
            stickers,
            vec![("A leaf".to_string(), b"OGPS-bytes".to_vec())]
        );
        assert!(parse("hello").is_err());
    }
}
