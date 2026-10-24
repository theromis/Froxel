//! Greedy face merge and T-junction splits.

use std::collections::HashMap;

use super::*;

pub(super) const NORMALS: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Outward-facing quads, CCW when looked at from outside. Same winding as
/// the viewport mesh.
pub(super) const QUADS: [[[f32; 3]; 4]; 6] = [
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

pub(super) const OFFSETS: [Cell; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

#[cfg(test)]
pub(super) fn build(session: &Session) -> Built {
    build_with(
        session,
        &MeshExport {
            vertex_colors: false,
            merge: true,
            export_hidden: false,
            split_objects: false,
        },
    )
}

pub(super) fn build_one(session: &Session, options: &MeshExport, only: usize) -> Built {
    build_filtered(session, options, Some(only))
}

pub(super) fn build_with(session: &Session, options: &MeshExport) -> Built {
    build_filtered(session, options, None)
}

fn build_filtered(session: &Session, options: &MeshExport, only: Option<usize>) -> Built {
    let mut built = Built {
        positions: Vec::new(),
        normals: Vec::new(),
        extra: Vec::new(),
        indices: Vec::new(),
        joints: Vec::new(),
        motion: Vec::new(),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
        blend: false,
    };
    let with_motion = session.has_motion();
    let mut quads = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        if only.is_some_and(|only| only != index) {
            continue;
        }
        if object.hidden && !options.export_hidden {
            continue;
        }
        let bone = if session.skeleton.bones.is_empty() {
            0
        } else {
            session
                .skeleton
                .joint_or_root(object.bone.as_deref())
                .unwrap_or(0)
        };
        for mut quad in merged_quads(&object.cells, options.merge) {
            quad.cell = (
                quad.cell.0 + object.translation.0,
                quad.cell.1 + object.translation.1,
                quad.cell.2 + object.translation.2,
            );
            quad.bone = bone;
            quads.push(quad);
        }
    }
    if options.merge {
        quads = split_per_bone(quads);
    }
    // Every attribute follows from the corner, the face, the slot, and the
    // bone, so neighbouring quads that agree on all four share the vertex.
    let mut shared: HashMap<([i32; 3], usize, u8, u16), u32> = HashMap::new();
    for quad in quads {
        let rgba = session.palette[quad.slot as usize];
        if rgba[3] != 255 {
            built.blend = true;
        }
        let color = [
            linear(rgba[0]),
            linear(rgba[1]),
            linear(rgba[2]),
            rgba[3] as f32 / 255.0,
        ];
        let uv = [(quad.slot as f32 + 0.5) / 256.0, 0.5];
        let normal = y_up(NORMALS[quad.face]);
        let mut index = [0u32; 4];
        for (n, corner) in quad_corners(quad.face, quad.cell, quad.w, quad.h)
            .into_iter()
            .enumerate()
        {
            let next = (built.positions.len() / 3) as u32;
            let key = (corner, quad.face, quad.slot, quad.bone);
            index[n] = *shared.entry(key).or_insert(next);
            if index[n] != next {
                continue;
            }
            let point = y_up([corner[0] as f32, corner[1] as f32, corner[2] as f32]);
            for axis in 0..3 {
                built.min[axis] = built.min[axis].min(point[axis]);
                built.max[axis] = built.max[axis].max(point[axis]);
                built.positions.push(point[axis]);
                built.normals.push(normal[axis]);
            }
            if options.vertex_colors {
                built.extra.extend_from_slice(&color);
            } else {
                built.extra.extend_from_slice(&uv);
            }
            if with_motion {
                built
                    .motion
                    .extend_from_slice(&session.motion_uv(quad.slot));
            }
            if !session.skeleton.bones.is_empty() {
                built.joints.extend_from_slice(&[quad.bone, 0, 0, 0]);
            }
        }
        built
            .indices
            .extend_from_slice(&[index[0], index[1], index[2], index[0], index[2], index[3]]);
    }
    if built.positions.is_empty() {
        built.min = [0.0; 3];
        built.max = [0.0; 3];
    }
    built
}

#[derive(Clone, Copy)]
pub(super) struct GreedyQuad {
    pub(super) face: usize,
    pub(super) cell: Cell,
    pub(super) w: i32,
    pub(super) h: i32,
    pub(super) slot: u8,
    pub(super) bone: u16,
}

/// Plane axis, then the two in-plane axes. Width grows along `u`.
pub(super) fn face_axes(face: usize) -> (usize, usize, usize) {
    match face {
        0 | 1 => (0, 1, 2),
        2 | 3 => (1, 0, 2),
        _ => (2, 0, 1),
    }
}

pub(super) fn axis_of(cell: Cell, axis: usize) -> i32 {
    [cell.0, cell.1, cell.2][axis]
}

pub(super) fn cell_on(face: usize, plane: i32, u: i32, v: i32) -> Cell {
    let (plane_axis, u_axis, v_axis) = face_axes(face);
    let mut coords = [0; 3];
    coords[plane_axis] = plane;
    coords[u_axis] = u;
    coords[v_axis] = v;
    (coords[0], coords[1], coords[2])
}

/// Exposed faces. `merge` grows coplanar runs of one palette slot, so two
/// slots never share a quad even when their motion differs. Scan
/// order is `(v, u)`, so an L splits the same way every time. `w` and `h`
/// are in voxels.
pub(super) fn merged_quads(
    cells: &std::collections::HashMap<Cell, u8>,
    merge: bool,
) -> Vec<GreedyQuad> {
    let mut slices: BTreeMap<(u8, i32), BTreeMap<(i32, i32), u8>> = BTreeMap::new();
    for (cell, slot) in cells {
        if *slot == 0 {
            continue;
        }
        for face in 0..6 {
            let offset = OFFSETS[face];
            let neighbor = (cell.0 + offset.0, cell.1 + offset.1, cell.2 + offset.2);
            if cells.contains_key(&neighbor) {
                continue;
            }
            let (plane_axis, u_axis, v_axis) = face_axes(face);
            slices
                .entry((face as u8, axis_of(*cell, plane_axis)))
                .or_default()
                .insert((axis_of(*cell, v_axis), axis_of(*cell, u_axis)), *slot);
        }
    }
    let mut merged = Vec::new();
    for ((face, plane), mask) in &mut slices {
        let keys: Vec<(i32, i32)> = mask.keys().copied().collect();
        for (v, u) in keys {
            let Some(slot) = mask.get(&(v, u)).copied() else {
                continue;
            };
            let mut w = 1;
            let mut h = 1;
            if merge {
                while mask.get(&(v, u + w)).copied() == Some(slot) {
                    w += 1;
                }
                loop {
                    let row = (0..w).all(|i| mask.get(&(v + h, u + i)).copied() == Some(slot));
                    if !row {
                        break;
                    }
                    h += 1;
                }
            }
            for j in 0..h {
                for i in 0..w {
                    mask.remove(&(v + j, u + i));
                }
            }
            merged.push(GreedyQuad {
                face: *face as usize,
                cell: cell_on(*face as usize, *plane, u, v),
                w,
                h,
                slot,
                bone: 0,
            });
        }
    }
    merged
}

/// Corners of a `w` by `h` rectangle, CCW from outside. A unit face is
/// [`QUADS`]; a longer side replaces that `1` with the rectangle size.
pub(super) fn quad_corners(face: usize, cell: Cell, w: i32, h: i32) -> [[i32; 3]; 4] {
    let (_, u_axis, v_axis) = face_axes(face);
    let mut corners = [[0; 3]; 4];
    for (i, unit) in QUADS[face].iter().enumerate() {
        let mut point = [unit[0] as i32, unit[1] as i32, unit[2] as i32];
        if point[u_axis] == 1 {
            point[u_axis] = w;
        }
        if point[v_axis] == 1 {
            point[v_axis] = h;
        }
        corners[i] = [cell.0 + point[0], cell.1 + point[1], cell.2 + point[2]];
    }
    corners
}

/// Geometric corner at the minimum `u` and `v` of a quad. Positive faces
/// sit on the far side of the cell, so that coordinate is one past `cell`.
pub(super) fn split_origin(quad: &GreedyQuad) -> [i32; 3] {
    let (normal, _, _) = face_axes(quad.face);
    let mut origin = [quad.cell.0, quad.cell.1, quad.cell.2];
    if quad.face % 2 == 0 {
        origin[normal] += 1;
    }
    origin
}

/// Cut a quad where a corner of another face lands in an open edge.
///
/// A neighbour on the same oriented plane and the same palette slot is
/// left alone: greedy merge already made those one surface. A different
/// plane or a different slot still cuts, or the seam cracks. Each cut adds
/// vertices on the shared edge, so this repeats until a pass adds nothing.
pub(super) fn split_junctions(quads: Vec<GreedyQuad>) -> Vec<GreedyQuad> {
    let limit = (quads.len() * 4).max(8);
    let mut quads = quads;
    for _ in 0..limit {
        let mut on_plane: BTreeMap<(usize, i32), Vec<([i32; 3], usize, u8)>> = BTreeMap::new();
        for quad in &quads {
            for corner in quad_corners(quad.face, quad.cell, quad.w, quad.h) {
                for axis in 0..3 {
                    on_plane
                        .entry((axis, corner[axis]))
                        .or_default()
                        .push((corner, quad.face, quad.slot));
                }
            }
        }
        let mut next = Vec::with_capacity(quads.len());
        let mut grew = false;
        for quad in &quads {
            let (normal, u_axis, v_axis) = face_axes(quad.face);
            let origin = split_origin(quad);
            let mut u_splits = vec![0, quad.w];
            let mut v_splits = vec![0, quad.h];
            if let Some(hits) = on_plane.get(&(normal, origin[normal])) {
                for (at, face, slot) in hits {
                    if *face == quad.face && *slot == quad.slot {
                        continue;
                    }
                    let u = at[u_axis] - origin[u_axis];
                    let v = at[v_axis] - origin[v_axis];
                    if u < 0 || u > quad.w || v < 0 || v > quad.h {
                        continue;
                    }
                    if (v == 0 || v == quad.h) && u > 0 && u < quad.w {
                        u_splits.push(u);
                    }
                    if (u == 0 || u == quad.w) && v > 0 && v < quad.h {
                        v_splits.push(v);
                    }
                }
            }
            u_splits.sort_unstable();
            u_splits.dedup();
            v_splits.sort_unstable();
            v_splits.dedup();
            if u_splits.len() > 2 || v_splits.len() > 2 {
                grew = true;
            }
            for pair_v in v_splits.windows(2) {
                for pair_u in u_splits.windows(2) {
                    let (u0, du) = (pair_u[0], pair_u[1] - pair_u[0]);
                    let (v0, dv) = (pair_v[0], pair_v[1] - pair_v[0]);
                    if du <= 0 || dv <= 0 {
                        continue;
                    }
                    let mut cell = origin;
                    cell[u_axis] += u0;
                    cell[v_axis] += v0;
                    if quad.face % 2 == 0 {
                        cell[normal] -= 1;
                    }
                    next.push(GreedyQuad {
                        face: quad.face,
                        cell: (cell[0], cell[1], cell[2]),
                        w: du,
                        h: dv,
                        slot: quad.slot,
                        bone: quad.bone,
                    });
                }
            }
        }
        quads = next;
        if !grew {
            break;
        }
    }
    quads
}

/// Split T-junctions inside one bone. A single bone keeps the input order
/// and calls the same splitter the rigid export uses.
pub(super) fn split_per_bone(quads: Vec<GreedyQuad>) -> Vec<GreedyQuad> {
    let mut ids = Vec::new();
    for quad in &quads {
        if !ids.contains(&quad.bone) {
            ids.push(quad.bone);
        }
    }
    if ids.len() <= 1 {
        return split_junctions(quads);
    }
    let mut out = Vec::new();
    for id in ids {
        let group: Vec<GreedyQuad> = quads
            .iter()
            .copied()
            .filter(|quad| quad.bone == id)
            .collect();
        out.extend(split_junctions(group));
    }
    out
}

pub(super) fn y_up(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

pub(super) fn linear(channel: u8) -> f32 {
    let s = channel as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}
