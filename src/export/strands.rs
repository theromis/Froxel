//! Strand boxes in mesh export: six faces per box, in the bind frame (the
//! object's translation, as cells are placed), with their own turned
//! normals. They are never merged with cell faces and never split for
//! T-junctions: they share no lattice with the grid, and a box face is a
//! whole rectangle.
//!
//! Vertex colours and the palette texture give a face its slot's colour; the
//! colour atlas gives each face one texel of its own, sampled at the texel's
//! centre. Ambient occlusion reads the cells at the nearest lattice corner
//! (cells darken strands; strands do not darken cells), and `bake` traces
//! real rays against cells and strands alike.

use super::bake::Patch;
use super::occlusion::Occlusion;
use super::quads::{linear, y_up};
use super::*;

/// One exported box face: corners in winding order seen from outside, the
/// outward normal, the slot, and the joint the object is bound to.
#[derive(Clone, Copy)]
pub(super) struct StrandFace {
    pub corners: [[f32; 3]; 4],
    pub normal: [f32; 3],
    pub slot: u8,
    pub bone: u16,
}

/// Every strand face of the exported objects (`only` one, or all, hidden
/// ones with `export_hidden`).
pub(super) fn faces(
    session: &Session,
    export_hidden: bool,
    only: Option<usize>,
) -> Vec<StrandFace> {
    let mut out = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        if object.strands.is_empty() || only.is_some_and(|only| only != index) {
            continue;
        }
        if object.hidden && !export_hidden {
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
        for b in session.strand_bind_boxes(index) {
            for (corners, normal) in b.faces() {
                out.push(StrandFace {
                    corners: corners.map(|c| c.to_array()),
                    normal: normal.to_array(),
                    slot: b.slot,
                    bone,
                });
            }
        }
    }
    out
}

/// How many faces [`faces`] gives, without building them.
pub(super) fn face_count(session: &Session, export_hidden: bool) -> usize {
    session
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| !o.strands.is_empty() && (!o.hidden || export_hidden))
        .map(|(index, _)| session.strand_boxes(index).len() * 6)
        .sum()
}

/// Whether an object has anything to export: a solid cell or a strand.
pub(super) fn has_geometry(object: &crate::state::Volume) -> bool {
    !object.strands.is_empty() || object.cells.values().any(|slot| *slot != 0)
}

/// The cell occlusion at a strand corner: the nearest lattice point, seen
/// along the axis nearest the face's normal.
pub(super) fn corner_shade(occlusion: &Occlusion, corner: [f32; 3], normal: [f32; 3]) -> f32 {
    let lattice = corner.map(|v| v.round() as i32);
    let a = normal.map(f32::abs);
    let axis = if a[0] >= a[1] && a[0] >= a[2] {
        0
    } else if a[1] >= a[2] {
        1
    } else {
        2
    };
    let face = axis * 2 + usize::from(normal[axis] < 0.0);
    occlusion.shade(lattice, face) as f32 / 255.0
}

/// The face as a bake patch: a corner and its two edges.
pub(super) fn patch(face: &StrandFace) -> Patch {
    let c = face.corners.map(|p| p.map(f64::from));
    Patch {
        corner: c[0],
        u: [0, 1, 2].map(|k| c[1][k] - c[0][k]),
        v: [0, 1, 2].map(|k| c[3][k] - c[0][k]),
        normal: face.normal.map(f64::from),
    }
}

/// Appends the faces to `built` with the palette texture or vertex colours.
pub(super) fn emit_plain(
    built: &mut Built,
    session: &Session,
    options: &MeshExport,
    faces: &[StrandFace],
    occlusion: Option<&Occlusion>,
    with_motion: bool,
) {
    for face in faces {
        let rgba = session.palette[face.slot as usize];
        if rgba[3] != 255 {
            built.blend = true;
        }
        let color = [
            linear(rgba[0]),
            linear(rgba[1]),
            linear(rgba[2]),
            rgba[3] as f32 / 255.0,
        ];
        let uv = [(face.slot as f32 + 0.5) / 256.0, 0.5];
        let shades = face
            .corners
            .map(|c| occlusion.map_or(1.0, |o| corner_shade(o, c, face.normal)));
        push_face(built, session, face, with_motion, |built, n| {
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
                if occlusion.is_some() {
                    built.shade.push(shades[n]);
                }
            }
        });
    }
}

/// Appends the faces to `built` for the colour atlas, face `k` sampling
/// the centre of texel `texels[k]`.
pub(super) fn emit_atlas(
    built: &mut Built,
    session: &Session,
    faces: &[StrandFace],
    texels: &[(u32, u32)],
    occlusion: Option<&Occlusion>,
    with_motion: bool,
) {
    let (width, height) = (built.atlas.0 as f32, built.atlas.1 as f32);
    for (face, (x, y)) in faces.iter().zip(texels) {
        let uv = [(*x as f32 + 0.5) / width, (*y as f32 + 0.5) / height];
        let shades = face
            .corners
            .map(|c| occlusion.map_or(1.0, |o| corner_shade(o, c, face.normal)));
        push_face(built, session, face, with_motion, |built, n| {
            built.extra.extend_from_slice(&uv);
            if occlusion.is_some() {
                built.shade.push(shades[n]);
            }
        });
    }
}

/// Positions, normals, motion, and joints of one face, with `extra` filling
/// the colour attributes of corner `n`, and its two triangles.
fn push_face(
    built: &mut Built,
    session: &Session,
    face: &StrandFace,
    with_motion: bool,
    mut extra: impl FnMut(&mut Built, usize),
) {
    let first = (built.positions.len() / 3) as u32;
    let normal = y_up(face.normal);
    for (n, corner) in face.corners.iter().enumerate() {
        let point = y_up(*corner);
        for axis in 0..3 {
            built.min[axis] = built.min[axis].min(point[axis]);
            built.max[axis] = built.max[axis].max(point[axis]);
            built.positions.push(point[axis]);
            built.normals.push(normal[axis]);
        }
        extra(built, n);
        if with_motion {
            built
                .motion
                .extend_from_slice(&session.motion_uv(face.slot));
        }
        if !session.skeleton.bones.is_empty() {
            built.joints.extend_from_slice(&[face.bone, 0, 0, 0]);
        }
    }
    built
        .indices
        .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// The atlas texels for `count` faces in rows below a `width` by `height`
/// atlas: the texels and the new height.
pub(super) fn atlas_rows(width: u32, height: u32, count: usize) -> (Vec<(u32, u32)>, u32) {
    if count == 0 {
        return (Vec::new(), height);
    }
    let width = width.max(1);
    let texels = (0..count as u32)
        .map(|k| (k % width, height + k / width))
        .collect();
    let rows = (count as u32).div_ceil(width);
    (texels, (height + rows).next_multiple_of(4))
}
