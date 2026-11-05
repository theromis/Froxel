//! MCP `collision_from_voxels`: axis-aligned boxes that cover an object's
//! solid cells, for a game's colliders.
//!
//! A hollow is an empty block `hole_size` wide on each axis, clipped to the
//! box it is tested in, so a hold under a deck is one, a hatch through a
//! one-cell deck is one (the block clips to the deck's thickness), and a dent
//! in a thick wall is not. No box of the cover holds a hollow.
//!
//! The cover starts from one box around the cells and splits every box that
//! holds a hollow, at the plane that leaves the least empty volume once both
//! sides shrink to their cells. Then `max_boxes` decides the rest: spare boxes
//! split the emptiest box further, and too many boxes merge, cheapest added
//! volume first, as far as no merged box holds a hollow. When no merge is
//! left the boxes with the least solid in them are dropped, and the count of
//! those comes back. `layer` and `bands` put z cuts on chosen planes unless
//! only another height can open a hollow; a box's faces stay on its cells, so
//! a walking surface stays exact. `min_feature` drops details thin on two
//! axes, such as vines and rope, first; a deck or a hull wall stays.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::state::Cell;

/// Most cells of an object's bounding box the cover works over.
const MAX_GRID: usize = 1 << 26;
/// Most boxes the splits may reach; beyond this the model needs a tolerance.
const MAX_SPLIT_BOXES: usize = 100_000;
/// Most planes tried along one axis of one box.
const MAX_PLANES: i32 = 128;

const SOLID: u8 = 1;
const TARGET: u8 = 2;
const BOXED: u8 = 4;

#[derive(Clone, Debug)]
pub struct CollisionSpec {
    /// Cover only these slots; empty is every slot.
    pub only: Vec<u8>,
    /// Slots left out.
    pub exclude: Vec<u8>,
    /// Z cuts lie on planes this many cells apart; 1 is any height.
    pub layer: i32,
    /// Ascending z planes for the cuts instead of `layer`.
    pub bands: Vec<i32>,
    /// Solid must hold a square this wide in an axis plane; 1 keeps all.
    pub min_feature: i32,
    /// The smallest empty block that stays open; 1 fills nothing.
    pub hole_size: i32,
    /// Most boxes; `None` splits only until no box holds a hollow.
    pub max_boxes: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct CollisionCover {
    /// Inclusive cell corners, lowest first.
    pub boxes: Vec<(Cell, Cell)>,
    /// Every solid cell of the object.
    pub solid: usize,
    /// Solid cells `only` and `exclude` left out.
    pub ignored_slots: usize,
    /// Solid cells `min_feature` left out.
    pub ignored_thin: usize,
    /// The cells the boxes aim to cover.
    pub target: usize,
    /// Target cells inside a box.
    pub covered: usize,
    /// Cells inside any box.
    pub box_cells: usize,
    /// Cells inside a box with no solid cell of any slot.
    pub empty_filled: usize,
    /// Boxes merged away.
    pub merged: usize,
    /// Boxes dropped to meet `max_boxes`.
    pub dropped: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct Bounds {
    min: [i32; 3],
    max: [i32; 3],
}

impl Bounds {
    /// Grow along `axis`, within `limit`, as far as it stays empty.
    fn grow_empty(&mut self, occupancy: &Occupancy, limit: &Bounds, axis: usize) {
        while self.min[axis] > limit.min[axis] {
            let mut more = *self;
            more.min[axis] -= 1;
            if occupancy.count(&more) > 0 {
                break;
            }
            *self = more;
        }
        while self.max[axis] < limit.max[axis] {
            let mut more = *self;
            more.max[axis] += 1;
            if occupancy.count(&more) > 0 {
                break;
            }
            *self = more;
        }
    }

    fn volume(&self) -> i64 {
        (0..3)
            .map(|axis| i64::from(self.max[axis] - self.min[axis] + 1))
            .product()
    }

    fn union(&self, other: &Bounds) -> Bounds {
        let mut out = *self;
        for axis in 0..3 {
            out.min[axis] = out.min[axis].min(other.min[axis]);
            out.max[axis] = out.max[axis].max(other.max[axis]);
        }
        out
    }

    fn contains(&self, other: &Bounds) -> bool {
        (0..3).all(|axis| self.min[axis] <= other.min[axis] && other.max[axis] <= self.max[axis])
    }

    /// Whether `hollow` still holds a block `size` wide beside this box.
    fn leaves_open(&self, hollow: &Bounds, size: i32) -> bool {
        let overlaps = (0..3)
            .all(|axis| hollow.min[axis] <= self.max[axis] && self.min[axis] <= hollow.max[axis]);
        !overlaps
            || (0..3).any(|axis| {
                let width = size.min(hollow.max[axis] - hollow.min[axis] + 1);
                self.min[axis] - hollow.min[axis] >= width
                    || hollow.max[axis] - self.max[axis] >= width
            })
    }

    /// At most `gap` empty cells apart on every axis.
    fn near(&self, other: &Bounds, gap: i32) -> bool {
        (0..3).all(|axis| {
            other.min[axis] <= self.max[axis] + 1 + gap
                && self.min[axis] <= other.max[axis] + 1 + gap
        })
    }
}

struct Grid {
    min: [i32; 3],
    dims: [usize; 3],
    flags: Vec<u8>,
}

impl Grid {
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let (x, y, z) = (
            (x - self.min[0]) as usize,
            (y - self.min[1]) as usize,
            (z - self.min[2]) as usize,
        );
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    fn mark(&mut self, cell: Cell, flag: u8) {
        let at = self.index(cell.0, cell.1, cell.2);
        self.flags[at] |= flag;
    }

    fn has(&self, cell: Cell, flag: u8) -> bool {
        self.flags[self.index(cell.0, cell.1, cell.2)] & flag != 0
    }
}

/// Cover `cells` with boxes under `spec`.
pub fn collision_cover(
    cells: &HashMap<Cell, u8>,
    spec: &CollisionSpec,
) -> Result<CollisionCover, String> {
    if cells.is_empty() {
        return Err("the object has no solid cells to cover".into());
    }
    let mut grid = solid_grid(cells)?;
    let mut cover = CollisionCover {
        solid: cells.len(),
        ..CollisionCover::default()
    };
    let mut target: Vec<Cell> = cells
        .iter()
        .filter(|(_, slot)| {
            (spec.only.is_empty() || spec.only.contains(slot)) && !spec.exclude.contains(slot)
        })
        .map(|(cell, _)| *cell)
        .collect();
    target.sort_unstable();
    cover.ignored_slots = cells.len() - target.len();
    if target.is_empty() {
        return Err("only and exclude leave no cell to cover".into());
    }
    let wide = keep_wide(&target, spec.min_feature);
    cover.ignored_thin = target.len() - wide.len();
    if wide.is_empty() {
        return Err(format!(
            "minFeature {} leaves no cell to cover: no solid holds a square that wide",
            spec.min_feature
        ));
    }
    cover.target = wide.len();
    for cell in &wide {
        grid.mark(*cell, TARGET);
    }
    let occupancy = Occupancy::new(&grid);
    let planes = cut_planes(&wide, spec);
    let (mut boxes, hollows) = split_hollows(&grid, &occupancy, &wide, &planes, spec)?;
    match spec.max_boxes {
        Some(budget) if boxes.len() < budget => {
            refine(&grid, &occupancy, &mut boxes, &planes, budget);
        }
        _ => {}
    }
    boxes = merge_boxes(&grid, &occupancy, boxes, &hollows, spec, &mut cover);
    if let Some(budget) = spec.max_boxes {
        drop_smallest(&occupancy, &mut boxes, budget, &mut cover);
    }
    boxes.sort_by_key(|b| (b.min[2], b.min[0], b.min[1], b.max[2], b.max[0], b.max[1]));
    for found in &boxes {
        for z in found.min[2]..=found.max[2] {
            for y in found.min[1]..=found.max[1] {
                for x in found.min[0]..=found.max[0] {
                    let at = grid.index(x, y, z);
                    let flags = grid.flags[at];
                    if flags & BOXED != 0 {
                        continue;
                    }
                    grid.flags[at] = flags | BOXED;
                    cover.box_cells += 1;
                    cover.covered += usize::from(flags & TARGET != 0);
                    cover.empty_filled += usize::from(flags & SOLID == 0);
                }
            }
        }
    }
    cover.boxes = boxes
        .iter()
        .map(|b| {
            (
                (b.min[0], b.min[1], b.min[2]),
                (b.max[0], b.max[1], b.max[2]),
            )
        })
        .collect();
    Ok(cover)
}

/// The grid over the object's cells.
fn solid_grid(cells: &HashMap<Cell, u8>) -> Result<Grid, String> {
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    for cell in cells.keys() {
        for (axis, value) in [cell.0, cell.1, cell.2].into_iter().enumerate() {
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    let dims = [0, 1, 2].map(|axis| (max[axis] - min[axis] + 1) as usize);
    if dims[0].saturating_mul(dims[1]).saturating_mul(dims[2]) > MAX_GRID {
        return Err(format!(
            "the object spans {} x {} x {} cells, too much to cover in one call",
            dims[0], dims[1], dims[2]
        ));
    }
    let mut grid = Grid {
        min,
        dims,
        flags: vec![0; dims[0] * dims[1] * dims[2]],
    };
    for cell in cells.keys() {
        grid.mark(*cell, SOLID);
    }
    Ok(grid)
}

/// The cells inside a full `size` square in some axis plane.
fn keep_wide(cells: &[Cell], size: i32) -> Vec<Cell> {
    if size <= 1 {
        return cells.to_vec();
    }
    let present: HashSet<Cell> = cells.iter().copied().collect();
    let mut kept: HashSet<Cell> = HashSet::new();
    let square = |corner: Cell, a: usize, b: usize| {
        let mut out = Vec::with_capacity((size * size) as usize);
        for i in 0..size {
            for j in 0..size {
                let mut p = [corner.0, corner.1, corner.2];
                p[a] += i;
                p[b] += j;
                out.push((p[0], p[1], p[2]));
            }
        }
        out
    };
    for corner in cells {
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let cells = square(*corner, a, b);
            if cells.iter().all(|cell| present.contains(cell)) {
                kept.extend(cells);
            }
        }
    }
    cells
        .iter()
        .copied()
        .filter(|cell| kept.contains(cell))
        .collect()
}

/// The z planes cuts lie on, none for any height.
fn cut_planes(cells: &[Cell], spec: &CollisionSpec) -> Vec<i32> {
    if !spec.bands.is_empty() {
        return spec.bands.clone();
    }
    if spec.layer <= 1 {
        return Vec::new();
    }
    let low = cells.iter().map(|c| c.2).min().unwrap_or(0);
    let high = cells.iter().map(|c| c.2).max().unwrap_or(0);
    (low..=high + 1).step_by(spec.layer as usize).collect()
}

/// The box around `cells`.
fn bounds_of(cells: &[Cell]) -> Bounds {
    let mut out = Bounds {
        min: [i32::MAX; 3],
        max: [i32::MIN; 3],
    };
    for cell in cells {
        for (axis, value) in [cell.0, cell.1, cell.2].into_iter().enumerate() {
            out.min[axis] = out.min[axis].min(value);
            out.max[axis] = out.max[axis].max(value);
        }
    }
    out
}

/// Split until no box holds a hollow. Also returns the hollows it opened.
fn split_hollows(
    grid: &Grid,
    occupancy: &Occupancy,
    cells: &[Cell],
    planes: &[i32],
    spec: &CollisionSpec,
) -> Result<(Vec<Bounds>, Vec<Bounds>), String> {
    let mut pending = vec![bounds_of(cells)];
    let mut done = Vec::new();
    let mut hollows = Vec::new();
    while let Some(found) = pending.pop() {
        let Some(hollow) = find_hollow(grid, occupancy, &found, spec.hole_size) else {
            done.push(found);
            continue;
        };
        hollows.push(hollow);
        let (first, second) = split(
            grid,
            occupancy,
            &found,
            planes,
            Some(&hollow),
            spec.hole_size,
        );
        pending.push(first);
        pending.push(second);
        if pending.len() + done.len() > MAX_SPLIT_BOXES {
            return Err(format!(
                "more than {MAX_SPLIT_BOXES} boxes: raise holeSize or minFeature, or leave slots out"
            ));
        }
    }
    Ok((done, hollows))
}

/// Split the emptiest boxes further while boxes are spare.
fn refine(
    grid: &Grid,
    occupancy: &Occupancy,
    boxes: &mut Vec<Bounds>,
    planes: &[i32],
    budget: usize,
) {
    let mut heap: BinaryHeap<(i64, usize)> = BinaryHeap::new();
    for (at, found) in boxes.iter().enumerate() {
        heap.push((occupancy.empty(found), at));
    }
    while boxes.len() < budget {
        let Some((empty, at)) = heap.pop() else {
            break;
        };
        if empty == 0 {
            break;
        }
        let (first, second) = split(grid, occupancy, &boxes[at], planes, None, 1);
        boxes[at] = first;
        heap.push((occupancy.empty(&first), at));
        boxes.push(second);
        heap.push((occupancy.empty(&second), boxes.len() - 1));
    }
}

/// Cut `found`, which holds an empty cell, at the plane that leaves the least
/// empty volume once each side shrinks to its cells, never through `keep`. A z
/// cut lies on `planes` unless no such cut, on any axis, removes any empty
/// volume.
fn split(
    grid: &Grid,
    occupancy: &Occupancy,
    found: &Bounds,
    planes: &[i32],
    keep: Option<&Bounds>,
    size: i32,
) -> (Bounds, Bounds) {
    let before = occupancy.empty(found);
    let best = best_cut(grid, occupancy, found, planes, keep, size);
    match best {
        Some((empty, first, second)) if empty < before || planes.is_empty() => (first, second),
        _ => {
            let any = best_cut(grid, occupancy, found, &[], keep, size).or(best);
            let (_, first, second) = any.expect("a shrunk box with an empty cell has a cut");
            (first, second)
        }
    }
}

/// The best cut and the empty volume it leaves. A cut that passes through
/// empty cells on both of its sides counts those twice over, so it goes
/// around a hole rather than through it. A cut that leaves a hollow too small
/// on both sides is taken only when every other cut does.
fn best_cut(
    grid: &Grid,
    occupancy: &Occupancy,
    found: &Bounds,
    planes: &[i32],
    keep: Option<&Bounds>,
    size: i32,
) -> Option<(i64, Bounds, Bounds)> {
    let mut best: Option<(i64, i64, i64, Bounds, Bounds)> = None;
    let mut slicing: Option<(i64, i64, i64, Bounds, Bounds)> = None;
    for axis in 0..3 {
        let (low, high) = (found.min[axis], found.max[axis]);
        let step = ((high - low) / MAX_PLANES).max(1);
        // `cut` is the first cell of the upper side.
        for cut in ((low + 1)..=high).step_by(step as usize) {
            if axis == 2 && !planes.is_empty() && planes.binary_search(&cut).is_err() {
                continue;
            }
            if keep.is_some_and(|keep| keep.min[axis] < cut && cut <= keep.max[axis]) {
                continue;
            }
            let mut below = *found;
            below.max[axis] = cut - 1;
            let mut above = *found;
            above.min[axis] = cut;
            let through = {
                let (mut under, mut over) = (*found, *found);
                under.min[axis] = cut - 1;
                under.max[axis] = cut - 1;
                over.min[axis] = cut;
                over.max[axis] = cut;
                occupancy.empty(&under).min(occupancy.empty(&over))
            };
            let (Some(below), Some(above)) = (occupancy.shrink(&below), occupancy.shrink(&above))
            else {
                continue;
            };
            let empty = occupancy.empty(&below) + occupancy.empty(&above);
            let score = empty + 2 * through;
            let off_middle = i64::from((2 * cut - low - high - 1).abs());
            let better = |held: &Option<(i64, i64, i64, Bounds, Bounds)>| {
                held.as_ref()
                    .is_none_or(|b| (score, off_middle) < (b.0, b.1))
            };
            if better(&best) {
                let entry = Some((score, off_middle, empty, below, above));
                if slices_hollow(grid, occupancy, found, axis, cut, size) {
                    if better(&slicing) {
                        slicing = entry;
                    }
                } else {
                    best = entry;
                }
            }
        }
    }
    best.or(slicing)
        .map(|(_, _, empty, below, above)| (empty, below, above))
}

/// Whether a cut of `found` at `cut` along `axis` leaves an empty block of
/// `found` too small on both sides to be a hollow.
fn slices_hollow(
    grid: &Grid,
    occupancy: &Occupancy,
    found: &Bounds,
    axis: usize,
    cut: i32,
    size: i32,
) -> bool {
    let block = [0, 1, 2].map(|at| size.min(found.max[at] - found.min[at] + 1));
    let wide = block[axis];
    if wide < 2 {
        return false;
    }
    let mut starts = [0, 1, 2].map(|at| (found.min[at], found.max[at] - block[at] + 1));
    starts[axis] = (
        (cut - wide + 1).max(found.min[axis]),
        (cut - 1).min(found.max[axis] - wide + 1),
    );
    for z in starts[2].0..=starts[2].1 {
        for y in starts[1].0..=starts[1].1 {
            for x in starts[0].0..=starts[0].1 {
                if grid.has((x, y, z), TARGET) {
                    continue;
                }
                let mut inside = Bounds {
                    min: [x, y, z],
                    max: [x + block[0] - 1, y + block[1] - 1, z + block[2] - 1],
                };
                if occupancy.count(&inside) > 0 {
                    continue;
                }
                inside.grow_empty(occupancy, found, axis);
                if cut - inside.min[axis] < wide && inside.max[axis] - cut + 1 < wide {
                    return true;
                }
            }
        }
    }
    false
}

/// A summed-volume table of the target cells: how many inside any box, O(1).
struct Occupancy {
    min: [i32; 3],
    stride: [usize; 2],
    sums: Vec<u32>,
}

impl Occupancy {
    fn new(grid: &Grid) -> Self {
        let [dx, dy, dz] = grid.dims;
        let stride = [dx + 1, (dx + 1) * (dy + 1)];
        let mut sums = vec![0u32; (dx + 1) * (dy + 1) * (dz + 1)];
        for z in 1..=dz {
            for y in 1..=dy {
                for x in 1..=dx {
                    let cell = grid.flags[(x - 1) + dx * ((y - 1) + dy * (z - 1))];
                    let at = x + stride[0] * y + stride[1] * z;
                    let (sx, sy, sz) = (1, stride[0], stride[1]);
                    let value = i64::from(u32::from(cell & TARGET != 0))
                        + i64::from(sums[at - sx])
                        + i64::from(sums[at - sy])
                        + i64::from(sums[at - sz])
                        - i64::from(sums[at - sx - sy])
                        - i64::from(sums[at - sx - sz])
                        - i64::from(sums[at - sy - sz])
                        + i64::from(sums[at - sx - sy - sz]);
                    sums[at] = value as u32;
                }
            }
        }
        Self {
            min: grid.min,
            stride,
            sums,
        }
    }

    /// Target cells inside `b`.
    fn count(&self, b: &Bounds) -> i64 {
        let low = [0, 1, 2].map(|axis| (b.min[axis] - self.min[axis]) as usize);
        let high = [0, 1, 2].map(|axis| (b.max[axis] - self.min[axis] + 1) as usize);
        let at = |x: usize, y: usize, z: usize| {
            i64::from(self.sums[x + self.stride[0] * y + self.stride[1] * z])
        };
        at(high[0], high[1], high[2])
            - at(low[0], high[1], high[2])
            - at(high[0], low[1], high[2])
            - at(high[0], high[1], low[2])
            + at(low[0], low[1], high[2])
            + at(low[0], high[1], low[2])
            + at(high[0], low[1], low[2])
            - at(low[0], low[1], low[2])
    }

    fn empty(&self, b: &Bounds) -> i64 {
        b.volume() - self.count(b)
    }

    /// `b` shrunk to the target cells inside it, `None` without any.
    fn shrink(&self, b: &Bounds) -> Option<Bounds> {
        if self.count(b) == 0 {
            return None;
        }
        let mut out = *b;
        for axis in 0..3 {
            // The lowest slice with a cell.
            let (mut low, mut high) = (out.min[axis], out.max[axis]);
            while low < high {
                let middle = low + (high - low) / 2;
                let mut part = out;
                part.max[axis] = middle;
                if self.count(&part) > 0 {
                    high = middle;
                } else {
                    low = middle + 1;
                }
            }
            out.min[axis] = low;
            // The highest slice with a cell.
            let (mut low, mut high) = (out.min[axis], out.max[axis]);
            while low < high {
                let middle = low + (high - low + 1) / 2;
                let mut part = out;
                part.min[axis] = middle;
                if self.count(&part) > 0 {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            out.max[axis] = low;
        }
        Some(out)
    }
}

/// A hollow in `b`, grown as far as it stays empty: an empty block `size`
/// wide on each axis, clipped to `b`.
fn find_hollow(grid: &Grid, occupancy: &Occupancy, b: &Bounds, size: i32) -> Option<Bounds> {
    let block = [0, 1, 2].map(|axis| size.min(b.max[axis] - b.min[axis] + 1));
    let cells: i64 = block.iter().map(|v| i64::from(*v)).product();
    if occupancy.empty(b) < cells {
        return None;
    }
    for z in b.min[2]..=b.max[2] - block[2] + 1 {
        for y in b.min[1]..=b.max[1] - block[1] + 1 {
            for x in b.min[0]..=b.max[0] - block[0] + 1 {
                if grid.has((x, y, z), TARGET) {
                    continue;
                }
                let mut inside = Bounds {
                    min: [x, y, z],
                    max: [x + block[0] - 1, y + block[1] - 1, z + block[2] - 1],
                };
                if occupancy.count(&inside) == 0 {
                    for axis in 0..3 {
                        inside.grow_empty(occupancy, b, axis);
                    }
                    return Some(inside);
                }
            }
        }
    }
    None
}

/// A merge candidate's cost: empty cells it adds, then the box's volume.
type Candidate = Reverse<(i64, i64, usize, usize)>;

/// Merge neighbouring boxes: those that fill nothing always, then the
/// cheapest while over `max_boxes`.
fn merge_boxes(
    grid: &Grid,
    occupancy: &Occupancy,
    boxes: Vec<Bounds>,
    hollows: &[Bounds],
    spec: &CollisionSpec,
    cover: &mut CollisionCover,
) -> Vec<Bounds> {
    let gap = spec.hole_size - 1;
    let budget = spec.max_boxes.unwrap_or(usize::MAX);
    let mut slots: Vec<Option<Bounds>> = boxes.into_iter().map(Some).collect();
    let mut alive = slots.len();
    let mut heap: BinaryHeap<Candidate> = BinaryHeap::new();
    let candidate = |slots: &[Option<Bounds>], a: usize, b: usize| -> Candidate {
        let (first, second) = (slots[a].expect("a live box"), slots[b].expect("a live box"));
        let union = first.union(&second);
        let added = occupancy.empty(&union) - occupancy.empty(&first) - occupancy.empty(&second);
        Reverse((added.max(0), union.volume(), a, b))
    };
    let mut by_x: Vec<usize> = (0..slots.len()).collect();
    by_x.sort_by_key(|at| slots[*at].expect("a live box").min[0]);
    for (place, &a) in by_x.iter().enumerate() {
        let first = slots[a].expect("a live box");
        for &b in &by_x[place + 1..] {
            let second = slots[b].expect("a live box");
            if second.min[0] > first.max[0] + 1 + gap {
                break;
            }
            if first.near(&second, gap) {
                heap.push(candidate(&slots, a, b));
            }
        }
    }
    while let Some(Reverse((added, _, a, b))) = heap.pop() {
        if added > 0 && alive <= budget {
            break;
        }
        let (Some(first), Some(second)) = (slots[a], slots[b]) else {
            continue;
        };
        let union = first.union(&second);
        if hollows
            .iter()
            .any(|hollow| !union.leaves_open(hollow, spec.hole_size))
            || find_hollow(grid, occupancy, &union, spec.hole_size).is_some()
        {
            continue;
        }
        slots[a] = None;
        slots[b] = None;
        alive -= 1;
        cover.merged += 1;
        for slot in slots.iter_mut() {
            if slot.is_some_and(|inner| union.contains(&inner)) {
                *slot = None;
                alive -= 1;
                cover.merged += 1;
            }
        }
        slots.push(Some(union));
        let new = slots.len() - 1;
        for other in 0..new {
            if slots[other].is_some_and(|box_| union.near(&box_, gap)) {
                heap.push(candidate(&slots, other, new));
            }
        }
    }
    slots.into_iter().flatten().collect()
}

/// Keep the `budget` boxes with the most target cells.
fn drop_smallest(
    occupancy: &Occupancy,
    boxes: &mut Vec<Bounds>,
    budget: usize,
    cover: &mut CollisionCover,
) {
    if boxes.len() <= budget {
        return;
    }
    boxes.sort_by_key(|b| Reverse(occupancy.count(b)));
    cover.dropped = boxes.len() - budget;
    boxes.truncate(budget);
}

#[cfg(test)]
#[path = "../../tests/collision.rs"]
mod tests;
