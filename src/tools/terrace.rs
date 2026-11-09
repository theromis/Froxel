//! MCP `deterrace`: the contour terraces a smooth mesh leaves when it is
//! voxelized (one-cell steps running in long even bands on gentle slopes)
//! re-quantized into blocks, or staggered.
//!
//! `blocks` cuts space into cubes of `size` cells on a grid through
//! `origin`; a cube is solid when at least `fill` of its cells are, and all
//! its cells then take its most common slot, so every step is `size` cells
//! tall and the form reads as the chunky cubes of a concept sheet. Only
//! cubes whose decision moves the surface by at most `tolerance` cells
//! change, so thin parts and silhouettes are kept within it.
//!
//! `stagger` works on the top surface: each column's highest solid cell
//! with empty above. Where a column stands one cell above a face neighbour
//! (a one-cell step), the step's edge moves by one cell, up on the low side
//! or down on the high side, in teeth `run` cells long: a checkerboard of
//! `run`-cell squares from a seeded phase per height says out or in, and a
//! seeded share `jitter` of teeth stays in place (never two side by side),
//! so no step runs in a long even line.

use std::collections::HashMap;

use crate::state::Cell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerraceMode {
    Blocks,
    Stagger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerraceSpec {
    pub mode: TerraceMode,
    /// Blocks: the cube side, 2..=4.
    pub size: i32,
    /// Blocks: the share of a cube's cells that makes it solid.
    pub fill: f64,
    /// Blocks: where the grid of cubes starts.
    pub origin: Cell,
    /// Blocks: how far, in cells, a cube's decision may move the surface;
    /// cubes that would move it further stay as they are.
    pub tolerance: i32,
    /// Stagger: the length of a tooth along the step, 1..=16.
    pub run: i32,
    /// Stagger: the share of teeth left in place, 0..=1.
    pub jitter: f64,
    pub seed: u64,
    /// Only cells inside this inclusive box change.
    pub region: Option<(Cell, Cell)>,
}

/// What `deterrace` did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deterraced {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub added: usize,
    pub removed: usize,
    /// One-cell steps on the top surface, and the longest straight one,
    /// before and after.
    pub steps: [usize; 2],
    pub longest: [usize; 2],
    /// Blocks kept as they were because they would move the surface more
    /// than the tolerance.
    pub kept: usize,
}

fn inside(region: &Option<(Cell, Cell)>, c: Cell) -> bool {
    region.is_none_or(|(a, b)| {
        (a.0..=b.0).contains(&c.0) && (a.1..=b.1).contains(&c.1) && (a.2..=b.2).contains(&c.2)
    })
}

/// Top surface heights: for each column `(x, y)`, the highest solid `z`
/// with empty above, of the column's highest run.
pub fn tops(cells: &HashMap<Cell, u8>) -> HashMap<(i32, i32), i32> {
    let mut out: HashMap<(i32, i32), i32> = HashMap::new();
    for c in cells.keys() {
        let top = out.entry((c.0, c.1)).or_insert(c.2);
        *top = (*top).max(c.2);
    }
    out
}

/// A straight run of one-cell steps: its first high column, the way the
/// steps face, the way the run goes, and its length.
type Run = ((i32, i32), (i32, i32), (i32, i32), usize);

/// The one-cell steps of a height map (pairs of face-neighbour columns one
/// apart) and every straight run of them: steps facing the same way at the
/// same height, side by side along a line across the step.
fn step_runs(tops: &HashMap<(i32, i32), i32>) -> (usize, Vec<Run>) {
    let mut count = 0;
    let mut runs = Vec::new();
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let step = |x: i32, y: i32| -> Option<i32> {
            let h = *tops.get(&(x, y))?;
            let low = *tops.get(&(x + dx, y + dy))?;
            (h == low + 1).then_some(h)
        };
        let (ax, ay) = (dy.abs(), dx.abs());
        let mut columns: Vec<(i32, i32)> = tops.keys().copied().collect();
        columns.sort_unstable();
        for (x, y) in columns {
            let Some(h) = step(x, y) else { continue };
            count += 1;
            // Count a run once, from its first step.
            if step(x - ax, y - ay) == Some(h) {
                continue;
            }
            let mut n = 1;
            while step(x + ax * n as i32, y + ay * n as i32) == Some(h) {
                n += 1;
            }
            runs.push(((x, y), (dx, dy), (ax, ay), n));
        }
    }
    (count, runs)
}

/// How many one-cell steps a height map has, and its longest straight run
/// of them.
pub fn one_cell_steps(tops: &HashMap<(i32, i32), i32>) -> (usize, usize) {
    let (count, runs) = step_runs(tops);
    (count, runs.iter().map(|r| r.3).max().unwrap_or(0))
}

fn hash(a: i64, b: i64, c: i64, seed: u64) -> u64 {
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    for v in [a, b, c] {
        h ^= v as u64;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 31;
    }
    h
}

/// Run `deterrace` on an object's cells.
pub fn deterrace(cells: &HashMap<Cell, u8>, spec: &TerraceSpec) -> Result<Deterraced, String> {
    if cells.is_empty() {
        return Err("the object is empty".into());
    }
    let before = one_cell_steps(&tops(cells));
    let mut out = match spec.mode {
        TerraceMode::Blocks => blocks(cells, spec)?,
        TerraceMode::Stagger => stagger(cells, spec)?,
    };
    let mut next = cells.clone();
    for &(x, y, z, slot) in &out.writes {
        if slot == 0 {
            next.remove(&(x, y, z));
        } else {
            next.insert((x, y, z), slot as u8);
        }
    }
    let after = one_cell_steps(&tops(&next));
    out.steps = [before.0, after.0];
    out.longest = [before.1, after.1];
    Ok(out)
}

fn blocks(cells: &HashMap<Cell, u8>, spec: &TerraceSpec) -> Result<Deterraced, String> {
    if !(2..=4).contains(&spec.size) {
        return Err("size must be 2..=4 cells".into());
    }
    if !(spec.fill > 0.0 && spec.fill <= 1.0) {
        return Err("fill must be above 0 and at most 1".into());
    }
    if !(0..=16).contains(&spec.tolerance) {
        return Err("tolerance must be 0..=16 cells".into());
    }
    let n = spec.size;
    let o = spec.origin;
    let block = |c: Cell| {
        (
            (c.0 - o.0).div_euclid(n),
            (c.1 - o.1).div_euclid(n),
            (c.2 - o.2).div_euclid(n),
        )
    };
    let mut seen: HashMap<Cell, ()> = HashMap::new();
    for c in cells.keys() {
        if inside(&spec.region, *c) {
            seen.insert(block(*c), ());
        }
    }
    let mut out = Deterraced::default();
    let total = (n * n * n) as f64;
    let mut blocks: Vec<Cell> = seen.into_keys().collect();
    blocks.sort_unstable();
    for b in blocks {
        let base = (b.0 * n + o.0, b.1 * n + o.1, b.2 * n + o.2);
        let mut members = Vec::with_capacity((n * n * n) as usize);
        let mut counts: HashMap<u8, usize> = HashMap::new();
        for dz in 0..n {
            for dy in 0..n {
                for dx in 0..n {
                    let c = (base.0 + dx, base.1 + dy, base.2 + dz);
                    let slot = cells.get(&c).copied();
                    if let Some(slot) = slot {
                        *counts.entry(slot).or_default() += 1;
                    }
                    members.push((c, slot));
                }
            }
        }
        let solid: usize = counts.values().sum();
        let fills = solid as f64 / total >= spec.fill - 1e-9;
        // The most common slot; ties to the lower slot.
        let slot = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(slot, _)| *slot);
        // How far the decision moves the surface: a cell that changes is
        // at most this many cells from a cell of the original that agrees.
        let moved = if fills {
            members
                .iter()
                .filter(|(_, s)| s.is_none())
                .map(|(c, _)| distance_to(cells, *c, true, n))
                .max()
                .unwrap_or(0)
        } else {
            members
                .iter()
                .filter(|(_, s)| s.is_some())
                .map(|(c, _)| distance_to(cells, *c, false, n))
                .max()
                .unwrap_or(0)
        };
        if moved > spec.tolerance {
            out.kept += 1;
            continue;
        }
        for (c, old) in members {
            if !inside(&spec.region, c) {
                continue;
            }
            match (fills, old, slot) {
                (true, None, Some(slot)) => {
                    out.writes.push((c.0, c.1, c.2, i32::from(slot)));
                    out.added += 1;
                }
                (true, Some(old), Some(slot)) if old != slot => {
                    out.writes.push((c.0, c.1, c.2, i32::from(slot)));
                }
                (false, Some(_), _) => {
                    out.writes.push((c.0, c.1, c.2, 0));
                    out.removed += 1;
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

/// Chebyshev distance from `c` to the nearest solid cell (`solid`) or
/// empty cell of `cells`, searched out to `limit` (and `limit + 1` past it).
fn distance_to(cells: &HashMap<Cell, u8>, c: Cell, solid: bool, limit: i32) -> i32 {
    for r in 1..=limit {
        for dz in -r..=r {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                        continue;
                    }
                    let found = cells.contains_key(&(c.0 + dx, c.1 + dy, c.2 + dz));
                    if found == solid {
                        return r;
                    }
                }
            }
        }
    }
    limit + 1
}

fn stagger(cells: &HashMap<Cell, u8>, spec: &TerraceSpec) -> Result<Deterraced, String> {
    if !(1..=16).contains(&spec.run) {
        return Err("run must be 1..=16 cells".into());
    }
    if !(0.0..=1.0).contains(&spec.jitter) {
        return Err("jitter must be 0..=1".into());
    }
    let tops = tops(cells);
    let run = spec.run;
    // Per column: -1 lowers it a cell, +1 raises it (with this slot).
    let mut change: HashMap<(i32, i32), (i32, u8)> = HashMap::new();
    let mut columns: Vec<(i32, i32)> = tops.keys().copied().collect();
    columns.sort_unstable();
    for (x, y) in columns {
        let h = tops[&(x, y)];
        let top = (x, y, h);
        if !inside(&spec.region, top) {
            continue;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let low = (x + dx, y + dy);
            if tops.get(&low) != Some(&(h - 1)) {
                continue;
            }
            // Teeth: a checkerboard of `run`-cell squares over the columns,
            // from a seeded phase per height, so the teeth along a step
            // alternate out and in whichever way the step faces, and a
            // corner column decides once. A jittered tooth stays put, never
            // two side by side.
            let phase = hash(i64::from(h), 0, -1, spec.seed);
            let px = (phase % run as u64) as i32;
            let py = ((phase >> 20) % run as u64) as i32;
            let tooth = ((x + px).div_euclid(run), (y + py).div_euclid(run));
            let still = |t: (i32, i32)| {
                let roll = hash(i64::from(t.0), i64::from(t.1), i64::from(h), spec.seed);
                (roll % 10_000) as f64 / 10_000.0 < spec.jitter
            };
            let behind = if dx != 0 {
                (tooth.0, tooth.1 - 1)
            } else {
                (tooth.0 - 1, tooth.1)
            };
            if still(tooth) && !still(behind) {
                continue;
            }
            let up = (tooth.0 + tooth.1 + h).rem_euclid(2) == 0;
            if up {
                // The low side rises to the step: the edge moves out.
                let raise = (low.0, low.1, h);
                if inside(&spec.region, raise) && !change.contains_key(&low) {
                    change.insert(low, (1, cells[&top]));
                }
            } else {
                // The high side drops: the edge moves in.
                change.entry((x, y)).or_insert((-1, 0));
            }
        }
    }
    // Where a tooth meets a jog of the old contour, steps can still line
    // up: break each run longer than `run + 1` at its middle, a few times.
    let limit = run as usize + 1;
    for _pass in 0..8 {
        let mut now = tops.clone();
        for (column, (dz, _)) in &change {
            *now.get_mut(column).expect("a changed column has a top") += dz;
        }
        let long: Vec<Run> = step_runs(&now)
            .1
            .into_iter()
            .filter(|r| r.3 > limit)
            .collect();
        if long.is_empty() {
            break;
        }
        for ((x, y), (dx, dy), (ax, ay), n) in long {
            // From the middle outwards, the first column (or the low cell
            // beside it) not changed yet.
            let mid = (n / 2) as i32;
            let order = (0..n as i32).map(|k| {
                if k % 2 == 0 {
                    mid + k / 2
                } else {
                    mid - (k + 1) / 2
                }
            });
            for at in order.filter(|at| (1..n as i32 - 1).contains(at) || n <= 2) {
                let high = (x + ax * at, y + ay * at);
                let low = (high.0 + dx, high.1 + dy);
                let h = now[&high];
                if !change.contains_key(&high) && inside(&spec.region, (high.0, high.1, h)) {
                    change.insert(high, (-1, 0));
                    break;
                }
                if !change.contains_key(&low)
                    && inside(&spec.region, (low.0, low.1, h))
                    && tops[&low] == h - 1
                {
                    let slot = cells
                        .get(&(high.0, high.1, tops[&high]))
                        .copied()
                        .unwrap_or(1);
                    change.insert(low, (1, slot));
                    break;
                }
            }
        }
    }
    let mut out = Deterraced::default();
    let mut keys: Vec<_> = change.into_iter().collect();
    keys.sort_unstable();
    for ((x, y), (dz, slot)) in keys {
        let h = tops[&(x, y)];
        if dz > 0 {
            out.writes.push((x, y, h + 1, i32::from(slot)));
            out.added += 1;
        } else {
            out.writes.push((x, y, h, 0));
            out.removed += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../../tests/terrace.rs"]
mod tests;
