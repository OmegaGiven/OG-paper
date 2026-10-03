// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! OG Paper: an open-source infinite canvas.

mod bucket;
mod crop;
#[cfg(any(test, target_arch = "wasm32"))]
mod demo;
mod diagram;
mod edit;
mod egui_io;
mod export;
mod font;
mod hotbar;
mod hotkeys;
mod images;
mod import;
mod joints;
mod layout;
mod library;
mod objects;
mod pdf;
mod prefs;
mod render;
mod search;
mod shapes;
mod share;
mod snapshot;
mod timeline;
mod ui;
mod uid;
#[cfg(target_arch = "wasm32")]
mod web;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use ogpaper_core::{hit, query, Camera, CellAddr, Change, DrawList, History, Params, Scene};
use web_time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use render::{Renderer, UiPaint, Wet};
use timeline::{Bookmark, Timeline};
use ui::{Action, Tool, UiState};

#[cfg(not(target_arch = "wasm32"))]
use ogpaper_file::OgpFile;

/// Real strokes all the way down to ~2 px; anything smaller draws nothing.
const VIEW: Params = Params {
    min_cell_px: 2.0,
    tile_px: 0.0,
    ancestor_levels: 8,
};
/// Eraser and picker reach, in points (scaled to physical pixels by `ppp`).
const ERASER_PT: f64 = 10.0;
const PICK_PT: f64 = 8.0;
/// How far (physical px) a finger may drift and still count as a tap.
const TAP_SLOP_PT: f64 = 10.0;

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Pixels per level-0 cell at scale 1.
const BASE_PX: f64 = 800.0;

/// What this app puts on the system clipboard when it copies (the copy
/// itself stays in the app).
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const CLIP_MARK: &str = "OG Paper selection (paste it into OG Paper)";

fn home_camera() -> Camera {
    Camera::new(CellAddr::new(0, 0, 0), [0.5, 0.5], BASE_PX)
}

/// What the pointer is doing right now.
#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    None,
    Ink,
    Erase,
    Pan,
    Pick,
    /// Dragging out a shape.
    Shape,
    /// Selecting, or moving / resizing / rotating the selection.
    Select,
    /// A bucket tap: where it went down (fills on release, unless it moved).
    Bucket([f64; 2]),
    /// Moving a canvas being imported.
    Import,
}

pub struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Renderer>,
    egui_ctx: egui::Context,
    egui_io: Option<egui_io::EguiIo>,
    ui: UiState,

    scene: Scene,
    cam: Camera,
    history: History,
    draw: DrawList,
    /// When each stroke appeared and disappeared.
    timeline: Timeline,
    /// The canvas id and merge log (Merge copy).
    share: share::Share,
    /// Shapes and texts (groups of strokes with their settings).
    objs: objects::Objects,
    edit: edit::EditState,
    bookmarks: Vec<Bookmark>,
    /// Viewing the canvas as it was after timeline event `.0`; `.1` holds the
    /// real deleted flags to put back.
    tl_view: Option<(usize, Vec<bool>)>,
    /// Left handle of the timeline window (first event shown).
    tl_from: usize,
    /// Animated flight to a view (bookmarks); any input cancels it.
    fly: Option<Camera>,
    /// Another canvas being placed (Import), until placed or cancelled.
    import: Option<import::Import>,
    /// A PDF being imported, a page per frame.
    pdf: Option<pdf::PdfJob>,
    /// A searched-for text being outlined: its group and when it started.
    flash: Option<(u32, Instant)>,
    fly_last: Instant,

    #[cfg(not(target_arch = "wasm32"))]
    file: Option<OgpFile>,
    /// Groups already written to the file.
    #[cfg(not(target_arch = "wasm32"))]
    groups_saved: usize,
    #[cfg(not(target_arch = "wasm32"))]
    open_path: Option<PathBuf>,
    #[cfg(target_arch = "wasm32")]
    pending_gpu: std::rc::Rc<std::cell::RefCell<Option<Renderer>>>,
    view_dirty_since: Option<Instant>,

    cursor: [f64; 2],
    mods: ModifiersState,
    space: bool,
    gesture: Gesture,
    /// The stroke being drawn: screen px + pressure.
    wet: Vec<[f32; 3]>,
    erased: Vec<u32>,
    touches: HashMap<u64, [f64; 2]>,
    touch_ink: Option<u64>,
    /// Multi-finger tap tracking: (started, most fingers, moved too far).
    multi_tap: Option<(Instant, usize, bool)>,
    /// Where each current touch started (tap vs. pinch/drag detection).
    touch_start: HashMap<u64, [f64; 2]>,
    message_until: Option<Instant>,
    /// The message the timer above is for (the UI sets messages too).
    message_seen: Option<String>,
    last_dbg: (u32, u32, u32),
    /// The last picture the color picker read, decoded.
    pick_img: std::cell::RefCell<Option<(u64, image::RgbaImage)>>,
}

impl App {
    pub fn new(open_path: Option<PathBuf>) -> Self {
        let _ = &open_path;
        // Light controls on light paper, whatever the system theme.
        let egui_ctx = egui::Context::default();
        #[cfg(not(target_arch = "wasm32"))]
        load_user_fonts();
        egui_ctx.set_theme(egui::Theme::Light);
        let mut ui = UiState::default();
        ui.load_saved(hotbar::load());
        let prefs = prefs::load();
        ui.grid = ui::GridMode::from_key(prefs.get("grid").map_or("off", |s| s.as_str()));
        ui.diagram = prefs.get("diagram").is_some_and(|v| v == "on");
        ui.radial_bar = prefs.get("radialbar").is_some_and(|v| v == "on");
        ui.dark = prefs.get("dark").is_some_and(|v| v == "on");
        ui.hide_tools = prefs.get("hide_tools").is_some_and(|v| v == "on");
        ui.hide_panel = prefs.get("hide_panel").is_some_and(|v| v == "on");
        ui.hide_bar = prefs.get("hide_bar").is_some_and(|v| v == "on");
        ui.layout = layout::Layout::decode(prefs.get("layout").map_or("", |s| s.as_str()));
        ui.keys = hotkeys::Keymap::load(prefs.get("keys").map_or("", |s| s.as_str()));
        Self {
            window: None,
            gpu: None,
            egui_ctx,
            egui_io: None,
            ui,
            scene: Scene::new(),
            cam: home_camera(),
            history: History::default(),
            draw: DrawList::default(),
            timeline: Timeline::default(),
            share: share::Share::fresh(),
            objs: objects::Objects::default(),
            edit: edit::EditState::default(),
            bookmarks: Vec::new(),
            tl_view: None,
            tl_from: 0,
            fly: None,
            import: None,
            pdf: None,
            flash: None,
            fly_last: Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
            #[cfg(not(target_arch = "wasm32"))]
            groups_saved: 0,
            #[cfg(not(target_arch = "wasm32"))]
            open_path,
            #[cfg(target_arch = "wasm32")]
            pending_gpu: Default::default(),
            view_dirty_since: None,
            cursor: [0.0; 2],
            mods: ModifiersState::empty(),
            space: false,
            gesture: Gesture::None,
            wet: Vec::new(),
            erased: Vec::new(),
            touches: HashMap::new(),
            touch_ink: None,
            multi_tap: None,
            touch_start: HashMap::new(),
            message_until: None,
            message_seen: None,
            last_dbg: (0, 0, 0),
            pick_img: Default::default(),
        }
    }

    /// Physical pixels per point: widths and touch radii are in points, so a
    /// 3 px pen looks the same on a 1x monitor and a 3x phone.
    fn ppp(&self) -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            web_dpr() as f64
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.window
                .as_ref()
                .map(|w| w.scale_factor())
                .unwrap_or(1.0)
        }
    }

    fn size(&self) -> [f64; 2] {
        self.gpu
            .as_ref()
            .map(|g| [g.config.width as f64, g.config.height as f64])
            .unwrap_or([1.0, 1.0])
    }

    fn centred(&self, p: [f64; 2]) -> [f64; 2] {
        let s = self.size();
        [p[0] - s[0] * 0.5, p[1] - s[1] * 0.5]
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.ui.message = Some(msg.into());
        // The same message again restarts its time.
        self.message_seen = None;
    }

    fn view_changed(&mut self) {
        self.view_dirty_since.get_or_insert_with(Instant::now);
        self.redraw();
    }

    // ---- ink -------------------------------------------------------------

    fn ink_add(&mut self, p: [f64; 2], pressure: f32) {
        let pressure =
            if self.ui.tool == Tool::Pen && !self.ui.pen.pressure && !self.ui.pen.advanced {
                1.0
            } else {
                pressure
            };
        let q = [p[0] as f32, p[1] as f32, pressure];
        if let Some(l) = self.wet.last() {
            if (l[0] - q[0]).hypot(l[1] - q[1]) < 1.2 {
                return;
            }
        }
        self.wet.push(q);
        self.redraw();
    }

    fn ink_commit(&mut self) {
        let tool = self.ui.tool;
        let Some(brush) = self.ui.ink().and_then(|i| i.brush_for(tool)) else {
            self.wet.clear();
            return;
        };
        if self.wet.is_empty() {
            return;
        }
        let ink = *self.ui.ink().expect("ink tool");
        let ppc = self.cam.ppc();
        let cam_pts: Vec<[f64; 2]> = self
            .wet
            .iter()
            .map(|p| {
                self.cam
                    .screen_to_cam(self.centred([p[0] as f64, p[1] as f64]))
            })
            .collect();
        let width_cam = ink.width as f64 * self.ppp() / ppc;
        let (cell, local, side) = Scene::anchor_for_min(&self.cam.cell, &cam_pts, width_cam);
        let pts: Vec<[f32; 4]> = local
            .iter()
            .zip(&self.wet)
            .map(|(l, w)| [l[0], l[1], w[2], 0.0])
            .collect();
        let style = ogpaper_core::Style {
            width: (width_cam / side) as f32,
            color: u32::from_le_bytes(ink.rgba()),
            brush,
            dash: if brush == ogpaper_core::Brush::Highlighter {
                ogpaper_core::Dash::Solid
            } else {
                ink.dash
            },
            // A fresh seed per stroke, so no two scatter alike.
            ext: (brush == ogpaper_core::Brush::Dabs).then(|| ogpaper_core::BrushParams {
                seed: ink.params.seed ^ (uid::new() as u32),
                ..ink.params
            }),
        };
        let id = self.scene.add_stroke_with(&cell, &pts, style, uid::new());
        self.wet.clear();
        self.history.record(Change::Added(vec![id]));
        self.note(id, true);
        #[cfg(target_arch = "wasm32")]
        {
            let z = self.cam.log10_zoom();
            web::stats(|s| {
                s.drawn += 1;
                s.deep_draw = s.deep_draw.max(z);
            });
        }
        if let Some(g) = self.gpu.as_mut() {
            g.sync(&self.scene);
        }
        self.persist_new(id);
        self.redraw();
    }

    fn erase_at(&mut self, p: [f64; 2]) {
        let hits = hit::strokes_near(
            &self.scene,
            &self.draw,
            [p[0] as f32, p[1] as f32],
            (ERASER_PT * self.ppp()) as f32,
        );
        // A shape or text goes as a whole.
        let mut ids = Vec::new();
        for id in hits {
            ids.extend_from_slice(self.objs.strokes(&self.objs.obj_of(id)));
        }
        for id in ids {
            if self.scene.delete(id) {
                self.erased.push(id);
                self.note(id, false);
                self.persist_deleted(id);
                #[cfg(target_arch = "wasm32")]
                web::stats(|s| s.erased += 1);
            }
        }
        self.redraw();
    }

    fn erase_end(&mut self) {
        if !self.erased.is_empty() {
            self.history
                .record(Change::Deleted(std::mem::take(&mut self.erased)));
        }
    }

    fn begin(&mut self, p: [f64; 2], pressure: f32) {
        // A tap on the canvas while a menu is open only closes the menu.
        if self.ui.menu_open() {
            self.ui.close_menus();
            self.gesture = Gesture::None;
            self.redraw();
            return;
        }
        self.fly = None;
        self.gesture = match self.ui.tool {
            _ if self.space => Gesture::Pan,
            // Placing an import: any drag moves it.
            _ if self.import.is_some() => Gesture::Import,
            Tool::Hand => Gesture::Pan,
            Tool::Eraser => Gesture::Erase,
            Tool::Picker => Gesture::Pick,
            Tool::Shapes => Gesture::Shape,
            Tool::Select | Tool::Lasso => Gesture::Select,
            Tool::Bucket => Gesture::Bucket(p),
            Tool::Text => Gesture::None,
            _ => Gesture::Ink,
        };
        if self.tl_view.is_some()
            && (matches!(
                self.gesture,
                Gesture::Ink | Gesture::Erase | Gesture::Shape | Gesture::Select
            ) || self.ui.tool == Tool::Text)
        {
            // The past is read-only: browse it instead.
            self.say("Viewing the timeline: close it (or restore this moment) to draw");
            self.gesture = Gesture::Pan;
        }
        match self.gesture {
            Gesture::Ink => {
                self.wet.clear();
                self.ink_add(p, pressure);
            }
            Gesture::Erase => self.erase_at(p),
            Gesture::Pick => self.pick_preview(p),
            Gesture::Shape => self.shape_begin(p),
            Gesture::Select => self.select_begin(p),
            Gesture::None if self.ui.tool == Tool::Text && self.tl_view.is_none() => {
                self.text_begin(p)
            }
            _ => {}
        }
    }

    fn moved(&mut self, from: [f64; 2], to: [f64; 2], pressure: f32) {
        match self.gesture {
            Gesture::Ink => self.ink_add(to, pressure),
            Gesture::Erase => {
                // Sweep the whole path so fast swipes cannot skip a stroke.
                let d = (to[0] - from[0]).hypot(to[1] - from[1]);
                let steps = (d / (ERASER_PT * self.ppp() * 0.5)).ceil().max(1.0) as usize;
                for i in 1..=steps {
                    let t = i as f64 / steps as f64;
                    self.erase_at([
                        from[0] + (to[0] - from[0]) * t,
                        from[1] + (to[1] - from[1]) * t,
                    ]);
                }
            }
            Gesture::Pan => {
                self.cam.pan_px(to[0] - from[0], to[1] - from[1]);
                self.view_changed();
            }
            Gesture::Pick => self.pick_preview(to),
            Gesture::Import => self.import_drag(from, to),
            Gesture::Shape => self.shape_move(to),
            Gesture::Select => self.select_move(to),
            Gesture::Bucket(a) => {
                if dist(a, to) > TAP_SLOP_PT * self.ppp() {
                    self.gesture = Gesture::None;
                }
            }
            Gesture::None => {}
        }
    }

    /// Color of the topmost stroke under screen point `p` (physical px).
    fn color_at(&self, p: [f64; 2]) -> Option<egui::Color32> {
        let ids = hit::strokes_near(
            &self.scene,
            &self.draw,
            [p[0] as f32, p[1] as f32],
            (PICK_PT * self.ppp()) as f32,
        );
        // The one drawn on top.
        let top = *ids.iter().max_by(|&&a, &&b| {
            let (za, zb) = (
                self.scene.strokes[a as usize].z,
                self.scene.strokes[b as usize].z,
            );
            za.total_cmp(&zb).then(a.cmp(&b))
        })?;
        if let Some(&(id, _, _)) = self.objs.image_of.get(&top) {
            return self.picture_color(top, id, p);
        }
        let [r, g, b, _] = self.scene.strokes[top as usize].color.to_le_bytes();
        Some(egui::Color32::from_rgb(r, g, b))
    }

    /// The color of picture `id` (placed by stroke `carrier`) under screen point `p`.
    fn picture_color(&self, carrier: u32, id: u64, p: [f64; 2]) -> Option<egui::Color32> {
        let inst = self.draw.strokes.iter().find(|i| i.stroke == carrier)?;
        let c: Vec<[f64; 2]> = self
            .scene
            .stroke_points(carrier)
            .iter()
            .map(|q| {
                [
                    (inst.ox + q[0] * inst.scale) as f64,
                    (inst.oy + q[1] * inst.scale) as f64,
                ]
            })
            .collect();
        if c.len() != 4 {
            return None;
        }
        // p = c0 + u (c1 - c0) + v (c3 - c0)
        let (a, b) = (
            [c[1][0] - c[0][0], c[1][1] - c[0][1]],
            [c[3][0] - c[0][0], c[3][1] - c[0][1]],
        );
        let d = [p[0] - c[0][0], p[1] - c[0][1]];
        let det = a[0] * b[1] - a[1] * b[0];
        if det.abs() < 1e-9 {
            return None;
        }
        let u = (d[0] * b[1] - d[1] * b[0]) / det;
        let v = (a[0] * d[1] - a[1] * d[0]) / det;
        // Through the crop to the whole picture.
        let cr = self
            .objs
            .image_of
            .get(&carrier)
            .map_or(objects::FULL_CROP, |e| e.2);
        let u = cr[0] as f64 + u.clamp(0.0, 1.0) * (cr[2] - cr[0]) as f64;
        let v = cr[1] as f64 + v.clamp(0.0, 1.0) * (cr[3] - cr[1]) as f64;
        let mut cache = self.pick_img.borrow_mut();
        if cache.as_ref().is_none_or(|(cid, _)| *cid != id) {
            let img = image::load_from_memory(&self.objs.images.get(&id)?.bytes).ok()?;
            *cache = Some((id, img.to_rgba8()));
        }
        let (_, img) = cache.as_ref()?;
        let x = ((u.clamp(0.0, 1.0) * img.width() as f64) as u32).min(img.width() - 1);
        let y = ((v.clamp(0.0, 1.0) * img.height() as f64) as u32).min(img.height() - 1);
        let [r, g, b, _] = img.get_pixel(x, y).0;
        Some(egui::Color32::from_rgb(r, g, b))
    }

    fn pick_preview(&mut self, p: [f64; 2]) {
        let ppp = self.ppp() as f32;
        self.ui.pick_preview = Some((
            egui::pos2(p[0] as f32 / ppp, p[1] as f32 / ppp),
            self.color_at(p),
        ));
        self.redraw();
    }

    /// Finish a pick: hand the color to the last ink tool and switch back to it.
    fn pick_end(&mut self) {
        let picked = self.ui.pick_preview.take().and_then(|(_, c)| c);
        match picked {
            Some(c) => {
                let back = self.ui.last_ink;
                self.ui.tool = back;
                let [r, g, b, _] = c.to_array();
                let col = u32::from_le_bytes([r, g, b, 255]);
                match back {
                    Tool::Shapes if self.ui.color_target == ui::ColorTarget::Fill => {
                        self.ui.shape.fill = col
                    }
                    Tool::Shapes if self.ui.color_target == ui::ColorTarget::Both => {
                        self.ui.shape.stroke = col;
                        self.ui.shape.fill = col;
                    }
                    Tool::Shapes => self.ui.shape.stroke = col,
                    Tool::Text => self.ui.text.color = col,
                    _ => {
                        if let Some(ink) = self.ui.ink() {
                            ink.color = c;
                        }
                    }
                }
            }
            None => self.say("No ink here — tap a stroke to take its color"),
        }
    }

    fn end(&mut self, cancel: bool) {
        match self.gesture {
            Gesture::Ink if !cancel => self.ink_commit(),
            Gesture::Ink => self.wet.clear(),
            Gesture::Erase => self.erase_end(),
            Gesture::Pick if !cancel => self.pick_end(),
            Gesture::Pick => self.ui.pick_preview = None,
            Gesture::Shape => self.shape_end(cancel),
            Gesture::Select => self.select_end(cancel),
            Gesture::Bucket(p) if !cancel && self.tl_view.is_none() => self.bucket_fill(p),
            _ => {}
        }
        self.gesture = Gesture::None;
        self.redraw();
    }

    // ---- undo / file -----------------------------------------------------

    fn undo_redo(&mut self, redo: bool) {
        if self.tl_view.is_some() {
            self.say("Close the timeline to undo or redo");
            return;
        }
        let changed = if redo {
            self.history.redo(&mut self.scene)
        } else {
            self.history.undo(&mut self.scene)
        };
        let changed = changed.unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        if !changed.is_empty() {
            web::stats(|s| s.undos += 1);
        }
        for id in changed {
            let alive = !self.scene.strokes[id as usize].deleted;
            self.note(id, alive);
            self.persist_deleted(id);
        }
        self.redraw();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_new(&mut self, id: u32) {
        if self.file.is_none() {
            match default_path().and_then(|p| OgpFile::create(&p).map_err(|e| e.to_string())) {
                Ok(f) => {
                    self.ui.file_name = file_label(f.path());
                    self.say(format!("Saving to {}", f.path().display()));
                    let _ = f.put_all(&self.scene);
                    self.file = Some(f);
                    self.groups_saved = 0;
                    self.persist_groups();
                    self.persist_share();
                    return;
                }
                Err(e) => self.say(format!("Could not create a file: {e}")),
            }
        }
        if let Some(f) = &self.file {
            if let Err(e) = f.put_stroke(&self.scene, id) {
                self.say(format!("Save failed: {e}"));
            }
        }
    }

    /// Write shapes and texts created since the last call.
    #[cfg(not(target_arch = "wasm32"))]
    fn persist_groups(&mut self) {
        let Some(f) = &self.file else {
            return;
        };
        for g in &self.objs.groups[self.groups_saved.min(self.objs.groups.len())..] {
            let fg = ogpaper_file::FileGroup {
                cell: g.cell.clone(),
                kind: match g.data {
                    objects::ObjData::Shape { .. } => "shape".into(),
                    objects::ObjData::Text { .. } => "text".into(),
                    objects::ObjData::Image { .. } => "image".into(),
                    objects::ObjData::Table { .. } => "table".into(),
                },
                data: snapshot::data_bytes(&g.data),
                strokes: g
                    .strokes
                    .iter()
                    .map(|&s| self.scene.strokes[s as usize].uid)
                    .collect(),
            };
            if let objects::ObjData::Image { id, .. } = g.data {
                if let Some(a) = self.objs.images.get(&id) {
                    let _ = f.put_image(id, &a.bytes);
                }
            }
            let _ = f.put_group(uid::new(), &fg);
        }
        self.groups_saved = self.objs.groups.len();
    }

    #[cfg(target_arch = "wasm32")]
    fn persist_groups(&mut self) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_deleted(&mut self, id: u32) {
        if let Some(f) = &self.file {
            let _ = f.put_deleted(&self.scene, id);
        }
    }

    // The web keeps the canvas in browser storage: the page asks for a
    // snapshot whenever something changed.
    #[cfg(target_arch = "wasm32")]
    fn persist_new(&mut self, _id: u32) {
        web::touch();
    }
    #[cfg(target_arch = "wasm32")]
    fn persist_deleted(&mut self, _id: u32) {
        web::touch();
    }

    fn save_view(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self.file {
            let _ = f.put_view(&self.cam);
        }
        #[cfg(target_arch = "wasm32")]
        if self.view_dirty_since.is_some() {
            web::touch();
        }
        self.view_dirty_since = None;
    }

    fn load_scene(&mut self, scene: Scene, cam: Camera) {
        self.timeline = Timeline::from_scene(&scene, timeline::now_ms());
        self.share = share::Share::loaded(&scene, None);
        self.scene = scene;
        self.cam = cam;
        self.history = History::default();
        self.tl_view = None;
        self.fly = None;
        self.wet.clear();
        self.objs = objects::Objects::default();
        self.edit = edit::EditState::default();
        self.ui.text_edit = None;
        #[cfg(target_arch = "wasm32")]
        web::touch();
        if let Some(g) = self.gpu.as_mut() {
            g.reset(&self.scene);
        }
        self.redraw();
    }

    /// Merge another copy of this canvas (.ogp, or a web copy .ogpt).
    #[cfg(not(target_arch = "wasm32"))]
    fn merge_file(&mut self, path: PathBuf) {
        let name = file_label(&path);
        if self.file.as_ref().is_some_and(|f| f.path() == path) {
            self.say("That is the file already open");
            return;
        }
        if path.extension().is_some_and(|e| e == "ogpt") {
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| snapshot::decode(&b, BASE_PX))
            {
                Ok(s) => self.merge_copy(s.scene, s.objs, s.share),
                Err(e) => self.say(format!("Could not read {name}: {e}")),
            }
            return;
        }
        match OgpFile::open(&path) {
            Ok((f, scene, _view, groups)) => {
                let mut objs = groups_from_file(&scene, groups);
                for (id, b) in f.images().unwrap_or_default() {
                    if let Ok(a) = images::load(b) {
                        objs.images.insert(id, a);
                    }
                }
                let copy = file_copy_log(&f);
                drop(f);
                self.merge_copy(scene, objs, copy);
            }
            Err(e) => self.say(format!("Could not read {name}: {e}")),
        }
    }

    /// Import another canvas file (.ogp, or a web copy .ogpt) to place.
    #[cfg(not(target_arch = "wasm32"))]
    fn import_file(&mut self, path: PathBuf) {
        let name = file_label(&path);
        let is_copy = path.extension().is_some_and(|e| e == "ogpt");
        if is_copy {
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| snapshot::decode(&b, BASE_PX))
            {
                Ok(s) => self.import_begin(s.scene, s.objs, &name),
                Err(e) => self.say(format!("Could not import {name}: {e}")),
            }
            return;
        }
        match OgpFile::open(&path) {
            Ok((f, scene, _view, groups)) => {
                let mut objs = groups_from_file(&scene, groups);
                for (id, b) in f.images().unwrap_or_default() {
                    if let Ok(a) = images::load(b) {
                        objs.images.insert(id, a);
                    }
                }
                drop(f);
                self.import_begin(scene, objs, &name);
            }
            Err(e) => self.say(format!("Could not import {name}: {e}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file(&mut self, path: PathBuf) {
        self.save_view();
        if !path.exists() {
            match OgpFile::create(&path) {
                Ok(f) => {
                    self.ui.file_name = file_label(&path);
                    self.file = Some(f);
                    self.load_scene(Scene::new(), home_camera());
                }
                Err(e) => self.say(format!("Could not create {}: {e}", path.display())),
            }
            return;
        }
        match OgpFile::open(&path) {
            Ok((f, scene, view, groups)) => {
                let cam = match view {
                    Some(v) => {
                        let mut c = Camera::new(v.cell, v.off, 800.0);
                        c.scale = v.scale;
                        c.normalize();
                        c
                    }
                    None => home_camera(),
                };
                self.ui.file_name = file_label(&path);
                let n = scene.strokes.iter().filter(|s| !s.deleted).count();
                self.file = Some(f);
                self.load_scene(scene, cam);
                self.objs = groups_from_file(&self.scene, groups);
                let copy = self.file.as_ref().and_then(file_copy_log);
                let had = copy.as_ref().is_some_and(|c| !c.events.is_empty());
                self.share = share::Share::loaded(&self.scene, copy);
                if !had {
                    self.persist_share();
                }
                if let Some(f) = &self.file {
                    for (id, b) in f.images().unwrap_or_default() {
                        if let Ok(a) = images::load(b) {
                            self.objs.images.insert(id, a);
                        }
                    }
                }
                self.groups_saved = self.objs.groups.len();
                self.say(format!("Opened {} ({n} strokes)", path.display()));
            }
            Err(e) => self.say(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn new_canvas(&mut self) {
        self.save_view();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.file = None;
        }
        self.ui.file_name = "Untitled".into();
        self.bookmarks.clear();
        self.load_scene(Scene::new(), home_camera());
    }

    /// Pick a font file, keep a copy in the fonts folder and use it.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn add_font_dialog(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Font (TrueType / OpenType)", &["ttf", "otf", "TTF", "OTF"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => return self.say(format!("Could not read {}: {e}", p.display())),
        };
        match font::register(None, "Yours", bytes, true) {
            Ok((id, name)) => {
                if let (Some(dir), Some(file)) = (fonts_dir(), p.file_name()) {
                    let _ = std::fs::create_dir_all(&dir)
                        .and_then(|_| std::fs::copy(&p, dir.join(file)));
                }
                self.font_added(id, &name);
            }
            Err(e) => self.say(format!("Could not add {}: {e}", p.display())),
        }
    }

    /// Use a font that was just added.
    fn font_added(&mut self, id: font::FontId, name: &str) {
        self.ui.text.font = id;
        if self.ui.tool.selects() && self.ui.sel.kind == ui::SelKind::Text {
            self.ui.sel.text.font = id;
        }
        self.say(format!("Added the font {name}"));
        self.redraw();
    }

    /// Let egui draw every outline font (the font picker shows each name in
    /// its own font).
    fn sync_egui_fonts(&mut self) {
        for f in font::list() {
            if f.outline && !self.ui.egui_fonts.contains(&f.name) {
                if let Some(bytes) = font::outline_data(&f.name) {
                    self.egui_ctx.add_font(egui::epaint::text::FontInsert::new(
                        &f.name,
                        egui::FontData::from_owned(bytes.to_vec()),
                        vec![egui::epaint::text::InsertFontFamily {
                            family: egui::FontFamily::Name(f.name.as_str().into()),
                            priority: egui::epaint::text::FontPriority::Highest,
                        }],
                    ));
                    self.ui.egui_fonts.insert(f.name.clone());
                }
            }
        }
    }

    fn action(&mut self, a: Action) {
        match a {
            Action::Duplicate
            | Action::Delete
            | Action::ToFront
            | Action::ToBack
            | Action::FlipH
            | Action::FlipV
            | Action::EditText => self.sel_action(a),
            Action::SaveSticker => self.save_sticker(),
            #[cfg(target_arch = "wasm32")]
            Action::Library => web::emit("library"),
            #[cfg(not(target_arch = "wasm32"))]
            Action::Library => {
                self.ui.lib_open = !self.ui.lib_open;
                if self.ui.lib_open {
                    self.lib_refresh();
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Action::LibPlace(i) => {
                if let Some((p, _)) = library::store::list().get(i) {
                    match std::fs::read(p)
                        .map_err(|e| e.to_string())
                        .and_then(|b| library::Sticker::decode(&b))
                    {
                        Ok(s) => self.place_sticker(&s, None),
                        Err(e) => self.say(format!("Could not place it: {e}")),
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Action::LibDelete(i) => {
                if let Some((p, _)) = library::store::list().get(i) {
                    let _ = std::fs::remove_file(p);
                }
                self.lib_refresh();
            }
            Action::Crop => self.crop_start(),
            Action::CropDone => self.crop_done(),
            Action::CropCancel => self.crop_cancel(),
            Action::TextDone => self.text_commit(),
            Action::TextCancel => self.text_cancel(),
            Action::Undo => {
                self.crop_cancel();
                self.undo_redo(false)
            }
            Action::Redo => {
                self.crop_cancel();
                self.undo_redo(true)
            }
            Action::Home => {
                self.fly = None;
                self.cam = home_camera();
                self.view_changed();
            }
            #[cfg(not(target_arch = "wasm32"))]
            Action::New => self.new_canvas(),
            // The page handles these on the web: it asks before replacing the
            // canvas, downloads / picks offline copies and shows its panels.
            #[cfg(target_arch = "wasm32")]
            Action::New => web::emit("new"),
            #[cfg(target_arch = "wasm32")]
            Action::Open => web::emit("open"),
            #[cfg(target_arch = "wasm32")]
            Action::SaveAs => web::emit("save"),
            #[cfg(target_arch = "wasm32")]
            Action::Bookmarks => web::emit("bookmarks"),
            Action::Search => {
                #[cfg(target_arch = "wasm32")]
                web::emit("search");
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.ui.search_open = !self.ui.search_open;
                    self.ui.search_focus = true;
                }
                self.redraw();
            }
            Action::ShowTools | Action::ShowPanel | Action::ShowBar => {
                let (key, flag) = match a {
                    Action::ShowTools => ("hide_tools", &mut self.ui.hide_tools),
                    Action::ShowPanel => ("hide_panel", &mut self.ui.hide_panel),
                    _ => ("hide_bar", &mut self.ui.hide_bar),
                };
                *flag = !*flag;
                let v = if *flag { "on" } else { "off" };
                let mut p = prefs::load();
                p.insert(key.into(), v.into());
                prefs::save(&p);
                self.redraw();
            }
            Action::Dark => {
                self.ui.dark = !self.ui.dark;
                let mut p = prefs::load();
                p.insert(
                    "dark".into(),
                    if self.ui.dark { "on" } else { "off" }.into(),
                );
                prefs::save(&p);
                self.redraw();
            }
            Action::RadialBar => {
                self.ui.radial_bar = !self.ui.radial_bar;
                self.ui.menu = ui::Menu::None;
                let mut p = prefs::load();
                p.insert(
                    "radialbar".into(),
                    if self.ui.radial_bar { "on" } else { "off" }.into(),
                );
                prefs::save(&p);
                self.redraw();
            }
            Action::Hotkeys => {
                self.ui.keys_open = !self.ui.keys_open;
                self.ui.key_capture = None;
                self.ui.menu = ui::Menu::None;
                self.redraw();
            }
            Action::SaveKeys => self.save_keys(),
            Action::EditLayout => {
                self.ui.layout_edit = true;
                self.ui.menu = ui::Menu::None;
                self.redraw();
            }
            Action::Diagram => {
                self.ui.diagram = !self.ui.diagram;
                let mut p = prefs::load();
                p.insert(
                    "diagram".into(),
                    if self.ui.diagram { "on" } else { "off" }.into(),
                );
                prefs::save(&p);
                self.say(if self.ui.diagram {
                    "Diagram mode: lines and arrows stick to shapes, texts and pictures"
                } else {
                    "Diagram mode off"
                });
                self.redraw();
            }
            Action::Grid => {
                self.ui.grid = self.ui.grid.next();
                let mut p = prefs::load();
                p.insert("grid".into(), self.ui.grid.key().into());
                prefs::save(&p);
                self.redraw();
            }
            #[cfg(target_arch = "wasm32")]
            Action::Timeline => web::emit("timeline"),
            #[cfg(target_arch = "wasm32")]
            Action::FullScreen => web::emit("fullscreen"),
            #[cfg(target_arch = "wasm32")]
            Action::Tour => web::emit("tour"),
            #[cfg(target_arch = "wasm32")]
            Action::AddFont => web::emit("font"),
            #[cfg(target_arch = "wasm32")]
            Action::Picture => web::emit("picture"),
            #[cfg(target_arch = "wasm32")]
            Action::Import => web::emit("import"),
            #[cfg(target_arch = "wasm32")]
            Action::MergeCopy => web::emit("merge"),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::MergeCopy => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OG Paper canvas", &["ogp", "ogpt"])
                    .pick_file()
                {
                    self.merge_file(p);
                }
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::Picture => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter(
                        "Pictures and PDFs",
                        &["png", "jpg", "jpeg", "gif", "webp", "svg", "pdf"],
                    )
                    .pick_file()
                {
                    self.open_dropped(p, None);
                }
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::AddFont => self.add_font_dialog(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::Import => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OG Paper canvas", &["ogp", "ogpt"])
                    .pick_file()
                {
                    self.import_file(p);
                }
            }
            Action::ImportPlace => self.import_finish(true),
            Action::ImportCancel => self.import_finish(false),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::Open => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("OG Paper canvas", &["ogp"])
                    .pick_file()
                {
                    self.open_file(p);
                }
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::SaveAs => {
                if let Some(mut p) = rfd::FileDialog::new()
                    .add_filter("OG Paper canvas", &["ogp"])
                    .set_file_name("Untitled.ogp")
                    .save_file()
                {
                    if p.extension().is_none() {
                        p.set_extension("ogp");
                    }
                    match OgpFile::create(&p).and_then(|f| f.put_all(&self.scene).map(|_| f)) {
                        Ok(f) => {
                            self.file = Some(f);
                            self.groups_saved = 0;
                            self.persist_groups();
                            let f = self.file.take().expect("file");
                            let _ = f.put_view(&self.cam);
                            self.ui.file_name = file_label(&p);
                            self.say(format!("Saved to {}", p.display()));
                            self.file = Some(f);
                        }
                        Err(e) => self.say(format!("Save failed: {e}")),
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            Action::Export => web::emit("export"),
            #[cfg(target_arch = "wasm32")]
            Action::Paste => web::emit("paste"),
            #[cfg(not(target_arch = "wasm32"))]
            Action::Paste => {
                // At the middle of the screen, not where the menu was tapped.
                let [w, h] = self.size();
                self.cursor = [w * 0.5, h * 0.5];
                if !self.system_paste() {
                    self.paste();
                }
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Action::Export => self.export_dialog(),
            #[allow(unreachable_patterns)]
            _ => self.say("Not available on this platform yet"),
        }
    }

    /// Save the selection to the library: a file on desktop, the page's
    /// storage on the web.
    fn save_sticker(&mut self) {
        let Some(s) = self.sticker_from_selection() else {
            return self.say("Select something to add it to the library");
        };
        let bytes = s.encode();
        #[cfg(target_arch = "wasm32")]
        {
            web::set_sticker(bytes);
            web::emit("sticker");
        }
        #[cfg(not(target_arch = "wasm32"))]
        match library::store::save("Sticker", &bytes) {
            Ok(_) => {
                self.say("Added to the library (Settings > Library)");
                if self.ui.lib_open {
                    self.lib_refresh();
                }
            }
            Err(e) => self.say(format!("Could not save it: {e}")),
        }
    }

    /// Reload the desktop library panel, with thumbnails.
    #[cfg(not(target_arch = "wasm32"))]
    fn lib_refresh(&mut self) {
        const SIDE: u32 = 176;
        self.ui.lib = library::store::list()
            .into_iter()
            .map(|(p, name)| {
                let thumb = std::fs::read(&p)
                    .ok()
                    .and_then(|b| library::Sticker::decode(&b).ok())
                    .and_then(|s| {
                        let svg = export::sticker_svg(&s, SIDE as f64);
                        let tree = resvg::usvg::Tree::from_str(&svg, &Default::default()).ok()?;
                        let mut pix = resvg::tiny_skia::Pixmap::new(SIDE, SIDE)?;
                        resvg::render(&tree, Default::default(), &mut pix.as_mut());
                        let px: Vec<u8> = pix
                            .pixels()
                            .iter()
                            .flat_map(|c| {
                                let c = c.demultiply();
                                [c.red(), c.green(), c.blue(), c.alpha()]
                            })
                            .collect();
                        Some((SIDE as usize, px))
                    });
                ui::LibEntry {
                    name,
                    thumb,
                    tex: None,
                }
            })
            .collect();
    }

    /// Save the selection (if any) or the view as PNG, JPEG, SVG or PDF; the
    /// format follows the file name's extension.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn export_dialog(&mut self) {
        let sel = self.ui.tool.selects() && !self.edit.selection.is_empty();
        let name = if sel { "Selection.png" } else { "View.png" };
        let Some(mut p) = rfd::FileDialog::new()
            .add_filter("PNG picture", &["png"])
            .add_filter("JPEG picture", &["jpg", "jpeg"])
            .add_filter("SVG drawing", &["svg"])
            .add_filter("PDF document", &["pdf"])
            .set_file_name(name)
            .save_file()
        else {
            return;
        };
        let fmt = p
            .extension()
            .and_then(|e| export::Format::from_key(&e.to_string_lossy()))
            .unwrap_or(export::Format::Png);
        if p.extension().is_none() {
            p.set_extension(fmt.ext());
        }
        let opts = export::Opts {
            selection: sel,
            background: true,
            scale: 2.0,
        };
        match self
            .export(fmt, &opts)
            .and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string()))
        {
            Ok(()) => self.say(format!("Exported {}", p.display())),
            Err(e) => self.say(format!("Export failed: {e}")),
        }
    }

    /// After copying: put a marker on the system clipboard, so pasting
    /// pastes the copy until something else is copied elsewhere.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn clip_mark(&mut self) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(CLIP_MARK);
        }
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn clip_mark(&mut self) {}

    /// Paste from the system clipboard: a picture, files, a table or text.
    /// False when it holds nothing to paste, or this app's own copy.
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn system_paste(&mut self) -> bool {
        let Ok(mut cb) = arboard::Clipboard::new() else {
            return false;
        };
        let text = cb.get_text().ok();
        if text.as_deref() == Some(CLIP_MARK) {
            return false;
        }
        let at = Some(self.cursor);
        // Spreadsheets put both a table and a picture of it on the clipboard:
        // take the table.
        if let Some(t) = &text {
            if objects::table_from_text(t).is_some() {
                self.paste_text(t, at);
                return true;
            }
        }
        if let Ok(img) = cb.get_image() {
            let rgba =
                image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into());
            match rgba
                .ok_or("bad picture".to_string())
                .and_then(images::from_rgba)
            {
                Ok(a) => self.insert_image(a, at),
                Err(e) => self.say(format!("Could not paste the picture: {e}")),
            }
            return true;
        }
        if let Ok(files) = cb.get().file_list() {
            if !files.is_empty() {
                for f in files {
                    self.open_dropped(f, at);
                }
                return true;
            }
        }
        match text {
            // Copied SVG markup (from a design tool or a web page) is a drawing.
            Some(t) if images::is_svg(&t) => {
                match images::from_svg(t.as_bytes()) {
                    Ok(a) => self.insert_image(a, at),
                    Err(e) => self.say(format!("Could not paste the drawing: {e}")),
                }
                true
            }
            Some(t) if !t.trim().is_empty() => {
                self.paste_text(&t, at);
                true
            }
            _ => false,
        }
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    fn system_paste(&mut self) -> bool {
        false
    }

    /// A file dropped on the window (or picked): a picture goes on the
    /// canvas, a canvas opens, text is pasted.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_dropped(&mut self, path: PathBuf, at: Option<[f64; 2]>) {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "ogp" {
            self.open_file(path);
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.say(format!("Could not read {}: {e}", path.display()));
                return;
            }
        };
        if ext == "pdf" {
            self.import_pdf(bytes, at, &file_label(&path));
            return;
        }
        if ext == "svg" {
            match images::from_svg(&bytes) {
                Ok(a) => self.insert_image(a, at),
                Err(e) => self.say(format!("Could not add {}: {e}", file_label(&path))),
            }
            return;
        }
        if matches!(ext.as_str(), "txt" | "tsv" | "md" | "csv") {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let text = if ext == "csv" {
                text.replace(',', "\t")
            } else {
                text
            };
            self.paste_text(&text, at);
            return;
        }
        match images::prepare(bytes) {
            Ok(a) => self.insert_image(a, at),
            Err(e) => self.say(format!("Could not add {}: {e}", file_label(&path))),
        }
    }

    fn shortcut(&mut self, key: &Key) -> bool {
        let ctrl = self.mods.control_key() || self.mods.super_key();
        let shift = self.mods.shift_key();
        let selecting = self.ui.tool.selects() && !self.edit.selection.is_empty();
        let step = if shift { 10.0 } else { 1.0 };
        // Waiting for a key to assign (Settings > Hotkeys).
        if let Some(id) = self.ui.key_capture.clone() {
            match key {
                Key::Named(NamedKey::Escape) => self.ui.key_capture = None,
                Key::Named(NamedKey::Backspace | NamedKey::Delete) => {
                    self.ui.keys.set(&id, None);
                    self.ui.key_capture = None;
                    self.save_keys();
                }
                Key::Named(
                    NamedKey::Control
                    | NamedKey::Shift
                    | NamedKey::Alt
                    | NamedKey::Super
                    | NamedKey::Meta,
                ) => {}
                k => {
                    if let Some(c) = self.combo(k) {
                        if c.fixed() {
                            self.say(format!(
                                "{} is kept for the app; pick another key",
                                c.label()
                            ));
                        } else {
                            let label = c.label();
                            if let Some(from) = self.ui.keys.set(&id, Some(c)) {
                                let name = hotkeys::bindings()
                                    .into_iter()
                                    .find(|b| b.id == from)
                                    .map_or(from, |b| b.label);
                                self.say(format!("{label} moved here from {name}"));
                            }
                            self.ui.key_capture = None;
                            self.save_keys();
                        }
                    }
                }
            }
            self.redraw();
            return true;
        }
        // Your hotkeys first (fixed keys are never in the keymap).
        if let Some(c) = self.combo(key) {
            if !c.fixed() {
                if let Some(id) = self.ui.keys.lookup(&c).map(str::to_string) {
                    self.run_binding(&id);
                    self.redraw();
                    return true;
                }
            }
        }
        let k = match key {
            Key::Character(c) => c.to_lowercase(),
            Key::Named(NamedKey::Home) => {
                self.action(Action::Home);
                return true;
            }
            Key::Named(NamedKey::Enter) if self.edit.crop.is_some() => {
                self.crop_done();
                return true;
            }
            Key::Named(NamedKey::Escape) if self.edit.crop.is_some() => {
                self.crop_cancel();
                return true;
            }
            Key::Named(NamedKey::Delete | NamedKey::Backspace) if selecting => {
                self.sel_action(Action::Delete);
                return true;
            }
            // Enter on a selected text (or table) opens it for editing.
            Key::Named(NamedKey::Enter) if selecting && self.edit.text.is_none() => {
                self.sel_action(Action::EditText);
                return true;
            }
            Key::Named(NamedKey::Enter) if self.import.is_some() => {
                self.import_finish(true);
                return true;
            }
            Key::Named(NamedKey::Escape) if self.import.is_some() => {
                self.import_finish(false);
                return true;
            }
            Key::Named(NamedKey::Escape) => {
                // Menus first; then the selection.
                if !self.ui.close_all() {
                    self.edit.selection.clear();
                }
                self.redraw();
                return true;
            }
            Key::Named(NamedKey::ArrowLeft) if selecting => {
                self.nudge(-step, 0.0);
                return true;
            }
            Key::Named(NamedKey::ArrowRight) if selecting => {
                self.nudge(step, 0.0);
                return true;
            }
            Key::Named(NamedKey::ArrowUp) if selecting => {
                self.nudge(0.0, -step);
                return true;
            }
            Key::Named(NamedKey::ArrowDown) if selecting => {
                self.nudge(0.0, step);
                return true;
            }
            _ => return false,
        };
        match (ctrl, shift, k.as_str()) {
            (true, _, "y") => self.action(Action::Redo),
            (true, _, "d") if selecting => self.sel_action(Action::Duplicate),
            (true, _, "c") if selecting => {
                self.copy_selection();
                self.clip_mark();
            }
            (true, _, "x") if selecting => {
                self.copy_selection();
                self.clip_mark();
                self.sel_action(Action::Delete);
            }
            (true, _, "v") => {
                if !self.system_paste() {
                    self.paste();
                }
            }
            (true, _, "a") => self.select_all_visible(),
            (true, _, "]") if selecting => self.sel_action(Action::ToFront),
            (true, _, "[") if selecting => self.sel_action(Action::ToBack),
            // Toolbars: [ and ] cycle, Alt+1–9 jump.
            (false, _, "[") => self.ui.cycle_bar(-1),
            (false, _, "]") => self.ui.cycle_bar(1),
            (false, _, d) if self.mods.alt_key() && d.len() == 1 && ("1"..="9").contains(&d) => {
                self.ui.switch_bar(d.parse::<usize>().expect("digit") - 1);
            }
            // The quick bar's slots.
            (false, _, d) if d.len() == 1 && ("1"..="9").contains(&d) => {
                let i = d.parse::<usize>().expect("digit") - 1;
                if !self.ui.use_slot(i) {
                    return false;
                }
            }
            _ => return false,
        }
        self.redraw();
        true
    }

    /// A key press as a hotkey combo (None for keys that can't be one).
    fn combo(&self, key: &Key) -> Option<hotkeys::Combo> {
        let k = match key {
            Key::Character(c) => {
                let c = c.to_lowercase();
                // Shifted digits and symbols come as their shifted character;
                // keep the character itself.
                if c.trim().is_empty() {
                    return None;
                }
                c
            }
            Key::Named(n) => match n {
                NamedKey::Control
                | NamedKey::Shift
                | NamedKey::Alt
                | NamedKey::Super
                | NamedKey::Meta => return None,
                n => format!("{n:?}").to_lowercase(),
            },
            _ => return None,
        };
        Some(hotkeys::Combo {
            ctrl: self.mods.control_key() || self.mods.super_key(),
            shift: self.mods.shift_key(),
            alt: self.mods.alt_key(),
            key: k,
        })
    }

    fn save_keys(&mut self) {
        let mut p = prefs::load();
        p.insert("keys".into(), self.ui.keys.save_text());
        prefs::save(&p);
    }

    /// Do what hotkey `id` stands for.
    fn run_binding(&mut self, id: &str) {
        let shape = |app: &mut App, k: shapes::ShapeKind| {
            app.ui.tool = Tool::Shapes;
            app.ui.shape.kind = k;
        };
        match id {
            "tool.brush" => self.ui.tool = Tool::Pen,
            "tool.texture" => self.ui.tool = Tool::Texture,
            "tool.highlighter" => self.ui.tool = Tool::Highlighter,
            "tool.bucket" => self.ui.tool = Tool::Bucket,
            "tool.eraser" => self.ui.tool = Tool::Eraser,
            "tool.select" => {
                // Again for the lasso.
                self.ui.tool = if self.ui.tool == Tool::Select {
                    Tool::Lasso
                } else {
                    Tool::Select
                }
            }
            "tool.lasso" => self.ui.tool = Tool::Lasso,
            "tool.shapes" => self.ui.tool = Tool::Shapes,
            "shape.rect" => shape(self, shapes::ShapeKind::Rect),
            "shape.ellipse" => shape(self, shapes::ShapeKind::Ellipse),
            "shape.diamond" => shape(self, shapes::ShapeKind::Diamond),
            "shape.arrow" => shape(self, shapes::ShapeKind::Arrow),
            "shape.line" => shape(self, shapes::ShapeKind::Line),
            "tool.text" => self.ui.tool = Tool::Text,
            "tool.hand" => self.ui.tool = Tool::Hand,
            "tool.picker" => {
                if !matches!(
                    self.ui.tool,
                    Tool::Picker | Tool::Eraser | Tool::Hand | Tool::Select | Tool::Lasso
                ) {
                    self.ui.last_ink = self.ui.tool;
                }
                self.ui.tool = Tool::Picker;
            }
            "brush.simple" => {
                self.ui.tool = Tool::Pen;
                self.ui.pen.advanced = false;
            }
            "cmd.undo" => self.action(Action::Undo),
            "cmd.redo" => self.action(Action::Redo),
            "cmd.new" => self.action(Action::New),
            "cmd.open" => self.action(Action::Open),
            "cmd.save" => self.action(Action::SaveAs),
            "cmd.export" => self.action(Action::Export),
            "cmd.paste" => self.action(Action::Paste),
            "cmd.picture" => self.action(Action::Picture),
            "cmd.import" => self.action(Action::Import),
            "cmd.merge" => self.action(Action::MergeCopy),
            "cmd.search" => self.action(Action::Search),
            "cmd.bookmarks" => self.action(Action::Bookmarks),
            "cmd.timeline" => self.action(Action::Timeline),
            "cmd.library" => self.action(Action::Library),
            "cmd.home" => self.action(Action::Home),
            "cmd.grid" => self.action(Action::Grid),
            "cmd.dark" => self.action(Action::Dark),
            "cmd.diagram" => self.action(Action::Diagram),
            "cmd.layout" => self.action(Action::EditLayout),
            "cmd.fullscreen" => self.action(Action::FullScreen),
            "cmd.hotkeys" => self.action(Action::Hotkeys),
            id => {
                // A brush or texture look.
                let Some(n) = id
                    .strip_prefix("look.")
                    .and_then(|n| n.parse::<usize>().ok())
                else {
                    return;
                };
                let Some(l) = ogpaper_core::brush::looks().into_iter().nth(n) else {
                    return;
                };
                let ink = if l.texture {
                    self.ui.tool = Tool::Texture;
                    &mut self.ui.texture
                } else {
                    self.ui.tool = Tool::Pen;
                    &mut self.ui.pen
                };
                ink.advanced = true;
                ink.params = ogpaper_core::BrushParams {
                    seed: ink.params.seed,
                    ..l.params
                };
                self.say(format!(
                    "{}: {}",
                    if l.texture { "Texture" } else { "Brush" },
                    l.name
                ));
            }
        }
    }

    // ---- touch -----------------------------------------------------------

    fn touch(&mut self, id: u64, phase: TouchPhase, pos: [f64; 2], pressure: f32) {
        let slop_scale = self.ppp();
        match phase {
            TouchPhase::Started => {
                self.fly = None;
                if self.touches.is_empty() {
                    self.touch_ink = Some(id);
                    self.multi_tap = None;
                    self.begin(pos, pressure);
                } else {
                    if self.touch_ink.take().is_some() {
                        // Second finger: the first touch becomes a pinch instead.
                        self.end(true);
                    }
                    let n = self.touches.len() + 1;
                    // Fingers already down that have travelled are a gesture, not a tap.
                    let travelled = self.touches.iter().any(|(k, p)| {
                        self.touch_start
                            .get(k)
                            .is_some_and(|s0| dist(*s0, *p) > TAP_SLOP_PT * slop_scale)
                    });
                    let e = self.multi_tap.get_or_insert((Instant::now(), n, false));
                    e.1 = e.1.max(n);
                    e.2 |= travelled;
                }
                self.touch_start.insert(id, pos);
                self.touches.insert(id, pos);
            }
            TouchPhase::Moved => {
                let Some(&old) = self.touches.get(&id) else {
                    return;
                };
                // A finger that travels from where it started makes this a
                // pinch/pan, never a multi-finger tap.
                if let (Some(e), Some(s0)) = (self.multi_tap.as_mut(), self.touch_start.get(&id)) {
                    if dist(*s0, pos) > TAP_SLOP_PT * slop_scale {
                        e.2 = true;
                    }
                }
                if self.touch_ink == Some(id) {
                    self.moved(old, pos, pressure);
                } else if self.touches.len() >= 2 {
                    let before: Vec<[f64; 2]> = self.touches.values().take(2).copied().collect();
                    self.touches.insert(id, pos);
                    let after: Vec<[f64; 2]> = self.touches.values().take(2).copied().collect();
                    let c =
                        |v: &Vec<[f64; 2]>| [(v[0][0] + v[1][0]) * 0.5, (v[0][1] + v[1][1]) * 0.5];
                    let d =
                        |v: &Vec<[f64; 2]>| (v[0][0] - v[1][0]).hypot(v[0][1] - v[1][1]).max(1.0);
                    let (c0, c1) = (c(&before), c(&after));
                    self.cam.pan_px(c1[0] - c0[0], c1[1] - c0[1]);
                    self.cam.zoom_at(d(&after) / d(&before), self.centred(c1));
                    self.view_changed();
                }
                self.touches.insert(id, pos);
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.touch_ink == Some(id) {
                    self.touch_ink = None;
                    self.end(phase == TouchPhase::Cancelled);
                }
                self.touches.remove(&id);
                self.touch_start.remove(&id);
                // Two-finger tap = undo, three-finger tap = redo.
                if self.touches.is_empty() {
                    if let Some((t0, n, moved)) = self.multi_tap.take() {
                        if !moved && t0.elapsed() < Duration::from_millis(300) {
                            match n {
                                2 => self.undo_redo(false),
                                3 => self.undo_redo(true),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- bookmarks and flying ---------------------------------------------

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn view_px(&self) -> f64 {
        let [w, h] = self.size();
        w.min(h).max(1.0)
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn bookmark_add(&mut self, name: String) {
        let name = if name.trim().is_empty() {
            format!("View {}", self.bookmarks.len() + 1)
        } else {
            name.trim().to_string()
        };
        self.say(format!("Bookmarked \"{name}\""));
        self.bookmarks.push(Bookmark {
            name,
            cam: self.cam.clone(),
            view_px: self.view_px(),
        });
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Start flying to bookmark `i`, framed for this screen.
    fn bookmark_go(&mut self, i: usize) {
        let Some(b) = self.bookmarks.get(i) else {
            return;
        };
        let mut target = b.cam.clone();
        target.base_px = self.cam.base_px;
        target.zoom_at(self.view_px() / b.view_px, [0.0, 0.0]);
        self.fly = Some(target);
        self.fly_last = Instant::now();
        self.redraw();
    }

    /// One frame of a flight: pan the target toward the centre and zoom
    /// about it, quickly across many decades and gently at the end. If the
    /// target is off screen, zoom out first until it is in view.
    fn fly_step(&mut self) {
        let Some(target) = self.fly.clone() else {
            return;
        };
        let now = Instant::now();
        let dt = (now - self.fly_last).as_secs_f64().clamp(0.001, 0.05);
        self.fly_last = now;
        let half = self.view_px() * 0.5;
        let p = self.cam.to_screen(&target.cell, target.off);
        let d = p[0].hypot(p[1]);
        let dz = target.log10_zoom() - self.cam.log10_zoom();
        if d < 0.5 && dz.abs() < 1e-3 || !d.is_finite() && dz.abs() < 1e-3 {
            self.cam = target;
            self.fly = None;
            self.view_changed();
            return;
        }
        if !d.is_finite() || d > half * 1.5 {
            // Out of sight: back out until it shows, faster the farther it is.
            let need = if d.is_finite() {
                (d / half).log10()
            } else {
                300.0
            };
            let speed = (need * 2.5).clamp(1.0, 18.0);
            self.cam
                .zoom_at(10f64.powf(-need.min(speed * dt)), [0.0, 0.0]);
        } else {
            let k = 1.0 - (-7.0 * dt).exp();
            self.cam.pan_px(-p[0] * k, -p[1] * k);
            let p = self.cam.to_screen(&target.cell, target.off);
            let speed = (dz.abs() * 2.5).clamp(0.4, 18.0);
            let step = dz.signum() * dz.abs().min(speed * dt);
            self.cam.zoom_at(10f64.powf(step), p);
        }
        self.view_changed();
    }

    // ---- timeline ----------------------------------------------------------

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Show the canvas as it was after event `i` (None: back to now).
    fn timeline_show(&mut self, i: Option<usize>) {
        self.timeline_range(i.map(|i| (0, i)));
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Show only the ink drawn between events `from` and `to` that is still
    /// there at `to` (None: back to now). `from` = 0 is the whole history up
    /// to `to`.
    fn timeline_range(&mut self, range: Option<(usize, usize)>) {
        match range {
            Some((from, i)) if !self.timeline.events.is_empty() => {
                if self.gesture == Gesture::Ink || self.gesture == Gesture::Erase {
                    self.end(true);
                }
                let i = i.min(self.timeline.events.len() - 1);
                let from = from.min(i);
                let live = match self.tl_view.take() {
                    Some((_, live)) => live,
                    None => self.scene.strokes.iter().map(|s| !s.deleted).collect(),
                };
                let vis = self
                    .timeline
                    .visible_between(from, i, self.scene.strokes.len());
                timeline::apply(&mut self.scene, &vis);
                self.tl_view = Some((i, live));
                self.tl_from = from;
            }
            _ => {
                if let Some((_, live)) = self.tl_view.take() {
                    timeline::apply(&mut self.scene, &live);
                }
                self.tl_from = 0;
            }
        }
        self.redraw();
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    /// Keep the moment on screen: leave the timeline with the canvas as it
    /// was then. Recorded as one erase and one redraw, so it can be undone.
    fn timeline_restore(&mut self) {
        // Restore always means the whole canvas at the window's end: the left
        // handle only narrows the view and never erases older work.
        if let Some((i, _)) = self.tl_view {
            if self.tl_from > 0 {
                let full = self.timeline.visible_after(i, self.scene.strokes.len());
                timeline::apply(&mut self.scene, &full);
            }
        }
        self.tl_from = 0;
        let Some((_, live)) = self.tl_view.take() else {
            return;
        };
        let (mut gone, mut back) = (Vec::new(), Vec::new());
        for (id, &was) in live.iter().enumerate() {
            let now = !self.scene.strokes[id].deleted;
            if was && !now {
                gone.push(id as u32);
            } else if !was && now {
                back.push(id as u32);
            }
        }
        for &id in &gone {
            self.note(id, false);
            self.persist_deleted(id);
        }
        for &id in &back {
            self.note(id, true);
            self.persist_deleted(id);
        }
        if !gone.is_empty() {
            self.history.record(Change::Deleted(gone));
        }
        if !back.is_empty() {
            self.history.record(Change::Added(back));
        }
        self.say("Restored that moment (undo to go back)");
        self.redraw();
    }

    // ---- web page bridge ---------------------------------------------------

    #[cfg(target_arch = "wasm32")]
    fn web_cmd(&mut self, c: web::Cmd) {
        use web::Cmd;
        let changes = !matches!(
            c,
            Cmd::Timeline(_)
                | Cmd::TimelineRange(_)
                | Cmd::Search
                | Cmd::Sticker(..)
                | Cmd::Import(_)
                | Cmd::Merge(_)
                | Cmd::SearchGo(_)
                | Cmd::Export(..)
                | Cmd::BookmarkGo(_)
                | Cmd::BookmarkToBar(_)
                | Cmd::Home
                | Cmd::Menu(_)
                | Cmd::Text(None)
                | Cmd::FontAdded(..)
                | Cmd::Copy(false)
        );
        match c {
            Cmd::Load(bytes, demo) => {
                let snap = bytes.map(|b| snapshot::decode(&b, BASE_PX));
                match snap {
                    Some(Ok(s)) => {
                        self.load_scene(s.scene, s.cam);
                        self.share = share::Share::loaded(&self.scene, s.share);
                        self.timeline = s.timeline;
                        self.bookmarks = s.bookmarks;
                        self.objs = s.objs;
                        self.ui.file_name = "Canvas".into();
                    }
                    other => {
                        if let Some(Err(e)) = other {
                            self.say(format!("Could not open that copy: {e}"));
                        }
                        if demo {
                            self.web_cmd(Cmd::Demo);
                        } else {
                            self.web_cmd(Cmd::Blank);
                        }
                    }
                }
            }
            Cmd::Demo => {
                let d = demo::build();
                let tl = d.timeline(timeline::now_ms());
                self.bookmarks = d.bookmarks();
                let [w, h] = self.size();
                let cam = demo::frame_cell(&d.worlds[0], w, h);
                self.load_scene(d.scene, cam);
                self.timeline = tl;
                self.ui.file_name = "Try-mode demo".into();
            }
            Cmd::Blank => self.new_canvas(),
            Cmd::Menu(items) => self.ui.app_items = items,
            Cmd::Home => {
                self.fly = Some(home_camera());
                self.fly_last = Instant::now();
            }
            Cmd::BookmarkAdd(name) => self.bookmark_add(name),
            Cmd::BookmarkGo(i) => self.bookmark_go(i),
            Cmd::BookmarkToBar(i) => {
                if let Some(b) = self.bookmarks.get(i) {
                    let v = hotbar::View {
                        name: b.name.clone(),
                        cam: hotbar::cam_text(&b.cam, b.view_px),
                    };
                    let m = self.ui.view_to_bar(v);
                    self.say(m);
                }
            }
            Cmd::BookmarkRemove(i) => {
                if i < self.bookmarks.len() {
                    self.bookmarks.remove(i);
                }
            }
            Cmd::BookmarkRename(i, name) => {
                if let Some(b) = self.bookmarks.get_mut(i) {
                    if !name.trim().is_empty() {
                        b.name = name.trim().to_string();
                    }
                }
            }
            Cmd::Text(Some(text)) => {
                if let Some(t) = self.edit.text.as_mut() {
                    t.text = text;
                }
                self.text_commit();
            }
            Cmd::Text(None) => self.text_cancel(),
            Cmd::FontAdded(id, name) => self.font_added(id, &name),
            Cmd::Copy(cut) => {
                self.copy_selection();
                if cut {
                    self.sel_action(Action::Delete);
                }
            }
            Cmd::PasteOwn => self.paste(),
            Cmd::PdfPage(page, i, n, at, name) => {
                let k = web_dpr() as f64;
                self.pdf_page(page, i, n, at.map(|p| [p[0] * k, p[1] * k]), &name);
            }
            Cmd::Picture(a, at) => {
                let k = web_dpr() as f64;
                self.insert_image(a, at.map(|p| [p[0] * k, p[1] * k]));
            }
            Cmd::PasteText(t, at) => {
                let k = web_dpr() as f64;
                self.paste_text(&t, at.map(|p| [p[0] * k, p[1] * k]));
            }
            Cmd::Timeline(i) => self.timeline_show(i),
            Cmd::TimelineRange(r) => self.timeline_range(r),
            Cmd::Search => {
                let hits = self.search(&web::search_query());
                let json: Vec<String> = hits
                    .iter()
                    .map(|h| {
                        format!(
                            "{{\"g\":{},\"text\":{},\"zoom\":{:.1}}}",
                            h.group,
                            web::json_str(&h.snippet),
                            h.zoom
                        )
                    })
                    .collect();
                web::set_search_results(format!("[{}]", json.join(",")));
            }
            Cmd::SearchGo(g) => self.search_go(g),
            Cmd::Merge(bytes) => match snapshot::decode(&bytes, BASE_PX) {
                Ok(s) => self.merge_copy(s.scene, s.objs, s.share),
                Err(e) => self.say(format!("Could not read that copy: {e}")),
            },
            Cmd::Import(bytes) => match snapshot::decode(&bytes, BASE_PX) {
                Ok(s) => self.import_begin(s.scene, s.objs, "that canvas"),
                Err(e) => self.say(format!("Could not import that file: {e}")),
            },
            Cmd::Sticker(bytes, at) => match library::Sticker::decode(&bytes) {
                Ok(s) => {
                    let k = web_dpr() as f64;
                    self.place_sticker(&s, at.map(|p| [p[0] * k, p[1] * k]))
                }
                Err(e) => self.say(format!("Could not place it: {e}")),
            },
            Cmd::Export(fmt, selection, background) => {
                let r = match export::Format::from_key(&fmt) {
                    Some(f) => self.export(
                        f,
                        &export::Opts {
                            selection: selection && !self.edit.selection.is_empty(),
                            background,
                            scale: 1.0,
                        },
                    ),
                    None => Err(format!("unknown format {fmt}")),
                };
                web::set_export(r);
            }
            Cmd::TimelineRestore => self.timeline_restore(),
        }
        if changes {
            web::touch();
        }
        self.redraw();
    }

    /// Hand the page our status (and a snapshot, if it asked for one).
    #[cfg(target_arch = "wasm32")]
    fn web_publish(&mut self) {
        web::set_has_selection(self.ui.tool.selects() && !self.edit.selection.is_empty());
        if web::snapshot_wanted() {
            // Saved as it is now, not as the timeline is showing it.
            let mut scene_flags = None;
            if let Some((_, live)) = &self.tl_view {
                scene_flags = Some(
                    self.scene
                        .strokes
                        .iter()
                        .map(|s| !s.deleted)
                        .collect::<Vec<_>>(),
                );
                let live = live.clone();
                timeline::apply(&mut self.scene, &live);
            }
            let bytes = snapshot::encode(
                &self.scene,
                &self.cam,
                &self.timeline,
                &self.bookmarks,
                &self.objs,
                &self.share.copy_log(),
            );
            if let Some(shown) = scene_flags {
                timeline::apply(&mut self.scene, &shown);
            }
            web::put_snapshot(bytes);
        }
        let st = web::get_stats();
        let marks: Vec<String> = self
            .bookmarks
            .iter()
            .map(|b| {
                format!(
                    "{{\"name\":{},\"zoom\":{:.2}}}",
                    web::json_str(&b.name),
                    b.cam.log10_zoom()
                )
            })
            .collect();
        let ev = &self.timeline.events;
        let tl = format!(
            "{{\"on\":{},\"i\":{},\"from\":{},\"tFrom\":{},\"n\":{},\"t\":{},\"first\":{},\"last\":{}}}",
            self.tl_view.is_some(),
            self.tl_view
                .as_ref()
                .map_or(ev.len().saturating_sub(1), |v| v.0),
            self.tl_from,
            ev.get(self.tl_from).map_or(0, |e| e.t),
            ev.len(),
            self.tl_view
                .as_ref()
                .and_then(|v| ev.get(v.0))
                .or(ev.last())
                .map_or(0, |e| e.t),
            ev.first().map_or(0, |e| e.t),
            ev.last().map_or(0, |e| e.t),
        );
        let deep = if st.deep_draw.is_finite() {
            st.deep_draw
        } else {
            -99.0
        };
        web::set_status(format!(
            "{{\"ready\":true,\"zoom\":{:.3},\"strokes\":{},\"drawn\":{},\"erased\":{},\"undos\":{},\"deepDraw\":{:.2},\"flying\":{},\"dirty\":{},\"bookmarks\":[{}],\"timeline\":{},\"dark\":{}}}",
            self.cam.log10_zoom(),
            self.scene.strokes.iter().filter(|s| !s.deleted).count(),
            st.drawn,
            st.erased,
            st.undos,
            deep,
            self.fly.is_some(),
            web::is_dirty(),
            marks.join(","),
            tl,
            self.ui.dark,
        ));
        if self.fly.is_some() {
            self.redraw();
        }
    }

    // ---- frame -----------------------------------------------------------

    fn frame(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        #[cfg(target_arch = "wasm32")]
        if self.gpu.is_none() {
            let ready = self.pending_gpu.borrow_mut().take();
            if let Some(mut g) = ready {
                let s = window.inner_size();
                g.resize(s.width, s.height);
                self.egui_io = Some(egui_io::EguiIo::new(
                    &self.egui_ctx,
                    &window,
                    g.max_texture_side(),
                ));
                self.gpu = Some(g);
            }
        }
        if self.gpu.is_none() {
            log::debug!("frame: no gpu yet");
            return;
        }
        // Web: size the drawing buffer from the browser (CSS size x devicePixelRatio).
        #[cfg(target_arch = "wasm32")]
        if let Some((w, h)) = web_canvas_size(&window) {
            let g = self.gpu.as_mut().expect("gpu");
            if g.config.width != w || g.config.height != h {
                g.resize(w, h);
            }
        }
        log::debug!("frame");
        #[cfg(target_arch = "wasm32")]
        for c in web::take_cmds() {
            self.web_cmd(c);
        }
        self.fly_step();
        // Every message (from the app or the UI) shows for 4 seconds.
        if self.ui.message != self.message_seen {
            self.message_seen = self.ui.message.clone();
            self.message_until = self
                .ui
                .message
                .is_some()
                .then(|| Instant::now() + Duration::from_secs(4));
        }
        if self.message_until.is_some_and(|t| Instant::now() > t) {
            self.ui.message = None;
            self.message_seen = None;
            self.message_until = None;
        }
        if self
            .view_dirty_since
            .is_some_and(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.save_view();
        }

        // UI
        self.ui.zoom_log10 = self.cam.log10_zoom();
        self.ui.timeline_on = self.tl_view.is_some();
        self.ui.can_undo = self.history.can_undo();
        self.ui.importing = self.import.is_some();
        self.ui.can_redo = self.history.can_redo();
        self.ui.strokes = self.scene.strokes.iter().filter(|s| !s.deleted).count();
        self.sync_egui_fonts();
        let pointer_down = self.egui_ctx.input(|i| i.pointer.any_down());
        self.sync_sel_panel(pointer_down);
        self.build_overlay();
        self.ui.view_now = Some(hotbar::View {
            name: String::new(),
            cam: hotbar::cam_text(&self.cam, self.view_px()),
        });
        let raw = self.egui_io.as_mut().expect("egui").take_input(&window);
        let mut actions = Vec::new();
        let out = self
            .egui_ctx
            .run_ui(raw, |ui| actions = ui::draw(ui.ctx(), &mut self.ui));
        self.egui_io
            .as_mut()
            .expect("egui")
            .output(&window, out.platform_output);
        for a in actions {
            self.action(a);
        }
        // A saved view tapped in the quick bar: fly there, framed as saved.
        if let Some(c) = self.ui.fly_to.take() {
            if let Some((mut target, view_px)) = hotbar::cam_parse(&c, self.cam.base_px) {
                target.zoom_at(self.view_px() / view_px.max(1.0), [0.0, 0.0]);
                self.fly = Some(target);
                self.fly_last = Instant::now();
                self.redraw();
            }
        }
        if std::mem::take(&mut self.ui.presets_dirty) {
            hotbar::save(&self.ui.saved());
        }
        if out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero())
        {
            window.request_redraw();
        }
        let prims = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        {
            let key = (
                window.inner_size().width,
                self.size()[0] as u32,
                (out.pixels_per_point * 100.0) as u32,
            );
            if self.last_dbg != key {
                self.last_dbg = key;
                log::info!(
                    "sizes: inner {:?} scale {} surface {:?} egui ppp {} content {:?}",
                    window.inner_size(),
                    window.scale_factor(),
                    self.size(),
                    out.pixels_per_point,
                    self.egui_ctx.content_rect()
                );
            }
        }

        #[cfg(target_arch = "wasm32")]
        self.web_publish();

        // A PDF import: one page per frame.
        if self.pdf.is_some() && self.pdf_step() {
            window.request_redraw();
        }
        // Search (desktop panel): refresh results as the query changes, and
        // fly to a picked one.
        if self.ui.search_open && self.ui.search_ran != self.ui.search_query {
            self.ui.search_ran = self.ui.search_query.clone();
            self.ui.search_hits = self.search(&self.ui.search_query);
        }
        if let Some(g) = self.ui.search_pick.take() {
            self.search_go(g);
        }
        if self.flash.is_some_and(|(_, t)| t.elapsed() < search::FLASH) {
            window.request_redraw();
        } else {
            self.flash = None;
        }
        // Canvas
        let [w, h] = self.size();
        query(&self.scene, &self.cam, w, h, VIEW, &mut self.draw);
        let ink = self.ui.ink().copied();
        let tool = self.ui.tool;
        let wet = match (
            self.gesture == Gesture::Ink,
            ink,
            ink.and_then(|i| i.brush_for(tool)),
        ) {
            (true, Some(ink), Some(brush)) => Some(Wet {
                params: (brush == ogpaper_core::Brush::Dabs).then_some(ink.params),
                pts: &self.wet,
                width_px: ink.width * self.ppp() as f32,
                color: u32::from_le_bytes(ink.rgba()),
                brush,
                dash: if brush == ogpaper_core::Brush::Highlighter {
                    ogpaper_core::Dash::Solid
                } else {
                    ink.dash
                },
            }),
            _ => None,
        };
        // Background grid on the canvas's own cells (see grid.wgsl).
        let grid = (self.ui.grid != ui::GridMode::Off).then(|| {
            let ppp = self.ppp();
            let ppc = self.cam.ppc();
            let [w, h] = self.size();
            let target = 22.0 * ppp;
            // Finest power-of-two subdivision of a camera cell still >= target / 2.
            let n = (ppc / (target * 0.5)).log2().floor().max(0.0);
            render::GridGpu {
                origin: [
                    (w * 0.5 - self.cam.off[0] * ppc) as f32,
                    (h * 0.5 - self.cam.off[1] * ppc) as f32,
                ],
                fine: (ppc / 2f64.powf(n)) as f32,
                target: target as f32,
                mode: if self.ui.grid == ui::GridMode::Lines {
                    1
                } else {
                    2
                },
                ppp: ppp as f32,
                ..Default::default()
            }
        });
        self.gpu.as_mut().expect("gpu").grid = grid;
        self.gpu.as_mut().expect("gpu").dark = self.ui.dark;
        let paint = UiPaint {
            prims,
            textures: out.textures_delta,
            pixels_per_point: out.pixels_per_point,
        };
        if !self
            .gpu
            .as_mut()
            .expect("gpu")
            .render(&self.scene, &self.draw, &self.objs, wet, paint)
        {
            log::debug!("frame not presented; retrying");
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    /// Wake up to hide a message when its time is up.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.message_until {
            Some(t) if Instant::now() >= t => {
                self.redraw();
                el.set_control_flow(ControlFlow::Wait);
            }
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn new_events(&mut self, _el: &ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. }) {
            self.redraw();
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => {
                let attrs = Window::default_attributes().with_title("OG Paper");
                #[cfg(target_arch = "wasm32")]
                let attrs = {
                    use winit::platform::web::WindowAttributesExtWebSys;
                    attrs.with_append(true)
                };
                let w = Arc::new(el.create_window(attrs).expect("create window"));
                #[cfg(target_arch = "wasm32")]
                web::set_window(w.clone());
                self.window = Some(w.clone());
                w
            }
        };
        if let Some(g) = self.gpu.as_mut() {
            g.resume(window.clone());
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let g = pollster::block_on(Renderer::new(window.clone())).expect("GPU init");
                self.egui_io = Some(egui_io::EguiIo::new(
                    &self.egui_ctx,
                    &window,
                    g.max_texture_side(),
                ));
                self.gpu = Some(g);
                if let Some(p) = self.open_path.take() {
                    self.open_file(p);
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                // The GPU is created asynchronously on the web; `frame` adopts it.
                let slot = self.pending_gpu.clone();
                let w = window.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match Renderer::new(w.clone()).await {
                        Ok(g) => *slot.borrow_mut() = Some(g),
                        Err(e) => log::error!("{e}"),
                    }
                    w.request_redraw();
                });
            }
        }
        self.ui.touch_ui = cfg!(target_os = "android") || is_touch_web();
        window.request_redraw();
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        self.save_view();
        if let Some(g) = self.gpu.as_mut() {
            g.suspend();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        // egui first: it takes events over its own panels.
        let consumed = match self.egui_io.as_mut() {
            Some(io) => {
                let (consumed, repaint) = io.on_event(&window, &event);
                if repaint {
                    window.request_redraw();
                }
                consumed
            }
            None => false,
        };
        // Also hit-test the UI at the cursor: egui's hover state lags a
        // frame, so a click with no mouse move before it would otherwise ink
        // under a button. (The root background layer covers the whole screen
        // and is canvas, not UI.)
        let ppp = self.egui_ctx.pixels_per_point() as f64;
        let at = egui::pos2((self.cursor[0] / ppp) as f32, (self.cursor[1] / ppp) as f32);
        let over_ui = (self.egui_ctx.is_pointer_over_egui()
            || self
                .egui_ctx
                .layer_id_at(at)
                .is_some_and(|l| l.order != egui::Order::Background))
            && self.gesture == Gesture::None;

        match event {
            WindowEvent::CloseRequested => {
                self.save_view();
                el.exit();
            }
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.resize(s.width, s.height);
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.frame(),
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::DroppedFile(path) => {
                self.open_dropped(path, Some(self.cursor));
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Key::Named(NamedKey::Space) = event.logical_key {
                    self.space = event.state == ElementState::Pressed;
                }
                if event.state == ElementState::Pressed
                    && !self.egui_ctx.egui_wants_keyboard_input()
                {
                    self.shortcut(&event.logical_key);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !over_ui => {
                self.fly = None;
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64 * 40.0, y as f64 * 40.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
                };
                if self.mods.shift_key() {
                    self.cam.pan_px(dx.max(dy), 0.0);
                } else {
                    self.cam
                        .zoom_at(1.0025f64.powf(dy), self.centred(self.cursor));
                }
                self.view_changed();
            }
            WindowEvent::PinchGesture { delta, .. } => {
                self.fly = None;
                self.cam.zoom_at(1.0 + delta, self.centred(self.cursor));
                self.view_changed();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = [position.x, position.y];
                let from = self.cursor;
                self.cursor = p;
                if self.gesture != Gesture::None {
                    self.moved(from, p, 1.0);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if !consumed && !over_ui => {
                    self.begin(self.cursor, 1.0)
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) if !over_ui => {
                    self.fly = None;
                    self.gesture = Gesture::Pan;
                }
                (ElementState::Released, _) if self.gesture != Gesture::None => self.end(false),
                _ => {}
            },
            WindowEvent::Touch(t) => {
                if consumed && self.touches.is_empty() {
                    return;
                }
                let pressure = t.force.map(|f| f.normalized() as f32).unwrap_or(1.0);
                self.touch(t.id, t.phase, [t.location.x, t.location.y], pressure);
            }
            _ => {}
        }
    }
}

/// Shapes and texts read from a file, matched to the strokes loaded.
#[cfg(not(target_arch = "wasm32"))]
fn groups_from_file(scene: &Scene, groups: Vec<ogpaper_file::FileGroup>) -> objects::Objects {
    let by_uid: HashMap<u128, u32> = scene
        .strokes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.uid, i as u32))
        .collect();
    let mut objs = objects::Objects::default();
    for g in groups {
        let Ok(data) = snapshot::get_data(&g.data) else {
            continue;
        };
        let strokes: Vec<u32> = g
            .strokes
            .iter()
            .filter_map(|u| by_uid.get(u).copied())
            .collect();
        if strokes.is_empty() {
            continue;
        }
        objs.add(objects::Group {
            cell: g.cell,
            data,
            strokes,
        });
    }
    objs
}

/// A file's canvas id and merge log, if it has an id.
#[cfg(not(target_arch = "wasm32"))]
fn file_copy_log(f: &OgpFile) -> Option<share::CopyLog> {
    let canvas = f.canvas_id().ok().flatten()?;
    let (events, replaces) = f.sync_log().unwrap_or_default();
    Some(share::CopyLog {
        canvas,
        events,
        replaces,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn file_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "canvas".into())
}

/// App-private storage on platforms without a home directory (Android).
#[cfg(not(target_arch = "wasm32"))]
static DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Fonts you added: ~/OG Paper/fonts/ (Android: the app's storage).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fonts_dir() -> Option<PathBuf> {
    let dir = match DATA_DIR.get() {
        Some(d) => d.join("fonts"),
        None => std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?
            .join("OG Paper")
            .join("fonts"),
    };
    Some(dir)
}

/// Register the fonts saved in the fonts folder.
#[cfg(not(target_arch = "wasm32"))]
fn load_user_fonts() {
    let Some(dir) = fonts_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let ext = p
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "ttf" || ext == "otf" {
            if let Ok(bytes) = std::fs::read(&p) {
                let _ = font::register(None, "Yours", bytes, true);
            }
        }
    }
}

/// Where a new canvas is saved before you pick a name: ~/OG Paper/.
#[cfg(not(target_arch = "wasm32"))]
fn default_path() -> Result<PathBuf, String> {
    let dir = match DATA_DIR.get() {
        // Android: the app's private storage.
        Some(d) => d.join("canvases"),
        None => std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or("no home directory")?
            .join("OG Paper"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for n in 1.. {
        let p = dir.join(if n == 1 {
            "Untitled.ogp".to_string()
        } else {
            format!("Untitled {n}.ogp")
        });
        if !p.exists() {
            return Ok(p);
        }
    }
    unreachable!()
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub fn run_desktop(open: Option<PathBuf>) {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .init();
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(open);
    el.run_app(&mut app).expect("event loop");
}

/// True on touch-first browsers (phones, tablets): bigger toolbar targets.
fn is_touch_web() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
            .is_some_and(|m| m.matches())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Browser entry point.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn web_start() {
    use winit::platform::web::EventLoopExtWebSys;
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    el.spawn_app(App::new(None));
}

/// The canvas's real drawing-buffer size, kept in sync with its CSS size and
/// the device pixel ratio (winit's report can lag or miss the ratio).
#[cfg(target_arch = "wasm32")]
pub(crate) fn web_canvas_size(window: &Window) -> Option<(u32, u32)> {
    use winit::platform::web::WindowExtWebSys;
    let c = window.canvas()?;
    let dpr = web_sys::window()?.device_pixel_ratio();
    let w = ((c.client_width() as f64 * dpr).round() as u32).max(1);
    let h = ((c.client_height() as f64 * dpr).round() as u32).max(1);
    if c.width() != w {
        c.set_width(w);
    }
    if c.height() != h {
        c.set_height(h);
    }
    Some((w, h))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn web_dpr() -> f32 {
    web_sys::window()
        .map(|w| w.device_pixel_ratio() as f32)
        .unwrap_or(1.0)
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("og-paper"),
    );
    if let Some(dir) = app.internal_data_path() {
        let _ = DATA_DIR.set(dir);
    }
    let el = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    let mut a = App::new(None);
    el.run_app(&mut a).expect("event loop");
}
