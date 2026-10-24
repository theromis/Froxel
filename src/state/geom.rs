//! Cell bounds, raycast, and the demo tree.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::Vec3;

use super::*;

pub(super) fn ensure_vox_extension(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("vox") => path,
        _ => path.with_extension("vox"),
    }
}

pub(super) fn rot90(p: Cell, axis: Axis, clockwise: bool) -> Cell {
    let (x, y, z) = p;
    match (axis, clockwise) {
        (Axis::Z, false) => (-y, x, z),
        (Axis::Z, true) => (y, -x, z),
        (Axis::X, false) => (x, -z, y),
        (Axis::X, true) => (x, z, -y),
        (Axis::Y, false) => (z, y, -x),
        (Axis::Y, true) => (-z, y, x),
    }
}

pub fn palette_slot_at(col: i32, row: i32) -> u8 {
    if !(0..8).contains(&col) || !(0..32).contains(&row) {
        return 0;
    }
    let n = (32 - 1 - row) * 8 + col + 1;
    if (1..256).contains(&n) {
        n as u8
    } else {
        0
    }
}

pub(super) fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t + 0.5).clamp(0.0, 255.0) as u8;
    [
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ]
}

pub(crate) fn occupied_bounds(cells: &HashMap<Cell, u8>) -> Option<(Cell, Cell)> {
    cell_bounds(cells.keys().copied())
}

/// SIZE box in local cells. A zero `dims` means "tight around the voxels".
/// An explicit box stays put when it is larger, and grows if a voxel falls
/// outside it.
pub(crate) fn resolved_box(volume: &Volume) -> Option<(Cell, [i32; 3])> {
    let occupied = occupied_bounds(&volume.cells);
    let explicit = volume.dims.0 > 0 && volume.dims.1 > 0 && volume.dims.2 > 0;
    if !explicit {
        let (min, max) = occupied?;
        return Some((
            min,
            [max.0 - min.0 + 1, max.1 - min.1 + 1, max.2 - min.2 + 1],
        ));
    }
    let mut min = volume.box_min;
    let mut dims = [volume.dims.0, volume.dims.1, volume.dims.2];
    if let Some((lo, hi)) = occupied {
        for axis in 0..3 {
            let low = [lo.0, lo.1, lo.2][axis];
            let high = [hi.0, hi.1, hi.2][axis];
            let mut corner = [min.0, min.1, min.2][axis];
            if low < corner {
                dims[axis] += corner - low;
                corner = low;
            }
            if high + 1 > corner + dims[axis] {
                dims[axis] = high + 1 - corner;
            }
            match axis {
                0 => min.0 = corner,
                1 => min.1 = corner,
                _ => min.2 = corner,
            }
        }
    }
    Some((min, dims))
}

pub(super) fn cell_bounds(cells: impl IntoIterator<Item = Cell>) -> Option<(Cell, Cell)> {
    let mut iter = cells.into_iter();
    let first = iter.next()?;
    let mut min = first;
    let mut max = first;
    for cell in iter {
        min.0 = min.0.min(cell.0);
        min.1 = min.1.min(cell.1);
        min.2 = min.2.min(cell.2);
        max.0 = max.0.max(cell.0);
        max.1 = max.1.max(cell.1);
        max.2 = max.2.max(cell.2);
    }
    Some((min, max))
}

pub(super) fn in_world(x: i32, y: i32, z: i32) -> bool {
    (-1024..1024).contains(&x) && (-1024..1024).contains(&y) && (-1024..1024).contains(&z)
}

pub(super) fn beside(volume: &Volume) -> Cell {
    let mut min_x = 0;
    let mut max_x = 0;
    let mut any = false;
    for cell in volume.cells.keys() {
        if !any {
            min_x = cell.0;
            max_x = cell.0;
            any = true;
        } else {
            min_x = min_x.min(cell.0);
            max_x = max_x.max(cell.0);
        }
    }
    let span = if any { max_x - min_x + 2 } else { 2 };
    (
        volume.translation.0 + span,
        volume.translation.1,
        volume.translation.2,
    )
}

pub(super) fn min_corner(cells: impl IntoIterator<Item = Cell>) -> Cell {
    let mut corner = (i32::MAX, i32::MAX, i32::MAX);
    for c in cells {
        corner.0 = corner.0.min(c.0);
        corner.1 = corner.1.min(c.1);
        corner.2 = corner.2.min(c.2);
    }
    corner
}

pub(super) fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

pub(super) fn world_extents(volume: &Volume) -> Option<(Cell, Cell)> {
    let (min, max) = cell_bounds(volume.cells.keys().copied())?;
    let shift = volume.translation;
    Some((
        (min.0 + shift.0, min.1 + shift.1, min.2 + shift.2),
        (max.0 + shift.0, max.1 + shift.1, max.2 + shift.2),
    ))
}

/// Integer center of an inclusive span: floor((lo + hi + 1) / 2).
pub(super) fn span_center(lo: i32, hi: i32) -> i32 {
    let s = lo as i64 + hi as i64 + 1;
    let center = if s >= 0 { s / 2 } else { -((-s + 1) / 2) };
    center as i32
}

pub(super) fn extents(cells: impl IntoIterator<Item = Cell>) -> (Cell, Cell) {
    let mut min = (i32::MAX, i32::MAX, i32::MAX);
    let mut max = (i32::MIN, i32::MIN, i32::MIN);
    for c in cells {
        min.0 = min.0.min(c.0);
        min.1 = min.1.min(c.1);
        min.2 = min.2.min(c.2);
        max.0 = max.0.max(c.0);
        max.1 = max.1.max(c.1);
        max.2 = max.2.max(c.2);
    }
    (min, max)
}

pub(super) fn mirror(c: Cell, axis: Axis, min: Cell, max: Cell) -> Cell {
    match axis {
        Axis::X => (min.0 + max.0 - c.0, c.1, c.2),
        Axis::Y => (c.0, min.1 + max.1 - c.1, c.2),
        Axis::Z => (c.0, c.1, min.2 + max.2 - c.2),
    }
}

pub(super) fn cells_in_aabb(cells: &HashMap<Cell, u8>, a: Cell, b: Cell) -> HashSet<Cell> {
    let min = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
    let max = (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2));
    cells
        .keys()
        .copied()
        .filter(|c| {
            c.0 >= min.0
                && c.0 <= max.0
                && c.1 >= min.1
                && c.1 <= max.1
                && c.2 >= min.2
                && c.2 <= max.2
        })
        .collect()
}

pub(super) fn flood_same(cells: &HashMap<Cell, u8>, seed: Cell, slot: u8) -> HashSet<Cell> {
    let mut seen = HashSet::new();
    let mut out = HashSet::new();
    let mut queue = vec![seed];
    seen.insert(seed);
    const STEPS: [Cell; 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    while let Some(cell) = queue.pop() {
        out.insert(cell);
        for (dx, dy, dz) in STEPS {
            let next = (cell.0 + dx, cell.1 + dy, cell.2 + dz);
            if !seen.insert(next) {
                continue;
            }
            if cells.get(&next).copied() == Some(slot) {
                queue.push(next);
            }
        }
    }
    out
}

pub(super) fn intersect_plane(origin: Vec3, dir: Vec3, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    let denom = normal.dot(dir);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = normal.dot(point - origin) / denom;
    if t < 0.0 {
        return None;
    }
    Some(origin + dir * t)
}

pub(super) fn intersect_axis_plane(origin: Vec3, dir: Vec3, axis: i32, plane: f32) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 1e-8 {
        return None;
    }
    let (along, component) = match axis {
        0 => (origin.x, dir.x),
        1 => (origin.y, dir.y),
        _ => (origin.z, dir.z),
    };
    if component.abs() < 1e-6 {
        return None;
    }
    let t = (plane - along) / component;
    if t < 0.0 {
        return None;
    }
    Some(origin + dir * t)
}

pub(super) fn plane_delta(axis: i32, anchor: Vec3, hit: Vec3) -> Cell {
    (
        if axis == 0 {
            0
        } else {
            (hit.x - anchor.x).round() as i32
        },
        if axis == 1 {
            0
        } else {
            (hit.y - anchor.y).round() as i32
        },
        if axis == 2 {
            0
        } else {
            (hit.z - anchor.z).round() as i32
        },
    )
}

pub(super) fn translate_cells(
    base: &HashMap<Cell, u8>,
    picked: &[Cell],
    delta: Cell,
) -> (HashMap<Cell, u8>, HashSet<Cell>) {
    let chosen: HashSet<Cell> = picked.iter().copied().collect();
    let mut next = HashMap::with_capacity(base.len());
    for (cell, slot) in base {
        if !chosen.contains(cell) {
            next.insert(*cell, *slot);
        }
    }
    let mut moved = HashSet::with_capacity(picked.len());
    for cell in picked {
        let Some(&slot) = base.get(cell) else {
            continue;
        };
        let at = (cell.0 + delta.0, cell.1 + delta.1, cell.2 + delta.2);
        next.insert(at, slot);
        moved.insert(at);
    }
    (next, moved)
}

pub(super) fn face_axis(normal: Cell) -> (i32, i32) {
    if normal.0 != 0 {
        (0, normal.0.signum())
    } else if normal.1 != 0 {
        (1, normal.1.signum())
    } else {
        (2, normal.2.signum())
    }
}

pub(super) fn plane_hit(origin: Vec3, dir: Vec3, anchor: Hit) -> Option<Hit> {
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 1e-8 {
        return None;
    }
    let (axis, sign) = face_axis(anchor.normal);
    let along = match axis {
        0 => origin.x,
        1 => origin.y,
        _ => origin.z,
    };
    let component = match axis {
        0 => dir.x,
        1 => dir.y,
        _ => dir.z,
    };
    if component.abs() < 1e-6 {
        return None;
    }
    let plane = match axis {
        0 => anchor.cell.0 as f32 + if sign > 0 { 1.0 } else { 0.0 },
        1 => anchor.cell.1 as f32 + if sign > 0 { 1.0 } else { 0.0 },
        _ => anchor.cell.2 as f32 + if sign > 0 { 1.0 } else { 0.0 },
    };
    let t = (plane - along) / component;
    if t < 0.0 {
        return None;
    }
    let point = origin + dir * t;
    let mut cell = (
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    );
    let solid = if sign > 0 {
        plane as i32 - 1
    } else {
        plane as i32
    };
    match axis {
        0 => cell.0 = solid,
        1 => cell.1 = solid,
        _ => cell.2 = solid,
    }
    Some(Hit {
        cell,
        normal: anchor.normal,
    })
}

/// Closest forward hit on the six walls of the occupied AABB. The returned
/// normal points into the volume, and the cell is the empty (or occupied)
/// cell on the inside of that wall.
pub(super) fn wall_hit(cells: &HashMap<Cell, u8>, origin: Vec3, dir: Vec3) -> Option<Hit> {
    let (min, max) = cell_bounds(cells.keys().copied())?;
    let walls = [
        [min.0 as f32, max.0 as f32 + 1.0],
        [min.1 as f32, max.1 as f32 + 1.0],
        [min.2 as f32, max.2 as f32 + 1.0],
    ];
    let ro = [origin.x, origin.y, origin.z];
    let rd = [dir.x, dir.y, dir.z];
    let mut best_t = f32::INFINITY;
    let mut best_axis = -1i32;
    let mut best_sign = 0i32;
    let mut best_h = [0.0f32; 3];
    for axis in 0..3 {
        if rd[axis].abs() < 1e-6 {
            continue;
        }
        for side in 0..2 {
            let plane = walls[axis][side];
            if side == 0 && ro[axis] < plane {
                continue;
            }
            if side == 1 && ro[axis] > plane {
                continue;
            }
            let t = (plane - ro[axis]) / rd[axis];
            if t < 0.0 || t > 4096.0 || t >= best_t {
                continue;
            }
            let hit = [ro[0] + t * rd[0], ro[1] + t * rd[1], ro[2] + t * rd[2]];
            let mut inside = true;
            for other in 0..3 {
                if other == axis {
                    continue;
                }
                if hit[other] < walls[other][0] || hit[other] > walls[other][1] {
                    inside = false;
                }
            }
            if !inside {
                continue;
            }
            best_t = t;
            best_axis = axis as i32;
            best_sign = if side == 0 { -1 } else { 1 };
            best_h = hit;
        }
    }
    if best_axis < 0 {
        return None;
    }
    let lo = [min.0, min.1, min.2];
    let hi = [max.0, max.1, max.2];
    let mut cell = [0i32; 3];
    for axis in 0..3 {
        cell[axis] = (best_h[axis].floor() as i32).clamp(lo[axis], hi[axis]);
    }
    let axis = best_axis as usize;
    cell[axis] = if best_sign < 0 { lo[axis] } else { hi[axis] };
    let wall_sign = if cell[axis] == lo[axis] { -1 } else { 1 };
    Some(Hit {
        cell: (cell[0], cell[1], cell[2]),
        normal: crate::tools::axis_delta(best_axis, -wall_sign),
    })
}

pub(super) fn map_footprint(offsets: &[Cell], axis: i32) -> Vec<Cell> {
    offsets
        .iter()
        .map(|(u, v, w)| match axis {
            0 => (*w, *u, *v),
            1 => (*u, *w, *v),
            _ => (*u, *v, *w),
        })
        .collect()
}

pub(super) fn expand_centers(centers: &[Cell], offsets: &[Cell], axis: i32) -> Vec<Cell> {
    let mapped = map_footprint(offsets, axis);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for center in centers {
        for delta in &mapped {
            let cell = crate::tools::add(*center, *delta);
            if seen.insert(cell) {
                out.push(cell);
            }
        }
    }
    out
}

pub(super) fn footprint(size: i32, shape: BrushShape, three_d: bool) -> Vec<Cell> {
    let edge = size.clamp(1, 63);
    let lo = -((edge - 1) / 2);
    let hi = edge / 2;
    let c = (lo + hi) as f32 * 0.5;
    let r2 = (edge as f32 * 0.5) * (edge as f32 * 0.5);
    let mut out = Vec::new();
    for w in lo..=hi {
        if !three_d && w != 0 {
            continue;
        }
        for v in lo..=hi {
            for u in lo..=hi {
                if shape == BrushShape::Circle {
                    let fu = u as f32 - c;
                    let fv = v as f32 - c;
                    let fw = if three_d { w as f32 - c } else { 0.0 };
                    if fu * fu + fv * fv + fw * fw > r2 {
                        continue;
                    }
                }
                out.push((u, v, w));
            }
        }
    }
    out
}

pub fn raycast(cells: &HashMap<Cell, u8>, origin: Vec3, dir: Vec3) -> Option<Hit> {
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 1e-8 {
        return None;
    }
    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;
    let step = |d: f32| if d >= 0.0 { 1 } else { -1 };
    let step_x = step(dir.x);
    let step_y = step(dir.y);
    let step_z = step(dir.z);
    let t_delta = |d: f32| {
        if d.abs() < 1e-8 {
            f32::INFINITY
        } else {
            (1.0 / d).abs()
        }
    };
    let t_max = |origin: f32, cell: i32, d: f32, step: i32| {
        if d.abs() < 1e-8 {
            f32::INFINITY
        } else if step > 0 {
            (cell as f32 + 1.0 - origin) / d
        } else {
            (cell as f32 - origin) / d
        }
    };
    let mut t_max_x = t_max(origin.x, x, dir.x, step_x);
    let mut t_max_y = t_max(origin.y, y, dir.y, step_y);
    let mut t_max_z = t_max(origin.z, z, dir.z, step_z);
    let t_delta_x = t_delta(dir.x);
    let t_delta_y = t_delta(dir.y);
    let t_delta_z = t_delta(dir.z);

    if cells.contains_key(&(x, y, z)) {
        return Some(Hit {
            cell: (x, y, z),
            normal: dominant_normal(-dir),
        });
    }

    for _ in 0..768 {
        let travel = t_max_x.min(t_max_y).min(t_max_z);
        if travel > 400.0 {
            break;
        }
        let normal = if t_max_x <= t_max_y && t_max_x <= t_max_z {
            x += step_x;
            t_max_x += t_delta_x;
            (-step_x, 0, 0)
        } else if t_max_y <= t_max_z {
            y += step_y;
            t_max_y += t_delta_y;
            (0, -step_y, 0)
        } else {
            z += step_z;
            t_max_z += t_delta_z;
            (0, 0, -step_z)
        };
        if cells.contains_key(&(x, y, z)) {
            return Some(Hit {
                cell: (x, y, z),
                normal,
            });
        }
    }
    None
}

pub(super) fn dominant_normal(dir: Vec3) -> Cell {
    let ax = dir.x.abs();
    let ay = dir.y.abs();
    let az = dir.z.abs();
    if ax >= ay && ax >= az {
        (dir.x.signum() as i32, 0, 0)
    } else if ay >= az {
        (0, dir.y.signum() as i32, 0)
    } else {
        (0, 0, dir.z.signum() as i32)
    }
}

pub(super) fn demo_tree() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for y in -7..=7 {
        for x in -7..=7 {
            if x * x + y * y <= 49 {
                cells.insert((x, y, 0), 250);
            }
        }
    }
    for z in 1..=6 {
        cells.insert((0, 0, z), 100);
        cells.insert((1, 0, z), 100);
        cells.insert((0, 1, z), 100);
    }
    let r = 5;
    for z in -r..=r {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y + z * z <= r * r {
                    cells.insert((x, y, z + 9), 150);
                }
            }
        }
    }
    cells
}
