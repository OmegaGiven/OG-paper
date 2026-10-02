// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! GPU renderer: canvas strokes (from data textures) plus the egui UI on top.
//!
//! Stroke records and points live in data textures rather than storage
//! buffers, so the same code runs on WebGL2 / GLES as well as WebGPU, Vulkan,
//! Metal and DX12.

use std::collections::HashMap;
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

/// The background grid for one frame (see `grid.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default, Debug)]
pub struct GridGpu {
    pub origin: [f32; 2],
    pub fine: f32,
    pub target: f32,
    pub mode: u32,
    pub _a: f32,
    pub ppp: f32,
    pub _b: f32,
}

pub struct Renderer {
    /// Background grid to draw under the ink this frame, if any.
    pub grid: Option<GridGpu>,
    grid_pipe: wgpu::RenderPipeline,
    grid_buf: wgpu::Buffer,
    grid_bind: wgpu::BindGroup,
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
    /// Pictures: pipeline, layout, sampler, quads, and a bind group per
    /// picture (`None`: it could not be decoded).
    img_pipe: wgpu::RenderPipeline,
    img_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    imgs: Growable,
    img_tex: HashMap<u64, Option<wgpu::BindGroup>>,
    egui: egui_wgpu::Renderer,
}

/// One picture quad: corners (screen px, clockwise from top-left), then
/// viewport size, opacity and padding.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ImgInst {
    corners: [f32; 8],
    misc: [f32; 4],
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

        let img_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let img_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });
        let img_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image"),
            bind_group_layouts: &[Some(&img_bgl)],
            immediate_size: 0,
        });
        let img_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let img_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image"),
            layout: Some(&img_layout),
            vertex: wgpu::VertexState {
                module: &img_shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImgInst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &img_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &img_shader,
                entry_point: Some("fs_image"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(over),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let (strokes, points) = upload_scene(&device, &queue, &Scene::new());
        let bind = make_bind(&device, &bgl, &globals, &strokes, &points);
        // Background grid: its own tiny pipeline with one uniform.
        let grid_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid"),
            size: std::mem::size_of::<GridGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let grid_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid"),
            layout: &grid_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_buf.as_entire_binding(),
            }],
        });
        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let grid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid"),
            bind_group_layouts: &[Some(&grid_bgl)],
            immediate_size: 0,
        });
        let grid_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&grid_layout),
            vertex: wgpu::VertexState {
                module: &grid_shader,
                entry_point: Some("vs_grid"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &grid_shader,
                entry_point: Some("fs_grid"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            grid: None,
            grid_pipe,
            grid_buf,
            grid_bind,
            instance,
            ink: Growable::new(&device, "ink", 1 << 16),
            hl: Growable::new(&device, "highlight", 1 << 14),
            tiles: Growable::new(&device, "tiles", 1 << 14),
            imgs: Growable::new(&device, "images", 1 << 12),
            img_pipe,
            img_bgl,
            sampler,
            img_tex: HashMap::new(),
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
        self.img_tex.clear();
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
        objs: &crate::objects::Objects,
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
        let (vw, vh) = (self.config.width as f32, self.config.height as f32);
        // Pictures in draw order: (ink instances before it, picture id, quad).
        let mut pics: Vec<(usize, u64, ImgInst)> = Vec::new();
        for i in &draw.strokes {
            let s = &scene.strokes[i.stroke as usize];
            if s.brush == Brush::Fill && s.color == 0 {
                // An invisible outline; a picture's corners if it places one.
                if let Some(&(id, opacity)) = objs.image_of.get(&i.stroke) {
                    let p = scene.stroke_points(i.stroke);
                    if p.len() == 4 {
                        let mut corners = [0.0; 8];
                        for (k, q) in p.iter().enumerate() {
                            corners[2 * k] = i.ox + q[0] * i.scale;
                            corners[2 * k + 1] = i.oy + q[1] * i.scale;
                        }
                        let misc = [vw, vh, opacity as f32 / 255.0, 0.0];
                        pics.push((ink.len(), id, ImgInst { corners, misc }));
                    }
                }
                continue;
            }
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
        for (_, id, _) in &pics {
            if !self.img_tex.contains_key(id) {
                let bind = objs.images.get(id).and_then(|a| self.picture(a));
                self.img_tex.insert(*id, bind);
            }
        }
        let quads: Vec<ImgInst> = pics.iter().map(|p| p.2).collect();
        self.imgs
            .upload(&self.device, &self.queue, bytemuck::cast_slice(&quads));

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
            if let Some(grid) = self.grid {
                self.queue
                    .write_buffer(&self.grid_buf, 0, bytemuck::bytes_of(&grid));
                pass.set_pipeline(&self.grid_pipe);
                pass.set_bind_group(0, &self.grid_bind, &[]);
                pass.draw(0..3, 0..1);
            }
            pass.set_bind_group(0, &self.bind, &[]);
            if !draw.tiles.is_empty() {
                pass.set_pipeline(&self.tile_pipe);
                pass.set_vertex_buffer(0, self.tiles.buf.slice(..));
                pass.draw(0..6, 0..draw.tiles.len() as u32);
            }
            // Ink in runs between pictures, so both keep their draw order.
            let mut from = 0;
            let ink_run = |pass: &mut wgpu::RenderPass<'static>, a: usize, b: usize| {
                if b > a {
                    pass.set_pipeline(&self.ink_pipe);
                    pass.set_bind_group(0, &self.bind, &[]);
                    pass.set_vertex_buffer(0, self.ink.buf.slice(..));
                    pass.draw(0..(MAX_SEG * 6) as u32, a as u32..b as u32);
                }
            };
            for (k, (at, id, _)) in pics.iter().enumerate() {
                ink_run(&mut pass, from, *at);
                from = *at;
                if let Some(Some(bind)) = self.img_tex.get(id) {
                    pass.set_pipeline(&self.img_pipe);
                    pass.set_bind_group(0, bind, &[]);
                    pass.set_vertex_buffer(0, self.imgs.buf.slice(..));
                    pass.draw(0..6, k as u32..k as u32 + 1);
                }
            }
            ink_run(&mut pass, from, ink.len());
            pass.set_bind_group(0, &self.bind, &[]);
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

impl Renderer {
    /// Upload a picture with its mip chain.
    fn picture(&self, a: &crate::images::Asset) -> Option<wgpu::BindGroup> {
        let levels = crate::images::mips(a, self.device.limits().max_texture_dimension_2d)?;
        let (w, h) = levels[0].dimensions();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (m, img) in levels.iter().enumerate() {
            let (lw, lh) = img.dimensions();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: m as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                img.as_raw(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * lw),
                    rows_per_image: Some(lh),
                },
                wgpu::Extent3d {
                    width: lw,
                    height: lh,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("picture"),
            layout: &self.img_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }))
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
        for src in [include_str!("shader.wgsl"), include_str!("image.wgsl")] {
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        }
    }
}
