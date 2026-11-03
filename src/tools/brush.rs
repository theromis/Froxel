//! The sculpt brushes behind MCP `brush`, after Blender's clay and grab
//! brushes: dabs along a path of points inflate, deflate, smooth, flatten,
//! or drag the surface, each with a radius, a strength, and a falloff.

use std::collections::{HashMap, HashSet};

use super::field::{distance, nearest, neighbours, Grid};
use super::surface::allowed;
use crate::state::Cell;

/// Most dabs one stroke may lay after spacing.
pub const MAX_DABS: usize = 4096;
/// Widest brush radius in cells.
pub const MAX_BRUSH_RADIUS: f64 = 64.0;
/// Most cell tests summed over the dabs.
const MAX_BRUSH_WORK: i64 = 96_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushOp {
    /// Push the surface out, up to strength × radius / 2 cells at a dab's centre.
    Inflate,
    /// Push it in the same way; MCP `dig` is the same brush.
    Deflate,
    /// Blur the solid and empty cells, so bumps and pits melt.
    Smooth,
    /// Cut and fill toward the mean plane of the surface under the dab.
    Flatten,
    /// Move the cells under the dab along a vector.
    Grab,
}

impl BrushOp {
    pub const NAMES: &'static str = "inflate, deflate (dig), smooth, flatten, or grab (drag)";

    pub fn parse(name: &str) -> Option<BrushOp> {
        Some(match name {
            "inflate" => BrushOp::Inflate,
            "deflate" | "dig" => BrushOp::Deflate,
            "smooth" => BrushOp::Smooth,
            "flatten" => BrushOp::Flatten,
            "grab" | "drag" => BrushOp::Grab,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            BrushOp::Inflate => "inflate",
            BrushOp::Deflate => "deflate",
            BrushOp::Smooth => "smooth",
            BrushOp::Flatten => "flatten",
            BrushOp::Grab => "grab",
        }
    }
}

/// How a dab's effect fades from its centre to its radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Falloff {
    /// Smoothstep: full at the centre, easing to nothing at the radius.
    Smooth,
    Linear,
    /// Full strength out to the radius.
    Constant,
}

impl Falloff {
    pub fn parse(name: &str) -> Option<Falloff> {
        Some(match name {
            "smooth" => Falloff::Smooth,
            "linear" => Falloff::Linear,
            "constant" | "sharp" => Falloff::Constant,
            _ => return None,
        })
    }

    /// The weight at `t`, the distance over the radius.
    fn weight(self, t: f64) -> f64 {
        if t >= 1.0 {
            return 0.0;
        }
        let t = t.max(0.0);
        match self {
            Falloff::Smooth => 1.0 - t * t * (3.0 - 2.0 * t),
            Falloff::Linear => 1.0 - t,
            Falloff::Constant => 1.0,
        }
    }
}

/// One brush point: a dab's centre, radius, and strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub at: [f64; 3],
    pub radius: f64,
    /// 0..=1.
    pub strength: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrushSpec {
    pub op: BrushOp,
    pub points: Vec<Dab>,
    pub falloff: Falloff,
    /// The brush changes only cells of these slots (and empty cells); empty
    /// is every slot.
    pub only: Vec<u8>,
    /// The slot of added cells; `None` takes the nearest solid cell's.
    pub palette: Option<u8>,
    /// Grab: the move at a dab's centre at strength 1, in cells.
    pub vector: [f64; 3],
    /// The most cells between two dabs; extra dabs are laid between points
    /// farther apart. `None` is half the radius; 0 lays one dab per point.
    /// Grab always lays one per point.
    pub spacing: Option<f64>,
}

#[derive(Debug, PartialEq)]
pub struct Brushed {
    /// `(x, y, z, palette)`; 0 removes.
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub dabs: usize,
    pub added: usize,
    pub removed: usize,
}

/// Run the brush along its points over `cells`. Each dab reads the cells as
/// the dabs before it left them.
pub fn brush(cells: &HashMap<Cell, u8>, spec: &BrushSpec) -> Result<Brushed, String> {
    if spec.points.is_empty() {
        return Err("a brush needs at least 1 point".into());
    }
    for (index, dab) in spec.points.iter().enumerate() {
        if dab.at.iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
            return Err(format!(
                "point {index} must be finite and within 4096 cells"
            ));
        }
        if !(0.5..=MAX_BRUSH_RADIUS).contains(&dab.radius) {
            return Err(format!(
                "point {index} radius {} must be 0.5..={MAX_BRUSH_RADIUS}",
                dab.radius
            ));
        }
        if !(0.0..=1.0).contains(&dab.strength) {
            return Err(format!("point {index} strength must be 0..=1"));
        }
        if spec.op == BrushOp::Grab {
            let reach = len(spec.vector) * dab.strength;
            if reach > dab.radius {
                return Err(format!(
                    "point {index}: the grab moves {reach:.1} cells, more than its radius {}; use a larger radius or several points",
                    dab.radius
                ));
            }
        }
    }
    if spec.op == BrushOp::Grab && len(spec.vector) < 1.0e-9 {
        return Err("grab needs a vector [dx, dy, dz] to move the cells by".into());
    }
    if let Some(spacing) = spec.spacing {
        if !(0.0..=256.0).contains(&spacing) {
            return Err("spacing must be 0..=256 cells".into());
        }
    }
    let dabs = if spec.op == BrushOp::Grab {
        spec.points.clone()
    } else {
        lay(&spec.points, spec.spacing)?
    };
    let mut work = 0i64;
    for dab in &dabs {
        let side = 2 * (dab.radius.ceil() as i64 + 3) + 1 + 2 * len(spec.vector).ceil() as i64;
        work += side * side * side;
    }
    if work > MAX_BRUSH_WORK {
        return Err(format!(
            "the stroke is too large to test ({work} cells over its dabs, limit {MAX_BRUSH_WORK}); use fewer or smaller dabs"
        ));
    }
    let mut now = cells.clone();
    let mut touched: HashSet<Cell> = HashSet::new();
    for dab in &dabs {
        let edits = dab_edits(&now, spec, dab)?;
        for (c, slot) in edits {
            touched.insert(c);
            if slot == 0 {
                now.remove(&c);
            } else {
                now.insert(c, slot);
            }
        }
    }
    let mut done = Brushed {
        writes: Vec::new(),
        dabs: dabs.len(),
        added: 0,
        removed: 0,
    };
    for c in touched {
        let (old, new) = (cells.get(&c).copied(), now.get(&c).copied());
        if old == new {
            continue;
        }
        match (old, new) {
            (None, Some(_)) => done.added += 1,
            (Some(_), None) => done.removed += 1,
            _ => {}
        }
        done.writes
            .push((c.0, c.1, c.2, i32::from(new.unwrap_or(0))));
    }
    done.writes.sort_unstable();
    Ok(done)
}

/// The dabs along `points`: the points, and between two points farther
/// apart than the spacing, evenly spaced dabs with the radius and strength
/// running between them.
fn lay(points: &[Dab], spacing: Option<f64>) -> Result<Vec<Dab>, String> {
    let mut out = vec![points[0]];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let gap = spacing.unwrap_or(a.radius.min(b.radius) * 0.5);
        let d = len(sub(b.at, a.at));
        let steps = if gap > 0.0 {
            ((d / gap).ceil() as usize).max(1)
        } else {
            1
        };
        if out.len() + steps > MAX_DABS {
            return Err(format!(
                "the stroke lays more than {MAX_DABS} dabs; use fewer points or a larger spacing"
            ));
        }
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            out.push(Dab {
                at: [
                    a.at[0] + (b.at[0] - a.at[0]) * t,
                    a.at[1] + (b.at[1] - a.at[1]) * t,
                    a.at[2] + (b.at[2] - a.at[2]) * t,
                ],
                radius: a.radius + (b.radius - a.radius) * t,
                strength: a.strength + (b.strength - a.strength) * t,
            });
        }
    }
    Ok(out)
}

/// The cells one dab changes, against `cells` before it.
fn dab_edits(
    cells: &HashMap<Cell, u8>,
    spec: &BrushSpec,
    dab: &Dab,
) -> Result<Vec<(Cell, u8)>, String> {
    let r = dab.radius;
    let push = dab.strength * r * 0.5;
    let reach = r.ceil() as i32 + push.ceil() as i32 + 2 + len(spec.vector).ceil() as i32;
    let c = (
        dab.at[0].round() as i32,
        dab.at[1].round() as i32,
        dab.at[2].round() as i32,
    );
    let grid = Grid::new(
        cells,
        (c.0 - reach, c.1 - reach, c.2 - reach),
        (c.0 + reach, c.1 + reach, c.2 + reach),
    )?;
    let weight = |cell: Cell| {
        let p = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
        spec.falloff.weight(len(sub(p, dab.at)) / r)
    };
    let free = |slot: u8| slot == 0 || allowed(&spec.only, slot);
    let mut edits = Vec::new();
    match spec.op {
        BrushOp::Inflate => {
            let seeds = |i: usize| grid.slots[i] != 0 && allowed(&spec.only, grid.slots[i]);
            let (dist, from) = nearest(&grid, seeds, push as f32 + 1.0);
            for i in 0..grid.len() {
                let cell = grid.cell(i);
                let w = weight(cell);
                if grid.slots[i] != 0 || w <= 0.0 || from[i] == u32::MAX {
                    continue;
                }
                if f64::from(dist[i]) - 0.5 <= push * w + 1.0e-9 {
                    let slot = spec.palette.unwrap_or(grid.slots[from[i] as usize]);
                    edits.push((cell, slot));
                }
            }
        }
        BrushOp::Deflate => {
            let (dist, _) = nearest(&grid, |i| grid.slots[i] == 0, push as f32 + 1.0);
            for i in 0..grid.len() {
                let cell = grid.cell(i);
                let w = weight(cell);
                if grid.slots[i] == 0 || !allowed(&spec.only, grid.slots[i]) || w <= 0.0 {
                    continue;
                }
                if f64::from(dist[i]) - 0.5 <= push * w + 1.0e-9 {
                    edits.push((cell, 0));
                }
            }
        }
        BrushOp::Smooth => {
            let passes = 1 + (dab.strength * 3.0).floor() as usize;
            let mut slots = grid.slots.clone();
            for _ in 0..passes {
                let mut next = slots.clone();
                for i in 0..grid.len() {
                    let cell = grid.cell(i);
                    // How far toward the blur a cell moves.
                    let w = (2.0 * weight(cell) * dab.strength).min(1.0);
                    if w <= 0.0 || !free(slots[i]) {
                        continue;
                    }
                    let (blur, colour) = blur(&grid, &slots, cell, &spec.only);
                    if slots[i] != 0 && w * (1.0 - blur) > 0.5 {
                        next[i] = 0;
                    } else if slots[i] == 0 && w * blur > 0.5 {
                        if let Some(colour) = colour {
                            next[i] = spec.palette.unwrap_or(colour);
                        }
                    }
                }
                slots = next;
            }
            for i in 0..grid.len() {
                if slots[i] != grid.slots[i] {
                    edits.push((grid.cell(i), slots[i]));
                }
            }
        }
        BrushOp::Flatten => {
            // The mean plane of the surface cells under the dab.
            let mut centroid = [0.0; 3];
            let mut normal = [0.0; 3];
            let mut total = 0.0;
            for i in 0..grid.len() {
                let cell = grid.cell(i);
                let w = weight(cell);
                if grid.slots[i] == 0 || w <= 0.0 {
                    continue;
                }
                let mut out = [0.0; 3];
                for (k, step) in super::surface::FACES.iter().enumerate() {
                    if grid.slot(super::add(cell, *step)) == 0 {
                        out[k / 2] += if k % 2 == 0 { 1.0 } else { -1.0 };
                    }
                }
                if out == [0.0; 3] {
                    continue;
                }
                for k in 0..3 {
                    centroid[k] += w * [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)][k];
                    normal[k] += w * out[k];
                }
                total += w;
            }
            let n = len(normal);
            if total <= 0.0 || n < 1.0e-9 {
                return Ok(edits);
            }
            let centroid = [
                centroid[0] / total,
                centroid[1] / total,
                centroid[2] / total,
            ];
            let normal = [normal[0] / n, normal[1] / n, normal[2] / n];
            let seeds = |i: usize| grid.slots[i] != 0 && allowed(&spec.only, grid.slots[i]);
            let (_, from) = nearest(&grid, seeds, 2.0 * r as f32 + 2.0);
            let height = |cell: Cell| {
                (f64::from(cell.0) - centroid[0]) * normal[0]
                    + (f64::from(cell.1) - centroid[1]) * normal[1]
                    + (f64::from(cell.2) - centroid[2]) * normal[2]
            };
            // The highest bump and the deepest pit under the dab: each moves
            // toward the plane by strength × falloff of its height.
            let (mut top, mut pit) = (0.5f64, 0.5f64);
            for i in 0..grid.len() {
                let cell = grid.cell(i);
                if weight(cell) <= 0.0 {
                    continue;
                }
                let h = height(cell);
                if grid.slots[i] != 0 && allowed(&spec.only, grid.slots[i]) {
                    top = top.max(h);
                } else if grid.slots[i] == 0 {
                    pit = pit.max(-h);
                }
            }
            for i in 0..grid.len() {
                let cell = grid.cell(i);
                let w = weight(cell);
                if w <= 0.0 {
                    continue;
                }
                let h = height(cell);
                let keep = 1.0 - dab.strength * w;
                let slot = grid.slots[i];
                if slot != 0 && allowed(&spec.only, slot) && h > 0.5 + keep * (top - 0.5) {
                    edits.push((cell, 0));
                } else if slot == 0 && -h > 0.5 + keep * (pit - 0.5) && from[i] != u32::MAX {
                    edits.push((cell, spec.palette.unwrap_or(grid.slots[from[i] as usize])));
                }
            }
        }
        BrushOp::Grab => {
            let v = [
                spec.vector[0] * dab.strength,
                spec.vector[1] * dab.strength,
                spec.vector[2] * dab.strength,
            ];
            for i in 0..grid.len() {
                let q = grid.cell(i);
                if !free(grid.slots[i]) {
                    continue;
                }
                // Where the cell now at q came from: p + v w(p) = q, with
                // p = q - v a, so a = w(q - v a), found by halving.
                let target = [f64::from(q.0), f64::from(q.1), f64::from(q.2)];
                let back = |a: f64| {
                    [
                        target[0] - v[0] * a,
                        target[1] - v[1] * a,
                        target[2] - v[2] * a,
                    ]
                };
                let moved = |a: f64| spec.falloff.weight(len(sub(back(a), dab.at)) / r);
                if moved(0.0) <= 0.0 && moved(1.0) <= 0.0 {
                    continue;
                }
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..24 {
                    let mid = (lo + hi) * 0.5;
                    if mid < moved(mid) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let p = back((lo + hi) * 0.5);
                let from = (
                    p[0].round() as i32,
                    p[1].round() as i32,
                    p[2].round() as i32,
                );
                let slot = grid.slot(from);
                let slot = if free(slot) { slot } else { 0 };
                if slot != grid.slots[i] {
                    edits.push((q, slot));
                }
            }
        }
    }
    Ok(edits)
}

/// The weighted share of solid cells around `cell` (itself 8, faces 4,
/// edges 2, corners 1), and the colour most of its changeable solid
/// neighbours hold.
fn blur(grid: &Grid, slots: &[u8], cell: Cell, only: &[u8]) -> (f64, Option<u8>) {
    let at = |c: Cell| grid.at(c).map_or(0, |i| slots[i]);
    let mut solid = if at(cell) != 0 { 8.0 } else { 0.0 };
    let mut tally: Vec<(u8, u32)> = Vec::new();
    for n in neighbours(cell) {
        let slot = at(n);
        if slot == 0 {
            continue;
        }
        let w = match distance(n, cell) {
            d if d < 1.1 => 4,
            d if d < 1.5 => 2,
            _ => 1,
        };
        solid += f64::from(w);
        if allowed(only, slot) {
            match tally.iter_mut().find(|t| t.0 == slot) {
                Some(t) => t.1 += w,
                None => tally.push((slot, w)),
            }
        }
    }
    let colour = tally
        .iter()
        .max_by_key(|t| (t.1, std::cmp::Reverse(t.0)))
        .map(|t| t.0);
    (solid / 64.0, colour)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
