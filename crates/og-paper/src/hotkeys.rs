// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Hotkeys you can set (Settings > Hotkeys): tools, brush and texture
//! looks, and commands. Defaults are the keys of old; what you change is
//! kept in the prefs store as `keys=id:combo,id:combo` (`-` = no key).
//! A few keys stay fixed: 1-9 (quick bar), Alt+1-9, [ and ], arrows,
//! Delete, Escape, Enter, Home, and Ctrl+C / X / V / A / D / Y.

use std::collections::BTreeMap;

/// A key with modifiers, e.g. `ctrl+shift+s`, `b`, `f5`.
#[derive(Clone, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// A lowercase character, or a named key in lowercase (`f5`, `tab`).
    pub key: String,
}

impl Combo {
    pub fn parse(s: &str) -> Option<Combo> {
        let mut c = Combo {
            ctrl: false,
            shift: false,
            alt: false,
            key: String::new(),
        };
        for part in s.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                "" => {}
                k => c.key = k.to_string(),
            }
        }
        (!c.key.is_empty()).then_some(c)
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "ctrl+";
        }
        if self.alt {
            s += "alt+";
        }
        if self.shift {
            s += "shift+";
        }
        s + &self.key
    }

    /// For people: `Ctrl+Shift+S`, `B`, `F5`.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.alt {
            s += "Alt+";
        }
        if self.shift {
            s += "Shift+";
        }
        let k = if self.key.chars().count() == 1 {
            self.key.to_uppercase()
        } else {
            let mut c = self.key.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        };
        s + &k
    }

    /// Keys the app keeps for itself.
    pub fn fixed(&self) -> bool {
        let k = self.key.as_str();
        let digit = k.len() == 1 && ("1"..="9").contains(&k);
        (digit && !self.ctrl)
            || (self.ctrl && !self.alt && matches!(k, "c" | "x" | "v" | "a" | "d" | "y"))
            || (!self.ctrl && !self.alt && matches!(k, "[" | "]"))
            || matches!(
                k,
                "escape"
                    | "enter"
                    | "delete"
                    | "backspace"
                    | "home"
                    | "arrowleft"
                    | "arrowright"
                    | "arrowup"
                    | "arrowdown"
                    | "space"
            )
    }
}

/// Something a hotkey can do.
pub struct Binding {
    pub id: String,
    pub label: String,
    pub group: &'static str,
    pub default: Option<&'static str>,
}

/// Everything that can have a hotkey, in menu order.
pub fn bindings() -> Vec<Binding> {
    let mut v = Vec::new();
    let mut add = |id: &str, label: &str, group: &'static str, default: Option<&'static str>| {
        v.push(Binding {
            id: id.into(),
            label: label.into(),
            group,
            default,
        });
    };
    for (id, label, d) in [
        ("tool.brush", "Brush", Some("b")),
        ("tool.texture", "Texture", Some("k")),
        ("tool.highlighter", "Highlighter", Some("m")),
        ("tool.bucket", "Bucket", Some("g")),
        ("tool.eraser", "Eraser", Some("e")),
        ("tool.select", "Select (again: Lasso)", Some("v")),
        ("tool.lasso", "Lasso", None),
        ("tool.shapes", "Shapes", Some("s")),
        ("shape.rect", "Rectangle", Some("r")),
        ("shape.ellipse", "Ellipse", Some("o")),
        ("shape.diamond", "Diamond", Some("d")),
        ("shape.arrow", "Arrow", Some("a")),
        ("shape.line", "Line", Some("l")),
        ("tool.text", "Text", Some("t")),
        ("tool.picker", "Color picker", Some("i")),
        ("tool.hand", "Pan", Some("h")),
    ] {
        add(id, label, "Tools", d);
    }
    add("brush.simple", "Brush: simple line", "Brush looks", None);
    for l in ogpaper_core::brush::looks() {
        let (group, kind) = if l.texture {
            ("Texture looks", "Texture")
        } else {
            ("Brush looks", "Brush")
        };
        add(
            &format!("look.{}", l.params.look),
            &format!("{kind}: {}", l.name),
            group,
            None,
        );
    }
    for (id, label, d) in [
        ("cmd.undo", "Undo", Some("ctrl+z")),
        ("cmd.redo", "Redo", Some("ctrl+shift+z")),
        ("cmd.pages", "Pages", None),
        ("cmd.new", "New canvas", Some("ctrl+n")),
        ("cmd.open", "Open", Some("ctrl+o")),
        ("cmd.save", "Save copy", Some("ctrl+s")),
        ("cmd.export", "Export", Some("ctrl+e")),
        ("cmd.paste", "Paste", None),
        ("cmd.picture", "Insert picture / PDF", None),
        ("cmd.import", "Import canvas", None),
        ("cmd.merge", "Merge copy", None),
        ("cmd.changes", "Save changes since last merge", None),
        ("cmd.folder", "Sync folder on/off", None),
        ("cmd.search", "Search text", Some("ctrl+f")),
        ("cmd.bookmarks", "Bookmarks", Some("ctrl+b")),
        ("cmd.timeline", "Timeline", Some("ctrl+h")),
        ("cmd.library", "Library", Some("ctrl+l")),
        ("cmd.home", "Fly home", None),
        ("cmd.grid", "Grid", Some("ctrl+g")),
        ("cmd.dark", "Dark mode", None),
        ("cmd.diagram", "Diagram mode", None),
        ("cmd.layout", "Edit layout", None),
        ("cmd.layoutmenu", "Layout window", None),
        ("cmd.fullscreen", "Full screen", Some("f11")),
        ("cmd.hotkeys", "Hotkeys", Some("ctrl+k")),
    ] {
        add(id, label, "Commands", d);
    }
    v
}

/// The keys in effect: defaults with the person's changes.
#[derive(Clone, Default, Debug)]
pub struct Keymap {
    /// id -> its key (None: unbound on purpose).
    pub keys: BTreeMap<String, Option<Combo>>,
    /// Only the changes, as saved.
    pub changed: BTreeMap<String, Option<Combo>>,
}

impl Keymap {
    /// Defaults, then `saved` (the prefs value) on top.
    pub fn load(saved: &str) -> Keymap {
        let mut m = Keymap::default();
        for b in bindings() {
            m.keys
                .insert(b.id.clone(), b.default.and_then(Combo::parse));
        }
        for part in saved.split(',') {
            let Some((id, c)) = part.split_once(':') else {
                continue;
            };
            if !m.keys.contains_key(id) {
                continue;
            }
            let c = if c.trim() == "-" {
                None
            } else {
                Combo::parse(c)
            };
            m.changed.insert(id.to_string(), c.clone());
            m.keys.insert(id.to_string(), c);
        }
        m
    }

    pub fn save_text(&self) -> String {
        self.changed
            .iter()
            .map(|(id, c)| format!("{id}:{}", c.as_ref().map_or("-".to_string(), Combo::text)))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// What `c` does, if anything.
    pub fn lookup(&self, c: &Combo) -> Option<&str> {
        self.keys
            .iter()
            .find(|(_, k)| k.as_ref() == Some(c))
            .map(|(id, _)| id.as_str())
    }

    /// Give `id` the key `c` (None clears it). A key already in use moves:
    /// returns the id it was taken from.
    pub fn set(&mut self, id: &str, c: Option<Combo>) -> Option<String> {
        let mut taken = None;
        if let Some(c) = &c {
            if let Some(other) = self.lookup(c).map(str::to_string) {
                if other != id {
                    self.keys.insert(other.clone(), None);
                    self.changed.insert(other.clone(), None);
                    taken = Some(other);
                }
            }
        }
        self.keys.insert(id.to_string(), c.clone());
        self.changed.insert(id.to_string(), c);
        taken
    }

    pub fn reset(&mut self) {
        *self = Keymap::load("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_parse_and_print() {
        let c = Combo::parse("Ctrl+Shift+S").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && c.key == "s");
        assert_eq!(c.text(), "ctrl+shift+s");
        assert_eq!(c.label(), "Ctrl+Shift+S");
        assert_eq!(Combo::parse("f5").unwrap().label(), "F5");
        assert!(Combo::parse("ctrl+").is_none());
        assert!(Combo::parse("3").unwrap().fixed());
        assert!(!Combo::parse("ctrl+3").unwrap().fixed());
    }

    #[test]
    fn keymap_changes_round_trip_and_keys_move() {
        let mut m = Keymap::load("");
        assert_eq!(m.lookup(&Combo::parse("b").unwrap()), Some("tool.brush"));
        // Giving B to the eraser takes it from the brush.
        assert_eq!(
            m.set("tool.eraser", Combo::parse("b")),
            Some("tool.brush".into())
        );
        assert_eq!(m.lookup(&Combo::parse("b").unwrap()), Some("tool.eraser"));
        assert_eq!(m.keys["tool.brush"], None);
        let back = Keymap::load(&m.save_text());
        assert_eq!(back.keys, m.keys);
        // Unknown ids in a saved value are ignored.
        assert_eq!(Keymap::load("nope:x").changed.len(), 0);
    }

    #[test]
    fn defaults_have_no_clashes() {
        let m = Keymap::load("");
        let mut seen = std::collections::HashSet::new();
        for c in m.keys.values().flatten() {
            assert!(seen.insert(c.clone()), "{c:?} twice");
            assert!(!c.fixed(), "{c:?} is a fixed key");
        }
    }
}
