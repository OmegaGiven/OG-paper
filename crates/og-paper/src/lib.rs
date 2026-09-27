// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! OG Paper: an open-source infinite canvas.

mod egui_io;
mod render;
mod ui;
mod uid;

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
use ui::{Action, Menu, Tool, UiState};

#[cfg(not(target_arch = "wasm32"))]
use ogpaper_file::OgpFile;

/// Real strokes all the way down to ~2 px; anything smaller draws nothing.
const VIEW: Params = Params {
    min_cell_px: 2.0,
    tile_px: 0.0,
    ancestor_levels: 8,
};
const ERASER_PX: f32 = 10.0;
/// How far (physical px) a finger may drift and still count as a tap.
const TAP_SLOP: f64 = 24.0;

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn home_camera() -> Camera {
    Camera::new(CellAddr::new(0, 0, 0), [0.5, 0.5], 800.0)
}

/// What the pointer is doing right now.
#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    None,
    Ink,
    Erase,
    Pan,
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

    #[cfg(not(target_arch = "wasm32"))]
    file: Option<OgpFile>,
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
    last_dbg: (u32, u32, u32),
}

impl App {
    pub fn new(open_path: Option<PathBuf>) -> Self {
        let _ = &open_path;
        Self {
            window: None,
            gpu: None,
            egui_ctx: egui::Context::default(),
            egui_io: None,
            ui: UiState::default(),
            scene: Scene::new(),
            cam: home_camera(),
            history: History::default(),
            draw: DrawList::default(),
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
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
            last_dbg: (0, 0, 0),
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
        self.message_until = Some(Instant::now() + Duration::from_secs(4));
    }

    fn view_changed(&mut self) {
        self.view_dirty_since.get_or_insert_with(Instant::now);
        self.redraw();
    }

    // ---- ink -------------------------------------------------------------

    fn ink_add(&mut self, p: [f64; 2], pressure: f32) {
        let pressure = if self.ui.tool == Tool::Pen && !self.ui.pen.pressure {
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
        let Some(brush) = self.ui.tool.brush() else {
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
        let width_cam = ink.width as f64 / ppc;
        let (cell, local, side) = Scene::anchor_for_min(&self.cam.cell, &cam_pts, width_cam);
        let pts: Vec<[f32; 4]> = local
            .iter()
            .zip(&self.wet)
            .map(|(l, w)| [l[0], l[1], w[2], 0.0])
            .collect();
        let [r, g, b, a] = ink.color.to_array();
        let color = u32::from_le_bytes([r, g, b, a]);
        let id = self.scene.add_stroke_styled(
            &cell,
            &pts,
            (width_cam / side) as f32,
            color,
            brush,
            uid::new(),
        );
        self.wet.clear();
        self.history.record(Change::Added(vec![id]));
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
            ERASER_PX,
        );
        for id in hits {
            if self.scene.delete(id) {
                self.erased.push(id);
                self.persist_deleted(id);
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
            self.ui.menu = Menu::None;
            self.gesture = Gesture::None;
            self.redraw();
            return;
        }
        self.gesture = match self.ui.tool {
            _ if self.space => Gesture::Pan,
            Tool::Hand => Gesture::Pan,
            Tool::Eraser => Gesture::Erase,
            _ => Gesture::Ink,
        };
        match self.gesture {
            Gesture::Ink => {
                self.wet.clear();
                self.ink_add(p, pressure);
            }
            Gesture::Erase => self.erase_at(p),
            _ => {}
        }
    }

    fn moved(&mut self, from: [f64; 2], to: [f64; 2], pressure: f32) {
        match self.gesture {
            Gesture::Ink => self.ink_add(to, pressure),
            Gesture::Erase => {
                // Sweep the whole path so fast swipes cannot skip a stroke.
                let d = (to[0] - from[0]).hypot(to[1] - from[1]);
                let steps = (d / (ERASER_PX as f64 * 0.5)).ceil().max(1.0) as usize;
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
            Gesture::None => {}
        }
    }

    fn end(&mut self, cancel: bool) {
        match self.gesture {
            Gesture::Ink if !cancel => self.ink_commit(),
            Gesture::Ink => self.wet.clear(),
            Gesture::Erase => self.erase_end(),
            _ => {}
        }
        self.gesture = Gesture::None;
        self.redraw();
    }

    // ---- undo / file -----------------------------------------------------

    fn undo_redo(&mut self, redo: bool) {
        let changed = if redo {
            self.history.redo(&mut self.scene)
        } else {
            self.history.undo(&mut self.scene)
        };
        for id in changed.unwrap_or_default() {
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

    #[cfg(not(target_arch = "wasm32"))]
    fn persist_deleted(&mut self, id: u32) {
        if let Some(f) = &self.file {
            let _ = f.put_deleted(&self.scene, id);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn persist_new(&mut self, _id: u32) {}
    #[cfg(target_arch = "wasm32")]
    fn persist_deleted(&mut self, _id: u32) {}

    fn save_view(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self.file {
            let _ = f.put_view(&self.cam);
        }
        self.view_dirty_since = None;
    }

    fn load_scene(&mut self, scene: Scene, cam: Camera) {
        self.scene = scene;
        self.cam = cam;
        self.history = History::default();
        self.wet.clear();
        if let Some(g) = self.gpu.as_mut() {
            g.reset(&self.scene);
        }
        self.redraw();
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
            Ok((f, scene, view)) => {
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
                self.say(format!("Opened {} ({n} strokes)", path.display()));
            }
            Err(e) => self.say(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn action(&mut self, a: Action) {
        match a {
            Action::Undo => self.undo_redo(false),
            Action::Redo => self.undo_redo(true),
            Action::Home => {
                self.cam = home_camera();
                self.view_changed();
            }
            Action::New => {
                self.save_view();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.file = None;
                }
                self.ui.file_name = "Untitled".into();
                self.load_scene(Scene::new(), home_camera());
            }
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
                            let _ = f.put_view(&self.cam);
                            self.ui.file_name = file_label(&p);
                            self.say(format!("Saved to {}", p.display()));
                            self.file = Some(f);
                        }
                        Err(e) => self.say(format!("Save failed: {e}")),
                    }
                }
            }
            #[allow(unreachable_patterns)]
            _ => self.say("Not available on this platform yet"),
        }
    }

    fn shortcut(&mut self, key: &Key) -> bool {
        let ctrl = self.mods.control_key() || self.mods.super_key();
        let shift = self.mods.shift_key();
        let k = match key {
            Key::Character(c) => c.to_lowercase(),
            Key::Named(NamedKey::Home) => {
                self.action(Action::Home);
                return true;
            }
            _ => return false,
        };
        match (ctrl, shift, k.as_str()) {
            (true, false, "z") => self.action(Action::Undo),
            (true, true, "z") | (true, _, "y") => self.action(Action::Redo),
            (true, _, "n") => self.action(Action::New),
            (true, _, "o") => self.action(Action::Open),
            (true, true, "s") | (true, false, "s") => self.action(Action::SaveAs),
            (false, _, "1") => self.ui.tool = Tool::Pen,
            (false, _, "2") => self.ui.tool = Tool::Marker,
            (false, _, "3") => self.ui.tool = Tool::Highlighter,
            (false, _, "e") => self.ui.tool = Tool::Eraser,
            (false, _, "h") => self.ui.tool = Tool::Hand,
            _ => return false,
        }
        self.redraw();
        true
    }

    // ---- touch -----------------------------------------------------------

    fn touch(&mut self, id: u64, phase: TouchPhase, pos: [f64; 2], pressure: f32) {
        match phase {
            TouchPhase::Started => {
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
                            .is_some_and(|s0| dist(*s0, *p) > TAP_SLOP)
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
                    if dist(*s0, pos) > TAP_SLOP {
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
        if self.message_until.is_some_and(|t| Instant::now() > t) {
            self.ui.message = None;
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
        self.ui.can_undo = self.history.can_undo();
        self.ui.can_redo = self.history.can_redo();
        self.ui.strokes = self.scene.strokes.iter().filter(|s| !s.deleted).count();
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

        // Canvas
        let [w, h] = self.size();
        query(&self.scene, &self.cam, w, h, VIEW, &mut self.draw);
        let ink = self.ui.ink().copied();
        let wet = match (self.gesture == Gesture::Ink, ink, self.ui.tool.brush()) {
            (true, Some(ink), Some(brush)) => {
                let [r, g, b, a] = ink.color.to_array();
                Some(Wet {
                    pts: &self.wet,
                    width_px: ink.width,
                    color: u32::from_le_bytes([r, g, b, a]),
                    brush,
                })
            }
            _ => None,
        };
        let paint = UiPaint {
            prims,
            textures: out.textures_delta,
            pixels_per_point: out.pixels_per_point,
        };
        if !self
            .gpu
            .as_mut()
            .expect("gpu")
            .render(&self.scene, &self.draw, wet, paint)
        {
            log::debug!("frame not presented; retrying");
            window.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
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
        let over_ui = self.egui_ctx.is_pointer_over_egui() && self.gesture == Gesture::None;

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

#[cfg(not(target_arch = "wasm32"))]
fn file_label(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "canvas".into())
}

/// Where a new canvas is saved before you pick a name: ~/OG Paper/.
#[cfg(not(target_arch = "wasm32"))]
fn default_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or("no home directory")?;
    let dir = home.join("OG Paper");
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
