//! Greedy face merge and T-junction splits.

use std::collections::HashMap;

use super::occlusion::Occlusion;
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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
            bake: None,
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
        shade: Vec::new(),
        indices: Vec::new(),
        joints: Vec::new(),
        motion: Vec::new(),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
        blend: false,
        atlas: (0, 0, Vec::new()),
    };
    let with_motion = session.has_motion();
    let atlas = options.color_atlas;
    let occlusion = Occlusion::new(session, options);
    let (vertex_ao, texel_ao) = options.ao_targets();
    // With the colour atlas a quad's `slot` is its merge class (motion and
    // opacity) plus one, and `colors` keeps each cell's real slot for painting.
    let classes = if atlas {
        merge_classes(session)
    } else {
        Vec::new()
    };
    let mut colors: HashMap<Cell, u8> = HashMap::new();
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
        let classed;
        let cells = if atlas {
            classed = object
                .cells
                .iter()
                .filter(|(_, slot)| **slot != 0)
                .map(|(cell, slot)| (*cell, classes[*slot as usize].0))
                .collect::<HashMap<Cell, u8>>();
            for (cell, slot) in &object.cells {
                if *slot != 0 {
                    let world = (
                        cell.0 + object.translation.0,
                        cell.1 + object.translation.1,
                        cell.2 + object.translation.2,
                    );
                    colors.insert(world, *slot);
                }
            }
            &classed
        } else {
            &object.cells
        };
        // Occlusion only in the atlas texels leaves the faces to merge as
        // they would without it.
        let offset = object.translation;
        let corners = occlusion.as_ref().filter(|_| vertex_ao).map(|occlusion| {
            move |face: usize, cell: Cell| {
                occlusion.corners(
                    face,
                    (cell.0 + offset.0, cell.1 + offset.1, cell.2 + offset.2),
                )
            }
        });
        let shaded = corners
            .as_ref()
            .map(|corners| corners as &dyn Fn(usize, Cell) -> [u8; 4]);
        for mut quad in merged_quads_shaded(cells, options.merge || atlas, shaded) {
            quad.cell = (
                quad.cell.0 + object.translation.0,
                quad.cell.1 + object.translation.1,
                quad.cell.2 + object.translation.2,
            );
            quad.bone = bone;
            quads.push(quad);
        }
    }
    if options.merge && !atlas {
        quads = split_per_bone(quads);
    }
    if atlas {
        let (width, height, blocks) = pack(&quads);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        // One light per texel, in the order the loop below paints them.
        let baked = options.bake.as_ref().map(|bake| {
            let faces = atlas_faces(&quads);
            super::bake::session_light(session, options, bake, &faces)
        });
        let mut next_light = 0;
        for (quad, (x0, y0)) in quads.iter().zip(&blocks) {
            let (normal, _, _) = face_axes(quad.face);
            let origin = split_origin(quad);
            let (_, u_axis, v_axis) = face_axes(quad.face);
            for j in 0..quad.h {
                for i in 0..quad.w {
                    let cell = cell_on(
                        quad.face,
                        axis_of(quad.cell, normal),
                        origin[u_axis] + i,
                        origin[v_axis] + j,
                    );
                    let slot = colors.get(&cell).copied().unwrap_or(0);
                    let mut rgba = session.palette[slot as usize];
                    if rgba[3] != 255 {
                        built.blend = true;
                    }
                    if let Some(occlusion) = occlusion.as_ref().filter(|_| texel_ao) {
                        // One texel per cell face: the mean of its corners.
                        let corners = occlusion.corners(quad.face, cell);
                        let shade = corners.iter().map(|c| *c as f32).sum::<f32>() / (4.0 * 255.0);
                        for channel in &mut rgba[..3] {
                            *channel = srgb(linear(*channel) * shade);
                        }
                    }
                    if let Some(baked) = &baked {
                        let light = baked[next_light];
                        next_light += 1;
                        for (channel, light) in rgba[..3].iter_mut().zip(light) {
                            *channel = srgb(linear(*channel) * light);
                        }
                    }
                    let at = (((y0 + j as u32) * width + x0 + i as u32) * 4) as usize;
                    pixels[at..at + 4].copy_from_slice(&rgba);
                }
            }
        }
        built.atlas = (width, height, pixels);
        emit_atlas(
            &mut built,
            session,
            &quads,
            &blocks,
            &classes,
            with_motion,
            occlusion.as_ref().filter(|_| vertex_ao),
        );
        if built.positions.is_empty() {
            built.min = [0.0; 3];
            built.max = [0.0; 3];
        }
        return built;
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
        let corners = quad_corners(quad.face, quad.cell, quad.w, quad.h);
        let shades = corners.map(|corner| {
            occlusion
                .as_ref()
                .filter(|_| vertex_ao)
                .map_or(1.0, |occlusion| {
                    occlusion.shade(corner, quad.face) as f32 / 255.0
                })
        });
        for (n, corner) in corners.into_iter().enumerate() {
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
                let s = shades[n];
                built.extra.extend_from_slice(&[
                    color[0] * s,
                    color[1] * s,
                    color[2] * s,
                    color[3],
                ]);
            } else {
                built.extra.extend_from_slice(&uv);
                if vertex_ao {
                    built.shade.push(shades[n]);
                }
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
        // Cut along the lighter diagonal, so one dark corner stays a corner
        // instead of a band across the face.
        let index = if shades[1] + shades[3] > shades[0] + shades[2] {
            [index[1], index[2], index[3], index[0]]
        } else {
            index
        };
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

/// A document's colour-atlas quads with no occlusion: world-space cells, each
/// cell's real slot, and the merge classes.
#[allow(clippy::type_complexity)]
pub(super) fn atlas_quads(
    session: &Session,
    export_hidden: bool,
) -> (Vec<GreedyQuad>, HashMap<Cell, u8>, Vec<(u8, u8)>) {
    let classes = merge_classes(session);
    let mut colors = HashMap::new();
    let mut quads = Vec::new();
    for object in &session.objects {
        if object.hidden && !export_hidden {
            continue;
        }
        let offset = object.translation;
        let classed: HashMap<Cell, u8> = object
            .cells
            .iter()
            .filter(|(_, slot)| **slot != 0)
            .map(|(cell, slot)| (*cell, classes[*slot as usize].0))
            .collect();
        for (cell, slot) in &object.cells {
            if *slot != 0 {
                colors.insert(
                    (cell.0 + offset.0, cell.1 + offset.1, cell.2 + offset.2),
                    *slot,
                );
            }
        }
        for mut quad in merged_quads_shaded(&classed, true, None) {
            quad.cell = (
                quad.cell.0 + offset.0,
                quad.cell.1 + offset.1,
                quad.cell.2 + offset.2,
            );
            quads.push(quad);
        }
    }
    (quads, colors, classes)
}

/// The cell face of every atlas texel, quad by quad, row by row.
pub(super) fn atlas_faces(quads: &[GreedyQuad]) -> Vec<(usize, Cell)> {
    let mut faces = Vec::new();
    for quad in quads {
        let (normal, u_axis, v_axis) = face_axes(quad.face);
        let origin = split_origin(quad);
        for j in 0..quad.h {
            for i in 0..quad.w {
                let cell = cell_on(
                    quad.face,
                    axis_of(quad.cell, normal),
                    origin[u_axis] + i,
                    origin[v_axis] + j,
                );
                faces.push((quad.face, cell));
            }
        }
    }
    faces
}

/// Colour-atlas faces. Instead of cutting a quad where a corner of another
/// face lands on its edge (which multiplies quads), each such corner becomes
/// a vertex on the quad's outline, and the outline is ear-clipped into
/// `n - 2` triangles with no zero-area ones. The seams stay closed, with one
/// pass, because the added vertices are corners that already exist.
pub(super) fn emit_atlas(
    built: &mut Built,
    session: &Session,
    quads: &[GreedyQuad],
    blocks: &[(u32, u32)],
    classes: &[(u8, u8)],
    with_motion: bool,
    occlusion: Option<&Occlusion>,
) {
    // Every corner on every axis plane through it, so a perpendicular face
    // that ends on a quad's edge is found as well as a coplanar one.
    let mut on_plane: HashMap<(usize, i32), Vec<[i32; 3]>> = HashMap::new();
    for quad in quads {
        for corner in quad_corners(quad.face, quad.cell, quad.w, quad.h) {
            for axis in 0..3 {
                on_plane
                    .entry((axis, corner[axis]))
                    .or_default()
                    .push(corner);
            }
        }
    }
    for corners in on_plane.values_mut() {
        corners.sort_unstable();
        corners.dedup();
    }
    let (width, height) = (built.atlas.0 as f32, built.atlas.1 as f32);
    for (quad, (x0, y0)) in quads.iter().zip(blocks) {
        let (normal, u_axis, v_axis) = face_axes(quad.face);
        let origin = split_origin(quad);
        let corners = quad_corners(quad.face, quad.cell, quad.w, quad.h);
        // The outline in the corners' order, with every other corner that
        // lies inside an edge inserted in order along that edge.
        let hits = &on_plane[&(normal, origin[normal])];
        let mut outline: Vec<[i32; 3]> = Vec::with_capacity(4);
        for edge in 0..4 {
            let (a, b) = (corners[edge], corners[(edge + 1) % 4]);
            outline.push(a);
            let axis = if a[u_axis] != b[u_axis] {
                u_axis
            } else {
                v_axis
            };
            let other = if axis == u_axis { v_axis } else { u_axis };
            let (lo, hi) = (a[axis].min(b[axis]), a[axis].max(b[axis]));
            let mut inside: Vec<[i32; 3]> = hits
                .iter()
                .filter(|hit| hit[other] == a[other] && hit[axis] > lo && hit[axis] < hi)
                .copied()
                .collect();
            inside.sort_unstable_by_key(|hit| (hit[axis] - a[axis]).abs());
            outline.extend(inside);
        }
        let shade = |corner: &[i32; 3]| {
            occlusion.map_or(1.0, |occlusion| {
                occlusion.shade(*corner, quad.face) as f32 / 255.0
            })
        };
        // The first ear cuts the diagonal from the last vertex to the second.
        // Start one later when the other diagonal is lighter.
        if outline.len() == 4
            && shade(&outline[0]) + shade(&outline[2]) > shade(&outline[1]) + shade(&outline[3])
        {
            outline.rotate_left(1);
        }
        let first = (built.positions.len() / 3) as u32;
        let normal_vec = y_up(NORMALS[quad.face]);
        let motion = session.motion_uv(classes[quad.slot as usize - 1].1);
        for corner in &outline {
            let point = y_up([corner[0] as f32, corner[1] as f32, corner[2] as f32]);
            for axis in 0..3 {
                built.min[axis] = built.min[axis].min(point[axis]);
                built.max[axis] = built.max[axis].max(point[axis]);
                built.positions.push(point[axis]);
                built.normals.push(normal_vec[axis]);
            }
            let u = (*x0 as i32 + corner[u_axis] - origin[u_axis]) as f32 / width;
            let v = (*y0 as i32 + corner[v_axis] - origin[v_axis]) as f32 / height;
            built.extra.extend_from_slice(&[u, v]);
            if occlusion.is_some() {
                built.shade.push(shade(corner));
            }
            if with_motion {
                built.motion.extend_from_slice(&motion);
            }
            if !session.skeleton.bones.is_empty() {
                built.joints.extend_from_slice(&[quad.bone, 0, 0, 0]);
            }
        }
        // Ear clipping on a convex outline: a vertex where the outline turns
        // is an ear. It is taken only if the rest keeps an area, or the points
        // left on one line would have no triangle.
        let flat: Vec<(i64, i64)> = outline
            .iter()
            .map(|p| (p[u_axis] as i64, p[v_axis] as i64))
            .collect();
        let turn = |a: usize, b: usize, c: usize| {
            let (a, b, c) = (flat[a], flat[b], flat[c]);
            (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0)
        };
        let area = |points: &[usize]| {
            let n = points.len();
            (0..n)
                .map(|i| {
                    let (a, b) = (flat[points[i]], flat[points[(i + 1) % n]]);
                    a.0 * b.1 - b.0 * a.1
                })
                .sum::<i64>()
        };
        let mut left: Vec<usize> = (0..outline.len()).collect();
        while left.len() > 3 {
            let n = left.len();
            let ear = (0..n)
                .find(|&i| {
                    if turn(left[(i + n - 1) % n], left[i], left[(i + 1) % n]) == 0 {
                        return false;
                    }
                    let mut rest = left.clone();
                    rest.remove(i);
                    area(&rest) != 0
                })
                .expect("a convex outline with area has an ear");
            let (a, b, c) = (left[(ear + n - 1) % n], left[ear], left[(ear + 1) % n]);
            built.indices.extend_from_slice(&[
                first + a as u32,
                first + b as u32,
                first + c as u32,
            ]);
            left.remove(ear);
        }
        built.indices.extend_from_slice(&[
            first + left[0] as u32,
            first + left[1] as u32,
            first + left[2] as u32,
        ]);
    }
}

/// The colour-atlas merge class of every palette slot, plus one so that 0
/// stays empty, and for each class a slot that carries its motion. Slots of
/// one motion and opacity share a class.
pub(super) fn merge_classes(session: &Session) -> Vec<(u8, u8)> {
    let mut keys: Vec<([u32; 2], bool)> = Vec::new();
    let mut representative: Vec<u8> = Vec::new();
    let mut classes = vec![(0u8, 0u8); 256];
    for slot in 1..=255u8 {
        let motion = session.motion_uv(slot);
        let key = (
            [motion[0].to_bits(), motion[1].to_bits()],
            session.palette[slot as usize][3] != 255,
        );
        let class = match keys.iter().position(|known| *known == key) {
            Some(class) => class,
            None => {
                keys.push(key);
                representative.push(slot);
                keys.len() - 1
            }
        };
        classes[slot as usize].0 = class as u8 + 1;
    }
    // Index by class - 1 for the representative slot.
    for (class, slot) in representative.into_iter().enumerate() {
        classes[class].1 = slot;
    }
    classes
}

/// Shelf-pack one block of `w` by `h` texels per quad, tallest first. The
/// width is a power of two near the square root of the texel count. Returns
/// the width, the height and each quad's block corner.
pub(super) fn pack(quads: &[GreedyQuad]) -> (u32, u32, Vec<(u32, u32)>) {
    let texels: u64 = quads.iter().map(|quad| (quad.w * quad.h) as u64).sum();
    let widest = quads.iter().map(|quad| quad.w as u32).max().unwrap_or(1);
    let mut width = 16u32;
    while (width as u64) * (width as u64) < texels + texels / 8 {
        width *= 2;
    }
    width = width.max(widest.next_power_of_two());
    let mut order: Vec<usize> = (0..quads.len()).collect();
    order.sort_by_key(|&index| {
        (
            std::cmp::Reverse(quads[index].h),
            std::cmp::Reverse(quads[index].w),
        )
    });
    let mut blocks = vec![(0u32, 0u32); quads.len()];
    let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
    for index in order {
        let (w, h) = (quads[index].w as u32, quads[index].h as u32);
        if x + w > width {
            y += row;
            x = 0;
            row = 0;
        }
        blocks[index] = (x, y);
        x += w;
        row = row.max(h);
    }
    let height = (y + row).max(1).next_multiple_of(4);
    (width, height, blocks)
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
#[cfg(test)]
pub(super) fn merged_quads(
    cells: &std::collections::HashMap<Cell, u8>,
    merge: bool,
) -> Vec<GreedyQuad> {
    merged_quads_shaded(cells, merge, None)
}

/// The quads an unmerged export has: the faces of each exported object's
/// solid cells that no cell of that object covers, as
/// [`merged_quads_shaded`] finds them.
pub(super) fn unmerged_faces(session: &Session, export_hidden: bool) -> usize {
    session
        .objects
        .iter()
        .filter(|object| !object.hidden || export_hidden)
        .map(|object| {
            object
                .cells
                .iter()
                .filter(|(_, slot)| **slot != 0)
                .map(|(cell, _)| {
                    OFFSETS
                        .iter()
                        .filter(|o| {
                            !object
                                .cells
                                .contains_key(&(cell.0 + o.0, cell.1 + o.1, cell.2 + o.2))
                        })
                        .count()
                })
                .sum::<usize>()
        })
        .sum()
}

/// [`merged_quads`] with baked occlusion. `shade` gives a face's corner
/// shades in `(u, v)` order (`00`, `10`, `01`, `11`). Two faces merge only
/// when their shades match too, and a run grows along `u` only while the
/// shades do not change along `u` (likewise `v`), so interpolating across the
/// merged rectangle gives every cell the shades it had alone.
pub(super) fn merged_quads_shaded(
    cells: &std::collections::HashMap<Cell, u8>,
    merge: bool,
    shade: Option<&dyn Fn(usize, Cell) -> [u8; 4]>,
) -> Vec<GreedyQuad> {
    let mut slices: BTreeMap<(u8, i32), BTreeMap<(i32, i32), (u8, [u8; 4])>> = BTreeMap::new();
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
            let corners = shade.map_or([255; 4], |shade| shade(face, *cell));
            slices
                .entry((face as u8, axis_of(*cell, plane_axis)))
                .or_default()
                .insert(
                    (axis_of(*cell, v_axis), axis_of(*cell, u_axis)),
                    (*slot, corners),
                );
        }
    }
    let mut merged = Vec::new();
    for ((face, plane), mask) in &mut slices {
        let keys: Vec<(i32, i32)> = mask.keys().copied().collect();
        for (v, u) in keys {
            let Some(key) = mask.get(&(v, u)).copied() else {
                continue;
            };
            let (slot, corners) = key;
            let flat_u = corners[0] == corners[1] && corners[2] == corners[3];
            let flat_v = corners[0] == corners[2] && corners[1] == corners[3];
            let mut w = 1;
            let mut h = 1;
            if merge {
                while flat_u && mask.get(&(v, u + w)).copied() == Some(key) {
                    w += 1;
                }
                loop {
                    let row =
                        flat_v && (0..w).all(|i| mask.get(&(v + h, u + i)).copied() == Some(key));
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

/// The sRGB byte of a linear channel, the inverse of [`linear`].
pub(super) fn srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let s = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

pub(super) fn linear(channel: u8) -> f32 {
    let s = channel as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}
