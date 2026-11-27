//! Bevelled cells for mesh export and the previews: each cell is its cube
//! cut by a plane along some of its twelve edges, `size` cells back from
//! the edge on both faces.
//!
//! - [`BevelMode::Cubes`] cuts every edge that touches open space, so each
//!   cube is rounded on its own and shallow grooves run between neighbours
//!   on the surface, the look of a model built from toy blocks.
//! - [`BevelMode::Edges`] cuts only the edges where the shape turns outward
//!   (both cells across the edge are empty), so flat runs stay whole and the
//!   silhouette softens. Where a cut edge ends against a wall, the wall's
//!   cell closes the end of the cut with a triangle on its own face.
//!
//! Faces two solid cells share are left out, except the corners one side's
//! cut uncovers. The result is closed: `tests/bevel.rs`
//! checks inside and outside by ray parity against the solid.

use glam::Vec3;

use crate::state::Cell;

/// Most bevel, in cells: two cuts on one face never meet.
pub const MAX_BEVEL: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BevelMode {
    Cubes,
    Edges,
}

impl BevelMode {
    pub const NAMES: &'static str = "cubes or edges";

    pub fn name(self) -> &'static str {
        match self {
            BevelMode::Cubes => "cubes",
            BevelMode::Edges => "edges",
        }
    }

    pub fn parse(name: &str) -> Option<BevelMode> {
        match name {
            "cubes" => Some(BevelMode::Cubes),
            "edges" => Some(BevelMode::Edges),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bevel {
    /// How far back from the edge the cut starts on each face, in cells.
    pub size: f32,
    pub mode: BevelMode,
}

/// One convex polygon: corners in winding order seen from outside, the
/// outward normal, and the cell's slot.
#[derive(Clone, Debug)]
pub struct Poly {
    pub points: Vec<Vec3>,
    pub normal: Vec3,
    pub slot: u8,
}

/// The twelve edges of a cube: two axes and the side of each.
fn edges() -> impl Iterator<Item = (usize, f32, usize, f32)> {
    let mut out = Vec::with_capacity(12);
    for a in 0..3 {
        for b in a + 1..3 {
            for sa in [-1.0, 1.0] {
                for sb in [-1.0, 1.0] {
                    out.push((a, sa, b, sb));
                }
            }
        }
    }
    out.into_iter()
}

fn unit(axis: usize) -> Vec3 {
    let mut v = Vec3::ZERO;
    v[axis] = 1.0;
    v
}

fn step(cell: Cell, axis: usize, sign: f32) -> Cell {
    let d = if sign > 0.0 { 1 } else { -1 };
    match axis {
        0 => (cell.0 + d, cell.1, cell.2),
        1 => (cell.0, cell.1 + d, cell.2),
        _ => (cell.0, cell.1, cell.2 + d),
    }
}

/// What a polygon of the clipped cube lies on.
#[derive(Clone, Copy, PartialEq)]
enum Tag {
    Face(usize, f32),
    Cut(usize, f32, usize, f32),
}

/// A cube of half extents `half` about the origin, as six faces.
fn cube(half: Vec3) -> Vec<(Vec<Vec3>, Tag)> {
    let mut faces = Vec::with_capacity(6);
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for sign in [-1.0f32, 1.0] {
            let at = |su: f32, sv: f32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign * half[axis];
                p[u] = su * half[u];
                p[v] = sv * half[v];
                p
            };
            // Counter-clockwise seen from outside along +axis; reversed on -axis.
            let mut quad = vec![at(-1.0, -1.0), at(1.0, -1.0), at(1.0, 1.0), at(-1.0, 1.0)];
            if sign < 0.0 {
                quad.reverse();
            }
            faces.push((quad, Tag::Face(axis, sign)));
        }
    }
    faces
}

/// Clip a convex polyhedron (its faces) to `normal · p <= offset`, closing
/// the cut with a new face tagged `tag`.
fn clip(
    faces: Vec<(Vec<Vec3>, Tag)>,
    normal: Vec3,
    offset: f32,
    tag: Tag,
) -> Vec<(Vec<Vec3>, Tag)> {
    const EPS: f32 = 1.0e-6;
    let mut out = Vec::with_capacity(faces.len() + 1);
    let mut cap: Vec<Vec3> = Vec::new();
    for (points, face_tag) in faces {
        let mut kept = Vec::with_capacity(points.len() + 2);
        for i in 0..points.len() {
            let (p, q) = (points[i], points[(i + 1) % points.len()]);
            let (dp, dq) = (normal.dot(p) - offset, normal.dot(q) - offset);
            if dp <= EPS {
                kept.push(p);
                if dp.abs() <= EPS {
                    cap.push(p);
                }
            }
            if (dp < -EPS && dq > EPS) || (dp > EPS && dq < -EPS) {
                let at = p + (q - p) * (dp / (dp - dq));
                kept.push(at);
                cap.push(at);
            }
        }
        if kept.len() >= 3 {
            out.push((kept, face_tag));
        }
    }
    // The cap: the cut's points around their centre, counter-clockwise
    // seen from outside (along `normal`).
    let mut unique: Vec<Vec3> = Vec::new();
    for p in cap {
        if !unique.iter().any(|q| q.distance_squared(p) < 1.0e-10) {
            unique.push(p);
        }
    }
    if unique.len() >= 3 {
        let centre = unique.iter().copied().sum::<Vec3>() / unique.len() as f32;
        let u = normal.any_orthonormal_vector();
        let v = normal.cross(u);
        unique.sort_by(|a, b| {
            let (da, db) = (*a - centre, *b - centre);
            da.dot(v)
                .atan2(da.dot(u))
                .total_cmp(&db.dot(v).atan2(db.dot(u)))
        });
        out.push((unique, tag));
    }
    out
}

/// The cube of half extents `half` with the `cut` edges cut `size` back.
fn cut_cube(
    half: Vec3,
    size: f32,
    cut: impl Fn(usize, f32, usize, f32) -> bool,
) -> Vec<(Vec<Vec3>, Tag)> {
    let mut faces = cube(half);
    for (a, sa, b, sb) in edges() {
        if !cut(a, sa, b, sb) {
            continue;
        }
        let n = unit(a) * sa + unit(b) * sb;
        let offset = half[a] + half[b] - size;
        faces = clip(
            faces,
            n.normalize(),
            offset / std::f32::consts::SQRT_2,
            Tag::Cut(a, sa, b, sb),
        );
    }
    faces
}

/// Whether a cell's edge is cut, by the mode and the cells across it.
fn is_cut(
    solid: &impl Fn(Cell) -> bool,
    mode: BevelMode,
    cell: Cell,
    edge: (usize, f32, usize, f32),
) -> bool {
    match mode {
        // A groove wherever the edge touches open space: an edge inside the
        // solid stays whole, or the grooves would meet in thin tunnels
        // along the lattice that a straight view looks through.
        BevelMode::Cubes => {
            let (a, sa, b, sb) = edge;
            let (n, m) = (step(cell, a, sa), step(cell, b, sb));
            !(solid(n) && solid(m) && solid(step(n, b, sb)))
        }
        BevelMode::Edges => {
            let (a, sa, b, sb) = edge;
            !solid(step(cell, a, sa)) && !solid(step(cell, b, sb))
        }
    }
}

/// The polygons of the `listed` solid cells, in geometric coordinates (cell
/// `(x, y, z)` spans `x..x + 1`), against every cell `solid` says is solid.
pub fn cell_polys(
    solid: impl Fn(Cell) -> bool,
    listed: &[(Cell, u8)],
    bevel: Bevel,
    out: &mut Vec<Poly>,
) {
    let size = bevel.size.clamp(0.0, MAX_BEVEL);
    for &(cell, slot) in listed {
        // A cell with all 26 neighbours solid shows nothing.
        let mut open = false;
        'scan: for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) != (0, 0, 0) && !solid((cell.0 + dx, cell.1 + dy, cell.2 + dz))
                    {
                        open = true;
                        break 'scan;
                    }
                }
            }
        }
        if !open {
            continue;
        }
        let centre = Vec3::new(cell.0 as f32, cell.1 as f32, cell.2 as f32) + Vec3::splat(0.5);
        let polys = cut_cube(Vec3::splat(0.5), size, |a, sa, b, sb| {
            is_cut(&solid, bevel.mode, cell, (a, sa, b, sb))
        });
        for (points, tag) in polys {
            match tag {
                Tag::Face(axis, sign) => {
                    let next = step(cell, axis, sign);
                    let normal = unit(axis) * sign;
                    if !solid(next) {
                        out.push(Poly {
                            points: points.iter().map(|p| *p + centre).collect(),
                            normal,
                            slot,
                        });
                    } else {
                        // The corners the neighbour's cut uncovers on this
                        // face: their cuts along the face's border agree, so
                        // only a corner's cut can differ.
                        for (p, sp, q, sq) in edges() {
                            if p == axis || q == axis {
                                continue;
                            }
                            let edge = (p, sp, q, sq);
                            // A cut along either border at this corner
                            // (both sides have it) already took the corner.
                            let border = |other: usize, side: f32| {
                                let edge = if axis < other {
                                    (axis, sign, other, side)
                                } else {
                                    (other, side, axis, sign)
                                };
                                is_cut(&solid, bevel.mode, cell, edge)
                            };
                            if is_cut(&solid, bevel.mode, cell, edge)
                                || !is_cut(&solid, bevel.mode, next, edge)
                                || border(p, sp)
                                || border(q, sq)
                            {
                                continue;
                            }
                            let corner = normal * 0.5 + unit(p) * (0.5 * sp) + unit(q) * (0.5 * sq);
                            let mut tri = vec![
                                corner,
                                corner - unit(p) * (sp * size),
                                corner - unit(q) * (sq * size),
                            ];
                            let facing = (tri[1] - tri[0]).cross(tri[2] - tri[0]).dot(normal);
                            if facing < 0.0 {
                                tri.swap(1, 2);
                            }
                            out.push(Poly {
                                points: tri.iter().map(|p| *p + centre).collect(),
                                normal,
                                slot,
                            });
                        }
                    }
                }
                Tag::Cut(a, sa, b, sb) => {
                    out.push(Poly {
                        points: points.iter().map(|p| *p + centre).collect(),
                        normal: (unit(a) * sa + unit(b) * sb).normalize(),
                        slot,
                    });
                }
            }
        }
    }
}

/// A strand box with every edge cut `size` back (at most 0.4 of its
/// thinnest side), in the frame its own matrix gives.
pub fn box_polys(b: &crate::tools::StrandBox, size: f32, out: &mut Vec<Poly>) {
    let half = Vec3::from_array(b.half);
    let size = size.clamp(0.0, MAX_BEVEL).min(0.8 * half.min_element());
    let axes = b.axes.map(Vec3::from_array);
    let centre = Vec3::from_array(b.center);
    let to_world = |p: Vec3| centre + axes[0] * p.x + axes[1] * p.y + axes[2] * p.z;
    for (points, tag) in cut_cube(half, size, |_, _, _, _| true) {
        let local = match tag {
            Tag::Face(axis, sign) => unit(axis) * sign,
            Tag::Cut(a, sa, b, sb) => (unit(a) * sa + unit(b) * sb).normalize(),
        };
        out.push(Poly {
            points: points.into_iter().map(to_world).collect(),
            normal: (axes[0] * local.x + axes[1] * local.y + axes[2] * local.z).normalize(),
            slot: b.slot,
        });
    }
}

#[cfg(test)]
#[path = "../tests/bevel.rs"]
mod tests;
