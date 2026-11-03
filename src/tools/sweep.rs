//! The sweep behind MCP `sweep`: a 2D cross-section filled along a
//! polyline or a Catmull-Rom curve, kept perpendicular to it.
//!
//! The profile's `u` runs to the left of the path (seen from above, walking
//! along it) and `v` up, so a wall profile stands on the path. The curve is
//! cut into thin slabs, each the profile in the plane across the path at one
//! sample; slabs overlap, and extra slabs turn about a sharp bend or a fast
//! twist so the outside of a bend has no gaps. The nearest slab decides
//! each cell, so the overlap never adds a layer.

use std::collections::HashMap;

use crate::state::Cell;

/// Most control points in one sweep.
pub const MAX_SWEEP_POINTS: usize = 1024;
/// Most slabs after sampling.
const MAX_SLABS: usize = 200_000;
/// Most cell tests summed over the slabs.
const MAX_SWEEP_WORK: i64 = 96_000_000;
/// Most cells one sweep covers.
const MAX_SWEEP_CELLS: usize = 2_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Profile {
    /// A closed polygon of `[u, v]` points, at least 3.
    Polygon(Vec<[f64; 2]>),
    /// A rectangle `width` across (centred on the path) and `height` up from
    /// it, its corners rounded by `radius`.
    Rect {
        width: f64,
        height: f64,
        radius: f64,
    },
}

impl Profile {
    fn check(&self) -> Result<(), String> {
        match self {
            Profile::Polygon(points) => {
                if points.len() < 3 || points.len() > 256 {
                    return Err("profile needs 3 to 256 [u, v] points".into());
                }
                if points
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 512.0)
                {
                    return Err("profile points must be finite and within 512 cells".into());
                }
            }
            Profile::Rect {
                width,
                height,
                radius,
            } => {
                if !(0.5..=512.0).contains(width) || !(0.5..=512.0).contains(height) {
                    return Err("width and height must be 0.5..=512".into());
                }
                if !(0.0..=width.min(*height) / 2.0).contains(radius) {
                    return Err(
                        "radius must be 0 up to half the smaller of width and height".into(),
                    );
                }
            }
        }
        Ok(())
    }

    /// `(u_min, u_max, v_min, v_max)`.
    fn bounds(&self) -> [f64; 4] {
        match self {
            Profile::Polygon(points) => {
                let mut b = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
                for p in points {
                    b[0] = b[0].min(p[0]);
                    b[1] = b[1].max(p[0]);
                    b[2] = b[2].min(p[1]);
                    b[3] = b[3].max(p[1]);
                }
                b
            }
            Profile::Rect { width, height, .. } => [-width / 2.0, width / 2.0, 0.0, *height],
        }
    }

    fn contains(&self, u: f64, v: f64) -> bool {
        match self {
            Profile::Polygon(points) => {
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let (a, b) = (points[i], points[j]);
                    if (a[1] > v) != (b[1] > v)
                        && u < (b[0] - a[0]) * (v - a[1]) / (b[1] - a[1]) + a[0]
                    {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
            Profile::Rect {
                width,
                height,
                radius,
            } => {
                let (hw, hh) = (width / 2.0, height / 2.0);
                let (du, dv) = (u.abs() - (hw - radius), (v - hh).abs() - (hh - radius));
                if du > *radius || dv > *radius {
                    return false;
                }
                du <= 0.0 || dv <= 0.0 || du * du + dv * dv <= radius * radius
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepPoint {
    /// Local cell coordinates. A whole number is the centre of that cell.
    pub at: [f64; 3],
    /// Degrees the profile turns about the path, counter-clockwise looking
    /// along it.
    pub twist: f64,
    /// Profile scale, 0.05..=16.
    pub scale: f64,
}

/// One cell inside a sweep, with where it sits for patterns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepCell {
    pub cell: Cell,
    /// Distance along the path from the first point, in cells.
    pub along: f64,
    /// Profile coordinates before twist and scale.
    pub u: f64,
    pub v: f64,
}

#[derive(Clone, Copy)]
struct Slab {
    at: [f64; 3],
    tangent: [f64; 3],
    left: [f64; 3],
    up: [f64; 3],
    along: f64,
    twist: f64,
    scale: f64,
    half: f64,
}

/// Every cell whose centre lies inside the swept profile, in the plane
/// across the path at the slab nearest that centre.
pub fn sweep(
    points: &[SweepPoint],
    curve: bool,
    profile: &Profile,
) -> Result<Vec<SweepCell>, String> {
    profile.check()?;
    if points.len() < 2 {
        return Err("a sweep needs at least 2 points".into());
    }
    if points.len() > MAX_SWEEP_POINTS {
        return Err(format!(
            "a sweep takes at most {MAX_SWEEP_POINTS} points; split it into several calls"
        ));
    }
    for (index, point) in points.iter().enumerate() {
        if point.at.iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
            return Err(format!(
                "point {index} must be finite and within 4096 cells"
            ));
        }
        if !point.twist.is_finite() || !(0.05..=16.0).contains(&point.scale) {
            return Err(format!(
                "point {index} needs a finite twist and a scale of 0.05..=16"
            ));
        }
    }
    let bounds = profile.bounds();
    let reach = bounds.iter().map(|v| v.abs()).fold(0.0f64, f64::max) * std::f64::consts::SQRT_2;
    let max_scale = points.iter().map(|p| p.scale).fold(0.0f64, f64::max);
    let slabs = slabs(points, curve, reach * max_scale)?;
    let mut work = 0i64;
    let boxes: Vec<([i32; 3], [i32; 3])> = slabs
        .iter()
        .map(|slab| {
            let b = slab_box(slab, &bounds);
            work += (0..3)
                .map(|i| i64::from(b.1[i] - b.0[i]) + 1)
                .product::<i64>();
            b
        })
        .collect();
    if work > MAX_SWEEP_WORK {
        return Err(format!(
            "the sweep is too large to test ({work} cells over its slabs, limit {MAX_SWEEP_WORK}); make the profile smaller or the path shorter, or split it"
        ));
    }
    // Each cell is decided by the slab whose plane is nearest its centre, so
    // overlapping slabs never add a layer the nearest slab leaves out. The
    // first pass finds the cells some slab covers, the second keeps the
    // nearest slab's answer for each.
    let test = |slab: &Slab, x: i32, y: i32, z: i32| -> Option<(f64, f64, f64, f64)> {
        let offset = sub([f64::from(x), f64::from(y), f64::from(z)], slab.at);
        let along = dot(offset, slab.tangent).abs();
        if along > slab.half {
            return None;
        }
        let (sin, cos) = (-slab.twist.to_radians()).sin_cos();
        let (u, v) = (dot(offset, slab.left), dot(offset, slab.up));
        let pu = snap((u * cos - v * sin) / slab.scale);
        let pv = snap((u * sin + v * cos) / slab.scale);
        Some((along, slab.along, pu, pv))
    };
    let mut found: HashMap<Cell, (f64, Option<(f64, f64, f64)>)> = HashMap::new();
    for (slab, (min, max)) in slabs.iter().zip(&boxes) {
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    if found.contains_key(&(x, y, z)) {
                        continue;
                    }
                    if let Some((_, along, pu, pv)) = test(slab, x, y, z) {
                        if profile.contains(pu, pv) {
                            found.insert((x, y, z), (f64::MAX, Some((along, pu, pv))));
                        }
                    }
                }
            }
        }
        if found.len() > MAX_SWEEP_CELLS {
            return Err(format!(
                "the sweep covers more than {MAX_SWEEP_CELLS} cells; make it smaller"
            ));
        }
    }
    for (slab, (min, max)) in slabs.iter().zip(&boxes) {
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    let Some(entry) = found.get_mut(&(x, y, z)) else {
                        continue;
                    };
                    let Some((distance, along, pu, pv)) = test(slab, x, y, z) else {
                        continue;
                    };
                    if distance < entry.0 {
                        *entry = (
                            distance,
                            profile.contains(pu, pv).then_some((along, pu, pv)),
                        );
                    }
                }
            }
        }
    }
    let mut cells: Vec<SweepCell> = found
        .into_iter()
        .filter_map(|(cell, (_, hit))| hit.map(|(along, u, v)| SweepCell { cell, along, u, v }))
        .collect();
    cells.sort_by_key(|c| (c.cell.2, c.cell.1, c.cell.0));
    Ok(cells)
}

/// Positions about every half cell along the path, with turning slabs
/// added where the frame turns faster than half a cell at `reach`.
fn slabs(points: &[SweepPoint], curve: bool, reach: f64) -> Result<Vec<Slab>, String> {
    // Samples: position, tangent, twist, scale.
    let mut samples: Vec<([f64; 3], [f64; 3], f64, f64)> = Vec::new();
    let last = points.len() - 1;
    for span in 0..last {
        let (a, b) = (points[span], points[span + 1]);
        let chord = len(sub(b.at, a.at));
        if chord < 1.0e-9 {
            continue;
        }
        let steps = (chord * 2.0).ceil().max(1.0) as usize;
        if samples.len() + steps > MAX_SLABS {
            return Err("the path is too long; split it into several calls".into());
        }
        let first = if samples.is_empty() || !curve { 0 } else { 1 };
        for step in first..=steps {
            let t = step as f64 / steps as f64;
            let (at, tangent) = if curve {
                let p0 = points[span.saturating_sub(1)].at;
                let p3 = points[(span + 2).min(last)].at;
                catmull(p0, a.at, b.at, p3, t)
            } else {
                (lerp3(a.at, b.at, t), sub(b.at, a.at))
            };
            let tangent = normalize(tangent)
                .unwrap_or_else(|| normalize(sub(b.at, a.at)).unwrap_or([1.0, 0.0, 0.0]));
            samples.push((
                at,
                tangent,
                a.twist + (b.twist - a.twist) * t,
                a.scale + (b.scale - a.scale) * t,
            ));
        }
    }
    if samples.is_empty() {
        return Err("the sweep's points are all in one place".into());
    }
    // Extra slabs where the frame turns fast.
    let mut fine = vec![samples[0]];
    for pair in samples.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let angle = dot(a.1, b.1).clamp(-1.0, 1.0).acos() + (b.2 - a.2).to_radians().abs();
        let extra = ((angle * reach / 0.5).ceil() as usize).saturating_sub(1);
        if fine.len() + extra > MAX_SLABS {
            return Err("the path turns too often; split it into several calls".into());
        }
        for k in 1..=extra {
            let t = k as f64 / (extra + 1) as f64;
            let tangent = normalize(lerp3(a.1, b.1, t)).unwrap_or(b.1);
            fine.push((
                lerp3(a.0, b.0, t),
                tangent,
                a.2 + (b.2 - a.2) * t,
                a.3 + (b.3 - a.3) * t,
            ));
        }
        fine.push(b);
    }
    let mut out: Vec<Slab> = Vec::with_capacity(fine.len());
    let mut along = 0.0;
    let mut left_before: Option<[f64; 3]> = None;
    for (index, &(at, tangent, twist, scale)) in fine.iter().enumerate() {
        if index > 0 {
            along += len(sub(at, fine[index - 1].0));
        }
        let left = normalize(cross([0.0, 0.0, 1.0], tangent))
            .filter(|_| tangent[2].abs() < 0.995)
            .or_else(|| {
                left_before.and_then(|l| normalize(sub(l, scale3(tangent, dot(l, tangent)))))
            })
            .unwrap_or([0.0, 1.0, 0.0]);
        left_before = Some(left);
        let gap = |other: usize| len(sub(fine[other].0, at));
        let mut half: f64 = 0.5;
        if index > 0 {
            half = half.max(gap(index - 1) / 2.0 + 0.05);
        }
        if index + 1 < fine.len() {
            half = half.max(gap(index + 1) / 2.0 + 0.05);
        }
        out.push(Slab {
            at,
            tangent,
            left,
            up: cross(tangent, left),
            along,
            twist,
            scale,
            half,
        });
    }
    Ok(out)
}

/// The cells around one slab: its profile bounds across, `half` along.
fn slab_box(slab: &Slab, bounds: &[f64; 4]) -> ([i32; 3], [i32; 3]) {
    // The profile's bounding rectangle, turned by the twist and scaled.
    let (sin, cos) = slab.twist.to_radians().sin_cos();
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for pu in [bounds[0], bounds[1]] {
        for pv in [bounds[2], bounds[3]] {
            let su = slab.scale * (pu * cos - pv * sin);
            let sv = slab.scale * (pu * sin + pv * cos);
            for sa in [-slab.half, slab.half] {
                for i in 0..3 {
                    let p = slab.at[i] + slab.left[i] * su + slab.up[i] * sv + slab.tangent[i] * sa;
                    min[i] = min[i].min(p);
                    max[i] = max[i].max(p);
                }
            }
        }
    }
    (min.map(|v| v.floor() as i32), max.map(|v| v.ceil() as i32))
}

fn catmull(p0: [f64; 3], p1: [f64; 3], p2: [f64; 3], p3: [f64; 3], t: f64) -> ([f64; 3], [f64; 3]) {
    let (t2, t3) = (t * t, t * t * t);
    let mut at = [0.0; 3];
    let mut d = [0.0; 3];
    for i in 0..3 {
        let a = 2.0 * p1[i];
        let b = p2[i] - p0[i];
        let c = 2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i];
        let e = 3.0 * p1[i] - p0[i] - 3.0 * p2[i] + p3[i];
        at[i] = 0.5 * (a + b * t + c * t2 + e * t3);
        d[i] = 0.5 * (b + 2.0 * c * t + 3.0 * e * t2);
    }
    (at, d)
}

/// Rounds away the error the frame leaves, so a centre on a profile edge
/// (a whole-number `v` on a flat top) falls the same way at every slab.
fn snap(value: f64) -> f64 {
    let fine = (value * 1.0e6).round() / 1.0e6;
    if (value - fine).abs() < 1.0e-7 {
        fine
    } else {
        value
    }
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = len(a);
    (l > 1.0e-9).then(|| scale3(a, 1.0 / l))
}
