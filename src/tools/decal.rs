//! MCP `decal`: lay one picture onto a curved surface around a point, so a
//! face's eyes and mouth keep their size and shape on a round head.
//!
//! `project_paint` casts parallel rays, so a picture stretches where the
//! surface turns away from the view. A decal instead unrolls the surface
//! around its centre (a discrete exponential map): surface cells are visited
//! outward in order of their distance along the surface, and each one takes
//! picture coordinates from its settled neighbours, stepped by the part of
//! the step that lies in their own tangent plane. The frame starts from the
//! centre's normal and `up`, and is carried from cell to cell with the
//! normal, so the picture's right and down follow the surface. Each inked
//! picture cell goes to the surface cell nearest its middle and every other
//! surface cell takes the ink under its own middle, `depth` cells into the
//! solid along its normal. Cells are never added or removed.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use super::silhouette::{quantize, Grid};
use super::{Colour, Ink, Mask};
use crate::state::Cell;

/// Most cells across or down a decal.
pub const MAX_DECAL_SIZE: f64 = 256.0;
/// Most cells a decal paints below the surface.
pub const MAX_DECAL_DEPTH: u32 = 16;
/// Largest `reach` of a decal's point.
pub const MAX_DECAL_REACH: f64 = 64.0;

/// Where a decal is centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// The surface cell nearest this point (a whole number is a cell's
    /// centre), within `reach`.
    Point([f64; 3]),
    /// The first solid cell a ray from `origin` along `direction` meets.
    Ray {
        origin: [f64; 3],
        direction: [f64; 3],
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecalSpec {
    pub mask: Mask,
    pub target: Target,
    /// Width in cells along the surface; the height follows the picture's
    /// aspect unless given.
    pub width: f64,
    pub height: Option<f64>,
    /// The picture's up, projected onto the surface at the centre.
    pub up: [f64; 3],
    pub colour: Colour,
    /// Share of a grid cell's footprint that must be inked, 0..1.
    pub coverage: f32,
    /// Cells into the solid along the normal, 1 being the surface alone.
    pub depth: u32,
    pub reach: f64,
    /// Degrees: the decal stops where the surface turns further than this
    /// from the centre's normal, so it does not wrap round a thin part.
    pub normal_limit: f64,
    /// Slots that may be repainted.
    pub only: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decaled {
    /// Every painted cell with its new slot.
    pub cells: Vec<(Cell, u8)>,
    /// The surface cell the decal is centred on.
    pub centre: Cell,
    /// The outward normal there.
    pub normal: [f64; 3],
    /// The picture's grid in cells, across and down.
    pub grid: [u32; 2],
    /// Grid cells the picture inks.
    pub covered: usize,
    /// Inked grid cells no surface cell landed on: the decal ran off the
    /// surface or past `normal_limit`.
    pub missed: usize,
    /// Surface cells unrolled.
    pub reached: usize,
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: V3) -> Option<V3> {
    let len = dot(a, a).sqrt();
    (len > 1e-9).then(|| a.map(|c| c / len))
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn pos(c: Cell) -> V3 {
    [f64::from(c.0), f64::from(c.1), f64::from(c.2)]
}

const FACES: [Cell; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

fn is_surface(cells: &HashMap<Cell, u8>, c: Cell) -> bool {
    cells.contains_key(&c)
        && FACES
            .iter()
            .any(|d| !cells.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2)))
}

/// The outward normal at a surface cell: the mean direction of the empty
/// cells within three cells, or `None` when they cancel.
pub(crate) fn smooth_normal(cells: &HashMap<Cell, u8>, c: Cell) -> Option<V3> {
    let mut sum = [0.0f64; 3];
    for dz in -3i32..=3 {
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let r2 = dx * dx + dy * dy + dz * dz;
                if r2 == 0 || r2 > 10 {
                    continue;
                }
                if cells.contains_key(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                    continue;
                }
                let len = f64::from(r2).sqrt();
                sum[0] += f64::from(dx) / len;
                sum[1] += f64::from(dy) / len;
                sum[2] += f64::from(dz) / len;
            }
        }
    }
    if dot(sum, sum) < 0.25 {
        // Fall back to the open faces.
        let mut open = [0.0f64; 3];
        for d in FACES {
            if !cells.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2)) {
                open = [
                    open[0] + f64::from(d.0),
                    open[1] + f64::from(d.1),
                    open[2] + f64::from(d.2),
                ];
            }
        }
        return normalize(open);
    }
    normalize(sum)
}

/// The surface cell a target lands on.
pub(crate) fn target_cell(
    cells: &HashMap<Cell, u8>,
    target: Target,
    reach: f64,
) -> Result<Cell, String> {
    match target {
        Target::Point(at) => {
            if at.iter().any(|v| !v.is_finite()) {
                return Err("point must be finite".into());
            }
            let r = reach.ceil() as i32;
            let base = at.map(|v| v.round() as i32);
            let mut best: Option<(f64, Cell)> = None;
            for dz in -r..=r {
                for dy in -r..=r {
                    for dx in -r..=r {
                        let c = (base[0] + dx, base[1] + dy, base[2] + dz);
                        let d = dot(sub(pos(c), at), sub(pos(c), at)).sqrt();
                        if d > reach + 1e-9 || !is_surface(cells, c) {
                            continue;
                        }
                        if best.is_none_or(|(bd, bc)| d < bd - 1e-9 || (d < bd + 1e-9 && c < bc)) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            best.map(|(_, c)| c)
                .ok_or_else(|| format!("no surface cell within reach {reach} of the point"))
        }
        Target::Ray { origin, direction } => {
            if origin
                .iter()
                .chain(direction.iter())
                .any(|v| !v.is_finite())
            {
                return Err("ray origin and direction must be finite".into());
            }
            let dir = normalize(direction).ok_or("ray direction must not be zero")?;
            let mut t = 0.0;
            while t < 4096.0 {
                let p = [0, 1, 2].map(|a| origin[a] + dir[a] * t);
                let c = (
                    p[0].round() as i32,
                    p[1].round() as i32,
                    p[2].round() as i32,
                );
                if cells.contains_key(&c) {
                    return Ok(c);
                }
                t += 0.1;
            }
            Err("the ray meets no solid cell".into())
        }
    }
}

/// Lay `spec`'s picture onto `cells`. `palette` quantises image colours.
pub fn decal(
    cells: &HashMap<Cell, u8>,
    spec: &DecalSpec,
    palette: &[[u8; 3]; 256],
) -> Result<Decaled, String> {
    if !(0.0..=1.0).contains(&spec.coverage) {
        return Err("coverage must be 0..1".into());
    }
    if !(1..=MAX_DECAL_DEPTH).contains(&spec.depth) {
        return Err(format!("depth must be 1..={MAX_DECAL_DEPTH}"));
    }
    if !(0.0..=MAX_DECAL_REACH).contains(&spec.reach) {
        return Err(format!("reach must be 0..={MAX_DECAL_REACH}"));
    }
    if !(1.0..=180.0).contains(&spec.normal_limit) {
        return Err("normalLimit must be 1..180 degrees".into());
    }
    let width = spec.width;
    let height = spec
        .height
        .unwrap_or_else(|| width * f64::from(spec.mask.height) / f64::from(spec.mask.width.max(1)));
    for (what, n) in [("width", width), ("height", height)] {
        if !(n >= 1.0 && n <= MAX_DECAL_SIZE) {
            return Err(format!(
                "the decal's {what} must be 1..={MAX_DECAL_SIZE} cells, not {n:.2}"
            ));
        }
    }
    match &spec.colour {
        Colour::Views { slots } => {
            if slots.is_empty() || slots.contains(&0) {
                return Err("slots must be palette indices 1..255".into());
            }
            if spec.mask.pixels.contains(&Some(Ink::Plain)) {
                return Err(
                    "a rows character has no legend entry: add it to legend or pass palette".into(),
                );
            }
        }
        Colour::Slot(0) => return Err("palette must be 1..255".into()),
        Colour::Slot(_) => {}
    }
    if let Some(only) = &spec.only {
        if only.is_empty() || only.contains(&0) {
            return Err("only must be palette indices 1..255".into());
        }
    }
    if cells.is_empty() {
        return Err("the active object has no cells to paint".into());
    }
    let centre = target_cell(cells, spec.target, spec.reach)?;
    let n0 = smooth_normal(cells, centre).ok_or("the surface at the point has no outward side")?;
    let up = normalize(spec.up).ok_or("up must not be zero")?;
    // The picture's up on the surface; a normal along `up` takes +Y.
    let up_t = normalize(sub(up, n0.map(|c| c * dot(up, n0))))
        .or_else(|| {
            let y = [0.0, 1.0, 0.0];
            normalize(sub(y, n0.map(|c| c * dot(y, n0))))
        })
        .ok_or("up is undefined at the point")?;
    let r0 = cross(up_t, n0);
    let d0 = cross(r0, n0);

    let cols = (width.round() as u32).max(1);
    let rows = (height.round() as u32).max(1);
    let grid = Grid::build(&spec.mask, cols, rows, spec.coverage);
    let limit = spec.normal_limit.to_radians().cos();
    // Steps between cells overstate the distance along a slope (a stair is
    // longer than its slope), so the walk goes further than the picture.
    let radius = 0.75 * (width * width + height * height).sqrt() + 3.0;

    // The unrolling: settled cells with picture coordinates and frames.
    struct Settled {
        uv: [f64; 2],
        right: V3,
        down: V3,
        normal: V3,
    }
    let mut settled: HashMap<Cell, Settled> = HashMap::new();
    let mut best: HashMap<Cell, f64> = HashMap::new();
    let key = |g: f64| Reverse((g * 1e6) as i64);
    let mut heap: BinaryHeap<(Reverse<i64>, Cell)> = BinaryHeap::new();
    heap.push((key(0.0), centre));
    best.insert(centre, 0.0);
    let mut normals: HashMap<Cell, Option<V3>> = HashMap::new();
    normals.insert(centre, Some(n0));
    while let Some((Reverse(_), q)) = heap.pop() {
        if settled.contains_key(&q) {
            continue;
        }
        let g = best[&q];
        let nq = normals
            .entry(q)
            .or_insert_with(|| smooth_normal(cells, q))
            .unwrap_or(n0);
        let entry = if q == centre {
            Settled {
                uv: [0.0, 0.0],
                right: r0,
                down: d0,
                normal: n0,
            }
        } else {
            // Coordinates from every settled neighbour, nearer ones weighing
            // more; the frame from the nearest-to-centre one.
            let (mut su, mut sv, mut sw) = (0.0, 0.0, 0.0);
            let mut parent: Option<(f64, Cell)> = None;
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let s = (q.0 + dx, q.1 + dy, q.2 + dz);
                        let Some(from) = settled.get(&s) else {
                            continue;
                        };
                        // The step's part along the surface: a stair of
                        // cells on a slope adds up to the slope's length.
                        let w = sub(pos(q), pos(s));
                        let t = [dot(w, from.right), dot(w, from.down)];
                        let weight = 1.0 / dot(w, w).sqrt();
                        su += (from.uv[0] + t[0]) * weight;
                        sv += (from.uv[1] + t[1]) * weight;
                        sw += weight;
                        let gs = best[&s];
                        if parent.is_none_or(|(pg, pc)| gs < pg || (gs == pg && s < pc)) {
                            parent = Some((gs, s));
                        }
                    }
                }
            }
            let Some((_, p)) = parent else {
                continue;
            };
            let from = &settled[&p];
            let right = normalize(sub(from.right, nq.map(|c| c * dot(from.right, nq))))
                .unwrap_or(from.right);
            Settled {
                uv: [su / sw, sv / sw],
                right,
                down: cross(right, nq),
                normal: nq,
            }
        };
        settled.insert(q, entry);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    let n = (q.0 + dx, q.1 + dy, q.2 + dz);
                    if settled.contains_key(&n) || !is_surface(cells, n) {
                        continue;
                    }
                    let step = f64::from(dx * dx + dy * dy + dz * dz).sqrt();
                    let gn = g + step;
                    if gn > radius {
                        continue;
                    }
                    let nn = *normals.entry(n).or_insert_with(|| smooth_normal(cells, n));
                    if nn.is_none_or(|nn| dot(nn, n0) < limit - 1e-9) {
                        continue;
                    }
                    if best.get(&n).is_none_or(|old| gn < *old - 1e-12) {
                        best.insert(n, gn);
                        heap.push((key(gn), n));
                    }
                }
            }
        }
    }

    let mut lookup: HashMap<[u8; 3], u8> = HashMap::new();
    let slot_of = |ink: Ink, lookup: &mut HashMap<[u8; 3], u8>| -> u8 {
        match (&spec.colour, ink) {
            (Colour::Slot(slot), _) => *slot,
            (Colour::Views { .. }, Ink::Slot(slot)) => slot,
            (Colour::Views { slots }, Ink::Rgb(color)) => *lookup
                .entry(color)
                .or_insert_with(|| quantize(color, palette, slots)),
            (Colour::Views { slots }, Ink::Plain) => slots[0],
        }
    };
    let may_paint = |slot: u8| spec.only.as_ref().is_none_or(|only| only.contains(&slot));
    let ink_at = |index: usize| grid.inside[index].then(|| grid.ink[index]).flatten();
    let index_of = |u: f64, v: f64| -> Option<usize> {
        let col = (u + f64::from(cols) / 2.0).floor();
        let row = (v + f64::from(rows) / 2.0).floor();
        (col >= 0.0 && row >= 0.0 && col < f64::from(cols) && row < f64::from(rows))
            .then(|| (row as u32 * cols + col as u32) as usize)
    };
    let mut order: Vec<&Cell> = settled.keys().collect();
    order.sort_unstable();
    // Each inked picture cell takes a surface cell of its own near its
    // middle, so a one-cell pupil is never lost between the surface's
    // cells: the picture cells with the fewest surface cells within a cell
    // of their middle choose first, each the nearest one still free.
    let mut candidates: Vec<Vec<(f64, Cell)>> = vec![Vec::new(); (cols * rows) as usize];
    for q in &order {
        let uv = settled[*q].uv;
        let (cu, cv) = (uv[0] + f64::from(cols) / 2.0, uv[1] + f64::from(rows) / 2.0);
        let (r0, r1) = ((cv - 1.5).floor().max(0.0), (cv + 1.5).floor());
        let (c0, c1) = ((cu - 1.5).floor().max(0.0), (cu + 1.5).floor());
        if r1 < 0.0 || c1 < 0.0 {
            continue;
        }
        for row in r0 as u32..=(r1 as u32).min(rows - 1) {
            for col in c0 as u32..=(c1 as u32).min(cols - 1) {
                let index = (row * cols + col) as usize;
                if ink_at(index).is_none() {
                    continue;
                }
                let d = (cu - f64::from(col) - 0.5).hypot(cv - f64::from(row) - 0.5);
                if d <= 1.0 {
                    candidates[index].push((d, **q));
                }
            }
        }
    }
    let mut picks: Vec<usize> = (0..candidates.len())
        .filter(|i| !candidates[*i].is_empty())
        .collect();
    picks.sort_by_key(|i| (candidates[*i].len(), *i));
    let mut claimed: HashMap<Cell, (f64, usize)> = HashMap::new();
    for index in picks {
        let list = &mut candidates[index];
        list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if let Some((d, q)) = list.iter().find(|(_, q)| !claimed.contains_key(q)) {
            claimed.insert(*q, (*d, index));
        }
    }
    let reachable: Vec<bool> = candidates.iter().map(|list| !list.is_empty()).collect();
    let mut hit = vec![false; (cols * rows) as usize];
    let mut painted: HashMap<Cell, u8> = HashMap::new();
    for q in order {
        let s = &settled[q];
        // A claimed cell takes its picture cell; any other takes the ink
        // under its middle.
        let index = match claimed.get(q) {
            Some((_, index)) => Some(*index),
            None => index_of(s.uv[0], s.uv[1]),
        };
        let Some(index) = index else {
            continue;
        };
        let Some(ink) = ink_at(index) else {
            continue;
        };
        hit[index] = true;
        let slot = slot_of(ink, &mut lookup);
        for k in 0..spec.depth {
            let at = [0, 1, 2].map(|a| pos(*q)[a] - s.normal[a] * f64::from(k));
            let c = (
                at[0].round() as i32,
                at[1].round() as i32,
                at[2].round() as i32,
            );
            match cells.get(&c) {
                Some(old) if may_paint(*old) => {
                    painted.entry(c).or_insert(slot);
                }
                _ => {}
            }
        }
    }
    let covered = grid.inside.iter().filter(|inside| **inside).count();
    let missed = (0..hit.len())
        .filter(|i| ink_at(*i).is_some() && !reachable[*i])
        .count();
    let mut cells_out: Vec<(Cell, u8)> = painted.into_iter().collect();
    cells_out.sort_unstable();
    Ok(Decaled {
        cells: cells_out,
        centre,
        normal: n0,
        grid: [cols, rows],
        covered,
        missed,
        reached: settled.len(),
    })
}

#[cfg(test)]
#[path = "../../tests/decal.rs"]
mod tests;
