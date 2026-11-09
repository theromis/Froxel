//! MCP `split_parts`: cut one blockout into its rig parts by seed points.
//!
//! Every solid cell goes to the part whose seed is nearest along a path
//! through the solid (geodesic distance: a multi-source Dijkstra over the
//! cells, 26- or 6-connected), so a jaw seed does not claim the cheek on
//! the far side of the mouth and a leg seed stops where the leg meets the
//! body. Euclidean distance is the other metric, and the fallback for cells
//! no path reaches. A part's optional cutting planes keep its wave on one
//! side of each plane, so a neck or a tail root can be cut at a set place.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::state::Cell;

/// Most parts in one call.
pub const MAX_PARTS: usize = 64;
/// Most seed points of one part.
pub const MAX_PART_POINTS: usize = 64;

/// A half-space a part keeps: cells whose centre `c` has
/// `(c - at) · normal >= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: [f64; 3],
    pub at: [f64; 3],
}

impl Plane {
    fn keeps(&self, cell: Cell) -> bool {
        let c = [
            f64::from(cell.0) + 0.5,
            f64::from(cell.1) + 0.5,
            f64::from(cell.2) + 0.5,
        ];
        (0..3)
            .map(|a| (c[a] - self.at[a]) * self.normal[a])
            .sum::<f64>()
            >= -1e-9
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartSeed {
    pub name: String,
    /// Points in the cells' frame; each snaps to the nearest solid cell.
    pub points: Vec<[f64; 3]>,
    pub planes: Vec<Plane>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Metric {
    Geodesic,
    Euclidean,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RigSplitSpec {
    pub parts: Vec<PartSeed>,
    pub metric: Metric,
    /// 6 or 26.
    pub connectivity: u32,
    /// How far, in cells, a seed point may snap to a solid cell.
    pub reach: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RigSplit {
    /// The part of every cell, an index into `spec.parts`.
    pub labels: HashMap<Cell, usize>,
    /// Each part's snapped seed cells.
    pub seeds: Vec<Vec<Cell>>,
    /// Cells per part.
    pub counts: Vec<usize>,
    /// Cells no geodesic path reached (assigned by Euclidean distance).
    pub unreached: usize,
}

#[derive(PartialEq)]
struct Item(f64, Cell, usize);

impl Eq for Item {}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        // A min-heap on distance, ties by cell and part for determinism.
        other
            .0
            .total_cmp(&self.0)
            .then_with(|| other.1.cmp(&self.1))
            .then_with(|| other.2.cmp(&self.2))
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn centre(cell: Cell) -> [f64; 3] {
    [
        f64::from(cell.0) + 0.5,
        f64::from(cell.1) + 0.5,
        f64::from(cell.2) + 0.5,
    ]
}

fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// The nearest solid cell to `point` (a cell's centre is its index plus a
/// half) within `reach` cells.
fn snap(cells: &HashMap<Cell, u8>, point: [f64; 3], reach: f64) -> Option<Cell> {
    let home = (
        point[0].floor() as i32,
        point[1].floor() as i32,
        point[2].floor() as i32,
    );
    if cells.contains_key(&home) {
        return Some(home);
    }
    let r = reach.ceil() as i32;
    let mut best: Option<(f64, Cell)> = None;
    for dz in -r..=r {
        for dy in -r..=r {
            for dx in -r..=r {
                let c = (home.0 + dx, home.1 + dy, home.2 + dz);
                if !cells.contains_key(&c) {
                    continue;
                }
                let d = dist2(centre(c), point);
                if d <= reach * reach + 1e-9
                    && best.is_none_or(|(bd, bc)| d < bd || (d == bd && c < bc))
                {
                    best = Some((d, c));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Assign every cell of `cells` to a part.
pub fn split_parts(cells: &HashMap<Cell, u8>, spec: &RigSplitSpec) -> Result<RigSplit, String> {
    if spec.parts.is_empty() || spec.parts.len() > MAX_PARTS {
        return Err(format!("parts must hold 1 to {MAX_PARTS} parts"));
    }
    if cells.is_empty() {
        return Err("the object has no cells to split".into());
    }
    if spec.connectivity != 6 && spec.connectivity != 26 {
        return Err("connectivity must be 6 or 26".into());
    }
    if !(0.0..=64.0).contains(&spec.reach) {
        return Err("reach must be 0..64 cells".into());
    }
    let mut seeds: Vec<Vec<Cell>> = Vec::with_capacity(spec.parts.len());
    for (i, part) in spec.parts.iter().enumerate() {
        if spec.parts[..i].iter().any(|p| p.name == part.name) {
            return Err(format!("part `{}` is named twice", part.name));
        }
        if part.points.is_empty() || part.points.len() > MAX_PART_POINTS {
            return Err(format!(
                "part `{}` needs 1 to {MAX_PART_POINTS} seed points",
                part.name
            ));
        }
        let mut list = Vec::new();
        for p in &part.points {
            let cell = snap(cells, *p, spec.reach).ok_or_else(|| {
                format!(
                    "part `{}`: no solid cell within {} cells of [{}, {}, {}]",
                    part.name, spec.reach, p[0], p[1], p[2]
                )
            })?;
            if !list.contains(&cell) {
                list.push(cell);
            }
        }
        seeds.push(list);
    }
    let allowed = |part: usize, cell: Cell| spec.parts[part].planes.iter().all(|p| p.keeps(cell));
    let mut labels: HashMap<Cell, usize> = HashMap::with_capacity(cells.len());
    let mut unreached = 0;
    if spec.metric == Metric::Geodesic {
        let mut steps: Vec<((i32, i32, i32), f64)> = Vec::new();
        for dz in -1i32..=1 {
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let n = dx.abs() + dy.abs() + dz.abs();
                    if n == 0 || (spec.connectivity == 6 && n > 1) {
                        continue;
                    }
                    steps.push(((dx, dy, dz), f64::from(n).sqrt()));
                }
            }
        }
        let mut best: HashMap<Cell, f64> = HashMap::with_capacity(cells.len());
        let mut heap = BinaryHeap::new();
        for (part, list) in seeds.iter().enumerate() {
            for cell in list {
                heap.push(Item(0.0, *cell, part));
            }
        }
        while let Some(Item(d, cell, part)) = heap.pop() {
            if labels.contains_key(&cell) {
                continue;
            }
            labels.insert(cell, part);
            for (step, cost) in &steps {
                let next = (cell.0 + step.0, cell.1 + step.1, cell.2 + step.2);
                if !cells.contains_key(&next) || labels.contains_key(&next) {
                    continue;
                }
                if !allowed(part, next) {
                    continue;
                }
                let nd = d + cost;
                if best.get(&next).is_none_or(|b| nd < *b - 1e-12) {
                    best.insert(next, nd);
                    heap.push(Item(nd, next, part));
                }
            }
        }
    }
    // Euclidean: every cell, or the cells no path reached.
    let points: Vec<(usize, [f64; 3])> = seeds
        .iter()
        .enumerate()
        .flat_map(|(part, list)| list.iter().map(move |c| (part, centre(*c))))
        .collect();
    let mut keys: Vec<Cell> = cells.keys().copied().collect();
    keys.sort_unstable();
    for cell in keys {
        if labels.contains_key(&cell) {
            continue;
        }
        if spec.metric == Metric::Geodesic {
            unreached += 1;
        }
        let c = centre(cell);
        let pick = |only_allowed: bool| {
            points
                .iter()
                .filter(|(part, _)| !only_allowed || allowed(*part, cell))
                .min_by(|a, b| dist2(a.1, c).total_cmp(&dist2(b.1, c)).then(a.0.cmp(&b.0)))
                .map(|(part, _)| *part)
        };
        let part = pick(true).or_else(|| pick(false)).unwrap_or(0);
        labels.insert(cell, part);
    }
    let mut counts = vec![0usize; spec.parts.len()];
    for part in labels.values() {
        counts[*part] += 1;
    }
    Ok(RigSplit {
        labels,
        seeds,
        counts,
        unreached,
    })
}

#[cfg(test)]
#[path = "../../tests/rig_split.rs"]
mod tests;
