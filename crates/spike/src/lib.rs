// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! Phase 0 spike: fly through a 1M-stroke canvas and a zoom chain ~10^48 deep.
//!
//! Desktop: wheel = zoom, right/middle drag = pan, left drag = draw,
//! A = auto-zoom, H = home, Esc = quit. `--bench` flies in and out once with
//! vsync off and prints frame-time stats.
//! Touch: one finger pans, two fingers pinch, double-tap toggles auto-zoom.

mod gpu;

use std::collections::HashMap;
use std::sync::Arc;
use web_time::{Duration, Instant};

use ogpaper_core::{gen, query, Camera, CellAddr, DrawList, Params, Scene};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use gpu::Gpu;

pub struct Options {
    pub mass: usize,
    pub depth: usize,
    pub bench: bool,
    /// Pack the mass pages solid (worst-case on-screen density).
    pub dense: bool,
    /// Start centred on chain step K instead of home.
    pub start_chain: Option<usize>,
    /// Start on the mass page at this level instead of home.
    pub start_mass: Option<i64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mass: 1_000_000,
            depth: 40,
            bench: false,
            dense: false,
            start_chain: None,
            start_mass: None,
        }
    }
}

/// A point on the mass page used by the benchmark sweep and screenshots.
fn mass_spot() -> (CellAddr, [f64; 2]) {
    (CellAddr::new(0, 1, 0), [0.37, 0.12])
}

/// Draw mode: when on, a single touch/pen draws instead of panning. Mouse
/// left-drag always draws.
static DRAW_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn draw_mode() -> bool {
    DRAW_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn set_draw_mode(on: bool) {
    DRAW_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn get_draw_mode() -> bool {
    draw_mode()
}

#[derive(Clone, Copy, PartialEq)]
enum Auto {
    Off,
    In,
    Out,
}

/// What auto-zoom flies toward.
#[derive(Clone, Copy, PartialEq)]
enum Tour {
    Chain,
    Mass,
}

#[derive(Default)]
struct BenchStats {
    frame_ms: Vec<f64>,
    query_us: Vec<f64>,
    max_strokes: u32,
    max_dots: u32,
    max_tiles: u32,
    max_zoom: f64,
    started_out: bool,
}

struct Spike {
    opts: Options,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    /// Web: the GPU is created asynchronously and dropped in here when ready.
    #[cfg(target_arch = "wasm32")]
    pending_gpu: std::rc::Rc<std::cell::RefCell<Option<Gpu>>>,
    scene: Scene,
    chain: Vec<CellAddr>,
    home: CellAddr,
    cam: Camera,
    draw: DrawList,
    auto: Auto,
    tour: Tour,
    // Input
    cursor: [f64; 2],
    panning: bool,
    wet: Vec<[f64; 2]>,
    drawing: bool,
    touches: HashMap<u64, [f64; 2]>,
    /// Touch id currently drawing (draw mode).
    touch_draw: Option<u64>,
    last_tap: Option<(Instant, [f64; 2])>,
    // Timing
    last_frame: Instant,
    fps_window: (Instant, u32, f64),
    bench: BenchStats,
    dirty: bool,
    build_secs: f64,
    /// Frames rendered so far.
    frames: u64,
}

impl Spike {
    fn new(opts: Options) -> Self {
        let t = Instant::now();
        let demo = gen::build_with(opts.mass, opts.depth, 7, opts.dense);
        log::info!(
            "built {} strokes / {} points / {} cells in {:?}",
            demo.scene.strokes.len(),
            demo.scene.points.len(),
            demo.scene.nodes.len(),
            t.elapsed()
        );
        let build_secs = t.elapsed().as_secs_f64();
        let cam = Camera::new(demo.home.clone(), [0.5, 0.25], 700.0);
        Self {
            opts,
            window: None,
            gpu: None,
            #[cfg(target_arch = "wasm32")]
            pending_gpu: Default::default(),
            scene: demo.scene,
            chain: demo.chain,
            home: demo.home,
            cam,
            draw: DrawList::default(),
            auto: Auto::Off,
            tour: Tour::Chain,
            cursor: [0.0; 2],
            panning: false,
            wet: Vec::new(),
            drawing: false,
            touches: HashMap::new(),
            touch_draw: None,
            last_tap: None,
            last_frame: Instant::now(),
            fps_window: (Instant::now(), 0, 0.0),
            bench: BenchStats::default(),
            dirty: true,
            build_secs,
            frames: 0,
        }
    }

    fn size(&self) -> [f64; 2] {
        self.gpu
            .as_ref()
            .map(|g| [g.config.width as f64, g.config.height as f64])
            .unwrap_or([1.0, 1.0])
    }

    /// Pixels from viewport centre.
    fn centred(&self, p: [f64; 2]) -> [f64; 2] {
        let s = self.size();
        [p[0] - s[0] * 0.5, p[1] - s[1] * 0.5]
    }

    fn go_home(&mut self) {
        self.cam = Camera::new(self.home.clone(), [0.5, 0.25], 700.0);
        self.dirty = true;
    }

    /// Auto-zoom: zoom toward the next chain cell, steering it to the centre.
    fn step_auto(&mut self, factor_in: f64) {
        if self.auto == Auto::Off {
            return;
        }
        let [w, h] = self.size();
        let target_px = 0.6 * w.min(h);
        let ppc = self.cam.ppc();
        // First chain cell still smaller than ~60% of the screen.
        let (target, last) = match self.tour {
            Tour::Chain => (
                self.chain
                    .iter()
                    .find(|c| c.side_in(&self.cam.cell) * ppc < target_px)
                    .map(|c| (c.clone(), [0.5, 0.5])),
                self.chain.last().map(|c| c.level).unwrap_or(0),
            ),
            // Down to where single letters fill the screen.
            Tour::Mass => (Some(mass_spot()), 9),
        };
        if let Some((t, local)) = target {
            let p = self.cam.to_screen(&t, local);
            if p[0].is_finite() && p[1].is_finite() {
                self.cam.pan_px(-p[0] * 0.08, -p[1] * 0.08);
            }
        }
        match self.auto {
            Auto::In => {
                self.cam.zoom_at(factor_in, [0.0, 0.0]);
                if self.cam.level() > last + 4 {
                    self.auto = Auto::Out;
                    self.bench.started_out = true;
                }
            }
            Auto::Out => {
                self.cam.zoom_at(1.0 / factor_in, [0.0, 0.0]);
                if self.cam.log10_zoom() < -0.6 {
                    self.auto = Auto::In;
                    if self.opts.bench {
                        // Bench: chain flight first, then a sweep over the dense page.
                        if self.tour == Tour::Chain {
                            self.tour = Tour::Mass;
                        } else {
                            self.auto = Auto::Off;
                        }
                    }
                }
            }
            Auto::Off => {}
        }
        self.dirty = true;
    }

    fn commit_wet(&mut self) {
        if self.wet.len() < 2 {
            self.wet.clear();
            return;
        }
        let cam_pts: Vec<[f64; 2]> = self
            .wet
            .iter()
            .map(|&p| self.cam.screen_to_cam(self.centred(p)))
            .collect();
        let ppc = self.cam.ppc();
        let first = self.scene.strokes.len();
        // Split into overlapping chunks of MAX_PTS; each chunk anchors to its own cell.
        let mut i = 0;
        while i + 1 < cam_pts.len() {
            let end = (i + gen::MAX_PTS).min(cam_pts.len());
            let (cell, local, side) = Scene::anchor_for(&self.cam.cell, &cam_pts[i..end]);
            let width = (3.0 / ppc / side) as f32;
            self.scene
                .add_stroke(&cell, &local, width, gen::rgba(200, 40, 90, 255));
            i = end - 1;
        }
        if let Some(g) = self.gpu.as_mut() {
            g.upload_new(&self.scene, first);
        }
        // Show the new stroke count right away.
        self.fps_window.0 -= Duration::from_secs(1);
        self.wet.clear();
        self.dirty = true;
    }

    fn frame(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if self.gpu.is_none() {
            let ready = self.pending_gpu.borrow_mut().take();
            if let Some(mut g) = ready {
                g.upload_new(&self.scene, 0);
                if let Some(w) = &self.window {
                    let s = w.inner_size();
                    g.resize(s.width, s.height);
                }
                self.gpu = Some(g);
            } else {
                return;
            }
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;

        if self.opts.bench {
            self.step_auto(1.04);
        } else {
            // ~6x zoom per second.
            self.step_auto(6f64.powf(dt.min(0.1)));
        }

        let [w, h] = self.size();
        let tq = Instant::now();
        query(
            &self.scene,
            &self.cam,
            w,
            h,
            Params::default(),
            &mut self.draw,
        );
        let q_us = tq.elapsed().as_secs_f64() * 1e6;

        let wet: Vec<[f32; 2]> = self
            .wet
            .iter()
            .map(|p| [p[0] as f32, p[1] as f32])
            .collect();
        if let Some(g) = self.gpu.as_mut() {
            g.render(&self.draw, &wet);
        }

        // Stats
        let st = self.draw.stats;
        self.frames += 1;
        self.fps_window.1 += 1;
        self.fps_window.2 = self.fps_window.2.max(q_us);
        if self.opts.bench && self.auto != Auto::Off && self.frames > 10 {
            self.bench.frame_ms.push(dt * 1e3);
            self.bench.query_us.push(q_us);
            self.bench.max_strokes = self.bench.max_strokes.max(st.strokes);
            self.bench.max_dots = self.bench.max_dots.max(st.dots);
            self.bench.max_tiles = self.bench.max_tiles.max(st.tiles);
            self.bench.max_zoom = self.bench.max_zoom.max(self.cam.log10_zoom());
        }
        let el = self.fps_window.0.elapsed();
        // Refresh at least every 0.5 s; also on the very first frame.
        if el >= Duration::from_millis(500) || self.frames == 1 {
            let fps = self.fps_window.1 as f64 / el.as_secs_f64();
            let msg = format!(
                "OG Paper spike | mode: {} | zoom 10^{:.1} (level {}) | {:.0} fps | {} strokes, {} tiles, {} dots | query max {:.2} ms | {} strokes in canvas",
                if draw_mode() { "DRAW (touch/pen draws)" } else { "PAN (touch pans; mouse left-drag draws)" },
                self.cam.log10_zoom(),
                self.cam.level(),
                fps,
                st.strokes,
                st.tiles,
                st.dots,
                self.fps_window.2 / 1e3,
                self.scene.strokes.len(),
            );
            if let Some(win) = &self.window {
                win.set_title(&msg);
            }
            #[cfg(target_arch = "wasm32")]
            set_hud(&msg.replace(" | ", "\n"));
            if cfg!(target_os = "android") || self.opts.bench {
                log::info!("{msg}");
            }
            self.fps_window = (Instant::now(), 0, 0.0);
        }
        self.dirty = false;
    }

    fn finish_bench(&self) {
        let pct = |v: &Vec<f64>, p: f64| {
            let mut s = v.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            s[((s.len() as f64 - 1.0) * p) as usize]
        };
        let b = &self.bench;
        if b.frame_ms.is_empty() {
            return;
        }
        let avg = b.frame_ms.iter().sum::<f64>() / b.frame_ms.len() as f64;
        println!("== OG Paper Phase 0 bench ==");
        println!(
            "canvas build: {:.1} s | peak memory: {}",
            self.build_secs,
            peak_rss()
        );
        println!(
            "canvas: {} strokes, {} cells",
            self.scene.strokes.len(),
            self.scene.nodes.len()
        );
        println!(
            "flight: 10^-0.6 -> 10^{:.1} -> back, {} frames",
            b.max_zoom,
            b.frame_ms.len()
        );
        println!(
            "frame ms: avg {:.2} ({:.0} fps) | p50 {:.2} | p95 {:.2} | p99 {:.2} | max {:.2}",
            avg,
            1e3 / avg,
            pct(&b.frame_ms, 0.5),
            pct(&b.frame_ms, 0.95),
            pct(&b.frame_ms, 0.99),
            pct(&b.frame_ms, 1.0)
        );
        println!(
            "query us: p50 {:.0} | p99 {:.0} | max {:.0}",
            pct(&b.query_us, 0.5),
            pct(&b.query_us, 0.99),
            pct(&b.query_us, 1.0)
        );
        println!(
            "max drawn per frame: {} strokes, {} tiles, {} dots",
            b.max_strokes, b.max_tiles, b.max_dots
        );
        println!("(frame = CPU query + upload + GPU draw, vsync off; first 10 frames excluded)");
    }

    fn handle_touch(&mut self, id: u64, phase: TouchPhase, pos: [f64; 2]) {
        match phase {
            TouchPhase::Started => {
                if self.touches.is_empty() {
                    if draw_mode() {
                        // One finger (or pen) draws.
                        self.touch_draw = Some(id);
                        self.wet = vec![pos];
                        self.dirty = true;
                    } else if let Some((t, p)) = self.last_tap {
                        // Double-tap toggles auto-zoom (pan mode only).
                        if t.elapsed() < Duration::from_millis(300)
                            && (p[0] - pos[0]).hypot(p[1] - pos[1]) < 40.0
                        {
                            self.auto = if self.auto == Auto::Off {
                                Auto::In
                            } else {
                                Auto::Off
                            };
                        }
                    }
                    self.last_tap = Some((Instant::now(), pos));
                } else {
                    // A second finger turns a stroke in progress into pan/pinch.
                    if self.touch_draw.take().is_some() {
                        self.wet.clear();
                    }
                    // Three-finger tap toggles draw mode.
                    if self.touches.len() == 2 {
                        set_draw_mode(!draw_mode());
                        log::info!("draw mode: {}", draw_mode());
                    }
                }
                self.touches.insert(id, pos);
            }
            TouchPhase::Moved => {
                if !self.touches.contains_key(&id) {
                    return;
                }
                if self.touch_draw == Some(id) {
                    let far = self
                        .wet
                        .last()
                        .map(|l| (l[0] - pos[0]).hypot(l[1] - pos[1]) >= 2.0)
                        .unwrap_or(true);
                    if far {
                        self.wet.push(pos);
                    }
                } else if self.touches.len() == 1 {
                    let old = self.touches[&id];
                    self.cam.pan_px(pos[0] - old[0], pos[1] - old[1]);
                } else {
                    // Pinch: compare centroid and spread of the first two touches.
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
                }
                self.touches.insert(id, pos);
                self.dirty = true;
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.touch_draw == Some(id) {
                    self.touch_draw = None;
                    if phase == TouchPhase::Ended {
                        self.commit_wet();
                    } else {
                        self.wet.clear();
                    }
                }
                self.touches.remove(&id);
                self.dirty = true;
            }
        }
    }
}

impl ApplicationHandler for Spike {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let window = match &self.window {
            Some(w) => w.clone(),
            None => {
                let attrs = Window::default_attributes().with_title("OG Paper spike");
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
        match self.gpu.as_mut() {
            Some(g) => g.resume(window.clone()),
            #[cfg(not(target_arch = "wasm32"))]
            None => {
                let mut g = pollster::block_on(Gpu::new(window.clone(), self.opts.bench))
                    .expect("GPU init");
                g.upload_new(&self.scene, 0);
                self.gpu = Some(g);
            }
            #[cfg(target_arch = "wasm32")]
            None => {
                let slot = self.pending_gpu.clone();
                let (w, bench) = (window.clone(), self.opts.bench);
                wasm_bindgen_futures::spawn_local(async move {
                    match Gpu::new(w.clone(), bench).await {
                        Ok(g) => *slot.borrow_mut() = Some(g),
                        Err(e) => set_hud(&e),
                    }
                    w.request_redraw();
                });
            }
        }
        if self.opts.bench {
            self.auto = Auto::In;
            // Start zoomed out on both pages.
            self.go_home();
        }
        if let Some(k) = self.opts.start_chain {
            // Chain cell k filling ~80% of the screen.
            let c = self.chain[k.min(self.chain.len() - 1)].clone();
            self.cam = Camera::new(c.clone(), [0.5, 0.5], 1000.0);
        }
        if let Some(l) = self.opts.start_mass {
            let (page, local) = mass_spot();
            let mut cam = Camera::new(CellAddr::new(l, 0, 0), [0.0, 0.0], 700.0);
            let o = page.origin_in(&cam.cell);
            let sd = page.side_in(&cam.cell);
            cam.off = [o[0] + local[0] * sd, o[1] + local[1] * sd];
            cam.normalize();
            self.cam = cam;
        }
        window.request_redraw();
    }

    fn suspended(&mut self, _el: &ActiveEventLoop) {
        if let Some(g) = self.gpu.as_mut() {
            g.suspend();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gpu.as_mut() {
                    g.resize(s.width, s.height);
                }
                self.dirty = true;
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if self.opts.bench && self.auto == Auto::Off && self.bench.started_out {
                    self.finish_bench();
                    el.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Character(c) if c.eq_ignore_ascii_case("a") => {
                        self.auto = if self.auto == Auto::Off {
                            Auto::In
                        } else {
                            Auto::Off
                        };
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("h") => self.go_home(),
                    Key::Character(c) if c.eq_ignore_ascii_case("d") => {
                        set_draw_mode(!draw_mode());
                        self.fps_window.0 -= Duration::from_secs(1); // refresh HUD now
                        self.dirty = true;
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                let at = self.centred(self.cursor);
                self.cam.zoom_at(1.2f64.powf(dy), at);
                self.dirty = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = [position.x, position.y];
                if self.panning {
                    self.cam
                        .pan_px(p[0] - self.cursor[0], p[1] - self.cursor[1]);
                    self.dirty = true;
                }
                if self.drawing {
                    let far = self
                        .wet
                        .last()
                        .map(|l| (l[0] - p[0]).hypot(l[1] - p[1]) >= 2.0)
                        .unwrap_or(true);
                    if far {
                        self.wet.push(p);
                        self.dirty = true;
                    }
                }
                self.cursor = p;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Right | MouseButton::Middle => self.panning = down,
                    MouseButton::Left => {
                        self.drawing = down;
                        if down {
                            self.wet = vec![self.cursor];
                        } else {
                            self.commit_wet();
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::Touch(t) => {
                self.handle_touch(t.id, t.phase, [t.location.x, t.location.y]);
            }
            _ => {}
        }
        if self.dirty {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        // Render on change: only animate continuously while auto-zooming.
        if self.auto != Auto::Off || self.dirty {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(el: EventLoop<()>, opts: Options) {
    el.set_control_flow(ControlFlow::Wait);
    let mut app = Spike::new(opts);
    el.run_app(&mut app).expect("event loop");
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub fn run_desktop(opts: Options) {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn"),
    )
    .init();
    let el = EventLoop::new().expect("event loop");
    run(el, opts);
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("og-spike"),
    );
    let el = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("event loop");
    run(el, Options::default());
}

#[cfg(target_arch = "wasm32")]
fn set_hud(text: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("hud"))
    {
        el.set_text_content(Some(text));
    }
}

/// Browser entry point. URL options: `?mass=N&depth=N&chain=K`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn web_start() {
    use winit::platform::web::EventLoopExtWebSys;
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|q| web_sys::UrlSearchParams::new_with_str(&q).ok());
    let get = |k: &str| {
        params
            .as_ref()
            .and_then(|p| p.get(k))
            .and_then(|v| v.parse::<u64>().ok())
    };
    let opts = Options {
        // Browser default is lighter: generation is single-threaded here.
        mass: get("mass").unwrap_or(250_000) as usize,
        depth: get("depth").unwrap_or(40) as usize,
        start_chain: get("chain").map(|v| v as usize),
        ..Options::default()
    };
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    el.spawn_app(Spike::new(opts));
}

/// Peak resident memory (Linux), for the bench report.
fn peak_rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM"))
                .map(|l| l.split_whitespace().nth(1).unwrap_or("0").to_string())
        })
        .and_then(|kb| kb.parse::<f64>().ok())
        .map(|kb| format!("{:.2} GB", kb / 1024.0 / 1024.0))
        .unwrap_or_else(|| "n/a".into())
}
