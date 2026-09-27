// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! wgpu renderer for the spike: all stroke points live on the GPU once;
//! each frame uploads only the small per-instance draw list.
//!
//! Stroke data lives in data textures (not storage buffers) so the same
//! renderer runs on WebGL2 and old GLES devices as well as WebGPU, Vulkan,
//! Metal and DX12.

use std::sync::Arc;

use ogpaper_core::{gen::MAX_PTS, DotInst, DrawList, Scene, StrokeInst, TileInst};
use winit::window::Window;

const MAX_SEG: u32 = (MAX_PTS - 1) as u32;
/// Room reserved for strokes drawn live, before textures must grow.
const HEADROOM_STROKES: usize = 65_536;
const HEADROOM_POINTS: usize = 1 << 20;
/// The last WET_PTS points (and top stroke slots) hold the wet (in-progress) stroke.
const WET_PTS: usize = 1024;
/// Preferred data texture width in texels; capped by the GPU's 2D limit.
/// Capacity is width x max rows (e.g. 8192 x 16384 = 134M points).
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
struct StrokeGpu {
    start: u32,
    len: u32,
    width: f32,
    color: u32,
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
            log::error!("{label}: {min_cap} elements exceed one {w}x{max} texture; the rest will not render");
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
            // Whole rows at once when aligned, else the rest of this row.
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

pub struct Gpu {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    #[allow(dead_code)]
    pub backend: wgpu::Backend,
    globals: wgpu::Buffer,
    strokes: DataTex,
    points: DataTex,
    /// Strokes/points of the scene already on the GPU.
    stroke_len: usize,
    point_len: usize,
    bgl: wgpu::BindGroupLayout,
    bind: wgpu::BindGroup,
    stroke_pipe: wgpu::RenderPipeline,
    dot_pipe: wgpu::RenderPipeline,
    tile_pipe: wgpu::RenderPipeline,
    tiles: wgpu::Buffer,
    tiles_cap: usize,
    inst: wgpu::Buffer,
    inst_cap: usize,
    dots: wgpu::Buffer,
    dots_cap: usize,
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

/// Pick a backend. On the web, WebGPU is used only if it really yields an
/// adapter (some browsers expose `navigator.gpu` with none, e.g. Brave on
/// Linux); otherwise WebGL2. A canvas can only ever hold one context type, so
/// the check happens before the surface is created.
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
        log::info!("no WebGPU adapter; falling back to WebGL2");
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

impl Gpu {
    /// Create the device and pipelines; call `upload_new` afterwards to load a scene.
    pub async fn new(window: Arc<Window>, no_vsync: bool) -> Result<Self, String> {
        let (instance, surface, adapter) = pick(window.clone()).await?;
        let info = adapter.get_info();
        log::info!(
            "GPU: {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );
        // Lowest common denominator (WebGL2) plus whatever texture size the adapter allows.
        let al = adapter.limits();
        let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(al);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("og-spike"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("device: {e}"))?;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present_mode = if no_vsync && caps.present_modes.contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else if no_vsync {
            wgpu::PresentMode::AutoNoVsync
        } else {
            wgpu::PresentMode::AutoVsync
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
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

        let tex_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
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
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let blend = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let stroke_attrs = wgpu::vertex_attr_array![0 => Uint32, 1 => Float32x2, 2 => Float32];
        let dot_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32];
        let tile_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Uint32x4, 2 => Uint32x4];
        let pipe = |vs: &str, fs: &str, stride: usize, attrs: &[wgpu::VertexAttribute]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
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
                    targets: &target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let stroke_pipe = pipe(
            "vs_stroke",
            "fs_stroke",
            std::mem::size_of::<StrokeInst>(),
            &stroke_attrs,
        );
        let dot_pipe = pipe(
            "vs_dot",
            "fs_dot",
            std::mem::size_of::<DotInst>(),
            &dot_attrs,
        );
        let tile_pipe = pipe(
            "vs_tile",
            "fs_tile",
            std::mem::size_of::<TileInst>(),
            &tile_attrs,
        );

        let empty = Scene::new();
        let (strokes, points) = upload_scene(&device, &queue, &empty);
        let bind = make_bind(&device, &bgl, &globals, &strokes, &points);
        let (inst_cap, dots_cap, tiles_cap) = (1 << 16, 1 << 16, 1 << 12);
        let tiles = instance_buffer(
            &device,
            "tiles",
            tiles_cap * std::mem::size_of::<TileInst>(),
        );
        let inst = instance_buffer(
            &device,
            "inst",
            inst_cap * std::mem::size_of::<StrokeInst>(),
        );
        let dots = instance_buffer(&device, "dots", dots_cap * std::mem::size_of::<DotInst>());
        Ok(Self {
            instance,
            device,
            queue,
            surface: Some(surface),
            config,
            backend: info.backend,
            globals,
            strokes,
            points,
            stroke_len: 0,
            point_len: 0,
            bgl,
            bind,
            stroke_pipe,
            dot_pipe,
            tile_pipe,
            tiles,
            tiles_cap,
            inst,
            inst_cap,
            dots,
            dots_cap,
        })
    }

    pub fn resume(&mut self, window: Arc<Window>) {
        if self.surface.is_none() {
            let s = self
                .instance
                .create_surface(window.clone())
                .expect("surface");
            let size = window.inner_size();
            self.config.width = size.width.max(1);
            self.config.height = size.height.max(1);
            s.configure(&self.device, &self.config);
            self.surface = Some(s);
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

    /// Upload strokes added to the scene since the last upload.
    pub fn upload_new(&mut self, scene: &Scene, _first: usize) {
        let need_s = scene.strokes.len() + WET_PTS;
        let need_p = scene.points.len() + WET_PTS;
        if need_s > self.strokes.cap || need_p > self.points.cap {
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
            log::info!(
                "GPU data: {:.0} MB points, {:.1} MB strokes",
                (self.points.cap * 8) as f64 / 1e6,
                (self.strokes.cap * 16) as f64 / 1e6
            );
        } else {
            let gs: Vec<StrokeGpu> = scene.strokes[self.stroke_len..]
                .iter()
                .map(to_gpu)
                .collect();
            self.strokes
                .write(&self.queue, self.stroke_len, bytemuck::cast_slice(&gs));
            self.points.write(
                &self.queue,
                self.point_len,
                bytemuck::cast_slice(&scene.points[self.point_len..]),
            );
        }
        self.stroke_len = scene.strokes.len();
        self.point_len = scene.points.len();
    }

    pub fn render(&mut self, draw: &DrawList, wet: &[[f32; 2]]) {
        let Some(surface) = &self.surface else { return };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
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

        // Instances: scene strokes, plus the wet stroke in reserved slots.
        let mut insts: Vec<StrokeInst> =
            Vec::with_capacity(draw.strokes.len() + wet.len() / MAX_SEG as usize + 1);
        insts.extend_from_slice(&draw.strokes);
        if wet.len() >= 2 {
            // Wet points are in screen pixels: identity transform. Longer wet strokes
            // are drawn as successive MAX_PTS windows over the reserved point region.
            let n = wet.len().min(WET_PTS);
            let base = self.points.cap - WET_PTS;
            self.points
                .write(&self.queue, base, bytemuck::cast_slice(&wet[..n]));
            let windows = (n - 1).div_ceil(MAX_SEG as usize);
            let slots: Vec<StrokeGpu> = (0..windows)
                .map(|k| StrokeGpu {
                    start: (base + k * MAX_SEG as usize) as u32,
                    len: (n - k * MAX_SEG as usize).min(MAX_PTS) as u32,
                    width: 3.0,
                    color: 0xFF5A28C8,
                })
                .collect();
            // Wet windows use the top stroke slots.
            let first_slot = self.strokes.cap - windows;
            self.strokes
                .write(&self.queue, first_slot, bytemuck::cast_slice(&slots));
            for k in 0..windows {
                insts.push(StrokeInst {
                    stroke: (first_slot + k) as u32,
                    ox: 0.0,
                    oy: 0.0,
                    scale: 1.0,
                });
            }
        }
        if insts.len() > self.inst_cap {
            self.inst_cap = insts.len().next_power_of_two();
            self.inst = instance_buffer(
                &self.device,
                "inst",
                self.inst_cap * std::mem::size_of::<StrokeInst>(),
            );
        }
        if draw.dots.len() > self.dots_cap {
            self.dots_cap = draw.dots.len().next_power_of_two();
            self.dots = instance_buffer(
                &self.device,
                "dots",
                self.dots_cap * std::mem::size_of::<DotInst>(),
            );
        }
        if draw.tiles.len() > self.tiles_cap {
            self.tiles_cap = draw.tiles.len().next_power_of_two();
            self.tiles = instance_buffer(
                &self.device,
                "tiles",
                self.tiles_cap * std::mem::size_of::<TileInst>(),
            );
        }
        if !draw.tiles.is_empty() {
            self.queue
                .write_buffer(&self.tiles, 0, as_bytes(&draw.tiles));
        }
        if !insts.is_empty() {
            self.queue.write_buffer(&self.inst, 0, as_bytes(&insts));
        }
        if !draw.dots.is_empty() {
            self.queue.write_buffer(&self.dots, 0, as_bytes(&draw.dots));
        }

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
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
            });
            pass.set_bind_group(0, &self.bind, &[]);
            if !draw.tiles.is_empty() {
                pass.set_pipeline(&self.tile_pipe);
                pass.set_vertex_buffer(0, self.tiles.slice(..));
                pass.draw(0..6, 0..draw.tiles.len() as u32);
            }
            if !draw.dots.is_empty() {
                pass.set_pipeline(&self.dot_pipe);
                pass.set_vertex_buffer(0, self.dots.slice(..));
                pass.draw(0..6, 0..draw.dots.len() as u32);
            }
            if !insts.is_empty() {
                pass.set_pipeline(&self.stroke_pipe);
                pass.set_vertex_buffer(0, self.inst.slice(..));
                pass.draw(0..MAX_SEG * 6, 0..insts.len() as u32);
            }
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}

fn to_gpu(s: &ogpaper_core::Stroke) -> StrokeGpu {
    StrokeGpu {
        start: s.start,
        len: s.len,
        width: s.width,
        color: s.color,
    }
}

fn instance_buffer(device: &wgpu::Device, label: &str, bytes: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload all strokes and points with headroom for live drawing.
fn upload_scene(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene) -> (DataTex, DataTex) {
    let strokes = DataTex::new(
        device,
        "strokes",
        wgpu::TextureFormat::Rgba32Uint,
        16,
        scene.strokes.len() + HEADROOM_STROKES + WET_PTS,
    );
    let points = DataTex::new(
        device,
        "points",
        wgpu::TextureFormat::Rg32Float,
        8,
        scene.points.len() + HEADROOM_POINTS + WET_PTS,
    );
    let gs: Vec<StrokeGpu> = scene.strokes.iter().map(to_gpu).collect();
    strokes.write(queue, 0, bytemuck::cast_slice(&gs));
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
        label: Some("bind"),
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
