//! MCP `surface_stroke`: a stroke through points that lands on the
//! object's surface.
//!
//! The path is sampled every half cell. Each sample snaps to the nearest
//! surface cell (a solid cell with an empty face neighbour) within `reach`;
//! where two snapped cells do not touch, the cells between them are snapped
//! too, so the line has no gap. The solid cells within `width / 2` of that cell and at most `depth`
//! cells into the solid take the slot, or are erased by slot 0 for a
//! groove. So a seam between shell plates or a marking on a curved body
//! follows the surface even where the points float above or sink below it.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::state::Cell;

use super::sweep::catmull;

/// Most points one stroke takes.
pub const MAX_STROKE_POINTS: usize = 1024;
/// Largest `depth` of a stroke.
pub const MAX_STROKE_DEPTH: u32 = 16;
/// Largest `reach` of a stroke.
pub const MAX_STROKE_REACH: f64 = 64.0;
/// Largest `width` of a stroke.
pub const MAX_STROKE_WIDTH: f64 = 32.0;

/// The bucket side of the surface lookup.
const BUCKET: i32 = 8;

/// A stroke over the surface.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeSpec {
    /// Points in local cells; a whole number is the centre of that cell.
    pub points: Vec<[f64; 3]>,
    /// A Catmull-Rom curve through the points instead of straight segments.
    pub smooth: bool,
    /// Width across the surface in cells.
    pub width: f64,
    /// Cells into the solid, 1 being the surface alone.
    pub depth: u32,
    /// How far a sample may be from the surface it snaps to.
    pub reach: f64,
    /// The slot painted; 0 erases (a groove).
    pub slot: u8,
    /// Only cells of these slots change.
    pub only: Option<Vec<u8>>,
}

/// What a stroke did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stroked {
    pub writes: Vec<(i32, i32, i32, i32)>,
    /// Path samples.
    pub samples: usize,
    /// Samples with no surface cell within reach.
    pub missed: usize,
    /// The farthest a sample moved to its surface cell.
    pub snap: f64,
    /// Length of the path in cells.
    pub length: f64,
}

/// The cells the stroke `spec` paints in `cells`.
pub fn surface_stroke(cells: &HashMap<Cell, u8>, spec: &StrokeSpec) -> Result<Stroked, String> {
    if !(2..=MAX_STROKE_POINTS).contains(&spec.points.len()) {
        return Err(format!("points must hold 2..={MAX_STROKE_POINTS} points"));
    }
    if spec.points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("points must be finite".into());
    }
    if !(0.5..=MAX_STROKE_WIDTH).contains(&spec.width) {
        return Err(format!("width must be 0.5..={MAX_STROKE_WIDTH}"));
    }
    if !(1..=MAX_STROKE_DEPTH).contains(&spec.depth) {
        return Err(format!("depth must be 1..={MAX_STROKE_DEPTH}"));
    }
    if !(0.0..=MAX_STROKE_REACH).contains(&spec.reach) {
        return Err(format!("reach must be 0..={MAX_STROKE_REACH}"));
    }
    let (samples, length) = sample_path(&spec.points, spec.smooth);
    // The surface, bucketed for the nearest lookup.
    let open = |c: Cell| !cells.contains_key(&c);
    let surface: Vec<Cell> = cells
        .keys()
        .copied()
        .filter(|c| neighbours(*c).into_iter().any(open))
        .collect();
    let mut buckets: HashMap<Cell, Vec<Cell>> = HashMap::new();
    for cell in &surface {
        buckets.entry(bucket(*cell)).or_default().push(*cell);
    }
    let mut snapped: Vec<Cell> = Vec::new();
    let mut missed = 0;
    let mut snap: f64 = 0.0;
    for at in &samples {
        match nearest(&buckets, *at, spec.reach) {
            Some((cell, distance)) => {
                snap = snap.max(distance);
                if let Some(&last) = snapped.last() {
                    if last != cell {
                        bridge(&buckets, last, cell, spec.reach, 0, &mut snapped);
                        snapped.push(cell);
                    }
                } else {
                    snapped.push(cell);
                }
            }
            None => missed += 1,
        }
    }
    // Depth below the surface, out to `depth`, from the surface inward.
    let mut layer: HashMap<Cell, u32> = surface.iter().map(|c| (*c, 1)).collect();
    let mut queue: VecDeque<Cell> = surface.iter().copied().collect();
    while let Some(cell) = queue.pop_front() {
        let next = layer[&cell] + 1;
        if next > spec.depth {
            continue;
        }
        for n in neighbours(cell) {
            if cells.contains_key(&n) && !layer.contains_key(&n) {
                layer.insert(n, next);
                queue.push_back(n);
            }
        }
    }
    let radius = spec.width / 2.0;
    let reach = radius.ceil() as i32;
    let mut painted: HashSet<Cell> = HashSet::new();
    for centre in &snapped {
        for dz in -reach..=reach {
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let d2 = f64::from(dx * dx + dy * dy + dz * dz);
                    if d2 > radius * radius + 1.0e-9 {
                        continue;
                    }
                    let cell = (centre.0 + dx, centre.1 + dy, centre.2 + dz);
                    let Some(slot) = cells.get(&cell) else {
                        continue;
                    };
                    if layer.get(&cell).is_none_or(|depth| *depth > spec.depth) {
                        continue;
                    }
                    if spec.only.as_ref().is_some_and(|only| !only.contains(slot)) {
                        continue;
                    }
                    painted.insert(cell);
                }
            }
        }
    }
    let mut writes: Vec<_> = painted
        .into_iter()
        .map(|(x, y, z)| (x, y, z, i32::from(spec.slot)))
        .collect();
    writes.sort_unstable();
    Ok(Stroked {
        writes,
        samples: samples.len(),
        missed,
        snap,
        length,
    })
}

/// Surface cells between two snapped cells `a` and `b` that are not
/// neighbours (a steep side makes consecutive samples jump), so the stroke
/// has no gap: the midpoint of the two snaps to the surface, and each half
/// is bridged again, until the cells touch (26-neighbours) or the midpoint
/// lands on one of them.
fn bridge(
    buckets: &HashMap<Cell, Vec<Cell>>,
    a: Cell,
    b: Cell,
    reach: f64,
    level: u32,
    out: &mut Vec<Cell>,
) {
    let touching = (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1 && (a.2 - b.2).abs() <= 1;
    if touching || level > 12 {
        return;
    }
    let mid = [
        f64::from(a.0 + b.0) / 2.0,
        f64::from(a.1 + b.1) / 2.0,
        f64::from(a.2 + b.2) / 2.0,
    ];
    let Some((cell, _)) = nearest(buckets, mid, reach.max(2.0)) else {
        return;
    };
    if cell == a || cell == b {
        return;
    }
    bridge(buckets, a, cell, reach, level + 1, out);
    out.push(cell);
    bridge(buckets, cell, b, reach, level + 1, out);
}

/// Points along the path every half cell (at least one per segment end),
/// and the path's length.
fn sample_path(points: &[[f64; 3]], smooth: bool) -> (Vec<[f64; 3]>, f64) {
    let mut out = vec![points[0]];
    let mut length = 0.0;
    for i in 0..points.len() - 1 {
        let (a, b) = (points[i], points[i + 1]);
        let span = distance(a, b);
        let steps = ((span / 0.5).ceil() as usize).max(1);
        let p0 = if i == 0 { a } else { points[i - 1] };
        let p3 = points.get(i + 2).copied().unwrap_or(b);
        let mut last = a;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let at = if smooth {
                catmull(p0, a, b, p3, t).0
            } else {
                [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
            };
            length += distance(last, at);
            last = at;
            out.push(at);
        }
    }
    (out, length)
}

/// The surface cell nearest `at` within `reach`, and its distance. Ties go
/// to the smallest cell, so a stroke is the same on every run.
fn nearest(buckets: &HashMap<Cell, Vec<Cell>>, at: [f64; 3], reach: f64) -> Option<(Cell, f64)> {
    let low = |v: f64| ((v - reach).floor() as i32).div_euclid(BUCKET);
    let high = |v: f64| ((v + reach).ceil() as i32).div_euclid(BUCKET);
    let mut best: Option<(Cell, f64)> = None;
    for bz in low(at[2])..=high(at[2]) {
        for by in low(at[1])..=high(at[1]) {
            for bx in low(at[0])..=high(at[0]) {
                let Some(list) = buckets.get(&(bx, by, bz)) else {
                    continue;
                };
                for cell in list {
                    let d = distance(at, [cell.0, cell.1, cell.2].map(f64::from));
                    if d > reach + 1.0e-9 {
                        continue;
                    }
                    let better = match best {
                        None => true,
                        Some((found, bd)) => d < bd - 1.0e-9 || (d < bd + 1.0e-9 && *cell < found),
                    };
                    if better {
                        best = Some((*cell, d));
                    }
                }
            }
        }
    }
    best
}

fn bucket(cell: Cell) -> Cell {
    (
        cell.0.div_euclid(BUCKET),
        cell.1.div_euclid(BUCKET),
        cell.2.div_euclid(BUCKET),
    )
}

fn neighbours(c: Cell) -> [Cell; 6] {
    [
        (c.0 + 1, c.1, c.2),
        (c.0 - 1, c.1, c.2),
        (c.0, c.1 + 1, c.2),
        (c.0, c.1 - 1, c.2),
        (c.0, c.1, c.2 + 1),
        (c.0, c.1, c.2 - 1),
    ]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
