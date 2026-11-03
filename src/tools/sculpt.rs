//! Hand-sculpting shapes behind MCP `fill_tube`, `carve`, and `smooth`: a
//! tapered tube along a polyline or a Catmull-Rom curve, and a neighbour
//! majority filter that keeps colours.

use std::collections::HashMap;
use std::f64::consts::TAU;

use crate::state::Cell;

/// Most cells one tube, carve, or smooth may cover.
pub const MAX_SCULPT_CELLS: i64 = 2_000_000;
/// Most control points in one tube.
pub const MAX_TUBE_POINTS: usize = 1024;
/// Thinnest tube radius. Below half a cell a sloped tube leaves gaps.
pub const MIN_TUBE_RADIUS: f64 = 0.5;
/// Widest tube radius.
pub const MAX_TUBE_RADIUS: f64 = 128.0;
/// Most segments after a smooth curve is sampled.
const MAX_TUBE_SEGMENTS: usize = 8192;
/// Most cell tests summed over the segment boxes.
const MAX_TUBE_WORK: i64 = 64_000_000;
/// Most smoothing passes in one call.
pub const MAX_SMOOTH_ITERATIONS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TubePoint {
    /// Local cell coordinates. A whole number is the centre of that cell.
    pub at: [f64; 3],
    pub radius: f64,
}

/// One cell inside a tube, with where it sits on the tube for patterns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TubeCell {
    pub cell: Cell,
    /// Distance along the axis from the first point, in cells.
    pub along: f64,
    /// Angle around the axis as a fraction of a turn, 0..1. It follows a
    /// rotation-minimising frame, so stripes do not twist at bends.
    pub around: f64,
}

/// A tube's cross-section. Round is a circle of the point's radius. A flat
/// tube keeps that radius across its width and `aspect` of it through its
/// thickness, at least half a cell. The thickness runs along the frame's
/// normal, which is up for a level tube, so a flat tube lies flat; `roll`
/// turns it about the axis. A blade tapers the thickness from the full value
/// at one edge to half a cell at the other, a wedge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TubeShape {
    /// Thickness over width, 0.05..=1.
    pub aspect: f64,
    /// Radians about the axis.
    pub roll: f64,
    pub blade: bool,
}

impl TubeShape {
    #[cfg(test)]
    pub const ROUND: TubeShape = TubeShape {
        aspect: 1.0,
        roll: 0.0,
        blade: false,
    };

    fn is_round(self) -> bool {
        self.aspect >= 1.0 && !self.blade
    }
}

/// How a tube with several palette indices lays them out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TubePattern {
    /// Bands along the length, `period` cells each.
    Rings { period: f64 },
    /// `sectors` slices of the circumference, running along the tube.
    Stripes { sectors: u32 },
    /// A stable per-cell pick.
    Random { seed: u64 },
}

impl TubePattern {
    /// Which of `count` palette indices `cell` takes.
    pub fn pick(self, cell: &TubeCell, count: usize) -> usize {
        let index = match self {
            TubePattern::Rings { period } => (cell.along / period.max(1.0e-6)).floor() as usize,
            TubePattern::Stripes { sectors } => (cell.around * f64::from(sectors)).floor() as usize,
            TubePattern::Random { seed } => {
                let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
                for v in [cell.cell.0, cell.cell.1, cell.cell.2] {
                    h ^= u64::from(v as u32);
                    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                    h ^= h >> 31;
                }
                h as usize
            }
        };
        index % count.max(1)
    }
}

struct Segment {
    a: [f64; 3],
    b: [f64; 3],
    ra: f64,
    rb: f64,
    along: f64,
    normal: [f64; 3],
    binormal: [f64; 3],
}

/// Every cell whose centre lies within the tube, and every cell the axis
/// passes through. Each segment is a capsule
/// whose radius runs linearly between its ends, so joints and ends are round.
/// `curve` true passes a Catmull-Rom curve through the points; false joins
/// them with straight segments.
#[cfg(test)]
pub fn tube(points: &[TubePoint], curve: bool) -> Result<Vec<TubeCell>, String> {
    tube_shaped(points, curve, TubeShape::ROUND)
}

/// [`tube`] with a flat or blade cross-section.
pub fn tube_shaped(
    points: &[TubePoint],
    curve: bool,
    shape: TubeShape,
) -> Result<Vec<TubeCell>, String> {
    if !(0.05..=1.0).contains(&shape.aspect) || !shape.roll.is_finite() {
        return Err("aspect must be 0.05..=1 and roll a finite angle".into());
    }
    if points.len() < 2 {
        return Err("a tube needs at least 2 points".into());
    }
    if points.len() > MAX_TUBE_POINTS {
        return Err(format!(
            "a tube takes at most {MAX_TUBE_POINTS} points; split it into several calls"
        ));
    }
    for (index, point) in points.iter().enumerate() {
        if point.at.iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
            return Err(format!(
                "point {index} must be finite and within 4096 cells"
            ));
        }
        if !point.radius.is_finite() || !(MIN_TUBE_RADIUS..=MAX_TUBE_RADIUS).contains(&point.radius)
        {
            return Err(format!(
                "point {index} radius {} must be {MIN_TUBE_RADIUS}..={MAX_TUBE_RADIUS}",
                point.radius
            ));
        }
    }
    let path = if curve {
        sample_curve(points)
    } else {
        points.to_vec()
    };
    if path.len() - 1 > MAX_TUBE_SEGMENTS {
        return Err(format!(
            "the curve needs more than {MAX_TUBE_SEGMENTS} segments; split it into several calls"
        ));
    }
    let mut segments = segments(&path);
    if shape.roll != 0.0 {
        let (sin, cos) = shape.roll.sin_cos();
        for segment in &mut segments {
            let (n, b) = (segment.normal, segment.binormal);
            segment.normal = add(scale(n, cos), scale(b, sin));
            segment.binormal = sub(scale(b, cos), scale(n, sin));
        }
    }
    let mut work = 0i64;
    for segment in &segments {
        let (min, max) = segment_box(segment);
        work += (0..3)
            .map(|i| i64::from(max[i] - min[i]) + 1)
            .product::<i64>();
    }
    if work > MAX_TUBE_WORK {
        return Err(format!(
            "the tube is too large to test ({work} cells over its segments, limit {MAX_TUBE_WORK}); make it thinner or shorter, or split it"
        ));
    }
    let mut best: HashMap<Cell, (f64, f64, f64)> = HashMap::new();
    for segment in &segments {
        let (min, max) = segment_box(segment);
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    consider(&mut best, segment, shape, (x, y, z), false);
                }
            }
        }
        // The cells the axis passes through, so a thin sloped tube stays
        // connected where no cell centre falls within its radius.
        let d = sub(segment.b, segment.a);
        let steps = (dot(d, d).sqrt() * 4.0).ceil().max(1.0) as usize;
        for step in 0..=steps {
            let p = add(segment.a, scale(d, step as f64 / steps as f64));
            let cell = (
                p[0].round() as i32,
                p[1].round() as i32,
                p[2].round() as i32,
            );
            consider(&mut best, segment, shape, cell, true);
        }
        if best.len() as i64 > MAX_SCULPT_CELLS {
            return Err(format!(
                "the tube covers more than {MAX_SCULPT_CELLS} cells; make it thinner or shorter"
            ));
        }
    }
    let mut cells: Vec<TubeCell> = best
        .into_iter()
        .map(|(cell, (_, along, around))| TubeCell {
            cell,
            along,
            around,
        })
        .collect();
    cells.sort_by_key(|cell| (cell.cell.2, cell.cell.1, cell.cell.0));
    Ok(cells)
}

/// Catmull-Rom through every point, the ends repeated, sampled about once
/// per cell of chord. Radii run linearly along each span.
fn sample_curve(points: &[TubePoint]) -> Vec<TubePoint> {
    let last = points.len() - 1;
    let mut out = vec![points[0]];
    for span in 0..last {
        let p0 = points[span.saturating_sub(1)].at;
        let p1 = points[span].at;
        let p2 = points[span + 1].at;
        let p3 = points[(span + 2).min(last)].at;
        let chord = dot(sub(p2, p1), sub(p2, p1)).sqrt();
        let steps = (chord.ceil() as usize).clamp(1, MAX_TUBE_SEGMENTS + 1);
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            let mut at = [0.0; 3];
            for i in 0..3 {
                at[i] = 0.5
                    * (2.0 * p1[i]
                        + (p2[i] - p0[i]) * t
                        + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
                        + (3.0 * p1[i] - p0[i] - 3.0 * p2[i] + p3[i]) * t3);
            }
            let radius = points[span].radius + (points[span + 1].radius - points[span].radius) * t;
            out.push(TubePoint { at, radius });
        }
        if out.len() > MAX_TUBE_SEGMENTS + 1 {
            break;
        }
    }
    out
}

fn segments(path: &[TubePoint]) -> Vec<Segment> {
    let mut out = Vec::with_capacity(path.len() - 1);
    let mut along = 0.0;
    let mut normal: Option<[f64; 3]> = None;
    for pair in path.windows(2) {
        let d = sub(pair[1].at, pair[0].at);
        let len = dot(d, d).sqrt();
        let tangent = if len > 1.0e-9 {
            scale(d, 1.0 / len)
        } else {
            out.last()
                .map(|s: &Segment| cross(s.normal, s.binormal))
                .unwrap_or([1.0, 0.0, 0.0])
        };
        // Parallel transport: the previous normal, made perpendicular again.
        let carried = normal.and_then(|n| normalize(sub(n, scale(tangent, dot(n, tangent)))));
        let n = carried.unwrap_or_else(|| {
            let reference = if tangent[2].abs() < 0.9 {
                [0.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            normalize(sub(reference, scale(tangent, dot(reference, tangent))))
                .unwrap_or([0.0, 1.0, 0.0])
        });
        normal = Some(n);
        out.push(Segment {
            a: pair[0].at,
            b: pair[1].at,
            ra: pair[0].radius,
            rb: pair[1].radius,
            along,
            normal: n,
            binormal: cross(tangent, n),
        });
        along += len;
    }
    out
}

/// Keep `cell` when its centre is inside `segment` (or `force`), with the
/// deepest segment's position on the tube.
fn consider(
    best: &mut HashMap<Cell, (f64, f64, f64)>,
    segment: &Segment,
    shape: TubeShape,
    cell: Cell,
    force: bool,
) {
    let c = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
    let d = sub(segment.b, segment.a);
    let len2 = dot(d, d);
    let t = if len2 > 0.0 {
        (dot(sub(c, segment.a), d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let offset = sub(c, add(segment.a, scale(d, t)));
    let dist = dot(offset, offset).sqrt();
    let radius = segment.ra + (segment.rb - segment.ra) * t;
    let depth = if shape.is_round() {
        dist - radius
    } else {
        // Width along the binormal, thickness along the normal, and past an
        // end along the axis.
        let width = dot(offset, segment.binormal) / radius;
        let tangent = cross(segment.normal, segment.binormal);
        let axial = dot(offset, tangent) / radius;
        let mut thick = (radius * shape.aspect).max(0.5);
        if shape.blade {
            thick = (thick * (1.0 - width) * 0.5).max(0.5);
        }
        let through = dot(offset, segment.normal) / thick;
        let measure = if shape.blade {
            (width * width + axial * axial).sqrt().max(through.abs())
        } else {
            (width * width + axial * axial + through * through).sqrt()
        };
        (measure - 1.0) * radius
    };
    if (depth > 1.0e-9 && !force) || best.get(&cell).is_some_and(|found| found.0 <= depth) {
        return;
    }
    let around = if dist > 1.0e-9 {
        let turn = dot(offset, segment.binormal).atan2(dot(offset, segment.normal)) / TAU;
        turn.rem_euclid(1.0)
    } else {
        0.0
    };
    best.insert(cell, (depth, segment.along + len2.sqrt() * t, around));
}

fn segment_box(segment: &Segment) -> ([i32; 3], [i32; 3]) {
    let r = segment.ra.max(segment.rb);
    let mut min = [0; 3];
    let mut max = [0; 3];
    for i in 0..3 {
        min[i] = (segment.a[i].min(segment.b[i]) - r).floor() as i32;
        max[i] = (segment.a[i].max(segment.b[i]) + r).ceil() as i32;
    }
    (min, max)
}

#[derive(Debug, PartialEq)]
pub struct Smoothed {
    /// Cells that differ from the input, as `(x, y, z, palette)`; 0 removes.
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub filled: usize,
    pub removed: usize,
    /// Passes run. Fewer than asked when a pass changed nothing.
    pub iterations: u32,
}

/// [`smooth`] after checking that the box holds at most
/// [`MAX_SCULPT_CELLS`] cells.
pub fn smooth_checked(
    cells: &HashMap<Cell, u8>,
    min: Cell,
    max: Cell,
    iterations: u32,
    fill_at: i32,
    remove_at: i32,
) -> Result<Smoothed, String> {
    let count = (i64::from(max.0 - min.0) + 1)
        * (i64::from(max.1 - min.1) + 1)
        * (i64::from(max.2 - min.2) + 1);
    if count > MAX_SCULPT_CELLS {
        return Err(format!(
            "box is larger than {MAX_SCULPT_CELLS} cells; smooth a smaller region"
        ));
    }
    Ok(smooth(cells, min, max, iterations, fill_at, remove_at))
}

/// Majority filter over the 26 neighbours of each cell in the inclusive box
/// `min..=max`. An empty cell with at least `fill_at` solid neighbours is
/// filled with the colour most of them hold (ties go to the colour more face
/// neighbours hold, then the lower index). A solid cell with at most
/// `remove_at` solid neighbours is removed. Each pass reads the result of the
/// one before. Cells outside the box are read but never changed. `fill_at`
/// 27 never fills and `remove_at` -1 never removes.
pub fn smooth(
    cells: &HashMap<Cell, u8>,
    min: Cell,
    max: Cell,
    iterations: u32,
    fill_at: i32,
    remove_at: i32,
) -> Smoothed {
    let dims = [
        (max.0 - min.0 + 3) as usize,
        (max.1 - min.1 + 3) as usize,
        (max.2 - min.2 + 3) as usize,
    ];
    let index = |x: usize, y: usize, z: usize| (z * dims[1] + y) * dims[0] + x;
    let mut grid = vec![0u8; dims[0] * dims[1] * dims[2]];
    for (&(x, y, z), &slot) in cells {
        let gx = x - min.0 + 1;
        let gy = y - min.1 + 1;
        let gz = z - min.2 + 1;
        if (0..dims[0] as i32).contains(&gx)
            && (0..dims[1] as i32).contains(&gy)
            && (0..dims[2] as i32).contains(&gz)
        {
            grid[index(gx as usize, gy as usize, gz as usize)] = slot;
        }
    }
    let original = grid.clone();
    let mut passes = 0;
    for _ in 0..iterations {
        let mut next = grid.clone();
        let mut changed = false;
        for z in 1..dims[2] - 1 {
            for y in 1..dims[1] - 1 {
                for x in 1..dims[0] - 1 {
                    let here = grid[index(x, y, z)];
                    // (colour, neighbours, face neighbours)
                    let mut tally: [(u8, u8, u8); 26] = [(0, 0, 0); 26];
                    let mut kinds = 0;
                    let mut solid = 0i32;
                    for dz in 0..3 {
                        for dy in 0..3 {
                            for dx in 0..3 {
                                if dx == 1 && dy == 1 && dz == 1 {
                                    continue;
                                }
                                let slot = grid[index(x + dx - 1, y + dy - 1, z + dz - 1)];
                                if slot == 0 {
                                    continue;
                                }
                                solid += 1;
                                let face = u8::from(
                                    (dx != 1) as u8 + (dy != 1) as u8 + (dz != 1) as u8 == 1,
                                );
                                match tally[..kinds].iter_mut().find(|t| t.0 == slot) {
                                    Some(t) => {
                                        t.1 += 1;
                                        t.2 += face;
                                    }
                                    None => {
                                        tally[kinds] = (slot, 1, face);
                                        kinds += 1;
                                    }
                                }
                            }
                        }
                    }
                    if here == 0 && solid >= fill_at {
                        let colour = tally[..kinds]
                            .iter()
                            .max_by_key(|t| (t.1, t.2, std::cmp::Reverse(t.0)))
                            .map(|t| t.0);
                        if let Some(colour) = colour {
                            next[index(x, y, z)] = colour;
                            changed = true;
                        }
                    } else if here != 0 && solid <= remove_at {
                        next[index(x, y, z)] = 0;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
        passes += 1;
        grid = next;
    }
    let mut out = Smoothed {
        writes: Vec::new(),
        filled: 0,
        removed: 0,
        iterations: passes,
    };
    for z in 1..dims[2] - 1 {
        for y in 1..dims[1] - 1 {
            for x in 1..dims[0] - 1 {
                let i = index(x, y, z);
                if grid[i] == original[i] {
                    continue;
                }
                if grid[i] == 0 {
                    out.removed += 1;
                } else if original[i] == 0 {
                    out.filled += 1;
                }
                out.writes.push((
                    x as i32 - 1 + min.0,
                    y as i32 - 1 + min.1,
                    z as i32 - 1 + min.2,
                    i32::from(grid[i]),
                ));
            }
        }
    }
    out
}

pub(super) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let len = dot(a, a).sqrt();
    (len > 1.0e-9).then(|| scale(a, 1.0 / len))
}
