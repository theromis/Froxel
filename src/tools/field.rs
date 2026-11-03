//! A dense box of cells and the distance from each cell to the nearest cell
//! of another kind, shared by `deform` `displace` and the sculpt brushes.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::state::Cell;

/// Most cells one dense box may hold.
pub const MAX_FIELD_CELLS: i64 = 16_000_000;

/// The cells of an inclusive box copied out of a map, row by row.
pub(super) struct Grid {
    pub(super) min: Cell,
    pub(super) dims: [i32; 3],
    pub(super) slots: Vec<u8>,
}

impl Grid {
    pub(super) fn new(cells: &HashMap<Cell, u8>, min: Cell, max: Cell) -> Result<Grid, String> {
        let dims = [max.0 - min.0 + 1, max.1 - min.1 + 1, max.2 - min.2 + 1];
        let count = dims.iter().map(|&d| i64::from(d.max(0))).product::<i64>();
        if count > MAX_FIELD_CELLS {
            return Err(format!(
                "the box to test holds {count} cells, more than {MAX_FIELD_CELLS}; work on a smaller region"
            ));
        }
        let mut grid = Grid {
            min,
            dims,
            slots: vec![0; count as usize],
        };
        if count as usize > cells.len() {
            for (&cell, &slot) in cells {
                if let Some(i) = grid.at(cell) {
                    grid.slots[i] = slot;
                }
            }
        } else {
            for i in 0..grid.slots.len() {
                grid.slots[i] = cells.get(&grid.cell(i)).copied().unwrap_or(0);
            }
        }
        Ok(grid)
    }

    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn at(&self, c: Cell) -> Option<usize> {
        let x = c.0 - self.min.0;
        let y = c.1 - self.min.1;
        let z = c.2 - self.min.2;
        if x < 0 || y < 0 || z < 0 || x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return None;
        }
        Some(((z * self.dims[1] + y) * self.dims[0] + x) as usize)
    }

    pub(super) fn cell(&self, i: usize) -> Cell {
        let i = i as i32;
        let x = i % self.dims[0];
        let y = (i / self.dims[0]) % self.dims[1];
        let z = i / (self.dims[0] * self.dims[1]);
        (x + self.min.0, y + self.min.1, z + self.min.2)
    }

    /// The slot at `c`; 0 outside the box.
    pub(super) fn slot(&self, c: Cell) -> u8 {
        self.at(c).map_or(0, |i| self.slots[i])
    }
}

/// For every cell of `grid`, the distance from its centre to the centre of
/// the nearest cell `seed` accepts, and that cell's index, searched up to
/// `limit` cells. Seeds are 0 from themselves; cells farther than `limit`
/// are `f32::INFINITY` with no index. The search carries the nearest seed
/// across the 26 neighbours, so a distance is close to Euclidean.
pub(super) fn nearest(
    grid: &Grid,
    seed: impl Fn(usize) -> bool,
    limit: f32,
) -> (Vec<f32>, Vec<u32>) {
    let mut dist = vec![f32::INFINITY; grid.len()];
    let mut from = vec![u32::MAX; grid.len()];
    let mut heap = BinaryHeap::new();
    for i in 0..grid.len() {
        if !seed(i) {
            continue;
        }
        dist[i] = 0.0;
        from[i] = i as u32;
        // Only a seed beside a non-seed starts the search.
        let c = grid.cell(i);
        let border = neighbours(c).any(|n| grid.at(n).is_some_and(|j| !seed(j)));
        if border {
            heap.push((Reverse(0u32), i as u32));
        }
    }
    while let Some((Reverse(key), i)) = heap.pop() {
        let i = i as usize;
        if key > (dist[i] * 1024.0) as u32 {
            continue;
        }
        let origin = grid.cell(from[i] as usize);
        for n in neighbours(grid.cell(i)) {
            let Some(j) = grid.at(n) else {
                continue;
            };
            let d = distance(n, origin);
            if d < dist[j] && d <= limit {
                dist[j] = d;
                from[j] = from[i];
                heap.push((Reverse((d * 1024.0) as u32), j as u32));
            }
        }
    }
    (dist, from)
}

/// The signed distance of every cell to the surface, half a cell out from
/// the solid cell centres: below 0 inside, above 0 outside, at most `limit`
/// either way (`±(limit + 1)` beyond it). With it, for each cell, the index
/// of the nearest solid cell (itself when solid), `u32::MAX` when none is
/// within `limit`.
pub(super) fn signed(grid: &Grid, limit: f32) -> (Vec<f32>, Vec<u32>) {
    let (outside, solid) = nearest(grid, |i| grid.slots[i] != 0, limit + 1.0);
    let (inside, _) = nearest(grid, |i| grid.slots[i] == 0, limit + 1.0);
    let sdf = (0..grid.len())
        .map(|i| {
            if grid.slots[i] != 0 {
                -(inside[i] - 0.5).min(limit + 1.0)
            } else {
                (outside[i] - 0.5).min(limit + 1.0)
            }
        })
        .collect();
    (sdf, solid)
}

pub(super) fn neighbours(c: Cell) -> impl Iterator<Item = Cell> {
    (0..27)
        .filter(|&k| k != 13)
        .map(move |k| (c.0 + k % 3 - 1, c.1 + (k / 3) % 3 - 1, c.2 + k / 9 - 1))
}

pub(super) fn distance(a: Cell, b: Cell) -> f32 {
    let dx = (a.0 - b.0) as f32;
    let dy = (a.1 - b.1) as f32;
    let dz = (a.2 - b.2) as f32;
    (dx * dx + dy * dy + dz * dz).sqrt()
}
