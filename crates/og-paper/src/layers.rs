// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Layers (Settings > Layers). Which layer a thing is on is its draw
//! order's band (see `ogpaper_core::scene::LAYER_SPAN`), so it is saved,
//! synced, undone and exported with what is drawn. What this module keeps
//! besides is each layer's name and whether it is hidden or locked, per
//! canvas, on this device.
//!
//! Moving a layer up or down swaps its band with its neighbour's, and
//! moving the selection to a layer puts it on that layer's top: both are
//! ordinary edits (one undo step each) that sync like any other.

use std::collections::BTreeMap;

use ogpaper_core::{layer_base, layer_of};

use crate::objects::ObjRef;
use crate::App;

/// What is kept about a layer besides its strokes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerMeta {
    pub name: String,
}

/// A layer as the Layers panel shows it, top first.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub layer: i32,
    pub name: String,
    pub hidden: bool,
    pub locked: bool,
    pub active: bool,
    /// Things on it.
    pub count: usize,
}

fn prefs_key(canvas: u128) -> String {
    format!("layers.{canvas:032x}")
}

/// `layer,hidden,locked,name;…` (names lose `;` and newlines).
fn encode(meta: &BTreeMap<i32, LayerMeta>, hidden: &[i32], locked: &[i32], active: i32) -> String {
    let mut out = format!("active {active};");
    for (k, m) in meta {
        let name: String = m
            .name
            .chars()
            .map(|c| {
                if matches!(c, ';' | '\n' | '\r') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        out += &format!(
            "{k},{},{},{name};",
            hidden.contains(k) as u8,
            locked.contains(k) as u8
        );
    }
    out
}

/// What `encode` wrote: names, hidden, locked, active.
fn decode(s: &str) -> (BTreeMap<i32, LayerMeta>, Vec<i32>, Vec<i32>, i32) {
    let (mut meta, mut hidden, mut locked, mut active) = (BTreeMap::new(), vec![], vec![], 0);
    for part in s.split(';').filter(|p| !p.is_empty()) {
        if let Some(a) = part.strip_prefix("active ") {
            active = a.trim().parse().unwrap_or(0);
            continue;
        }
        let mut w = part.splitn(4, ',');
        let (Some(Ok(k)), Some(h), Some(l), Some(name)) = (
            w.next().map(str::parse::<i32>),
            w.next(),
            w.next(),
            w.next(),
        ) else {
            continue;
        };
        if h == "1" {
            hidden.push(k);
        }
        if l == "1" {
            locked.push(k);
        }
        meta.insert(
            k,
            LayerMeta {
                name: name.to_string(),
            },
        );
    }
    (meta, hidden, locked, active)
}

impl App {
    /// This canvas's layers, as kept on this device (on opening a canvas).
    pub(crate) fn layers_load(&mut self) {
        let canvas = self.share.canvas;
        self.layers_for = Some(canvas);
        let (meta, hidden, locked, active) = crate::prefs::load()
            .get(&prefs_key(canvas))
            .map(|s| decode(s))
            .unwrap_or_default();
        self.layer_meta = meta;
        self.scene.hidden_layers = hidden;
        self.scene.locked_layers = locked;
        self.scene.layer = active;
    }

    fn layers_save(&self) {
        let mut p = crate::prefs::load();
        p.insert(
            prefs_key(self.share.canvas),
            encode(
                &self.layer_meta,
                &self.scene.hidden_layers,
                &self.scene.locked_layers,
                self.scene.layer,
            ),
        );
        crate::prefs::save(&p);
    }

    /// Every layer: in use, named, or active; bottom first.
    fn layers_all(&self) -> Vec<i32> {
        let mut v = self.scene.layers_in_use();
        v.extend(self.layer_meta.keys().copied());
        v.push(self.scene.layer);
        v.sort_unstable();
        v.dedup();
        v
    }

    fn layer_name(&self, k: i32) -> String {
        self.layer_meta
            .get(&k)
            .map(|m| m.name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                let pos = self.layers_all().iter().position(|&x| x == k).unwrap_or(0);
                format!("Layer {}", pos + 1)
            })
    }

    /// The rows for the Layers panel, top first.
    pub(crate) fn layer_rows(&self) -> Vec<LayerRow> {
        let mut count: BTreeMap<i32, usize> = BTreeMap::new();
        let mut seen = std::collections::HashSet::new();
        for (i, s) in self.scene.strokes.iter().enumerate() {
            if !s.deleted && seen.insert(self.objs.obj_of(i as u32)) {
                *count.entry(layer_of(s.z)).or_default() += 1;
            }
        }
        self.layers_all()
            .into_iter()
            .rev()
            .map(|k| LayerRow {
                layer: k,
                name: self.layer_name(k),
                hidden: self.scene.hidden_layers.contains(&k),
                locked: self.scene.locked_layers.contains(&k),
                active: k == self.scene.layer,
                count: count.get(&k).copied().unwrap_or(0),
            })
            .collect()
    }

    /// Everything on layer `k`, bottom to top.
    fn layer_objects(&self, k: i32) -> Vec<ObjRef> {
        let mut ids: Vec<u32> = (0..self.scene.strokes.len() as u32)
            .filter(|&i| {
                let s = &self.scene.strokes[i as usize];
                !s.deleted && layer_of(s.z) == k
            })
            .collect();
        ids.sort_by(|a, b| {
            self.scene.strokes[*a as usize]
                .z
                .total_cmp(&self.scene.strokes[*b as usize].z)
        });
        let mut seen = std::collections::HashSet::new();
        ids.into_iter()
            .map(|i| self.objs.obj_of(i))
            .filter(|r| seen.insert(*r))
            .collect()
    }

    /// Replace `refs` with copies whose draw orders go through `f`, as one
    /// undo step. With `select`, the copies are the selection after;
    /// else the selection is what it was (less what moved).
    fn restack(&mut self, refs: Vec<ObjRef>, select: bool, f: impl Fn(f64) -> f64) {
        if refs.is_empty() {
            return;
        }
        let saved = std::mem::replace(&mut self.edit.selection, refs);
        let diagram = std::mem::replace(&mut self.ui.diagram, false);
        self.replace_selection(|app, r| {
            Some(match r {
                ObjRef::Ink(id) => {
                    let z = f(app.scene.strokes[id as usize].z);
                    app.ink_copy(id, None, None, Some(z))
                }
                ObjRef::Group(g) => {
                    let (lo, hi) = app.z_range(&app.objs.groups[g as usize].strokes.clone());
                    app.group_copy(g, |d| d, Some((f(lo), f(hi))))
                }
            })
        });
        self.ui.diagram = diagram;
        if !select {
            self.edit.selection = saved;
        }
        self.sel_alive();
    }

    /// A new, empty layer above all the others; it becomes the active one.
    pub(crate) fn layer_new(&mut self) {
        let k = self.layers_all().last().copied().unwrap_or(0) + 1;
        let n = self.layers_all().len() + 1;
        self.layer_meta.insert(
            k,
            LayerMeta {
                name: format!("Layer {n}"),
            },
        );
        self.scene.layer = k;
        self.layers_save();
        self.say(format!("Drawing on {}", self.layer_name(k)));
        self.redraw();
    }

    pub(crate) fn layer_pick(&mut self, k: i32) {
        self.scene.layer = k;
        self.layers_save();
        self.redraw();
    }

    pub(crate) fn layer_rename(&mut self, k: i32, name: String) {
        let name: String = name.trim().chars().take(60).collect();
        self.layer_meta.entry(k).or_default().name = name;
        self.layers_save();
    }

    /// Show or hide layer `k` (what is selected on it is let go).
    pub(crate) fn layer_toggle_hidden(&mut self, k: i32) {
        toggle(&mut self.scene.hidden_layers, k);
        self.deselect_layer(k);
        self.layers_save();
        self.redraw();
    }

    pub(crate) fn layer_toggle_locked(&mut self, k: i32) {
        toggle(&mut self.scene.locked_layers, k);
        self.deselect_layer(k);
        self.layers_save();
        self.redraw();
    }

    fn deselect_layer(&mut self, k: i32) {
        let scene = &self.scene;
        let objs = &self.objs;
        self.edit.selection.retain(|r| {
            objs.strokes(r)
                .first()
                .is_none_or(|&s| layer_of(scene.strokes[s as usize].z) != k)
        });
    }

    /// Swap layer `k` with the next one up (`up`) or down.
    pub(crate) fn layer_move(&mut self, k: i32, up: bool) {
        let all = self.layers_all();
        let Some(i) = all.iter().position(|&x| x == k) else {
            return;
        };
        let j = if up { i + 1 } else { i.wrapping_sub(1) };
        let Some(&m) = all.get(j) else {
            return;
        };
        // Both layers' things, each into the other's band.
        let shift = layer_base(m) - layer_base(k);
        let mut refs = self.layer_objects(k);
        refs.extend(self.layer_objects(m));
        self.restack(refs, false, |z| match layer_of(z) {
            l if l == k => z + shift,
            _ => z - shift,
        });
        // Their names and states go with them (and back, with undo).
        self.layer_swap_meta(k, m);
        self.layer_undo.push((self.history.undo_len(), k, m));
        self.layer_redo.clear();
        self.layers_save();
        self.redraw();
    }

    /// Swap layers `k` and `m`'s names, states and which is active.
    fn layer_swap_meta(&mut self, k: i32, m: i32) {
        // Names shown by position ("Layer 2") are kept as they read now.
        for x in [k, m] {
            let name = self.layer_name(x);
            self.layer_meta.entry(x).or_default().name = name;
        }
        let (a, b) = (self.layer_meta.remove(&k), self.layer_meta.remove(&m));
        if let Some(a) = a {
            self.layer_meta.insert(m, a);
        }
        if let Some(b) = b {
            self.layer_meta.insert(k, b);
        }
        for list in [&mut self.scene.hidden_layers, &mut self.scene.locked_layers] {
            for x in list.iter_mut() {
                if *x == k {
                    *x = m;
                } else if *x == m {
                    *x = k;
                }
            }
        }
        if self.scene.layer == k {
            self.scene.layer = m;
        } else if self.scene.layer == m {
            self.scene.layer = k;
        }
    }

    /// After an undo (`redo` false) or redo of the step at `depth` (the
    /// undo steps there were with it done): a layer move's names and
    /// states follow its strokes.
    pub(crate) fn layer_undo_redo(&mut self, redo: bool, depth: usize) {
        let (from, to) = if redo {
            (&mut self.layer_redo, &mut self.layer_undo)
        } else {
            (&mut self.layer_undo, &mut self.layer_redo)
        };
        if let Some(&(d, k, m)) = from.last().filter(|e| e.0 == depth) {
            from.pop();
            to.push((d, k, m));
            self.layer_swap_meta(k, m);
            self.layers_save();
        }
        if !self.history.can_redo() {
            self.layer_redo.clear();
        }
    }

    /// Delete layer `k` and everything on it (one undo step).
    pub(crate) fn layer_delete(&mut self, k: i32) {
        let all = self.layers_all();
        if all.len() < 2 {
            self.say("The last layer stays: clear it instead");
            return;
        }
        let refs = self.layer_objects(k);
        if !refs.is_empty() {
            self.edit.selection = refs;
            self.delete_selection();
        }
        self.layer_meta.remove(&k);
        self.scene.hidden_layers.retain(|&x| x != k);
        self.scene.locked_layers.retain(|&x| x != k);
        if self.scene.layer == k {
            let i = all.iter().position(|&x| x == k).unwrap_or(0);
            self.scene.layer = if i > 0 { all[i - 1] } else { all[1] };
        }
        self.layers_save();
        self.redraw();
    }

    /// Move the selection onto layer `k`, on its top, keeping its order.
    pub(crate) fn layer_take_selection(&mut self, k: i32) {
        self.sel_alive();
        let refs = self.sel_sorted_pub();
        if refs.is_empty() {
            self.say("Select something to move to this layer");
            return;
        }
        let lo = refs
            .iter()
            .flat_map(|r| self.objs.strokes(r).iter())
            .map(|&s| self.scene.strokes[s as usize].z)
            .fold(f64::MAX, f64::min);
        let to = self.scene.layer_top(k) + 1.0 - lo;
        self.restack(refs, true, |z| z + to);
        self.say(format!("Moved to {}", self.layer_name(k)));
        self.redraw();
    }

    /// Before drawing: refused on a hidden or locked active layer.
    pub(crate) fn layer_blocks_drawing(&mut self) -> bool {
        let k = self.scene.layer;
        let why = if self.scene.hidden_layers.contains(&k) {
            "hidden"
        } else if self.scene.locked_layers.contains(&k) {
            "locked"
        } else {
            return false;
        };
        self.say(format!(
            "{} is {why}: show or unlock it in Layers, or pick another layer",
            self.layer_name(k)
        ));
        true
    }
}

fn toggle(v: &mut Vec<i32>, k: i32) {
    if let Some(i) = v.iter().position(|&x| x == k) {
        v.remove(i);
    } else {
        v.push(k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn add(app: &mut App, text: &str) {
        let r = app.run_commands(&json!([{"add": "text", "text": text, "x": 0, "y": 0}]));
        assert_eq!(r[0]["ok"], true);
    }

    fn texts_bottom_up(app: &App) -> Vec<String> {
        let mut v: Vec<(f64, String)> = app
            .objs
            .groups
            .iter()
            .filter(|g| {
                g.strokes
                    .first()
                    .is_some_and(|&s| !app.scene.strokes[s as usize].deleted)
            })
            .filter_map(|g| match &g.data {
                crate::objects::ObjData::Text { text, .. } => {
                    Some((app.scene.strokes[g.strokes[0] as usize].z, text.clone()))
                }
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|x| x.1).collect()
    }

    #[test]
    fn layers_keep_order_hide_lock_and_move() {
        let mut app = App::new(None);
        add(&mut app, "base");
        app.layer_new();
        add(&mut app, "top");
        // A new layer draws above the old one, whatever was added first.
        app.layer_pick(0);
        add(&mut app, "base 2");
        assert_eq!(texts_bottom_up(&app), ["base", "base 2", "top"]);
        let rows = app.layer_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].count, rows[1].count), (1, 2));
        assert!(rows[1].active);

        // Hidden: not drawn (the scene skips it).
        let top = rows[0].layer;
        app.layer_toggle_hidden(top);
        let shown = (0..app.scene.strokes.len() as u32)
            .filter(|&i| app.scene.shown(i))
            .count();
        let all = app.scene.strokes.iter().filter(|s| !s.deleted).count();
        assert!(shown < all);
        app.layer_toggle_hidden(top);
        // Locked: not editable.
        app.layer_toggle_locked(top);
        let g = app
            .objs
            .groups
            .iter()
            .position(
                |g| matches!(&g.data, crate::objects::ObjData::Text { text, .. } if text == "top"),
            )
            .unwrap();
        let s = app.objs.groups[g].strokes[0];
        assert!(!app.scene.editable(s));
        app.layer_toggle_locked(top);

        // Move the base layer up: its things now draw above, its name too.
        app.layer_rename(0, "Sketch".into());
        let names = |app: &App| {
            app.layer_rows()
                .iter()
                .map(|r| r.name.clone())
                .collect::<Vec<_>>()
        };
        let before = names(&app);
        app.layer_move(0, true);
        assert_eq!(texts_bottom_up(&app), ["top", "base", "base 2"]);
        assert_eq!(names(&app)[0], "Sketch");
        // One undo puts it back, names and all; redo again.
        app.undo_redo(false);
        assert_eq!(texts_bottom_up(&app), ["base", "base 2", "top"]);
        assert_eq!(names(&app), before);
        app.undo_redo(true);
        assert_eq!(names(&app)[0], "Sketch");
        app.undo_redo(false);

        // Move "base 2" to the top layer.
        let g = app
            .objs
            .groups
            .iter()
            .enumerate()
            .rposition(|(i, g)| {
                matches!(&g.data, crate::objects::ObjData::Text { text, .. } if text == "base 2")
                    && app.objs.alive(&app.scene, &ObjRef::Group(i as u32))
            })
            .unwrap();
        app.edit.selection = vec![ObjRef::Group(g as u32)];
        app.layer_take_selection(top);
        assert_eq!(texts_bottom_up(&app), ["base", "top", "base 2"]);

        // Delete the top layer: its things go (one undo step brings them back).
        app.layer_delete(top);
        assert_eq!(texts_bottom_up(&app), ["base"]);
        app.undo_redo(false);
        assert_eq!(texts_bottom_up(&app), ["base", "top", "base 2"]);
    }

    #[test]
    fn layer_states_round_trip() {
        let mut meta = BTreeMap::new();
        meta.insert(
            0,
            LayerMeta {
                name: "Sketch".into(),
            },
        );
        meta.insert(
            2,
            LayerMeta {
                name: "Ink; final".into(),
            },
        );
        let s = encode(&meta, &[2], &[0], 2);
        let (m, h, l, a) = decode(&s);
        assert_eq!(m.get(&0).unwrap().name, "Sketch");
        assert_eq!(m.get(&2).unwrap().name, "Ink  final");
        assert_eq!((h, l, a), (vec![2], vec![0], 2));
    }
}
