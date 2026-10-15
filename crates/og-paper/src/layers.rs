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

use crate::objects::{ObjData, ObjRef};
use crate::App;

/// What is kept about a layer besides its strokes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerMeta {
    pub name: String,
    pub fx: LayerFx,
}

/// How a layer's picture mixes with what is under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Add,
    Darken,
    Lighten,
}

impl Blend {
    pub const ALL: [Blend; 6] = [
        Blend::Normal,
        Blend::Multiply,
        Blend::Screen,
        Blend::Add,
        Blend::Darken,
        Blend::Lighten,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Multiply => "Multiply",
            Blend::Screen => "Screen",
            Blend::Add => "Add",
            Blend::Darken => "Darken",
            Blend::Lighten => "Lighten",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Blend::Normal => "normal",
            Blend::Multiply => "multiply",
            Blend::Screen => "screen",
            Blend::Add => "add",
            Blend::Darken => "darken",
            Blend::Lighten => "lighten",
        }
    }

    pub fn from_key(k: &str) -> Option<Blend> {
        Blend::ALL
            .into_iter()
            .find(|b| b.key() == k.trim().to_ascii_lowercase())
    }
}

/// A layer's look as a whole: its opacity, how it mixes with what is under
/// it, and whether it shows only where the layer under it has something
/// (a clipping mask).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerFx {
    pub opacity: f32,
    pub blend: Blend,
    pub clip: bool,
}

impl Default for LayerFx {
    fn default() -> Self {
        LayerFx {
            opacity: 1.0,
            blend: Blend::Normal,
            clip: false,
        }
    }
}

impl LayerFx {
    pub fn plain(&self) -> bool {
        *self == LayerFx::default()
    }
}

/// A layer drawn on its own and then laid over the rest (see `layer_fx`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxDraw {
    pub fx: LayerFx,
    /// It is clipped to this layer (drawn on its own too, as the mask).
    pub clip_to: Option<i32>,
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
    pub fx: LayerFx,
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
        if !m.fx.plain() {
            out += &format!(
                "fx {k},{},{},{};",
                m.fx.opacity,
                m.fx.blend.key(),
                m.fx.clip as u8
            );
        }
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
        if let Some(f) = part.strip_prefix("fx ") {
            let w: Vec<&str> = f.split(',').collect();
            if let [k, o, b, c] = w[..] {
                if let Ok(k) = k.parse::<i32>() {
                    meta.entry(k).or_insert_with(LayerMeta::default).fx = LayerFx {
                        opacity: o.parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0),
                        blend: Blend::from_key(b).unwrap_or_default(),
                        clip: c == "1",
                    };
                }
            }
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
        meta.entry(k).or_insert_with(LayerMeta::default).name = name.to_string();
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
        self.layer_local = meta;
        self.scene.hidden_layers = hidden;
        self.scene.locked_layers = locked;
        self.scene.layer = active;
    }

    fn layers_save(&self) {
        let mut p = crate::prefs::load();
        p.insert(
            prefs_key(self.share.canvas),
            encode(
                &self.layer_local,
                &self.scene.hidden_layers,
                &self.scene.locked_layers,
                self.scene.layer,
            ),
        );
        crate::prefs::save(&p);
    }

    /// The layers' names and looks: those kept on the canvas (`LayerLook`,
    /// the newest of each layer's), over those kept on this device, with a
    /// look being dragged on top.
    pub(crate) fn layers_refresh(&mut self) {
        let mut meta = self.layer_local.clone();
        for &g in &self.objs.looks {
            let grp = &self.objs.groups[g as usize];
            let Some(&s) = grp.strokes.first() else {
                continue;
            };
            let st = &self.scene.strokes[s as usize];
            if st.deleted {
                continue;
            }
            if let ObjData::LayerLook { name, fx, .. } = &grp.data {
                meta.insert(
                    layer_of(st.z),
                    LayerMeta {
                        name: name.clone(),
                        fx: *fx,
                    },
                );
            }
        }
        if let Some((k, fx)) = self.layer_preview {
            meta.entry(k).or_default().fx = fx;
        }
        self.layer_meta = meta;
    }

    /// Keep layer `k`'s name and look on the canvas (one undo step; it
    /// replaces the one there was).
    fn write_look(&mut self, k: i32, name: String, fx: LayerFx) {
        let old: Vec<u32> = self
            .objs
            .looks
            .iter()
            .flat_map(|&g| self.objs.groups[g as usize].strokes.clone())
            .filter(|&s| {
                let st = &self.scene.strokes[s as usize];
                !st.deleted && layer_of(st.z) == k
            })
            .collect();
        let z = self.scene.layer_bottom(k) - 1.0;
        let [w, h] = self.size();
        let at = self.px_to_cam([w * 0.5, h * 0.5]);
        let data = ObjData::LayerLook {
            geom: crate::shapes::Geom {
                center: at,
                half: [0.0, 0.0],
                rot: 0.0,
                pts: vec![],
            },
            name,
            fx,
        };
        let (_, ids) = self.add_group(&data, Some((z, z)));
        self.record_edit(old, ids);
        self.layers_refresh();
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
            if !s.deleted
                && !self.objs.look_strokes.contains(&(i as u32))
                && seen.insert(self.objs.obj_of(i as u32))
            {
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
                fx: self.layer_meta.get(&k).map(|m| m.fx).unwrap_or_default(),
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
        self.scene.layer = k;
        self.write_look(k, format!("Layer {n}"), LayerFx::default());
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
        let fx = self.layer_meta.get(&k).map(|m| m.fx).unwrap_or_default();
        self.write_look(k, name, fx);
    }

    /// Show or hide layer `k` (what is selected on it is let go).
    pub(crate) fn layer_toggle_hidden(&mut self, k: i32) {
        toggle(&mut self.scene.hidden_layers, k);
        self.deselect_layer(k);
        self.layers_save();
        self.redraw();
    }

    /// Change layer `k`'s look; `save` when the change is done (a slider
    /// let go), so a drag doesn't write the prefs every frame.
    pub(crate) fn layer_set_fx(&mut self, k: i32, f: impl FnOnce(&mut LayerFx), save: bool) {
        let mut fx = self
            .layer_preview
            .filter(|p| p.0 == k)
            .map(|p| p.1)
            .unwrap_or_else(|| self.layer_meta.get(&k).map(|m| m.fx).unwrap_or_default());
        f(&mut fx);
        if save {
            self.layer_preview = None;
            let name = self.layer_name(k);
            self.write_look(k, name, fx);
        } else {
            self.layer_preview = Some((k, fx));
            self.layers_refresh();
        }
        self.redraw();
    }

    /// The layers drawn on their own this frame (not plain, or the mask of
    /// one clipped to it), with how to lay each over the rest.
    pub(crate) fn layer_fx(&self) -> std::collections::HashMap<i32, FxDraw> {
        let mut out = std::collections::HashMap::new();
        if self.layer_meta.values().all(|m| m.fx.plain()) {
            return out;
        }
        let mut layers: Vec<i32> = self.layer_meta.keys().copied().collect();
        layers.extend(self.scene.layer_bands());
        layers.sort_unstable();
        layers.dedup();
        let fx = |k: i32| self.layer_meta.get(&k).map(|m| m.fx).unwrap_or_default();
        for (i, &k) in layers.iter().enumerate() {
            let f = fx(k);
            if f.plain() {
                continue;
            }
            // Clipped: to the nearest layer under it that is not clipped.
            let clip_to = if f.clip {
                let base = layers[..i].iter().rev().copied().find(|&b| !fx(b).clip);
                match base {
                    Some(b) => {
                        out.entry(b).or_insert(FxDraw {
                            fx: fx(b),
                            clip_to: None,
                        });
                        Some(b)
                    }
                    None => None,
                }
            } else {
                None
            };
            out.insert(k, FxDraw { fx: f, clip_to });
        }
        out
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
            self.layer_local.entry(x).or_default().name = name;
        }
        let (a, b) = (self.layer_local.remove(&k), self.layer_local.remove(&m));
        if let Some(a) = a {
            self.layer_local.insert(m, a);
        }
        if let Some(b) = b {
            self.layer_local.insert(k, b);
        }
        self.layers_refresh();
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
        self.layer_local.remove(&k);
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
                ..Default::default()
            },
        );
        meta.insert(
            2,
            LayerMeta {
                name: "Ink; final".into(),
                fx: LayerFx {
                    opacity: 0.5,
                    blend: Blend::Multiply,
                    clip: true,
                },
            },
        );
        let s = encode(&meta, &[2], &[0], 2);
        let (m, h, l, a) = decode(&s);
        assert_eq!(m.get(&0).unwrap().name, "Sketch");
        assert_eq!(m.get(&2).unwrap().name, "Ink  final");
        assert_eq!(
            m.get(&2).unwrap().fx,
            LayerFx {
                opacity: 0.5,
                blend: Blend::Multiply,
                clip: true
            }
        );
        assert!(m.get(&0).unwrap().fx.plain());
        assert_eq!((h, l, a), (vec![2], vec![0], 2));
    }

    #[test]
    fn layer_looks_plan_and_export() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        add(&mut app, "base");
        app.layer_new();
        let top = app.scene.layer;
        add(&mut app, "clipped");
        app.layer_new();
        let faded = app.scene.layer;
        add(&mut app, "faded");
        assert!(app.layer_fx().is_empty(), "plain layers draw straight on");
        app.layer_set_fx(top, |f| f.clip = true, true);
        // A drag shows the look before it is set.
        app.layer_set_fx(faded, |f| f.opacity = 0.7, false);
        assert_eq!(app.layer_fx()[&faded].fx.opacity, 0.7);
        app.layer_set_fx(
            faded,
            |f| {
                f.opacity = 0.4;
                f.blend = Blend::Multiply;
            },
            true,
        );
        // Kept on the canvas: another copy of it has the same looks.
        let bytes = crate::snapshot::encode(
            &app.scene,
            &app.cam,
            &app.timeline,
            &[],
            &app.objs,
            &app.share.copy_log(),
        );
        let snap = crate::snapshot::decode(&bytes, 800.0).unwrap();
        let mut other = App::new(None);
        other.scene = snap.scene;
        other.objs = snap.objs;
        other.layers_refresh();
        assert_eq!(
            other.layer_fx().len(),
            3,
            "the looks travel with the canvas"
        );
        assert_eq!(other.layer_meta[&faded].fx.blend, Blend::Multiply);
        // Not something to pick or list.
        let list = app.run_commands(&serde_json::json!([{"get": "objects"}]))[0]["objects"].clone();
        assert!(
            list.as_array()
                .unwrap()
                .iter()
                .all(|o| o["kind"] != "layer"),
            "{list}"
        );
        let fx = app.layer_fx();
        // The clipped layer and the one it is clipped to, and the faded one.
        assert_eq!(fx.len(), 3, "{fx:?}");
        assert_eq!(fx[&top].clip_to, Some(0));
        assert_eq!(fx[&0].clip_to, None);
        assert_eq!(fx[&faded].fx.opacity, 0.4);
        // Exported: groups with the look, the clipped one masked by the base.
        app.size_override = Some([800.0, 600.0]);
        let (w, h) = (800.0, 600.0);
        ogpaper_core::query(&app.scene, &app.cam, w, h, crate::VIEW, &mut app.draw);
        let svg = String::from_utf8(
            app.export(
                crate::export::Format::Svg,
                &crate::export::Opts {
                    selection: false,
                    background: false,
                    scale: 1.0,
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(svg.contains("<g id=\"layer0\">"), "{svg}");
        assert!(svg.contains(&format!("mask=\"url(#clip{top})\"")), "{svg}");
        assert!(svg.contains("<use href=\"#layer0\"/>"));
        assert!(
            svg.contains("opacity=\"0.4\" style=\"mix-blend-mode:multiply\""),
            "{svg}"
        );
        let pdf = app
            .export(
                crate::export::Format::Pdf,
                &crate::export::Opts {
                    selection: false,
                    background: false,
                    scale: 1.0,
                },
            )
            .unwrap();
        let pdf = String::from_utf8_lossy(&pdf);
        assert!(pdf.contains("/BM /Multiply"));
        assert!(
            pdf.contains("/SMask << /Type /Mask /S /Alpha /G "),
            "the clipped layer is masked"
        );
    }

    /// `OGP_DUMP=dir cargo test -- --ignored clip_pdf`: a PDF to look at.
    #[test]
    #[ignore]
    fn clip_pdf() {
        let mut app = App::new(None);
        app.cam = crate::home_camera();
        app.size_override = Some([800.0, 600.0]);
        app.run_commands(&json!([{"add": "shape", "kind": "rect", "x": -200, "y": -60, "w": 120, "h": 120, "fill": "#2050d0", "color": "#2050d0"}]));
        app.layer_new();
        let k = app.scene.layer;
        app.run_commands(&json!([{"add": "shape", "kind": "rect", "x": -170, "y": -30, "w": 260, "h": 60, "fill": "#d03030", "color": "#d03030"}]));
        app.layer_set_fx(k, |f| f.clip = true, true);
        ogpaper_core::query(
            &app.scene,
            &app.cam,
            800.0,
            600.0,
            crate::VIEW,
            &mut app.draw,
        );
        let opts = crate::export::Opts {
            selection: false,
            background: true,
            scale: 1.0,
        };
        let dir = std::env::var("OGP_DUMP")
            .unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        std::fs::write(
            format!("{dir}/clip.pdf"),
            app.export(crate::export::Format::Pdf, &opts).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/clip.png"),
            app.export(crate::export::Format::Png, &opts).unwrap(),
        )
        .unwrap();
    }
}
