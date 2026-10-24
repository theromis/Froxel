//! Exposed-face meshes, split into 16³ chunks.
//!
//! A chunk is rebuilt only when its own cells change or a neighbor's
//! occupancy changes (a shared face appears or disappears). Camera motion
//! does not touch this data.

use std::collections::{HashMap, HashSet};

use crate::state::Cell;

pub const CHUNK: i32 = 16;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vert {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    /// x sway amount, y bob amount, z height above the object's base, w unused.
    /// The viewport shader reads this only while motion preview is on.
    pub motion: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Vert>() == 52);

#[derive(Clone, Default)]
pub struct Mesh {
    pub verts: Vec<Vert>,
    pub indices: Vec<u32>,
}

impl Mesh {
    #[cfg(test)]
    fn face_count(&self) -> usize {
        self.indices.len() / 6
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

pub fn translate_mesh(mesh: &mut Mesh, delta: Cell) {
    let shift = [delta.0 as f32, delta.1 as f32, delta.2 as f32];
    for vert in &mut mesh.verts {
        vert.pos[0] += shift[0];
        vert.pos[1] += shift[1];
        vert.pos[2] += shift[2];
    }
}

pub fn chunk_key(cell: Cell) -> (i32, i32, i32) {
    (
        cell.0.div_euclid(CHUNK),
        cell.1.div_euclid(CHUNK),
        cell.2.div_euclid(CHUNK),
    )
}

/// Order-independent fingerprint of one chunk, including the sRGB color
/// baked into each vertex. A palette edit changes the hash of every chunk
/// that uses the edited slot.
pub fn occupancy_hash(cells: &[(Cell, [u8; 3])]) -> u64 {
    let mut sum = cells.len() as u64;
    let mut xor = 0u64;
    for (cell, rgb) in cells {
        let h = mix(cell.0, cell.1, cell.2, *rgb);
        sum = sum.wrapping_add(h);
        xor ^= h;
    }
    sum ^ xor.rotate_left(1)
}

fn mix(x: i32, y: i32, z: i32, rgb: [u8; 3]) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B1_85EB_CA87)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ ((rgb[0] as u64) << 16)
        ^ ((rgb[1] as u64) << 8)
        ^ (rgb[2] as u64);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h
}

/// Faces of `chunk` that are not covered by a neighbor in `cells`.
pub fn mesh_chunk(
    cells: &HashMap<Cell, u8>,
    chunk: (i32, i32, i32),
    color: impl Fn(u8) -> [u8; 3],
    motion: impl Fn(u8) -> [f32; 2],
) -> Mesh {
    let base = cells.keys().map(|cell| cell.1).min().unwrap_or(0) as f32;
    let mut listed = Vec::new();
    for (&cell, &slot) in cells {
        if chunk_key(cell) == chunk {
            listed.push((cell, slot, color(slot), motion(slot)));
        }
    }
    let mut mesh = Mesh::default();
    for (cell, _slot, rgb, motion) in &listed {
        for face in 0..6 {
            let (dx, dy, dz) = OFFSETS[face];
            let neighbor = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
            if cells.contains_key(&neighbor) {
                continue;
            }
            push_quad(
                &mut mesh, *cell, face, *rgb, 0.0, motion[0], motion[1], base,
            );
        }
    }
    mesh
}

/// Boundary of a selection: faces that touch air or an unselected cell.
pub fn selection_shell(cells: &HashMap<Cell, u8>, picked: &HashSet<Cell>) -> Mesh {
    let mut mesh = Mesh::default();
    for cell in picked {
        if !cells.contains_key(cell) {
            continue;
        }
        for face in 0..6 {
            let (dx, dy, dz) = OFFSETS[face];
            let neighbor = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
            if picked.contains(&neighbor) {
                continue;
            }
            push_quad(&mut mesh, *cell, face, [255, 214, 90], 0.03, 0.0, 0.0, 0.0);
        }
    }
    mesh
}

pub fn highlight(cell: Cell, normal: Cell) -> Option<Mesh> {
    let face = OFFSETS.iter().position(|o| *o == normal)?;
    let mut mesh = Mesh::default();
    push_quad(&mut mesh, cell, face, [255, 214, 90], 0.04, 0.0, 0.0, 0.0);
    Some(mesh)
}

pub fn ground_grid(min: Cell, max: Cell) -> Mesh {
    let mut mesh = Mesh::default();
    // Sit just under the bottom faces so the grid does not z-fight them.
    let z = min.2 as f32 - 0.02;
    let x0 = min.0 as f32 - 2.0;
    let x1 = max.0 as f32 + 3.0;
    let y0 = min.1 as f32 - 2.0;
    let y1 = max.1 as f32 + 3.0;
    let color = [46, 50, 58];
    let steps = 12;
    let dx = (x1 - x0) / steps as f32;
    let dy = (y1 - y0) / steps as f32;
    for i in 0..=steps {
        let x = x0 + dx * i as f32;
        push_ribbon(&mut mesh, [x, y0, z], [x, y1, z], true, color);
        let y = y0 + dy * i as f32;
        push_ribbon(&mut mesh, [x0, y, z], [x1, y, z], false, color);
    }
    mesh
}

const OFFSETS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

const NORMALS: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Outward-facing quads, CCW when looked at from outside.
const QUADS: [[[f32; 3]; 4]; 6] = [
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
    [
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
    [
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    ],
];

fn push_quad(
    mesh: &mut Mesh,
    cell: Cell,
    face: usize,
    rgb: [u8; 3],
    push: f32,
    sway: f32,
    bob: f32,
    base_y: f32,
) {
    let base = mesh.verts.len() as u32;
    let color = [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ];
    let normal = NORMALS[face];
    let origin = [cell.0 as f32, cell.1 as f32, cell.2 as f32];
    for corner in QUADS[face] {
        mesh.verts.push(Vert {
            pos: [
                origin[0] + corner[0] + normal[0] * push,
                origin[1] + corner[1] + normal[1] * push,
                origin[2] + corner[2] + normal[2] * push,
            ],
            normal,
            color,
            motion: [sway, bob, origin[1] + corner[1] - base_y, 0.0],
        });
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_ribbon(mesh: &mut Mesh, a: [f32; 3], b: [f32; 3], along_y: bool, rgb: [u8; 3]) {
    let w = 0.025;
    let p = if along_y {
        [
            [a[0] - w, a[1], a[2]],
            [a[0] + w, a[1], a[2]],
            [b[0] + w, b[1], b[2]],
            [b[0] - w, b[1], b[2]],
        ]
    } else {
        [
            [a[0], a[1] - w, a[2]],
            [a[0], a[1] + w, a[2]],
            [b[0], b[1] + w, b[2]],
            [b[0], b[1] - w, b[2]],
        ]
    };
    let base = mesh.verts.len() as u32;
    let color = [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ];
    for corner in p {
        mesh.verts.push(Vert {
            pos: corner,
            normal: [0.0, 0.0, 1.0],
            color,
            motion: [0.0; 4],
        });
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Chunks whose occupancy changed, plus their six neighbors so a face that
/// used to be exposed on the other side of the boundary is rebuilt too.
pub fn dirty_chunks(
    previous: &HashMap<(i32, i32, i32), u64>,
    next: &HashMap<(i32, i32, i32), u64>,
) -> Vec<(i32, i32, i32)> {
    let mut dirty = std::collections::HashSet::new();
    for (key, hash) in next {
        if previous.get(key) != Some(hash) {
            mark(&mut dirty, *key);
        }
    }
    for key in previous.keys() {
        if !next.contains_key(key) {
            mark(&mut dirty, *key);
        }
    }
    dirty.into_iter().collect()
}

fn mark(dirty: &mut std::collections::HashSet<(i32, i32, i32)>, key: (i32, i32, i32)) {
    dirty.insert(key);
    for (dx, dy, dz) in OFFSETS {
        dirty.insert((key.0 + dx, key.1 + dy, key.2 + dz));
    }
}

#[cfg(test)]
#[path = "../tests/mesh.rs"]
mod tests;
