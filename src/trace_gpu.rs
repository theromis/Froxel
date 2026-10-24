//! GPU path tracer for Render mode.
//!
//! The viewport already has a wgpu device, so this compute shader runs on
//! Metal, Vulkan, or DX12. One dispatch
//! accumulates a few samples into a buffer. A small pass then tonemaps into
//! the texture egui shows. The CPU tracer remains the fallback for a scene
//! that does not fit in one grid, and it is what the tests exercise.

use std::sync::mpsc;

use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::state::{GlassMode, MaterialType, Session};
use crate::trace::{self, GpuLight};

const SAMPLES_PER_FRAME: u32 = 4;
const PIXEL_STRIDE: u64 = 64;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    eye: [f32; 4],
    target: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    origin: [i32; 4],
    dims: [u32; 4],
    /// width, height, first sample, max bounces
    misc0: [u32; 4],
    /// flags, nee candidates, light count, samples in this dispatch
    misc1: [u32; 4],
    /// aperture, focus, sky intensity, horizon softness
    cam: [f32; 4],
    /// sun disc cosine, indirect clamp, ema floor, unused
    tone: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMat {
    ty: u32,
    emission_power: u32,
    roughness: f32,
    metallic: f32,
    emission: f32,
    transparency: f32,
    ior: f32,
    pad: f32,
}

pub struct Tracer {
    trace_pipeline: wgpu::ComputePipeline,
    display_pipeline: wgpu::ComputePipeline,
    blit_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    materials: wgpu::Buffer,
    palette: wgpu::Buffer,
    voxels: wgpu::Buffer,
    lights: wgpu::Buffer,
    pixels: wgpu::Buffer,
    linear: wgpu::Texture,
    linear_view: wgpu::TextureView,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    blit_bind: wgpu::BindGroup,
    size: [u32; 2],
    samples: u32,
    origin: [i32; 4],
    dims: [u32; 4],
    light_count: u32,
    scene_key: Option<u64>,
    frame_key: u64,
    packed: bool,
    epoch: u64,
    /// Mixed into the sampler. The viewport leaves this at zero.
    seed: u32,
    oidn_done: bool,
}

pub struct Frame {
    pub gpu: bool,
    pub samples: u32,
    pub size: [u32; 2],
    pub epoch: u64,
    pub repaint: bool,
}

impl Tracer {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path-trace"),
            source: wgpu::ShaderSource::Wgsl(TRACE.into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path-trace-blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT.into()),
        });
        let layout = bind_layout(device);
        let trace_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("path-trace"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("path-trace"),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                }),
            ),
            module: &shader,
            entry_point: Some("trace_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let display_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("path-trace-display"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("path-trace-display"),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                }),
            ),
            module: &shader,
            entry_point: Some("display_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path-trace-blit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("path-trace-blit"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("path-trace-blit"),
                    bind_group_layouts: &[Some(&blit_layout)],
                    immediate_size: 0,
                }),
            ),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("path-trace-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let materials = zeros(
            device,
            "path-trace-materials",
            256 * std::mem::size_of::<GpuMat>(),
        );
        let palette = zeros(device, "path-trace-palette", 256 * 16);
        let voxels = zeros(device, "path-trace-voxels", 4);
        let lights = zeros(device, "path-trace-lights", std::mem::size_of::<GpuLight>());
        let pixels = pixel_buffer(device, PIXEL_STRIDE as usize);
        let (linear, linear_view) = linear_target(device, 1, 1);
        let (color, color_view) = color_target(device, 1, 1);
        let bind = bind_group(
            device,
            &layout,
            &uniforms,
            &materials,
            &palette,
            &voxels,
            &lights,
            &pixels,
            &linear_view,
        );
        let blit_bind = blit_group(device, &blit_layout, &linear_view);
        Self {
            trace_pipeline,
            display_pipeline,
            blit_pipeline,
            layout,
            blit_layout,
            uniforms,
            materials,
            palette,
            voxels,
            lights,
            pixels,
            linear,
            linear_view,
            color,
            color_view,
            bind,
            blit_bind,
            size: [1, 1],
            samples: 0,
            origin: [0; 4],
            dims: [1, 1, 1, 0],
            light_count: 0,
            scene_key: None,
            frame_key: 0,
            packed: false,
            epoch: 1,
            seed: 0,
            oidn_done: false,
        }
    }

    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.color
    }

    /// Trace the next batch. `Frame.gpu` is false when the scene does not fit
    /// a single grid and the caller should use the CPU tracer.
    pub fn advance(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        session: &Session,
        size: [u32; 2],
    ) -> Frame {
        let scene_key = trace::scene_key(session);
        if self.scene_key != Some(scene_key) {
            let Some(scene) = trace::gpu_scene(session) else {
                self.scene_key = Some(scene_key);
                self.packed = false;
                return self.frame(false);
            };
            self.upload_scene(device, queue, session, &scene);
            self.scene_key = Some(scene_key);
            self.packed = true;
            self.frame_key = 0;
        }
        if !self.packed {
            return self.frame(false);
        }
        let key = trace::frame_key(session, size);
        let size_changed = self.size != size;
        if size_changed {
            self.resize(device, size);
        }
        if size_changed || self.frame_key != key {
            self.samples = 0;
            self.oidn_done = false;
            self.frame_key = key;
        }
        let cap = session.max_samples.clamp(1, 8192) as u32;
        let batch = cap.saturating_sub(self.samples).min(SAMPLES_PER_FRAME);
        if batch > 0 {
            self.dispatch(device, queue, session, batch);
            self.samples += batch;
        }
        let mut frame = self.frame(true);
        frame.repaint = self.samples < cap;
        frame
    }

    pub fn save_png(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &std::path::Path,
        session: &crate::state::Session,
    ) -> Result<(), String> {
        let mut rgba = self.read_color(device, queue)?;
        if session.show_bones {
            crate::trace::draw_bone_overlay(&mut rgba, self.size[0], self.size[1], session);
        }
        crate::trace::write_rgba_png(path, self.size[0], self.size[1], &rgba)
    }

    fn read_color(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<u8>, String> {
        let width = self.size[0];
        let height = self.size[1];
        let padded = padded_row(width);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("path-trace-readback"),
            size: (padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("path-trace-readback"),
        });
        encoder.copy_texture_to_buffer(
            self.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| format!("Could not read the render: {err}"))?;
        rx.recv()
            .map_err(|_| "Render readback was dropped.".to_string())?
            .map_err(|err| format!("Could not read the render: {err}"))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|err| format!("Could not read the render: {err}"))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded as usize;
            rgba.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(rgba)
    }

    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed;
    }

    /// Denoise the finished accumulation into the display texture.
    /// Without the `oidn` feature this leaves the shader's edge filter in place.
    pub fn finish_oidn(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        session: &Session,
    ) -> Result<(), String> {
        if !crate::denoise::available() || !session.denoise || self.oidn_done || self.samples == 0 {
            return Ok(());
        }
        let cap = session.render_sample_cap();
        if self.samples < cap {
            return Ok(());
        }
        let guides = self.read_pixels(device, queue)?;
        let count = (self.size[0] * self.size[1]) as usize;
        let n = self.samples as f32;
        let mut color = Vec::with_capacity(count);
        let mut albedo = Vec::with_capacity(count);
        let mut normal = Vec::with_capacity(count);
        for pixel in guides.iter().take(count) {
            color.push([pixel.sum[0] / n, pixel.sum[1] / n, pixel.sum[2] / n]);
            albedo.push([pixel.albedo[0], pixel.albedo[1], pixel.albedo[2]]);
            normal.push([pixel.normal[0], pixel.normal[1], pixel.normal[2]]);
        }
        crate::denoise::denoise_buffers(&mut color, &albedo, &normal, self.size[0], self.size[1])?;
        let rgba = crate::trace::tonemap_rgba(&color, session.aces);
        let width = self.size[0];
        let height = self.size[1];
        let row = padded_row(width);
        let mut packed = vec![0u8; (row * height) as usize];
        for y in 0..height as usize {
            let src = y * (width as usize) * 4;
            let dst = y * row as usize;
            packed[dst..dst + (width as usize) * 4]
                .copy_from_slice(&rgba[src..src + (width as usize) * 4]);
        }
        queue.write_texture(
            self.color.as_image_copy(),
            &packed,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.oidn_done = true;
        Ok(())
    }

    pub fn read_guides(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(Vec<[f32; 3]>, Vec<[f32; 3]>), String> {
        let pixels = self.read_pixels(device, queue)?;
        let count = (self.size[0] * self.size[1]) as usize;
        let mut albedo = Vec::with_capacity(count);
        let mut normal = Vec::with_capacity(count);
        for pixel in pixels.iter().take(count) {
            albedo.push([pixel.albedo[0], pixel.albedo[1], pixel.albedo[2]]);
            normal.push([pixel.normal[0], pixel.normal[1], pixel.normal[2]]);
        }
        Ok((albedo, normal))
    }

    fn read_pixels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Vec<GpuPixel>, String> {
        let count = (self.size[0] as u64) * (self.size[1] as u64);
        let bytes = count * PIXEL_STRIDE;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("path-trace-pixel-readback"),
            size: bytes.max(4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("path-trace-pixel-readback"),
        });
        encoder.copy_buffer_to_buffer(&self.pixels, 0, &buffer, 0, bytes);
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| format!("Could not read the render: {err}"))?;
        rx.recv()
            .map_err(|_| "Render readback was dropped.".to_string())?
            .map_err(|err| format!("Could not read the render: {err}"))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|err| format!("Could not read the render: {err}"))?;
        let pixels = bytemuck::cast_slice(&mapped[..bytes as usize]).to_vec();
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }

    fn frame(&self, gpu: bool) -> Frame {
        Frame {
            gpu,
            samples: self.samples,
            size: self.size,
            epoch: self.epoch,
            repaint: false,
        }
    }

    fn upload_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        session: &Session,
        scene: &trace::GpuScene,
    ) {
        let mats: Vec<GpuMat> = session
            .materials
            .iter()
            .map(|material| GpuMat {
                ty: match material.ty {
                    MaterialType::Diffuse => 0,
                    MaterialType::Metal => 1,
                    MaterialType::Emissive => 2,
                    MaterialType::Glass => 3,
                },
                emission_power: material.emission_power.max(1) as u32,
                roughness: material.roughness,
                metallic: material.metallic,
                emission: material.emission,
                transparency: material.transparency,
                ior: material.ior,
                pad: 0.0,
            })
            .collect();
        let palette: Vec<[f32; 4]> = session
            .palette
            .iter()
            .map(|rgba| {
                let rgb = trace::linear_rgb(*rgba);
                [rgb[0], rgb[1], rgb[2], 1.0]
            })
            .collect();
        queue.write_buffer(&self.materials, 0, bytemuck::cast_slice(&mats));
        queue.write_buffer(&self.palette, 0, bytemuck::cast_slice(&palette));
        let cells = if scene.cells.is_empty() {
            vec![0u32]
        } else {
            scene.cells.clone()
        };
        self.voxels = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("path-trace-voxels"),
            contents: bytemuck::cast_slice(&cells),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        let lights = if scene.lights.is_empty() {
            vec![GpuLight {
                point: [0.0; 4],
                normal: [0.0, 0.0, 1.0, 0.0],
                radiance: [0.0; 4],
            }]
        } else {
            scene.lights.clone()
        };
        self.lights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("path-trace-lights"),
            contents: bytemuck::cast_slice(&lights),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        self.origin = scene.origin;
        self.dims = scene.dims;
        self.light_count = scene.lights.len() as u32;
        self.rebind(device);
    }

    fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let pixels = (size[0] as u64) * (size[1] as u64) * PIXEL_STRIDE;
        self.pixels = pixel_buffer(device, pixels.max(PIXEL_STRIDE) as usize);
        let (linear, linear_view) = linear_target(device, size[0], size[1]);
        let (color, color_view) = color_target(device, size[0], size[1]);
        self.linear = linear;
        self.linear_view = linear_view;
        self.color = color;
        self.color_view = color_view;
        self.size = size;
        self.epoch = self.epoch.wrapping_add(1);
        self.rebind(device);
    }

    fn rebind(&mut self, device: &wgpu::Device) {
        self.bind = bind_group(
            device,
            &self.layout,
            &self.uniforms,
            &self.materials,
            &self.palette,
            &self.voxels,
            &self.lights,
            &self.pixels,
            &self.linear_view,
        );
        self.blit_bind = blit_group(device, &self.blit_layout, &self.linear_view);
    }

    fn dispatch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        session: &Session,
        batch: u32,
    ) {
        let uniforms = uniforms_for(session, self, batch);
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("path-trace"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("path-trace"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.trace_pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(self.size[0].div_ceil(8), self.size[1].div_ceil(8), 1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("path-trace-display"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.display_pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(self.size[0].div_ceil(8), self.size[1].div_ceil(8), 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("path-trace-blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.blit_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn uniforms_for(session: &Session, tracer: &Tracer, batch: u32) -> Uniforms {
    let eye = session.eye();
    let mut flags = 0u32;
    if session.stochastic_gi {
        flags |= 1;
    }
    if session.aces {
        flags |= 2;
    }
    if session.denoise && !crate::denoise::available() {
        flags |= 4;
    }
    if session.atmospheric_sky {
        flags |= 8;
    }
    if session.power_weighted_nee {
        flags |= 16;
    }
    if session.glass_mode == GlassMode::Absorption {
        flags |= 32;
    }
    let sun = sun_direction(session);
    Uniforms {
        eye: [eye.x, eye.y, eye.z, 0.0],
        target: [session.target.x, session.target.y, session.target.z, 0.0],
        sun_dir: sun,
        sun_color: [
            session.sun_color[0],
            session.sun_color[1],
            session.sun_color[2],
            session.sun_intensity.max(0.0),
        ],
        origin: tracer.origin,
        dims: tracer.dims,
        misc0: [
            tracer.size[0],
            tracer.size[1],
            tracer.samples,
            session.max_bounces.clamp(1, 8) as u32,
        ],
        misc1: [
            flags,
            session.nee_ris.clamp(1, 32) as u32,
            tracer.light_count,
            batch,
        ],
        cam: [
            session.aperture.max(0.0),
            session.focus_distance,
            session.sky_intensity.max(0.0),
            session.horizon_softness.max(0.0),
        ],
        tone: [
            (session.sun_disc.max(0.1).to_radians() * 0.5).cos(),
            session.indirect_clamp,
            session.ema_floor.clamp(0.0, 1.0),
            f32::from_bits(tracer.seed),
        ],
    }
}

fn sun_direction(session: &Session) -> [f32; 4] {
    let e = session.sun_elevation.to_radians();
    let a = session.sun_azimuth.to_radians();
    let dir = glam::Vec3::new(a.cos() * e.cos(), a.sin() * e.cos(), e.sin());
    let dir = if dir.length_squared() < 1.0e-6 {
        glam::Vec3::new(0.3, 0.2, 0.9).normalize()
    } else {
        dir.normalize()
    };
    [dir.x, dir.y, dir.z, 0.0]
}

fn zeros(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pixel_buffer(device: &wgpu::Device, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("path-trace-pixels"),
        size: size.max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn padded_row(width: u32) -> u32 {
    width.saturating_mul(4).div_ceil(256) * 256
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuPixel {
    sum: [f32; 4],
    shown: [f32; 4],
    albedo: [f32; 4],
    normal: [f32; 4],
}

pub struct GpuFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub albedo: Vec<[f32; 3]>,
    pub normal: Vec<[f32; 3]>,
}

/// Path-trace `session` on a headless wgpu device. `session.max_samples` is the cap.
pub fn render_headless(session: &Session, size: [u32; 2], seed: u32) -> Result<GpuFrame, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|err| format!("no GPU adapter: {err}"))?;
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|err| format!("could not open a GPU device: {err}"))?;
    let mut tracer = Tracer::new(&device);
    tracer.set_seed(seed);
    let cap = session.render_sample_cap();
    loop {
        let frame = tracer.advance(&device, &queue, session, size);
        if !frame.gpu {
            return Err("this scene does not fit one grid; omit --gpu".into());
        }
        if frame.samples >= cap {
            break;
        }
    }
    tracer.finish_oidn(&device, &queue, session)?;
    let rgba = tracer.read_color(&device, &queue)?;
    let (albedo, normal) = tracer.read_guides(&device, &queue)?;
    Ok(GpuFrame {
        width: size[0],
        height: size[1],
        rgba,
        albedo,
        normal,
    })
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn raw() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
}

fn linear_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("path-trace-linear"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn color_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("path-trace-color"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn bind_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("path-trace"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            storage(1, true),
            storage(2, true),
            storage(3, true),
            storage(4, true),
            storage(5, false),
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ],
    })
}

fn bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    materials: &wgpu::Buffer,
    palette: &wgpu::Buffer,
    voxels: &wgpu::Buffer,
    lights: &wgpu::Buffer,
    pixels: &wgpu::Buffer,
    linear: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("path-trace"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: materials.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: palette.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: voxels.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: lights.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: pixels.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(linear),
            },
        ],
    })
}

fn blit_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    linear: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("path-trace-blit"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(linear),
        }],
    })
}

const TRACE: &str = include_str!("trace.wgsl");

const BLIT: &str = include_str!("blit.wgsl");

#[cfg(test)]
#[path = "../tests/trace_gpu.rs"]
mod tests;
