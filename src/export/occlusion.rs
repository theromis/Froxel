//! Baked ambient occlusion: one shade per face corner, from the cells around
//! that corner, so crevices and the undersides of overhangs export darker.
//!
//! A shade depends only on the lattice point and the face direction, never on
//! which quad asks, so vertices stay shared and a T-junction vertex gets the
//! same value as the corners it sits between.

use std::collections::HashSet;

use super::quads::face_axes;
use super::*;

/// Largest `ao_radius` a caller may ask for, in cells.
pub const MAX_RADIUS: u32 = 8;

pub(super) struct Occlusion {
    solid: HashSet<Cell>,
    strength: f32,
    /// Per face: cells in front of a corner beyond the four that touch it,
    /// as offsets from the corner, with a cosine over squared distance weight.
    kernels: [Vec<(Cell, f32)>; 6],
}

impl Occlusion {
    /// `None` when the strength is 0, so the export is unchanged.
    pub(super) fn new(session: &Session, options: &MeshExport) -> Option<Occlusion> {
        if options.ao_strength <= 0.0 {
            return None;
        }
        // Every exported object occludes, so a face next to another object's
        // cells darkens too. Translucent slots let the light through.
        let mut solid = HashSet::new();
        for object in &session.objects {
            if object.hidden && !options.export_hidden {
                continue;
            }
            for (cell, slot) in &object.cells {
                if *slot != 0 && session.palette[*slot as usize][3] == 255 {
                    solid.insert((
                        cell.0 + object.translation.0,
                        cell.1 + object.translation.1,
                        cell.2 + object.translation.2,
                    ));
                }
            }
        }
        let radius = options.ao_radius.min(MAX_RADIUS) as i32;
        let kernels = std::array::from_fn(|face| kernel(face, radius));
        Some(Occlusion {
            solid,
            strength: options.ao_strength.min(1.0),
            kernels,
        })
    }

    /// Light left at a corner of a face, 0..=255. 255 is unoccluded. The
    /// corner term counts the four cells in front of the corner (two sides
    /// and the diagonal; two sides alone count as three) and keeps a quarter
    /// of the light per count. The hemisphere term is the weighted share of
    /// open cells farther out, up to `ao_radius`.
    pub(super) fn shade(&self, corner: [i32; 3], face: usize) -> u8 {
        let (normal, u_axis, v_axis) = face_axes(face);
        let layer = if face % 2 == 0 {
            corner[normal]
        } else {
            corner[normal] - 1
        };
        let mut near = [false; 4];
        for (n, (du, dv)) in [(-1, -1), (0, -1), (-1, 0), (0, 0)].into_iter().enumerate() {
            let mut cell = [0; 3];
            cell[normal] = layer;
            cell[u_axis] = corner[u_axis] + du;
            cell[v_axis] = corner[v_axis] + dv;
            near[n] = self.solid.contains(&(cell[0], cell[1], cell[2]));
        }
        let count = near.iter().filter(|hit| **hit).count();
        let diagonal = count == 2 && (near[0] == near[3]);
        let corner_term = if diagonal { 3 } else { count.min(3) };
        let mut light = 1.0 - 0.25 * corner_term as f32;
        let kernel = &self.kernels[face];
        if !kernel.is_empty() {
            let (mut open, mut total) = (0.0, 0.0);
            for (offset, weight) in kernel {
                total += weight;
                let cell = (
                    corner[0] + offset.0,
                    corner[1] + offset.1,
                    corner[2] + offset.2,
                );
                if !self.solid.contains(&cell) {
                    open += weight;
                }
            }
            light *= open / total;
        }
        let shade = 1.0 - self.strength * (1.0 - light);
        (shade * 255.0).round().clamp(0.0, 255.0) as u8
    }

    /// The shades at a unit face's corners, in `(u, v)` order: `(0, 0)`,
    /// `(1, 0)`, `(0, 1)`, `(1, 1)`. `cell` is in world space.
    pub(super) fn corners(&self, face: usize, cell: Cell) -> [u8; 4] {
        let (normal, u_axis, v_axis) = face_axes(face);
        let mut base = [cell.0, cell.1, cell.2];
        if face % 2 == 0 {
            base[normal] += 1;
        }
        let at = |du: i32, dv: i32| {
            let mut point = base;
            point[u_axis] += du;
            point[v_axis] += dv;
            self.shade(point, face)
        };
        [at(0, 0), at(1, 0), at(0, 1), at(1, 1)]
    }
}

/// Cells in front of a corner, centre within `radius` of it, less the four
/// that touch the corner (the corner term has those).
fn kernel(face: usize, radius: i32) -> Vec<(Cell, f32)> {
    let (normal, _, _) = face_axes(face);
    let sign = if face % 2 == 0 { 1.0 } else { -1.0 };
    let reach = radius as f32;
    let mut out = Vec::new();
    for x in -radius..radius {
        for y in -radius..radius {
            for z in -radius..radius {
                let offset = [x, y, z];
                let r = offset.map(|o| o as f32 + 0.5);
                let along = r[normal] * sign;
                if along <= 0.0 {
                    continue;
                }
                let length = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                let touching = along < 1.0 && (0..3).all(|a| a == normal || r[a].abs() < 1.0);
                if length > reach || touching {
                    continue;
                }
                out.push(((x, y, z), along / (length * length * length)));
            }
        }
    }
    out
}
