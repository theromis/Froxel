//! Faces outside the greedy merge: strand boxes, and every cell when the
//! export bevels (`bevel.rs`). Each is a convex polygon in the bind frame
//! (the object's translation, as cells are placed) with its own normal. They
//! are never merged and never split for T-junctions.
//!
//! Vertex colours and the palette texture give a face its slot's colour; the
//! colour atlas gives each face one texel of its own, sampled at the texel's
//! centre. Ambient occlusion reads the cells at the nearest lattice corner,
//! and `bake` traces real rays against cells and strands alike.

use super::bake::Patch;
use super::occlusion::Occlusion;
use super::quads::{linear, y_up};
use super::*;

/// One exported polygon: corners in winding order seen from outside, the
/// outward normal, the slot, and the joint the object is bound to.
#[derive(Clone)]
pub(super) struct PolyFace {
    pub points: Vec<[f32; 3]>,
    pub normal: [f32; 3],
    pub slot: u8,
    pub bone: u16,
}

fn joint(session: &Session, object: &crate::state::Volume) -> u16 {
    if session.skeleton.bones.is_empty() {
        0
    } else {
        session
            .skeleton
            .joint_or_root(object.bone.as_deref())
            .unwrap_or(0)
    }
}

fn exported(
    object: &crate::state::Volume,
    index: usize,
    only: Option<usize>,
    hidden: bool,
) -> bool {
    only.is_none_or(|only| only == index) && (!object.hidden || hidden)
}

/// Every strand face of the exported objects (`only` one, or all, hidden
/// ones with `export_hidden`), bevelled with `bevel`.
pub(super) fn strand_faces(
    session: &Session,
    export_hidden: bool,
    only: Option<usize>,
    bevel: Option<crate::bevel::Bevel>,
) -> Vec<PolyFace> {
    let mut out = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        if object.strands.is_empty() || !exported(object, index, only, export_hidden) {
            continue;
        }
        let bone = joint(session, object);
        match bevel {
            Some(bevel) => {
                let t = object.translation;
                let shift = glam::Vec3::new(t.0 as f32, t.1 as f32, t.2 as f32);
                let mut polys = Vec::new();
                for b in session.strand_boxes(index) {
                    crate::bevel::box_polys(&b, bevel.size, &mut polys);
                }
                out.extend(polys.into_iter().map(|p| PolyFace {
                    points: p.points.iter().map(|q| (*q + shift).to_array()).collect(),
                    normal: p.normal.to_array(),
                    slot: p.slot,
                    bone,
                }));
            }
            None => {
                for b in session.strand_bind_boxes(index) {
                    for (corners, normal) in b.faces() {
                        out.push(PolyFace {
                            points: corners.iter().map(|c| c.to_array()).collect(),
                            normal: normal.to_array(),
                            slot: b.slot,
                            bone,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The bevelled cells of the exported objects, each object against its own
/// cells. With the colour atlas, `slot_of` maps a cell's slot (its class).
pub(super) fn cell_faces(
    session: &Session,
    export_hidden: bool,
    only: Option<usize>,
    bevel: crate::bevel::Bevel,
) -> Vec<PolyFace> {
    let mut out = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        if !exported(object, index, only, export_hidden) {
            continue;
        }
        let listed: Vec<(Cell, u8)> = object
            .cells
            .iter()
            .filter(|(_, slot)| **slot != 0)
            .map(|(cell, slot)| (*cell, *slot))
            .collect();
        if listed.is_empty() {
            continue;
        }
        let bone = joint(session, object);
        let t = object.translation;
        let shift = glam::Vec3::new(t.0 as f32, t.1 as f32, t.2 as f32);
        let mut polys = Vec::new();
        crate::bevel::cell_polys(
            |c| object.cells.get(&c).is_some_and(|slot| *slot != 0),
            &listed,
            bevel,
            &mut polys,
        );
        out.extend(polys.into_iter().map(|p| PolyFace {
            points: p.points.iter().map(|q| (*q + shift).to_array()).collect(),
            normal: p.normal.to_array(),
            slot: p.slot,
            bone,
        }));
    }
    out
}

/// How many strand faces [`strand_faces`] gives unbevelled, without building them.
pub(super) fn strand_face_count(session: &Session, export_hidden: bool) -> usize {
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

/// The cell occlusion at a corner: the nearest lattice point, seen along
/// the axis nearest the face's normal.
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

/// The face as a bake patch: the rectangle about it in its own plane.
pub(super) fn patch(face: &PolyFace) -> Patch {
    let p = face
        .points
        .iter()
        .map(|q| glam::DVec3::from_array(q.map(f64::from)))
        .collect::<Vec<_>>();
    let normal = glam::DVec3::from_array(face.normal.map(f64::from));
    let u = (p[1] - p[0]).normalize_or(normal.any_orthonormal_vector());
    let v = normal.cross(u);
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in &p {
        let d = *q - p[0];
        for (k, axis) in [u, v].into_iter().enumerate() {
            lo[k] = lo[k].min(d.dot(axis));
            hi[k] = hi[k].max(d.dot(axis));
        }
    }
    let corner = p[0] + u * lo[0] + v * lo[1];
    Patch {
        corner: corner.to_array(),
        u: (u * (hi[0] - lo[0])).to_array(),
        v: (v * (hi[1] - lo[1])).to_array(),
        normal: normal.to_array(),
    }
}

/// Appends the faces to `built` with the palette texture or vertex colours.
pub(super) fn emit_plain(
    built: &mut Built,
    session: &Session,
    options: &MeshExport,
    faces: &[PolyFace],
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
        let shades: Vec<f32> = face
            .points
            .iter()
            .map(|c| occlusion.map_or(1.0, |o| corner_shade(o, *c, face.normal)))
            .collect();
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
    faces: &[PolyFace],
    texels: &[(u32, u32)],
    occlusion: Option<&Occlusion>,
    with_motion: bool,
) {
    let (width, height) = (built.atlas.0 as f32, built.atlas.1 as f32);
    for (face, (x, y)) in faces.iter().zip(texels) {
        let uv = [(*x as f32 + 0.5) / width, (*y as f32 + 0.5) / height];
        let shades: Vec<f32> = face
            .points
            .iter()
            .map(|c| occlusion.map_or(1.0, |o| corner_shade(o, *c, face.normal)))
            .collect();
        push_face(built, session, face, with_motion, |built, n| {
            built.extra.extend_from_slice(&uv);
            if occlusion.is_some() {
                built.shade.push(shades[n]);
            }
        });
    }
}

/// Positions, normals, motion, and joints of one face, with `extra` filling
/// the colour attributes of corner `n`, and its triangles as a fan.
fn push_face(
    built: &mut Built,
    session: &Session,
    face: &PolyFace,
    with_motion: bool,
    mut extra: impl FnMut(&mut Built, usize),
) {
    let first = (built.positions.len() / 3) as u32;
    let normal = y_up(face.normal);
    for (n, corner) in face.points.iter().enumerate() {
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
    for k in 1..face.points.len() as u32 - 1 {
        built
            .indices
            .extend_from_slice(&[first, first + k, first + k + 1]);
    }
}

/// The atlas texels for `count` faces in rows below a `width` by `height`
/// atlas: the texels, the width (wider when the rows would outgrow it), and
/// the new height.
pub(super) fn atlas_rows(width: u32, height: u32, count: usize) -> (Vec<(u32, u32)>, u32, u32) {
    if count == 0 {
        return (Vec::new(), width, height);
    }
    let mut width = width.max(1);
    while (count as u64).div_ceil(width as u64) > (width as u64).max(height as u64 + 64) {
        width *= 2;
    }
    let texels = (0..count as u32)
        .map(|k| (k % width, height + k / width))
        .collect();
    let rows = (count as u32).div_ceil(width);
    (texels, width, (height + rows).next_multiple_of(4))
}

#[cfg(test)]
#[path = "../../tests/bevel_export.rs"]
mod tests;
