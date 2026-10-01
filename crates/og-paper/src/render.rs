// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! GPU renderer: canvas strokes (from data textures) plus the egui UI on top.
//!
//! Stroke records and points live in data textures rather than storage
//! buffers, so the same code runs on WebGL2 / GLES as well as WebGPU, Vulkan,
//! Metal and DX12.

use std::sync::Arc;

use ogpaper_core::{Brush, Dash, DrawList, Scene, TileInst};
use winit::window::Window;

/// Segments per instance; must match MAX_SEG in the shader.
const MAX_SEG: usize = 15;
/// Room for strokes added while drawing, before textures are rebuilt.
const HEADROOM_STROKES: usize = 16_384;
const HEADROOM_POINTS: usize = 1 << 19;
/// Points reserved at the end of the point texture for the wet stroke.
pub const WET_PTS: usize = 8192;
const TEX_W_MAX: u32 = 8192;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    tex_w: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StrokeRec {
    start: u32,
    len_brush: u32,
    width: f32,
    color: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct InstGpu {
    stroke: u32,
    first: u32,
    ox: f32,
    oy: f32,
    scale: f32,
    _pad: u32,
}

/// The stroke being drawn: screen-pixel points with pressure.
pub struct Wet<'a> {
    pub pts: &'a [[f32; 3]],
    pub width_px: f32,
    pub color: u32,
    pub brush: Brush,
    pub dash: Dash,
}

/// egui output for this frame.
pub struct UiPaint {
    pub prims: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

/// Top byte of `len_brush`: brush in bits 0-3, dash pattern in bits 4-5.
fn rec(start: u32, len: u32, brush: Brush, dash: Dash, width: f32, color: u32) -> StrokeRec {
    let kind = brush as u32 | (dash as u32) << 4;
    StrokeRec {
        start,
        len_brush: len.min(0xFF_FFFF) | (kind << 24),
        width,
        color,
    }
}

fn as_bytes<T>(v: &[T]) -> &[u8] {
    // Safety: only used for #[repr(C)] plain-old-data instance structs.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

/// A 1-D array stored row-major in a `w`-wide 2-D texture.
struct DataTex {
    w: u32,
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    cap: usize,
    texel: usize,
}

impl DataTex {
    fn new(
        device: &wgpu::Device,
        label: &str,
        format: wgpu::TextureFormat,
        texel: usize,
        min_cap: usize,
    ) -> Self {
        let max = device.limits().max_texture_dimension_2d;
        let w = max.min(TEX_W_MAX);
        let mut rows = min_cap.div_ceil(w as usize).max(1);
        if rows > max as usize {
            log::error!("{label}: {min_cap} elements exceed one {w}x{max} texture");
            rows = max as usize;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: rows as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            w,
            tex,
            view,
            cap: rows * w as usize,
            texel,
        }
    }

    /// Write `data` (whole texels) starting at element `start`.
    fn write(&self, queue: &wgpu::Queue, start: usize, data: &[u8]) {
        let t = self.texel;
        let n = data.len() / t;
        let w = self.w as usize;
        let mut i = 0;
        while i < n {
            let idx = start + i;
            let (x, y) = (idx % w, idx / w);
            let (width, rows) = if x == 0 && n - i >= w {
                (w, (n - i) / w)
            } else {
                ((w - x).min(n - i), 1)
            };
            let count = width * rows;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: x as u32,
                        y: y as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data[i * t..(i + count) * t],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some((width * t) as u32),
                    rows_per_image: Some(rows as u32),
                },
                wgpu::Extent3d {
                    width: width as u32,
                    height: rows as u32,
                    depth_or_array_layers: 1,
                },
            );
            i += count;
        }
    }
}

pub struct Renderer {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    globals: wgpu::Buffer,
    strokes: DataTex,
    points: DataTex,
    stroke_len: usize,
    point_len: usize,
    bgl: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    ink_pipe: wgpu::RenderPipeline,
    hl_pipe: wgpu::RenderPipeline,
    tile_pipe: wgpu::RenderPipeline,
    ink: Growable,
    hl: Growable,
    tiles: Growable,
    egui: egui_wgpu::Renderer,
}

/// A vertex buffer that grows to fit.
struct Growable {
    label: &'static str,
    buf: wgpu::Buffer,
    cap: usize,
}

impl Growable {
    fn new(device: &wgpu::Device, label: &'static str, cap: usize) -> Self {
        Self {
            label,
            buf: vertex_buffer(device, label, cap),
            cap,
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        if bytes.len() > self.cap {
            self.cap = bytes.len().next_power_of_two();
            self.buf = vertex_buffer(device, self.label, self.cap);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buf, 0, bytes);
        }
    }
}

fn vertex_buffer(device: &wgpu::Device, label: &str, bytes: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn adapter_opts<'a, 'b>(
    surface: Option<&'a wgpu::Surface<'b>>,
) -> wgpu::RequestAdapterOptions<'a, 'b> {
    wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }
}

/// Pick a backend. On the web, WebGPU only if it really yields an adapter,
/// else WebGL2 (a canvas can only ever hold one context type).
async fn pick(
    window: Arc<Window>,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let base = wgpu::InstanceDescriptor::new_without_display_handle;
        let webgpu = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..base()
        });
        if webgpu.request_adapter(&adapter_opts(None)).await.is_ok() {
            let s = webgpu
                .create_surface(window)
                .map_err(|e| format!("WebGPU surface: {e}"))?;
            if let Ok(a) = webgpu.request_adapter(&adapter_opts(Some(&s))).await {
                return Ok((webgpu, s, a));
            }
            return Err("WebGPU adapter vanished; reload the page.".into());
        }
        let gl = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..base()
        });
        let s = gl
            .create_surface(window)
            .map_err(|e| format!("WebGL2 surface: {e}"))?;
        let a = gl
            .request_adapter(&adapter_opts(Some(&s)))
            .await
            .map_err(|_| "This browser offers neither WebGPU nor WebGL2.".to_string())?;
        Ok((gl, s, a))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let s = instance
            .create_surface(window)
            .map_err(|e| format!("surface: {e}"))?;
        let a = instance
            .request_adapter(&adapter_opts(Some(&s)))
            .await
            .map_err(|e| format!("no GPU adapter: {e}"))?;
        Ok((instance, s, a))
    }
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let (instance, surface, adapter) = pick(window.clone()).await?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);
        let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("og-paper"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        // Colors are sRGB bytes end to end (UI and ink alike), so use a surface
        // that stores them as-is rather than one that re-encodes to sRGB.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Fill polygons read their outline in the fragment stage too.
        let tex_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1, wgpu::TextureSampleType::Uint),
                tex_entry(2, wgpu::TextureSampleType::Float { filterable: false }),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canvas"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let over = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        let min = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Min,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Max,
            },
        };
        let stroke_attrs =
            wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Float32x2, 3 => Float32];
        let tile_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Uint32x4, 2 => Uint32x4];
        let pipe = |vs: &str,
                    fs: &str,
                    blend: wgpu::BlendState,
                    stride: usize,
                    attrs: &[wgpu::VertexAttribute]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let inst = std::mem::size_of::<InstGpu>();
        let ink_pipe = pipe("vs_stroke", "fs_stroke", over, inst, &stroke_attrs);
        let hl_pipe = pipe("vs_stroke", "fs_highlight", min, inst, &stroke_attrs);
        let tile_pipe = pipe(
            "vs_tile",
            "fs_tile",
            over,
            std::mem::size_of::<TileInst>(),
            &tile_attrs,
        );

        let (strokes, points) = upload_scene(&device, &queue, &Scene::new());
        let bind = make_bind(&device, &bgl, &globals, &strokes, &points);
        let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            instance,
            ink: Growable::new(&device, "ink", 1 << 16),
            hl: Growable::new(&device, "highlight", 1 << 14),
            tiles: Growable::new(&device, "tiles", 1 << 14),
            device,
            queue,
            surface: Some(surface),
            config,
            globals,
            strokes,
            points,
            stroke_len: 0,
            point_len: 0,
            bgl,
            bind,
            ink_pipe,
            hl_pipe,
            tile_pipe,
            egui,
        })
    }

    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    pub fn resume(&mut self, window: Arc<Window>) {
        if self.surface.is_none() {
            if let Ok(s) = self.instance.create_surface(window.clone()) {
                let size = window.inner_size();
                self.config.width = size.width.max(1);
                self.config.height = size.height.max(1);
                s.configure(&self.device, &self.config);
                self.surface = Some(s);
            }
        }
    }

    pub fn suspend(&mut self) {
        self.surface = None;
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }

    /// Replace everything on the GPU with `scene` (new/open).
    pub fn reset(&mut self, scene: &Scene) {
        self.stroke_len = 0;
        self.point_len = 0;
        self.rebuild(scene);
    }

    fn rebuild(&mut self, scene: &Scene) {
        let (s, p) = upload_scene(&self.device, &self.queue, scene);
        self.strokes = s;
        self.points = p;
        self.bind = make_bind(
            &self.device,
            &self.bgl,
            &self.globals,
            &self.strokes,
            &self.points,
        );
        self.stroke_len = scene.strokes.len();
        self.point_len = scene.points.len();
    }

    /// Re-upload the points of strokes whose points were changed in place
    /// (the live preview while moving, resizing or rotating a selection).
    pub fn update_points(&mut self, scene: &Scene, ids: &[u32]) {
        self.sync(scene);
        for &id in ids {
            let s = &scene.strokes[id as usize];
            let (a, b) = (s.start as usize, (s.start + s.len) as usize);
            if b <= self.point_len {
                self.points
                    .write(&self.queue, a, bytemuck::cast_slice(&scene.points[a..b]));
            }
        }
    }

    /// Upload strokes added to the scene since the last sync.
    pub fn sync(&mut self, scene: &Scene) {
        if scene.strokes.len() == self.stroke_len {
            return;
        }
        if scene.strokes.len() + 1 > self.strokes.cap
            || scene.points.len() + WET_PTS > self.points.cap
        {
            self.rebuild(scene);
            return;
        }
        let recs: Vec<StrokeRec> = scene.strokes[self.stroke_len..]
            .iter()
            .map(to_rec)
            .collect();
        self.strokes
            .write(&self.queue, self.stroke_len, bytemuck::cast_slice(&recs));
        self.points.write(
            &self.queue,
            self.point_len,
            bytemuck::cast_slice(&scene.points[self.point_len..]),
        );
        self.stroke_len = scene.strokes.len();
        self.point_len = scene.points.len();
    }

    /// Draw a frame. Returns false if no frame could be acquired (the caller
    /// should try again on the next redraw).
    pub fn render(
        &mut self,
        scene: &Scene,
        draw: &DrawList,
        wet: Option<Wet<'_>>,
        ui: UiPaint,
    ) -> bool {
        let Some(surface) = &self.surface else {
            return false;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, &self.config);
                return false;
            }
            other => {
                log::debug!("no frame: {:?}", std::mem::discriminant(&other));
                return false;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                viewport: [self.config.width as f32, self.config.height as f32],
                tex_w: self.points.w,
                _pad: 0,
            }),
        );

        // One instance per window of MAX_SEG segments; highlighter separately.
        let mut ink = Vec::with_capacity(draw.strokes.len());
        let mut hl = Vec::new();
        for i in &draw.strokes {
            let s = &scene.strokes[i.stroke as usize];
            let list = if s.brush == Brush::Highlighter {
                &mut hl
            } else {
                &mut ink
            };
            let len = if s.brush == Brush::Fill {
                1 // one instance covers the whole polygon
            } else {
                s.len as usize
            };
            push_windows(list, i.stroke, len, i.ox, i.oy, i.scale);
        }
        if let Some(w) = wet.filter(|w| !w.pts.is_empty()) {
            let n = w.pts.len().min(WET_PTS);
            let mut along = 0.0f32;
            let src = &w.pts[w.pts.len() - n..];
            let pts: Vec<[f32; 4]> = src
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if i > 0 {
                        along += (p[0] - src[i - 1][0]).hypot(p[1] - src[i - 1][1]);
                    }
                    [p[0], p[1], p[2], along]
                })
                .collect();
            let base = self.points.cap - WET_PTS;
            self.points
                .write(&self.queue, base, bytemuck::cast_slice(&pts));
            let slot = self.strokes.cap - 1;
            let r = rec(base as u32, n as u32, w.brush, w.dash, w.width_px, w.color);
            self.strokes
                .write(&self.queue, slot, bytemuck::bytes_of(&r));
            let list = if w.brush == Brush::Highlighter {
                &mut hl
            } else {
                &mut ink
            };
            push_windows(list, slot as u32, n, 0.0, 0.0, 1.0);
        }
        self.ink
            .upload(&self.device, &self.queue, bytemuck::cast_slice(&ink));
        self.hl
            .upload(&self.device, &self.queue, bytemuck::cast_slice(&hl));
        self.tiles
            .upload(&self.device, &self.queue, as_bytes(&draw.tiles));

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ui.pixels_per_point,
        };
        for (id, delta) in &ui.textures.set {
            for d in delta.iter() {
                self.egui.update_texture(&self.device, &self.queue, *id, d);
            }
        }
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let extra =
            self.egui
                .update_buffers(&self.device, &self.queue, &mut enc, &ui.prims, &screen);
        {
            let mut pass = enc
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("main"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.96,
                                g: 0.95,
                                b: 0.92,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            pass.set_bind_group(0, &self.bind, &[]);
            if !draw.tiles.is_empty() {
                pass.set_pipeline(&self.tile_pipe);
                pass.set_vertex_buffer(0, self.tiles.buf.slice(..));
                pass.draw(0..6, 0..draw.tiles.len() as u32);
            }
            if !ink.is_empty() {
                pass.set_pipeline(&self.ink_pipe);
                pass.set_vertex_buffer(0, self.ink.buf.slice(..));
                pass.draw(0..(MAX_SEG * 6) as u32, 0..ink.len() as u32);
            }
            if !hl.is_empty() {
                pass.set_pipeline(&self.hl_pipe);
                pass.set_vertex_buffer(0, self.hl.buf.slice(..));
                pass.draw(0..(MAX_SEG * 6) as u32, 0..hl.len() as u32);
            }
            self.egui.render(&mut pass, &ui.prims, &screen);
        }
        self.queue.submit(extra.into_iter().chain([enc.finish()]));
        self.queue.present(frame);
        for id in &ui.textures.free {
            self.egui.free_texture(id);
        }
        true
    }
}

fn push_windows(list: &mut Vec<InstGpu>, stroke: u32, len: usize, ox: f32, oy: f32, scale: f32) {
    let windows = len.saturating_sub(1).div_ceil(MAX_SEG).max(1);
    for k in 0..windows {
        list.push(InstGpu {
            stroke,
            first: (k * MAX_SEG) as u32,
            ox,
            oy,
            scale,
            _pad: 0,
        });
    }
}

fn to_rec(s: &ogpaper_core::Stroke) -> StrokeRec {
    rec(s.start, s.len, s.brush, s.dash, s.width, s.color)
}

fn upload_scene(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene) -> (DataTex, DataTex) {
    let strokes = DataTex::new(
        device,
        "strokes",
        wgpu::TextureFormat::Rgba32Uint,
        16,
        scene.strokes.len() + HEADROOM_STROKES + 1,
    );
    let points = DataTex::new(
        device,
        "points",
        wgpu::TextureFormat::Rgba32Float,
        16,
        scene.points.len() + HEADROOM_POINTS + WET_PTS,
    );
    let recs: Vec<StrokeRec> = scene.strokes.iter().map(to_rec).collect();
    strokes.write(queue, 0, bytemuck::cast_slice(&recs));
    points.write(queue, 0, bytemuck::cast_slice(&scene.points));
    (strokes, points)
}

fn make_bind(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    strokes: &DataTex,
    points: &DataTex,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("canvas"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&strokes.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&points.view),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    /// The canvas shader parses and validates (wgpu would only find out at
    /// pipeline creation, on the user's machine).
    #[test]
    fn shader_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(include_str!("shader.wgsl"))
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(include_str!("shader.wgsl"))));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
