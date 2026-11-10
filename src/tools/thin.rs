//! The `minFeature` check of MCP `deform` and `import_mesh`: the parts of an
//! object thinner than a given number of cells, which a small creature
//! needs to know before a fin, an ear, or a leg vanishes or reads as noise.
//!
//! A cell's thickness is the shortest of the three solid runs through it
//! (along x, y, and z): a body is thick every way, a one-cell fin has a run
//! of 1 across it, and a leg one cell wide has runs of 1 across two axes.
//! The thin cells group into parts by 26-neighbour contact.

use std::collections::{HashMap, HashSet};

use crate::state::Cell;

/// Largest `minFeature` in cells.
pub const MAX_MIN_FEATURE: u32 = 16;
/// Most parts a report lists (the largest first).
const MAX_PARTS: usize = 16;

/// One connected thin part.
#[derive(Clone, Debug, PartialEq)]
pub struct ThinPart {
    pub cells: usize,
    /// Inclusive bounds.
    pub min: Cell,
    pub max: Cell,
    /// The thinnest run in the part, in cells.
    pub thickness: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThinReport {
    pub min_feature: u32,
    /// Cells thinner than `min_feature`.
    pub cells: usize,
    /// Connected thin parts, the largest first, at most 16.
    pub parts: Vec<ThinPart>,
    /// How many parts there are in all.
    pub count: usize,
}

/// The run of solid cells through `c` along `axis`.
fn run(cells: &HashSet<Cell>, c: Cell, axis: usize, cap: u32) -> u32 {
    let step = |c: Cell, d: i32| match axis {
        0 => (c.0 + d, c.1, c.2),
        1 => (c.0, c.1 + d, c.2),
        _ => (c.0, c.1, c.2 + d),
    };
    let mut n = 1;
    for sign in [-1, 1] {
        let mut p = step(c, sign);
        while n < cap && cells.contains(&p) {
            n += 1;
            p = step(p, sign);
        }
    }
    n
}

/// The parts of `cells` thinner than `min_feature` cells.
pub fn thin_parts(cells: &HashMap<Cell, u8>, min_feature: u32) -> Result<ThinReport, String> {
    if !(2..=MAX_MIN_FEATURE).contains(&min_feature) {
        return Err(format!("minFeature must be 2..={MAX_MIN_FEATURE} cells"));
    }
    let solid: HashSet<Cell> = cells.keys().copied().collect();
    let mut thin: HashMap<Cell, u32> = HashMap::new();
    for &c in &solid {
        let t = (0..3)
            .map(|axis| run(&solid, c, axis, min_feature))
            .min()
            .unwrap_or(1);
        if t < min_feature {
            thin.insert(c, t);
        }
    }
    let mut seen: HashSet<Cell> = HashSet::new();
    let mut parts = Vec::new();
    let mut keys: Vec<Cell> = thin.keys().copied().collect();
    keys.sort_unstable();
    for start in keys {
        if !seen.insert(start) {
            continue;
        }
        let mut stack = vec![start];
        let mut part = ThinPart {
            cells: 0,
            min: start,
            max: start,
            thickness: u32::MAX,
        };
        while let Some(c) = stack.pop() {
            part.cells += 1;
            part.min = (
                part.min.0.min(c.0),
                part.min.1.min(c.1),
                part.min.2.min(c.2),
            );
            part.max = (
                part.max.0.max(c.0),
                part.max.1.max(c.1),
                part.max.2.max(c.2),
            );
            part.thickness = part.thickness.min(thin[&c]);
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = (c.0 + dx, c.1 + dy, c.2 + dz);
                        if thin.contains_key(&n) && seen.insert(n) {
                            stack.push(n);
                        }
                    }
                }
            }
        }
        parts.push(part);
    }
    parts.sort_by(|a, b| b.cells.cmp(&a.cells).then(a.min.cmp(&b.min)));
    let count = parts.len();
    parts.truncate(MAX_PARTS);
    Ok(ThinReport {
        min_feature,
        cells: thin.len(),
        parts,
        count,
    })
}

#[cfg(test)]
#[path = "../../tests/thin.rs"]
mod tests;
