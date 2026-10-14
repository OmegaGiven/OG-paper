// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Saved colors (Swatches, under every color dial): + keeps the color in
//! use, a tap takes one, a right click or a long press lets it go. Kept in
//! the person's preferences, so every tool and every canvas shares them.

use std::cell::RefCell;

const KEY: &str = "swatches";
const MAX: usize = 24;

thread_local! {
    static CACHE: RefCell<Option<Vec<u32>>> = const { RefCell::new(None) };
}

fn decode(s: &str) -> Vec<u32> {
    s.split(',')
        .filter_map(|h| u32::from_str_radix(h.trim(), 16).ok())
        .take(MAX)
        .collect()
}

fn encode(v: &[u32]) -> String {
    v.iter()
        .map(|c| format!("{c:08x}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The saved colors (RGBA8, as `u32::from_le_bytes`), oldest first.
pub fn list() -> Vec<u32> {
    CACHE.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| {
                crate::prefs::load()
                    .get(KEY)
                    .map(|s| decode(s))
                    .unwrap_or_default()
            })
            .clone()
    })
}

fn store(v: Vec<u32>) {
    let mut p = crate::prefs::load();
    p.insert(KEY.into(), encode(&v));
    crate::prefs::save(&p);
    CACHE.with(|c| *c.borrow_mut() = Some(v));
}

/// Keep `color` (once; the oldest goes when there are too many).
pub fn add(color: u32) {
    let mut v = list();
    v.retain(|&c| c != color);
    v.push(color);
    if v.len() > MAX {
        v.remove(0);
    }
    store(v);
}

pub fn remove(color: u32) {
    let mut v = list();
    v.retain(|&c| c != color);
    store(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_keep_order_once_each() {
        assert_eq!(decode(&encode(&[1, 0xff00_00ff])), vec![1, 0xff00_00ff]);
        add(0x1111_11ff);
        add(0x2222_22ff);
        add(0x1111_11ff);
        let v = list();
        let i = v.iter().position(|&c| c == 0x2222_22ff).unwrap();
        let j = v.iter().position(|&c| c == 0x1111_11ff).unwrap();
        assert!(i < j, "re-adding moves it last");
        remove(0x2222_22ff);
        assert!(!list().contains(&0x2222_22ff));
        CACHE.with(|c| *c.borrow_mut() = None);
        assert!(list().contains(&0x1111_11ff), "kept in prefs");
    }
}
