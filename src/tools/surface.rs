//! Shared pieces of the stone tools (`joints` and the surface tools that
//! follow): an inclusive region, the six face directions, how deep a cell
//! lies under the surface, and a smooth value noise.

use std::collections::{HashMap, VecDeque};

use super::debris::hash01;
use super::sculpt::MAX_SCULPT_CELLS;
use crate::state::Cell;

/// The six face directions: +x, -x, +y, -y, +z, -z.
pub(super) const FACES: [Cell; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// An inclusive box of cells, checked against the sculpt cell limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub min: Cell,
    pub max: Cell,
}

impl Region {
    /// The box of two corners in any order, at most `MAX_SCULPT_CELLS` cells.
    pub fn new(a: Cell, b: Cell, tool: &str) -> Result<Region, String> {
        let min = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
        let max = (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2));
        let volume = (i64::from(max.0 - min.0) + 1)
            * (i64::from(max.1 - min.1) + 1)
            * (i64::from(max.2 - min.2) + 1);
        if volume > MAX_SCULPT_CELLS {
            return Err(format!(
                "region is larger than {MAX_SCULPT_CELLS} cells; {tool} a smaller one"
            ));
        }
        Ok(Region { min, max })
    }

    pub fn contains(&self, c: Cell) -> bool {
        (self.min.0..=self.max.0).contains(&c.0)
            && (self.min.1..=self.max.1).contains(&c.1)
            && (self.min.2..=self.max.2).contains(&c.2)
    }

    /// The solid cells inside, sorted so every pass visits them in one order.
    pub(super) fn solid(&self, cells: &HashMap<Cell, u8>) -> Vec<Cell> {
        let mut found: Vec<Cell> = cells
            .keys()
            .copied()
            .filter(|cell| self.contains(*cell))
            .collect();
        found.sort_unstable_by_key(|c| (c.2, c.1, c.0));
        found
    }
}

pub(super) fn step(c: Cell, d: Cell) -> Cell {
    (c.0 + d.0, c.1 + d.1, c.2 + d.2)
}

/// How deep each solid cell of `region` lies under the surface (1 for a
/// cell with an empty neighbour, 2 for one behind it, up to `limit`), with
/// the surface cell it was reached from, so a cut decided on the surface can
/// run straight in. Cells deeper than `limit` are left out. Empty cells
/// outside the region count, so a region's edge inside a solid wall is not
/// a surface.
pub(super) fn depths_from(
    cells: &HashMap<Cell, u8>,
    region: &Region,
    limit: u32,
) -> HashMap<Cell, (u32, Cell)> {
    let mut depth: HashMap<Cell, (u32, Cell)> = HashMap::new();
    let mut queue = VecDeque::new();
    for cell in region.solid(cells) {
        if FACES.iter().any(|d| !cells.contains_key(&step(cell, *d))) {
            depth.insert(cell, (1, cell));
            queue.push_back(cell);
        }
    }
    while let Some(cell) = queue.pop_front() {
        let (at, origin) = depth[&cell];
        if at + 1 > limit {
            continue;
        }
        for d in FACES {
            let n = step(cell, d);
            if region.contains(n) && cells.contains_key(&n) && !depth.contains_key(&n) {
                depth.insert(n, (at + 1, origin));
                queue.push_back(n);
            }
        }
    }
    depth
}

/// The outward surface direction near `c`: the mean offset of the empty
/// cells within two cells. `None` deep inside, where nothing is empty.
pub(super) fn surface_normal(cells: &HashMap<Cell, u8>, c: Cell) -> Option<[f64; 3]> {
    let mut sum = [0.0f64; 3];
    for dz in -2..=2 {
        for dy in -2..=2 {
            for dx in -2..=2 {
                if !cells.contains_key(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                    sum[0] += f64::from(dx);
                    sum[1] += f64::from(dy);
                    sum[2] += f64::from(dz);
                }
            }
        }
    }
    let len = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
    (len > 1.0e-9).then(|| [sum[0] / len, sum[1] / len, sum[2] / len])
}

/// Smooth value noise in 0..1 with features about `scale` cells wide.
pub(super) fn noise3(p: [f64; 3], scale: f64, seed: u64) -> f64 {
    let p = [p[0] / scale, p[1] / scale, p[2] / scale];
    let base = [p[0].floor(), p[1].floor(), p[2].floor()];
    let t = [
        fade(p[0] - base[0]),
        fade(p[1] - base[1]),
        fade(p[2] - base[2]),
    ];
    let at = |dx: i64, dy: i64, dz: i64| {
        hash01(
            base[0] as i64 + dx,
            base[1] as i64 + dy,
            base[2] as i64 + dz,
            seed,
        )
    };
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(at(0, 0, 0), at(1, 0, 0), t[0]);
    let x10 = lerp(at(0, 1, 0), at(1, 1, 0), t[0]);
    let x01 = lerp(at(0, 0, 1), at(1, 0, 1), t[0]);
    let x11 = lerp(at(0, 1, 1), at(1, 1, 1), t[0]);
    lerp(lerp(x00, x10, t[1]), lerp(x01, x11, t[1]), t[2])
}

fn fade(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// A stable 0..1 value for one cell and a salt.
pub(super) fn cell01(c: Cell, seed: u64, salt: u64) -> f64 {
    hash01(
        i64::from(c.0),
        i64::from(c.1),
        i64::from(c.2),
        seed ^ salt.wrapping_mul(0x5851_f42d_4c95_7f2d),
    )
}

/// Cells whose slot is listed, or every cell when `only` is empty.
pub(super) fn allowed(only: &[u8], slot: u8) -> bool {
    only.is_empty() || only.contains(&slot)
}

pub(super) fn check_slot(slot: u8, what: &str) -> Result<(), String> {
    if slot == 0 {
        return Err(format!("{what} must be a palette index 1..=255"));
    }
    Ok(())
}

pub(super) fn check_slots(slots: &[u8], what: &str) -> Result<(), String> {
    if slots.contains(&0) {
        return Err(format!("{what} must hold palette indices 1..=255"));
    }
    Ok(())
}

pub(super) fn check_unit(value: f64, what: &str) -> Result<(), String> {
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("{what} must be 0..=1"));
    }
    Ok(())
}
