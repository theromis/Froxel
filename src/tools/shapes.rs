//! Brush shapes: line, circle, square, box, flood, and extrude.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::state::Cell;

use super::*;

/// The bevel actually used for a box of `dims`: at most half the shortest side.
pub fn rounded_radius(radius: f64, dims: [f64; 3]) -> f64 {
    radius.min(dims[0].min(dims[1]).min(dims[2]) * 0.5)
}

/// Whether `point` lies in the box `min..max` with edges rounded by
/// `radius`: its distance to the box inset by `radius` is at most `radius`.
pub fn rounded_inside(point: [f64; 3], min: [f64; 3], max: [f64; 3], radius: f64) -> bool {
    let mut dist2 = 0.0;
    for axis in 0..3 {
        let low = min[axis] + radius;
        let high = max[axis] - radius;
        let delta = if point[axis] < low {
            low - point[axis]
        } else if point[axis] > high {
            point[axis] - high
        } else {
            0.0
        };
        dist2 += delta * delta;
    }
    dist2 <= radius * radius + 1.0e-9
}

pub fn bresenham(a: Cell, b: Cell) -> Vec<Cell> {
    let (mut x, mut y, mut z) = a;
    let dx = (b.0 - a.0).abs();
    let dy = (b.1 - a.1).abs();
    let dz = (b.2 - a.2).abs();
    let sx = if a.0 < b.0 { 1 } else { -1 };
    let sy = if a.1 < b.1 { 1 } else { -1 };
    let sz = if a.2 < b.2 { 1 } else { -1 };
    let dm = dx.max(dy).max(dz);
    let mut ex = dm / 2;
    let mut ey = dm / 2;
    let mut ez = dm / 2;
    let mut out = Vec::with_capacity(dm as usize + 1);
    out.push((x, y, z));
    for _ in 0..dm {
        ex -= dx;
        if ex < 0 {
            ex += dm;
            x += sx;
        }
        ey -= dy;
        if ey < 0 {
            ey += dm;
            y += sy;
        }
        ez -= dz;
        if ez < 0 {
            ez += dm;
            z += sz;
        }
        out.push((x, y, z));
    }
    out
}

pub fn circle(anchor: Cell, end: Cell, axis: i32, fill: bool, even: bool) -> Vec<Cell> {
    disc(anchor, end, axis, fill, even, true)
}

pub fn square(anchor: Cell, end: Cell, axis: i32, fill: bool, even: bool) -> Vec<Cell> {
    disc(anchor, end, axis, fill, even, false)
}

fn disc(anchor: Cell, end: Cell, axis: i32, fill: bool, even: bool, round: bool) -> Vec<Cell> {
    let (au, av) = project(axis, anchor);
    let (eu, ev) = project(axis, end);
    let du = eu - au;
    let dv = ev - av;
    let c = if even { 0.5 } else { 0.0 };
    let (radius, bound) = if round {
        let fdu = du as f32 - c;
        let fdv = dv as f32 - c;
        let radius2 = fdu * fdu + fdv * fdv;
        (radius2.sqrt(), radius2.sqrt().ceil() as i32 + 1)
    } else {
        let half = (du as f32 - c).abs().max((dv as f32 - c).abs());
        (half, half.ceil() as i32 + 1)
    };
    let radius2 = if round {
        let fdu = du as f32 - c;
        let fdv = dv as f32 - c;
        fdu * fdu + fdv * fdv
    } else {
        0.0
    };
    let half = if round { 0.0 } else { radius };
    let inside = |i: i32, j: i32| {
        let fi = i as f32 - c;
        let fj = j as f32 - c;
        if round {
            fi * fi + fj * fj <= radius2
        } else {
            fi.abs().max(fj.abs()) <= half
        }
    };
    let bound = bound.clamp(0, 256);
    let mut out = Vec::new();
    for j in -bound..=bound {
        for i in -bound..=bound {
            if !inside(i, j) {
                continue;
            }
            if !fill && inside(i + 1, j) && inside(i - 1, j) && inside(i, j + 1) && inside(i, j - 1)
            {
                continue;
            }
            out.push(offset(axis, anchor, i, j));
        }
    }
    out
}

pub fn cube(a: Cell, b: Cell, fill: bool) -> Vec<Cell> {
    let x0 = a.0.min(b.0);
    let x1 = a.0.max(b.0);
    let y0 = a.1.min(b.1);
    let y1 = a.1.max(b.1);
    let z0 = a.2.min(b.2);
    let z1 = a.2.max(b.2);
    let nx = x1 - x0 + 1;
    let ny = y1 - y0 + 1;
    let nz = z1 - z0 + 1;
    let thin = nx == 1 || ny == 1 || nz == 1;
    let mut out = Vec::new();
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let shell = x == x0 || x == x1 || y == y0 || y == y1 || z == z0 || z == z1;
                if fill || thin || shell {
                    out.push((x, y, z));
                }
            }
        }
    }
    out
}

/// Exposed face connected to `seed`. `match_color == 0` accepts any
/// occupied cell. The walk stays inside the volume's in-plane bounds.
pub fn flood_face(
    cells: &HashMap<Cell, u8>,
    seed: Cell,
    axis: i32,
    sign: i32,
    match_color: u8,
    eight: bool,
) -> Vec<Cell> {
    let Some(&seed_slot) = cells.get(&seed) else {
        return Vec::new();
    };
    if match_color != 0 && seed_slot != match_color {
        return Vec::new();
    }
    let normal = axis_delta(axis, sign);
    if cells.contains_key(&add(seed, normal)) {
        return Vec::new();
    }
    let (u_axis, v_axis) = plane_axes(axis);
    let mut u_min = i32::MAX;
    let mut u_max = i32::MIN;
    let mut v_min = i32::MAX;
    let mut v_max = i32::MIN;
    for cell in cells.keys() {
        let u = coord(*cell, u_axis);
        let v = coord(*cell, v_axis);
        u_min = u_min.min(u);
        u_max = u_max.max(u);
        v_min = v_min.min(v);
        v_max = v_max.max(v);
    }
    let qualifies = |cell: Cell| {
        let Some(&slot) = cells.get(&cell) else {
            return false;
        };
        if match_color != 0 && slot != match_color {
            return false;
        }
        !cells.contains_key(&add(cell, normal))
    };
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut out = Vec::new();
    queue.push_back(seed);
    seen.insert(seed);
    while let Some(cell) = queue.pop_front() {
        if !qualifies(cell) {
            continue;
        }
        out.push(cell);
        if out.len() >= 100_000 {
            break;
        }
        for (du, dv) in neighbors(eight) {
            let next = set_plane(cell, axis, u_axis, v_axis, du, dv);
            let u = coord(next, u_axis);
            let v = coord(next, v_axis);
            if u < u_min || u > u_max || v < v_min || v > v_max {
                continue;
            }
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    out
}

/// Empty cells on the wall plane through `seed`, inside the inclusive
/// in-plane bounds. The seed must be empty. Stops at occupied cells.
pub fn flood_empty(
    cells: &HashMap<Cell, u8>,
    seed: Cell,
    axis: i32,
    u_min: i32,
    u_max: i32,
    v_min: i32,
    v_max: i32,
    eight: bool,
) -> Vec<Cell> {
    if cells.contains_key(&seed) {
        return Vec::new();
    }
    let (u_axis, v_axis) = plane_axes(axis);
    let u = coord(seed, u_axis);
    let v = coord(seed, v_axis);
    if u < u_min || u > u_max || v < v_min || v > v_max {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut out = Vec::new();
    queue.push_back(seed);
    seen.insert(seed);
    while let Some(cell) = queue.pop_front() {
        if cells.contains_key(&cell) {
            continue;
        }
        out.push(cell);
        if out.len() >= 100_000 {
            break;
        }
        for (du, dv) in neighbors(eight) {
            let next = set_plane(cell, axis, u_axis, v_axis, du, dv);
            let u = coord(next, u_axis);
            let v = coord(next, v_axis);
            if u < u_min || u > u_max || v < v_min || v > v_max {
                continue;
            }
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    out
}

pub fn extrude(seeds: &[Cell], axis: i32, sign: i32, steps: i32, outward: bool) -> Vec<Cell> {
    if steps == 0 || seeds.is_empty() {
        return Vec::new();
    }
    let n = steps.unsigned_abs() as i32;
    let dir = if outward { sign } else { -sign };
    let mut out = Vec::with_capacity(seeds.len() * n as usize);
    let start = if outward { 1 } else { 0 };
    for k in start..start + n {
        let delta = axis_delta(axis, dir * k);
        for seed in seeds {
            out.push(add(*seed, delta));
        }
    }
    out
}

fn project(axis: i32, cell: Cell) -> (i32, i32) {
    match axis {
        0 => (cell.1, cell.2),
        1 => (cell.0, cell.2),
        _ => (cell.0, cell.1),
    }
}

fn offset(axis: i32, anchor: Cell, i: i32, j: i32) -> Cell {
    match axis {
        0 => (anchor.0, anchor.1 + i, anchor.2 + j),
        1 => (anchor.0 + i, anchor.1, anchor.2 + j),
        _ => (anchor.0 + i, anchor.1 + j, anchor.2),
    }
}

fn set_plane(cell: Cell, axis: i32, u_axis: usize, v_axis: usize, du: i32, dv: i32) -> Cell {
    let mut next = cell;
    let u = coord(cell, u_axis) + du;
    let v = coord(cell, v_axis) + dv;
    next = put(next, u_axis, u);
    next = put(next, v_axis, v);
    let _ = axis;
    next
}

fn put(cell: Cell, axis: usize, value: i32) -> Cell {
    match axis {
        0 => (value, cell.1, cell.2),
        1 => (cell.0, value, cell.2),
        _ => (cell.0, cell.1, value),
    }
}

fn neighbors(eight: bool) -> Vec<(i32, i32)> {
    let mut n = vec![(1, 0), (-1, 0), (0, 1), (0, -1)];
    if eight {
        n.extend([(1, 1), (1, -1), (-1, 1), (-1, -1)]);
    }
    n
}
