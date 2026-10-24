//! Voxel grid, ray march, and the GPU scene pack.

use super::*;
use crate::state::Cell;
use glam::Mat4;

pub(super) fn build_world(session: &Session) -> World {
    let mut voxels = HashMap::new();
    for (index, object) in session.objects.iter().enumerate() {
        if object.hidden {
            continue;
        }
        let delta = session.skin_delta(index);
        if let Some(delta) = delta.filter(|delta| !is_rigid(delta)) {
            scaled_cells(object, delta, &mut voxels);
            continue;
        }
        for (cell, slot) in &object.cells {
            if *slot == 0 {
                continue;
            }
            let world = if let Some(delta) = delta {
                let center = Vec3::new(
                    (cell.0 + object.translation.0) as f32 + 0.5,
                    (cell.1 + object.translation.1) as f32 + 0.5,
                    (cell.2 + object.translation.2) as f32 + 0.5,
                );
                let point = delta.transform_point3(center);
                (
                    point.x.floor() as i32,
                    point.y.floor() as i32,
                    point.z.floor() as i32,
                )
            } else {
                (
                    cell.0 + object.translation.0,
                    cell.1 + object.translation.1,
                    cell.2 + object.translation.2,
                )
            };
            voxels.insert(world, *slot);
        }
    }
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    for cell in voxels.keys() {
        min[0] = min[0].min(cell.0);
        min[1] = min[1].min(cell.1);
        min[2] = min[2].min(cell.2);
        max[0] = max[0].max(cell.0);
        max[1] = max[1].max(cell.1);
        max[2] = max[2].max(cell.2);
    }
    let mut lights = Vec::new();
    let offsets = [
        (Vec3::X, Vec3::new(1.0, 0.5, 0.5)),
        (-Vec3::X, Vec3::new(0.0, 0.5, 0.5)),
        (Vec3::Y, Vec3::new(0.5, 1.0, 0.5)),
        (-Vec3::Y, Vec3::new(0.5, 0.0, 0.5)),
        (Vec3::Z, Vec3::new(0.5, 0.5, 1.0)),
        (-Vec3::Z, Vec3::new(0.5, 0.5, 0.0)),
    ];
    for (cell, slot) in &voxels {
        let material = session.materials[*slot as usize];
        if material.ty != MaterialType::Emissive || material.emission <= 0.0 {
            continue;
        }
        let origin = Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32);
        let radiance = scale(
            linear_rgb(session.palette[*slot as usize]),
            emission_gain(&material),
        );
        for (normal, at) in offsets {
            let neighbor = (
                cell.0 + normal.x as i32,
                cell.1 + normal.y as i32,
                cell.2 + normal.z as i32,
            );
            if voxels.contains_key(&neighbor) {
                continue;
            }
            let power = radiance[0] + radiance[1] + radiance[2];
            if power <= 0.0 {
                continue;
            }
            lights.push(Light {
                point: origin + at,
                normal,
                radiance,
                power,
            });
        }
    }
    let count = voxels.len();
    if voxels.is_empty() {
        min = [0; 3];
        max = [0; 3];
    }
    World {
        grid: pack_grid(&voxels, min, max),
        lights,
        min,
        max,
        count,
    }
}

pub(super) fn pack_grid(
    voxels: &HashMap<(i32, i32, i32), u8>,
    min: [i32; 3],
    max: [i32; 3],
) -> Grid {
    let mut dims = [0usize; 3];
    let mut volume = 1usize;
    for axis in 0..3 {
        let n = (max[axis] as i64 - min[axis] as i64 + 1).max(0) as usize;
        if n > 4096 {
            return Grid::Sparse(voxels.clone());
        }
        volume = volume.saturating_mul(n);
        dims[axis] = n;
    }
    if volume == 0 || volume > 32_000_000 {
        return Grid::Sparse(voxels.clone());
    }
    let mut cells = vec![0u8; volume];
    for (cell, slot) in voxels {
        let x = (cell.0 - min[0]) as usize;
        let y = (cell.1 - min[1]) as usize;
        let z = (cell.2 - min[2]) as usize;
        cells[x + dims[0] * (y + dims[1] * z)] = *slot;
    }
    Grid::Dense {
        origin: min,
        dims,
        cells,
    }
}

impl World {
    fn slot(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let slot = match &self.grid {
            Grid::Dense {
                origin,
                dims,
                cells,
            } => {
                let ix = x - origin[0];
                let iy = y - origin[1];
                let iz = z - origin[2];
                if ix < 0
                    || iy < 0
                    || iz < 0
                    || ix >= dims[0] as i32
                    || iy >= dims[1] as i32
                    || iz >= dims[2] as i32
                {
                    return None;
                }
                cells[ix as usize + dims[0] * (iy as usize + dims[1] * iz as usize)]
            }
            Grid::Sparse(map) => return map.get(&(x, y, z)).copied(),
        };
        if slot == 0 {
            None
        } else {
            Some(slot)
        }
    }
}

/// One grid the GPU can index. `None` when the scene is too spread out to pack.
pub struct GpuScene {
    pub origin: [i32; 4],
    pub dims: [u32; 4],
    pub cells: Vec<u32>,
    pub lights: Vec<GpuLight>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuLight {
    pub point: [f32; 4],
    pub normal: [f32; 4],
    /// xyz radiance, w power.
    pub radiance: [f32; 4],
}

pub fn gpu_scene(session: &Session) -> Option<GpuScene> {
    let world = build_world(session);
    let Grid::Dense {
        origin,
        dims,
        cells,
    } = &world.grid
    else {
        return None;
    };
    let lights = world
        .lights
        .iter()
        .take(4096)
        .map(|light| GpuLight {
            point: [light.point.x, light.point.y, light.point.z, 1.0],
            normal: [light.normal.x, light.normal.y, light.normal.z, 0.0],
            radiance: [
                light.radiance[0],
                light.radiance[1],
                light.radiance[2],
                light.power,
            ],
        })
        .collect();
    Some(GpuScene {
        origin: [origin[0], origin[1], origin[2], 0],
        dims: [dims[0] as u32, dims[1] as u32, dims[2] as u32, 0],
        cells: cells.iter().map(|slot| u32::from(*slot)).collect(),
        lights,
    })
}

pub(super) fn march(world: &World, origin: Vec3, dir: Vec3) -> Option<Hit> {
    if world.count == 0 || dir.length_squared() < 1.0e-8 {
        return None;
    }
    let Some(entry) = ray_box_entry(origin, dir, world.min, world.max) else {
        return None;
    };
    let origin = origin + dir * entry;
    let mut cell = [
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    ];
    let step = [
        if dir.x >= 0.0 { 1 } else { -1 },
        if dir.y >= 0.0 { 1 } else { -1 },
        if dir.z >= 0.0 { 1 } else { -1 },
    ];
    let delta = [
        if dir.x.abs() < 1.0e-8 {
            f32::MAX
        } else {
            (1.0 / dir.x).abs()
        },
        if dir.y.abs() < 1.0e-8 {
            f32::MAX
        } else {
            (1.0 / dir.y).abs()
        },
        if dir.z.abs() < 1.0e-8 {
            f32::MAX
        } else {
            (1.0 / dir.z).abs()
        },
    ];
    let mut tmax = [0.0; 3];
    for axis in 0..3 {
        let start = if step[axis] > 0 {
            (cell[axis] + 1) as f32
        } else {
            cell[axis] as f32
        };
        let coord = [origin.x, origin.y, origin.z][axis];
        let component = [dir.x, dir.y, dir.z][axis];
        tmax[axis] = if component.abs() < 1.0e-8 {
            f32::MAX
        } else {
            (start - coord) / component
        };
    }
    let mut normal = -dir.normalize_or_zero();
    if let Some(slot) = world.slot(cell[0], cell[1], cell[2]) {
        return Some(hit_at(cell, normal, slot));
    }
    for _ in 0..MAX_STEPS {
        let axis = if tmax[0] <= tmax[1] && tmax[0] <= tmax[2] {
            0
        } else if tmax[1] <= tmax[2] {
            1
        } else {
            2
        };
        cell[axis] += step[axis];
        tmax[axis] += delta[axis];
        normal = Vec3::ZERO;
        match axis {
            0 => normal.x = -step[0] as f32,
            1 => normal.y = -step[1] as f32,
            _ => normal.z = -step[2] as f32,
        }
        if cell[axis] < world.min[axis] - 1 || cell[axis] > world.max[axis] + 1 {
            return None;
        }
        if let Some(slot) = world.slot(cell[0], cell[1], cell[2]) {
            return Some(hit_at(cell, normal, slot));
        }
    }
    None
}

pub(super) fn hit_at(cell: [i32; 3], normal: Vec3, slot: u8) -> Hit {
    let point =
        Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32) + Vec3::splat(0.5) + normal * 0.5;
    Hit {
        point,
        normal,
        slot,
    }
}

pub(super) fn occluded(world: &World, origin: Vec3, dir: Vec3, max_dist: f32) -> bool {
    let Some(hit) = march(world, origin + dir * 0.002, dir) else {
        return false;
    };
    (hit.point - origin).dot(dir) < max_dist
}

pub(super) fn ray_box_entry(origin: Vec3, dir: Vec3, min: [i32; 3], max: [i32; 3]) -> Option<f32> {
    let mut t0 = 0.0_f32;
    let mut t1 = f32::MAX;
    let lo = [min[0] as f32, min[1] as f32, min[2] as f32];
    let hi = [
        max[0] as f32 + 1.0,
        max[1] as f32 + 1.0,
        max[2] as f32 + 1.0,
    ];
    let o = [origin.x, origin.y, origin.z];
    let d = [dir.x, dir.y, dir.z];
    for axis in 0..3 {
        if d[axis].abs() < 1.0e-8 {
            if o[axis] < lo[axis] || o[axis] > hi[axis] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[axis];
        let mut a = (lo[axis] - o[axis]) * inv;
        let mut b = (hi[axis] - o[axis]) * inv;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    if t1 < 0.0 {
        None
    } else {
        Some(t0.max(0.0))
    }
}

/// Whether a pose map keeps lengths: no bone or parent in the pose scales.
pub(super) fn is_rigid(delta: &Mat4) -> bool {
    [delta.x_axis, delta.y_axis, delta.z_axis]
        .iter()
        .all(|axis| (axis.truncate().length() - 1.0).abs() < 1e-4)
}

/// Cells this many or more in the posed box are not resampled.
const MAX_SCALED_CELLS: i64 = 4_000_000;

/// A scaled part, resampled cell by cell from the posed box back into the
/// bind pose so a grown part has no holes and a shrunk one keeps its shape.
/// A part flattened to nothing on an axis is left out.
fn scaled_cells(object: &crate::state::Volume, delta: Mat4, voxels: &mut HashMap<Cell, u8>) {
    if delta.determinant().abs() < 1e-6 || object.cells.is_empty() {
        return;
    }
    let shift = Vec3::new(
        object.translation.0 as f32,
        object.translation.1 as f32,
        object.translation.2 as f32,
    );
    let mut low = Vec3::splat(f32::MAX);
    let mut high = Vec3::splat(f32::MIN);
    for cell in object.cells.keys() {
        let base = Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32) + shift;
        for corner in 0..8 {
            let offset = Vec3::new(
                (corner & 1) as f32,
                ((corner >> 1) & 1) as f32,
                ((corner >> 2) & 1) as f32,
            );
            let point = delta.transform_point3(base + offset);
            low = low.min(point);
            high = high.max(point);
        }
    }
    let low = low.floor().as_ivec3();
    let high = high.ceil().as_ivec3();
    let size = (high - low).as_i64vec3();
    if size.x * size.y * size.z > MAX_SCALED_CELLS {
        return;
    }
    let inverse = delta.inverse();
    for x in low.x..high.x {
        for y in low.y..high.y {
            for z in low.z..high.z {
                let center = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let source = inverse.transform_point3(center) - shift;
                let cell = (
                    source.x.floor() as i32,
                    source.y.floor() as i32,
                    source.z.floor() as i32,
                );
                if let Some(&slot) = object.cells.get(&cell) {
                    if slot != 0 {
                        voxels.insert((x, y, z), slot);
                    }
                }
            }
        }
    }
}
