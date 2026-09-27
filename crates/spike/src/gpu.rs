// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 OmegaGiven and contributors

//! wgpu renderer for the spike: all stroke points live on the GPU once;
//! each frame uploads only the small per-instance draw list.

use std::sync::Arc;

use ogpaper_core::{gen::MAX_PTS, DotInst, DrawList, Scene, StrokeInst, TileInst};
use wgpu::util::DeviceExt;
use winit::window::Window;

const MAX_SEG: u32 = (MAX_PTS - 1) as u32;
/// Room reserved for strokes drawn live, before buffers must grow.
const HEADROOM_STROKES: usize = 65_536;
const HEADROOM_POINTS: usize = 1 << 20;
/// The last stroke slot / last MAX_PTS points hold the wet (in-progress) stroke.
const WET_PTS: usize = 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
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

pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    no_vsync: bool,
    globals: wgpu::Buffer,
    strokes: wgpu::Buffer,
    points: wgpu::Buffer,
    stroke_cap: usize,
    point_cap: usize,
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

impl Gpu {
    pub async fn new(window: Arc<Window>, scene: &Scene, no_vsync: bool) -> Self {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("adapter");
        let info = adapter.get_info();
        log::info!(
            "GPU: {} ({:?}, {:?})",
            info.name,
            info.backend,
            info.device_type
        );
        let al = adapter.limits();
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
            max_buffer_size: al.max_buffer_size,
            ..wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("og-spike"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .expect("device");

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
                storage_entry(1),
                storage_entry(2),
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
        let tiles_cap = 1 << 12;
        let tiles = instance_buffer(
            &device,
            "tiles",
            tiles_cap * std::mem::size_of::<TileInst>(),
        );

        let (strokes, points, stroke_cap, point_cap) = upload_scene(&device, scene);
        let bind = make_bind(&device, &bgl, &globals, &strokes, &points);
        let inst_cap = 1 << 16;
        let dots_cap = 1 << 16;
        let inst = instance_buffer(
            &device,
            "inst",
            inst_cap * std::mem::size_of::<StrokeInst>(),
        );
        let dots = instance_buffer(&device, "dots", dots_cap * std::mem::size_of::<DotInst>());
        log::info!(
            "GPU buffers: {:.0} MB points, {:.1} MB strokes",
            (point_cap * 8) as f64 / 1e6,
            (stroke_cap * 16) as f64 / 1e6
        );

        Self {
            instance,
            adapter,
            device,
            queue,
            surface: Some(surface),
            config,
            no_vsync,
            globals,
            strokes,
            points,
            stroke_cap,
            point_cap,
            stroke_len: scene.strokes.len(),
            point_len: scene.points.len(),
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
        }
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
            let _ = &self.adapter;
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
        let need_s = scene.strokes.len() + 1;
        let need_p = scene.points.len() + WET_PTS;
        if need_s > self.stroke_cap || need_p > self.point_cap {
            let (s, p, sc, pc) = upload_scene(&self.device, scene);
            self.strokes = s;
            self.points = p;
            self.stroke_cap = sc;
            self.point_cap = pc;
            self.bind = make_bind(
                &self.device,
                &self.bgl,
                &self.globals,
                &self.strokes,
                &self.points,
            );
        } else {
            let gs: Vec<StrokeGpu> = scene.strokes[self.stroke_len..]
                .iter()
                .map(to_gpu)
                .collect();
            self.queue.write_buffer(
                &self.strokes,
                (self.stroke_len * 16) as u64,
                bytemuck::cast_slice(&gs),
            );
            self.queue.write_buffer(
                &self.points,
                (self.point_len * 8) as u64,
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
        let _ = self.no_vsync;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&Globals {
                viewport: [self.config.width as f32, self.config.height as f32],
                _pad: [0.0; 2],
            }),
        );

        // Instances: scene strokes, plus the wet stroke in its reserved slot.
        let wet_slot = (self.stroke_cap - 1) as u32;
        let mut insts: Vec<StrokeInst> =
            Vec::with_capacity(draw.strokes.len() + wet.len() / MAX_SEG as usize + 1);
        insts.extend_from_slice(&draw.strokes);
        if wet.len() >= 2 {
            // Wet points are in screen pixels: identity transform. Longer wet strokes
            // are drawn as successive MAX_PTS windows over the reserved point region.
            let n = wet.len().min(WET_PTS);
            let base = self.point_cap - WET_PTS;
            self.queue.write_buffer(
                &self.points,
                (base * 8) as u64,
                bytemuck::cast_slice(&wet[..n]),
            );
            let windows = (n - 1).div_ceil(MAX_SEG as usize);
            let mut slots = Vec::with_capacity(windows);
            for k in 0..windows {
                let start = base + k * MAX_SEG as usize;
                let len = (n - k * MAX_SEG as usize).min(MAX_PTS);
                slots.push(StrokeGpu {
                    start: start as u32,
                    len: len as u32,
                    width: 3.0,
                    color: 0xFF5A28C8,
                });
            }
            // Wet windows use the top stroke slots, counting down.
            let first_slot = wet_slot as usize + 1 - windows;
            self.queue.write_buffer(
                &self.strokes,
                (first_slot * 16) as u64,
                bytemuck::cast_slice(&slots),
            );
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

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
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
fn upload_scene(
    device: &wgpu::Device,
    scene: &Scene,
) -> (wgpu::Buffer, wgpu::Buffer, usize, usize) {
    let stroke_cap = scene.strokes.len() + HEADROOM_STROKES;
    let point_cap = scene.points.len() + HEADROOM_POINTS + WET_PTS;
    let mut gs: Vec<StrokeGpu> = scene.strokes.iter().map(to_gpu).collect();
    gs.resize(
        stroke_cap,
        StrokeGpu {
            start: 0,
            len: 0,
            width: 0.0,
            color: 0,
        },
    );
    let mut pts = scene.points.clone();
    pts.resize(point_cap, [0.0; 2]);
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
    let strokes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("strokes"),
        contents: bytemuck::cast_slice(&gs),
        usage,
    });
    let points = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("points"),
        contents: bytemuck::cast_slice(&pts),
        usage,
    });
    (strokes, points, stroke_cap, point_cap)
}

fn make_bind(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    strokes: &wgpu::Buffer,
    points: &wgpu::Buffer,
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
                resource: strokes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: points.as_entire_binding(),
            },
        ],
    })
}
