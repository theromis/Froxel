//! Chunk meshes drawn with the same wgpu device as egui.
//!
//! The color target is a texture egui samples in the viewport widget. The
//! draw itself runs in the callback `prepare` step, before egui's pass, so
//! the widget shows this frame's image. Depth lives on that target, which
//! keeps it off egui's UI pass.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;
use glam::{Mat4, Vec3};

use crate::mesh::{self, Mesh};
use crate::state::Session;

pub type SharedGpu = Arc<Mutex<SceneGpu>>;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    sun: [f32; 4],
    /// x time in seconds, y 1 when motion preview is on.
    anim: [f32; 4],
}

struct GpuMesh {
    occupancy: u64,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

pub struct SceneGpu {
    size: [u32; 2],
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    highlight_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    chunks: HashMap<(usize, i32, i32, i32), GpuMesh>,
    /// Each object's strand boxes, keyed by object index.
    strands: HashMap<usize, GpuMesh>,
    grid: Option<GpuMesh>,
    grid_key: Option<(i32, i32, i32, i32, i32, i32)>,
    hover: Option<GpuMesh>,
    hover_key: Option<(crate::state::Cell, crate::state::Cell, [u32; 16])>,
    selection: Option<GpuMesh>,
    selection_key: Option<(u64, u64, [u32; 16])>,
    bones: Option<GpuMesh>,
    bones_key: u64,
    /// The Strand tool's point handles, and the strand being placed.
    handles: Option<GpuMesh>,
    placing: Option<GpuMesh>,
    strand_tool_key: u64,
    epoch: u64,
}

impl SceneGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voxels"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voxels-uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxels-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voxels-uniforms"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voxels"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let less = wgpu::CompareFunction::Less;
        let pipeline = make_pipeline(device, &pipeline_layout, &shader, 0, 0.0, less);
        let highlight_pipeline = make_pipeline(device, &pipeline_layout, &shader, -2, -1.0, less);
        // The Strand tool's handles show through what is in front of them:
        // a strand's points lie inside its cubes.
        let overlay_pipeline = make_pipeline(
            device,
            &pipeline_layout,
            &shader,
            0,
            0.0,
            wgpu::CompareFunction::Always,
        );
        let (color, color_view, depth, depth_view) = targets(device, 1, 1);
        Self {
            size: [1, 1],
            color,
            color_view,
            depth,
            depth_view,
            pipeline,
            highlight_pipeline,
            overlay_pipeline,
            bind_group,
            uniforms,
            chunks: HashMap::new(),
            strands: HashMap::new(),
            handles: None,
            placing: None,
            strand_tool_key: 0,
            grid: None,
            grid_key: None,
            hover: None,
            hover_key: None,
            selection: None,
            selection_key: None,
            bones: None,
            bones_key: 0,
            epoch: 0,
        }
    }

    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.color
    }

    /// Returns true when the texture egui is sampling was replaced.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        let width = width.max(1);
        let height = height.max(1);
        if self.size == [width, height] {
            return false;
        }
        let (color, color_view, depth, depth_view) = targets(device, width, height);
        self.color = color;
        self.color_view = color_view;
        self.depth = depth;
        self.depth_view = depth_view;
        self.size = [width, height];
        true
    }

    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        session: &Session,
        view_proj: Mat4,
        time: f32,
    ) {
        if session.mesh_epoch != self.epoch {
            self.sync_chunks(device, session);
            self.sync_strands(device, session);
            self.sync_grid(device, session);
            self.epoch = session.mesh_epoch;
        }
        let sun = session.sun_dir();
        let uniforms = Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            sun: [sun.x, sun.y, sun.z, session.sun_intensity],
            anim: [
                time,
                if session.motion_preview { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        self.sync_hover(device, session);
        self.sync_selection(device, session);
        self.sync_bones(device, session);
        self.sync_strand_tool(device, session);
    }

    fn sync_strand_tool(&mut self, device: &wgpu::Device, session: &Session) {
        let handles = session.strand_handles();
        let placing = session.placing_strand();
        let mut key: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bits: u64| key = (key ^ bits).wrapping_mul(0x0100_0000_01b3);
        for (at, selected, new) in &handles {
            for v in at.to_array() {
                mix(v.to_bits() as u64);
            }
            mix(u64::from(*selected) | u64::from(*new) << 1);
        }
        if let Some(strand) = &placing {
            for b in strand.boxes() {
                for v in b.center.iter().chain(b.half.iter()) {
                    mix(v.to_bits() as u64);
                }
                mix(u64::from(b.slot));
            }
        }
        mix(session.mesh_epoch);
        if key == self.strand_tool_key {
            return;
        }
        self.strand_tool_key = key;
        self.handles = if handles.is_empty() {
            None
        } else {
            let mut built = Mesh::default();
            let mut previous: Option<(Vec3, bool)> = None;
            for (at, selected, new) in &handles {
                let color = if *selected {
                    [1.0, 0.84, 0.35]
                } else if *new {
                    [0.35, 0.85, 1.0]
                } else {
                    [0.92, 0.95, 1.0]
                };
                if let Some((from, was_new)) = previous {
                    if was_new == *new {
                        push_link(&mut built, from, *at, 0.05, [0.7, 0.75, 0.8]);
                    }
                }
                push_cube(&mut built, *at, 0.2, color);
                previous = Some((*at, *new));
            }
            upload(device, &built, key)
        };
        self.placing = placing.and_then(|strand| {
            let index = session.selected;
            let object = &session.objects[index];
            let mut built = mesh::strand_mesh(
                &strand.boxes(),
                mesh::motion_base(&object.cells),
                |slot| session.color(slot),
                |_| [0.0, 0.0],
            );
            mesh::translate_mesh(&mut built, object.translation);
            if let Some(delta) = session.skin_delta(index) {
                mesh::transform_mesh(&mut built, delta);
            }
            upload(device, &built, key)
        });
    }

    fn sync_chunks(&mut self, device: &wgpu::Device, session: &Session) {
        let mut live = std::collections::HashSet::new();
        for (index, object) in session.objects.iter().enumerate() {
            if object.hidden {
                continue;
            }
            let delta = session.skin_delta(index);
            let bevel = session.view_bevel();
            // One pass puts each cell in its chunk; the hashes and the
            // remesh below read only those lists.
            let mut buckets: HashMap<(i32, i32, i32), Vec<(crate::state::Cell, u8)>> =
                HashMap::new();
            for (&cell, &slot) in &object.cells {
                buckets
                    .entry(mesh::chunk_key(cell))
                    .or_default()
                    .push((cell, slot));
            }
            let base = mesh::motion_base(&object.cells);
            let next: HashMap<(i32, i32, i32), u64> = buckets
                .iter()
                .map(|(key, listed)| {
                    let colored: Vec<(crate::state::Cell, [u8; 3])> = listed
                        .iter()
                        .map(|(cell, slot)| (*cell, session.color(*slot)))
                        .collect();
                    let mut hash = mesh::occupancy_hash(&colored);
                    let mut moves = false;
                    for (cell, slot) in listed {
                        let uv = session.motion_uv(*slot);
                        if uv == [0.0, 0.0] {
                            continue;
                        }
                        moves = true;
                        hash = hash.wrapping_add(
                            (cell.0 as u64).wrapping_mul(0x9E37_79B1)
                                ^ (uv[0].to_bits() as u64)
                                ^ (uv[1].to_bits() as u64).wrapping_mul(0xC2B2_AE3D),
                        );
                    }
                    // Motion heights are measured from the object's lowest
                    // cell, so a moving chunk changes when that does.
                    if moves {
                        hash = hash
                            .wrapping_mul(0x9E37_79B1)
                            .wrapping_add(base as u32 as u64);
                    }
                    if let Some(bevel) = bevel {
                        // A bevelled chunk reads its neighbours' cells too.
                        hash = hash
                            .wrapping_mul(0x9E37_79B1)
                            .wrapping_add(
                                u64::from(bevel.size.to_bits()) ^ (bevel.mode as u64) << 40,
                            )
                            .wrapping_add(mesh::occupancy_hash(&neighbourhood(
                                &object.cells,
                                *key,
                            )));
                    }
                    // The mesh is placed at the object's translation, so a
                    // move rebuilds it.
                    for value in [
                        object.translation.0,
                        object.translation.1,
                        object.translation.2,
                    ] {
                        hash = hash
                            .wrapping_mul(0x9E37_79B1)
                            .wrapping_add(value as u32 as u64);
                    }
                    // The chunk mesh is baked in world space, so the object
                    // transform belongs in the hash. Without it a position or
                    // rotation edit left the old chunk on the GPU until
                    // something else dirtied it.
                    for matrix in [session.object_to_world(index)].into_iter().chain(delta) {
                        for column in matrix.to_cols_array_2d() {
                            for value in column {
                                hash = hash
                                    .wrapping_mul(0x9E37_79B1)
                                    .wrapping_add(value.to_bits() as u64);
                            }
                        }
                    }
                    (*key, hash)
                })
                .collect();
            let previous: HashMap<(i32, i32, i32), u64> = self
                .chunks
                .iter()
                .filter_map(|((object_index, x, y, z), chunk)| {
                    (*object_index == index).then_some(((*x, *y, *z), chunk.occupancy))
                })
                .collect();
            for key in mesh::dirty_chunks(&previous, &next) {
                let full = (index, key.0, key.1, key.2);
                match next.get(&key).copied() {
                    Some(hash) => {
                        let listed = buckets.get(&key).map(Vec::as_slice).unwrap_or(&[]);
                        let mut built = match bevel {
                            Some(bevel) => mesh::bevel_listed(
                                &object.cells,
                                listed,
                                bevel,
                                base,
                                |slot| session.color(slot),
                                |slot| session.motion_uv(slot),
                            ),
                            None => mesh::mesh_listed(
                                &object.cells,
                                listed,
                                base,
                                |slot| session.color(slot),
                                |slot| session.motion_uv(slot),
                            ),
                        };
                        mesh::translate_mesh(&mut built, object.translation);
                        if let Some(delta) = delta {
                            for vert in &mut built.verts {
                                let point = delta.transform_point3(Vec3::from_array(vert.pos));
                                vert.pos = point.to_array();
                                let normal = delta.transform_vector3(Vec3::from_array(vert.normal));
                                vert.normal = normal.normalize_or_zero().to_array();
                            }
                        }
                        if built.is_empty() {
                            self.chunks.remove(&full);
                        } else if let Some(gpu) = upload(device, &built, hash) {
                            self.chunks.insert(full, gpu);
                        }
                    }
                    None => {
                        self.chunks.remove(&full);
                    }
                }
            }
            for key in next.keys() {
                live.insert((index, key.0, key.1, key.2));
            }
        }
        self.chunks.retain(|key, _| live.contains(key));
    }

    fn sync_strands(&mut self, device: &wgpu::Device, session: &Session) {
        let mut live = std::collections::HashSet::new();
        for (index, object) in session.objects.iter().enumerate() {
            if object.hidden || object.strands.is_empty() {
                continue;
            }
            let delta = session.skin_delta(index);
            let base = mesh::motion_base(&object.cells);
            let boxes = session.strand_boxes(index);
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            let mut mix = |bits: u64| {
                hash = (hash ^ bits).wrapping_mul(0x0100_0000_01b3);
            };
            for b in &boxes {
                for v in b
                    .center
                    .iter()
                    .chain(b.half.iter())
                    .chain(b.axes.iter().flatten())
                {
                    mix(v.to_bits() as u64);
                }
                let rgb = session.color(b.slot);
                let uv = session.motion_uv(b.slot);
                mix(u64::from(b.slot)
                    | (rgb[0] as u64) << 8
                    | (rgb[1] as u64) << 16
                    | (rgb[2] as u64) << 24);
                mix(uv[0].to_bits() as u64 | (uv[1].to_bits() as u64) << 32);
            }
            mix(base as u32 as u64);
            if let Some(bevel) = session.view_bevel() {
                mix(u64::from(bevel.size.to_bits()) | 1 << 40);
            }
            for v in [
                object.translation.0,
                object.translation.1,
                object.translation.2,
            ] {
                mix(v as u32 as u64);
            }
            if let Some(delta) = delta {
                for value in delta.to_cols_array() {
                    mix(value.to_bits() as u64);
                }
            }
            live.insert(index);
            if self
                .strands
                .get(&index)
                .is_some_and(|m| m.occupancy == hash)
            {
                continue;
            }
            let mut built = match session.view_bevel() {
                Some(bevel) => mesh::bevel_strands(
                    &boxes,
                    bevel.size,
                    base,
                    |slot| session.color(slot),
                    |slot| session.motion_uv(slot),
                ),
                None => mesh::strand_mesh(
                    &boxes,
                    base,
                    |slot| session.color(slot),
                    |slot| session.motion_uv(slot),
                ),
            };
            mesh::translate_mesh(&mut built, object.translation);
            if let Some(delta) = delta {
                mesh::transform_mesh(&mut built, delta);
            }
            match upload(device, &built, hash) {
                Some(gpu) => {
                    self.strands.insert(index, gpu);
                }
                None => {
                    self.strands.remove(&index);
                }
            }
        }
        self.strands.retain(|key, _| live.contains(key));
    }

    fn sync_grid(&mut self, device: &wgpu::Device, session: &Session) {
        let Some((min, max)) = session.world_bounds() else {
            self.grid = None;
            self.grid_key = None;
            return;
        };
        let key = (min.0, min.1, min.2, max.0, max.1, max.2);
        if self.grid_key == Some(key) {
            return;
        }
        let built = mesh::ground_grid(min, max);
        self.grid = upload(device, &built, 0);
        self.grid_key = Some(key);
    }

    fn sync_hover(&mut self, device: &wgpu::Device, session: &Session) {
        let map = session.object_to_world(session.selected);
        let key = session
            .hover
            .as_ref()
            .map(|hit| (hit.cell, hit.normal, mesh::map_key(map)));
        if key == self.hover_key {
            return;
        }
        self.hover_key = key;
        let Some((cell, normal, _)) = key else {
            self.hover = None;
            return;
        };
        let Some(mut built) = mesh::highlight(cell, normal) else {
            self.hover = None;
            return;
        };
        mesh::transform_mesh(&mut built, map);
        self.hover = upload(device, &built, 0);
    }

    fn sync_selection(&mut self, device: &wgpu::Device, session: &Session) {
        let map = session.object_to_world(session.selected);
        let key = (
            session.selection_epoch,
            session.mesh_epoch,
            mesh::map_key(map),
        );
        if self.selection_key == Some(key) {
            return;
        }
        self.selection_key = Some(key);
        let mut built = mesh::selection_shell(&session.active().cells, &session.picked);
        mesh::transform_mesh(&mut built, map);
        self.selection = upload(device, &built, 0);
    }

    fn sync_bones(&mut self, device: &wgpu::Device, session: &Session) {
        let key = bone_key(session);
        if key == self.bones_key {
            return;
        }
        self.bones_key = key;
        self.bones = if key == 0 {
            None
        } else {
            upload(device, &bone_mesh(session), key)
        };
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("voxels"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        for chunk in self.chunks.values() {
            draw(&mut pass, chunk);
        }
        for strands in self.strands.values() {
            draw(&mut pass, strands);
        }
        if let Some(placing) = &self.placing {
            draw(&mut pass, placing);
        }
        if let Some(grid) = &self.grid {
            draw(&mut pass, grid);
        }
        if let Some(selection) = &self.selection {
            pass.set_pipeline(&self.highlight_pipeline);
            draw(&mut pass, selection);
        }
        if let Some(hover) = &self.hover {
            pass.set_pipeline(&self.highlight_pipeline);
            draw(&mut pass, hover);
        }
        if let Some(bones) = &self.bones {
            pass.set_pipeline(&self.highlight_pipeline);
            draw(&mut pass, bones);
        }
        if let Some(handles) = &self.handles {
            pass.set_pipeline(&self.overlay_pipeline);
            draw(&mut pass, handles);
        }
    }
}

/// The cells one cell around a chunk outside it, whose changes reshape its
/// bevel.
fn neighbourhood(
    cells: &HashMap<crate::state::Cell, u8>,
    key: (i32, i32, i32),
) -> Vec<(crate::state::Cell, [u8; 3])> {
    let lo = (
        key.0 * mesh::CHUNK - 1,
        key.1 * mesh::CHUNK - 1,
        key.2 * mesh::CHUNK - 1,
    );
    let hi = (
        lo.0 + mesh::CHUNK + 1,
        lo.1 + mesh::CHUNK + 1,
        lo.2 + mesh::CHUNK + 1,
    );
    let inside = |v: i32, a: i32, b: i32| v >= a && v <= b;
    cells
        .iter()
        .filter(|(cell, _)| {
            inside(cell.0, lo.0, hi.0)
                && inside(cell.1, lo.1, hi.1)
                && inside(cell.2, lo.2, hi.2)
                && mesh::chunk_key(**cell) != key
        })
        .map(|(cell, slot)| (*cell, [*slot, 0, 0]))
        .collect()
}

fn bone_key(session: &Session) -> u64 {
    if session.skeleton.bones.is_empty() {
        return 0;
    }
    let Ok(joints) = crate::rig::posed_joints(&session.skeleton, &session.pose) else {
        return 1;
    };
    let mut hash = joints.len() as u64;
    for (index, (point, _)) in joints.iter().enumerate() {
        hash = hash
            .wrapping_mul(0x9E37_79B1)
            .wrapping_add(point.x.to_bits() as u64);
        hash = hash
            .wrapping_mul(0x9E37_79B1)
            .wrapping_add(point.y.to_bits() as u64);
        hash = hash
            .wrapping_mul(0x9E37_79B1)
            .wrapping_add(point.z.to_bits() as u64);
        let parent = session.skeleton.parent_index(index).unwrap_or(usize::MAX) as u64;
        hash = hash.wrapping_add(parent);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

fn bone_mesh(session: &Session) -> Mesh {
    let mut mesh = Mesh::default();
    let Ok(joints) = crate::rig::posed_joints(&session.skeleton, &session.pose) else {
        return mesh;
    };
    let color = [1.0, 0.75, 0.2];
    for (index, (point, _)) in joints.iter().enumerate() {
        push_cube(&mut mesh, *point, 0.1, color);
        if let Some(parent) = session.skeleton.parent_index(index) {
            push_link(&mut mesh, joints[parent].0, *point, 0.04, color);
        }
    }
    mesh
}

fn push_cube(mesh: &mut Mesh, center: Vec3, half: f32, color: [f32; 3]) {
    let min = center - Vec3::splat(half);
    let max = center + Vec3::splat(half);
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    let faces = [
        ([0, 1, 2, 3], Vec3::new(0.0, 0.0, -1.0)),
        ([4, 7, 6, 5], Vec3::new(0.0, 0.0, 1.0)),
        ([0, 4, 5, 1], Vec3::new(0.0, -1.0, 0.0)),
        ([3, 2, 6, 7], Vec3::new(0.0, 1.0, 0.0)),
        ([0, 3, 7, 4], Vec3::new(-1.0, 0.0, 0.0)),
        ([1, 5, 6, 2], Vec3::new(1.0, 0.0, 0.0)),
    ];
    for (quad, normal) in faces {
        push_quad(
            mesh,
            [
                corners[quad[0]],
                corners[quad[1]],
                corners[quad[2]],
                corners[quad[3]],
            ],
            normal,
            color,
        );
    }
}

fn push_link(mesh: &mut Mesh, from: Vec3, to: Vec3, radius: f32, color: [f32; 3]) {
    let dir = to - from;
    if dir.length_squared() < 1.0e-8 {
        return;
    }
    let dir = dir.normalize();
    let helper = if dir.z.abs() < 0.9 { Vec3::Z } else { Vec3::Y };
    let side = dir.cross(helper).normalize();
    let up = side.cross(dir).normalize();
    let s = side * radius;
    let u = up * radius;
    let corners = [
        from + s + u,
        from - s + u,
        from - s - u,
        from + s - u,
        to + s + u,
        to - s + u,
        to - s - u,
        to + s - u,
    ];
    let faces = [
        ([0, 4, 5, 1], up),
        ([3, 2, 6, 7], -up),
        ([0, 3, 7, 4], side),
        ([1, 5, 6, 2], -side),
    ];
    for (quad, normal) in faces {
        push_quad(
            mesh,
            [
                corners[quad[0]],
                corners[quad[1]],
                corners[quad[2]],
                corners[quad[3]],
            ],
            normal,
            color,
        );
    }
}

fn push_quad(mesh: &mut Mesh, corners: [Vec3; 4], normal: Vec3, color: [f32; 3]) {
    let start = mesh.verts.len() as u32;
    let normal = normal.normalize_or_zero().to_array();
    for corner in corners {
        mesh.verts.push(crate::mesh::Vert {
            pos: corner.to_array(),
            normal,
            color,
            motion: [0.0; 4],
        });
    }
    mesh.indices
        .extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
}

fn draw<'a>(pass: &mut wgpu::RenderPass<'a>, mesh: &'a GpuMesh) {
    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
}

fn upload(device: &wgpu::Device, mesh: &Mesh, occupancy: u64) -> Option<GpuMesh> {
    if mesh.is_empty() {
        return None;
    }
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("voxel-vertices"),
        contents: bytemuck::cast_slice(&mesh.verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("voxel-indices"),
        contents: bytemuck::cast_slice(&mesh.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Some(GpuMesh {
        occupancy,
        vertices,
        indices,
        index_count: mesh.indices.len() as u32,
    })
}

fn targets(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Texture,
    wgpu::TextureView,
) {
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("voxel-color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("voxel-depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
    (color, color_view, depth, depth_view)
}

/// egui samples user textures as gamma-encoded `Rgba8Unorm`. The sRGB render
/// targets list that format as a view format, so egui reads the encoded bytes
/// instead of decoding them to linear.
pub fn egui_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    depth_bias: i32,
    depth_bias_slope_scale: f32,
    depth_compare: wgpu::CompareFunction,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxels"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<mesh::Vert>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 12,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 24,
                        shader_location: 2,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 36,
                        shader_location: 3,
                    },
                ],
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: depth_bias,
                slope_scale: depth_bias_slope_scale,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
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
    })
}

/// sRGB 22, 24, 28, written as linear because the target encodes sRGB.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.0078,
    g: 0.0091,
    b: 0.0120,
    a: 1.0,
};

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
    anim: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct In {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) motion: vec4<f32>,
}
struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(i: In) -> Out {
    var o: Out;
    // Sway bends along x and grows with height above the object base.
    // Bob moves the vertex up and down. anim.y is 0 unless preview is on.
    let sway = sin(u.anim.x * 1.8) * i.motion.x * i.motion.z * 0.05 * u.anim.y;
    let bob = sin(u.anim.x * 2.4) * i.motion.y * 0.15 * u.anim.y;
    let pos = i.pos + vec3<f32>(sway, bob, 0.0);
    o.clip = u.view_proj * vec4<f32>(pos, 1.0);
    o.normal = i.normal;
    o.color = i.color;
    return o;
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let ndl = max(dot(n, u.sun.xyz), 0.0);
    let key = clamp(0.45 + u.sun.w * 0.35, 0.45, 1.5);
    let shade = clamp(0.30 + key * ndl + 0.08 * n.z, 0.12, 1.35);
    return vec4<f32>(to_linear(i.color) * shade, 1.0);
}
"#;

struct ViewportCallback;

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = callback_resources.get::<SharedGpu>() {
            let gpu = gpu.clone();
            gpu.lock().expect("voxel viewport").encode(encoder);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::epaint::PaintCallbackInfo,
        _render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &CallbackResources,
    ) {
    }
}

pub fn paint_callback(rect: egui::Rect) -> egui::epaint::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(rect, ViewportCallback)
}
