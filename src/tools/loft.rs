//! The loft behind MCP `loft`: a solid along a curve whose cross-section
//! changes from point to point. Each point's section is an ellipse with two
//! radii and a roll, cut flat on one side, with a noisy outline, so one call
//! makes a trunk that tapers, bends, and is egg-shaped and knobbly.

use std::collections::HashMap;
use std::f64::consts::TAU;

use super::sculpt::{
    add, cross, dot, normalize, scale, sub, TubeCell, MAX_SCULPT_CELLS, MAX_TUBE_POINTS,
};
use super::surface::noise3;
use crate::state::Cell;

/// Widest loft radius.
pub const MAX_LOFT_RADIUS: f64 = 128.0;
/// Thinnest loft radius.
pub const MIN_LOFT_RADIUS: f64 = 0.5;
/// Most samples along the path.
const MAX_LOFT_SAMPLES: usize = 16_384;
/// Most cell tests summed over the segment boxes.
const MAX_LOFT_WORK: i64 = 96_000_000;

/// One control point: where it is and the section there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoftPoint {
    pub at: [f64; 3],
    pub section: Section,
}

/// A cross-section. `rx` is the half-width across the path and `ry` the
/// half-height along the frame's normal, which is up for a level path;
/// `roll` turns both about the path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub rx: f64,
    pub ry: f64,
    /// Radians about the path.
    pub roll: f64,
    /// -1..=1: above 0 cuts the top flat at `ry * (1 - flat)` (1 halves the
    /// ellipse); below 0 cuts the bottom the same way.
    pub flat: f64,
    /// The outline's noise as a fraction of the radius, 0..=0.9.
    pub noise: f64,
    /// About how many lumps around the outline, 0..=64.
    pub frequency: f64,
}

impl Section {
    fn lerp(self, other: Section, t: f64) -> Section {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Section {
            rx: mix(self.rx, other.rx),
            ry: mix(self.ry, other.ry),
            roll: mix(self.roll, other.roll),
            flat: mix(self.flat, other.flat),
            noise: mix(self.noise, other.noise),
            frequency: mix(self.frequency, other.frequency),
        }
    }

    /// The farthest the outline reaches from the path.
    fn reach(self) -> f64 {
        self.rx.max(self.ry) * (1.0 + self.noise) + 0.5
    }
}

/// How the loft closes at its two ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caps {
    /// Cut square across the path at the end points.
    Flat,
    /// Rounded like a tube's ends.
    Round,
}

struct Sample {
    at: [f64; 3],
    section: Section,
    along: f64,
    tangent: [f64; 3],
    normal: [f64; 3],
}

/// Every cell whose centre is inside the loft, with where it sits along and
/// around the path for patterns. A cell is decided at the nearest point of
/// the path. `curve` true passes a Catmull-Rom curve through the points;
/// false joins them straight. Sections run linearly between the points.
/// `seed` picks the outline noise.
pub fn loft(
    points: &[LoftPoint],
    curve: bool,
    caps: Caps,
    seed: u64,
) -> Result<Vec<TubeCell>, String> {
    if points.len() < 2 {
        return Err("a loft needs at least 2 points".into());
    }
    if points.len() > MAX_TUBE_POINTS {
        return Err(format!(
            "a loft takes at most {MAX_TUBE_POINTS} points; split it into several calls"
        ));
    }
    for (index, point) in points.iter().enumerate() {
        if point.at.iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
            return Err(format!(
                "point {index} must be finite and within 4096 cells"
            ));
        }
        let s = point.section;
        for (name, r) in [("rx", s.rx), ("ry", s.ry)] {
            if !r.is_finite() || !(MIN_LOFT_RADIUS..=MAX_LOFT_RADIUS).contains(&r) {
                return Err(format!(
                    "point {index} {name} {r} must be {MIN_LOFT_RADIUS}..={MAX_LOFT_RADIUS}"
                ));
            }
        }
        if !s.roll.is_finite() {
            return Err(format!("point {index} roll must be a finite angle"));
        }
        if !(-1.0..=1.0).contains(&s.flat) {
            return Err(format!("point {index} flatTop must be -1..=1"));
        }
        if !(0.0..=0.9).contains(&s.noise) {
            return Err(format!("point {index} noise must be 0..=0.9"));
        }
        if !(0.0..=64.0).contains(&s.frequency) {
            return Err(format!("point {index} frequency must be 0..=64"));
        }
    }
    let samples = samples(points, curve)?;
    let last = samples.len() - 1;
    let mut work = 0i64;
    for pair in samples.windows(2) {
        let (min, max) = segment_box(&pair[0], &pair[1]);
        work += (0..3)
            .map(|i| i64::from(max[i] - min[i]) + 1)
            .product::<i64>();
    }
    if work > MAX_LOFT_WORK {
        return Err(format!(
            "the loft is too large to test ({work} cells over its segments, limit {MAX_LOFT_WORK}); make it thinner or shorter, or split it"
        ));
    }
    // The nearest point of the path to each cell: segment, t, distance².
    let mut nearest: HashMap<Cell, (u32, f64, f64)> = HashMap::new();
    for (index, pair) in samples.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let reach = a.section.reach().max(b.section.reach());
        let (min, max) = segment_box(a, b);
        let d = sub(b.at, a.at);
        let len2 = dot(d, d);
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                for x in min[0]..=max[0] {
                    let c = [f64::from(x), f64::from(y), f64::from(z)];
                    let t = if len2 > 0.0 {
                        (dot(sub(c, a.at), d) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let off = sub(c, add(a.at, scale(d, t)));
                    let dist2 = dot(off, off);
                    if dist2 > reach * reach {
                        continue;
                    }
                    let entry = nearest
                        .entry((x, y, z))
                        .or_insert((u32::MAX, 0.0, f64::MAX));
                    if dist2 < entry.2 {
                        *entry = (index as u32, t, dist2);
                    }
                }
            }
        }
        if nearest.len() as i64 > 4 * MAX_SCULPT_CELLS {
            return Err(format!(
                "the loft covers more than {MAX_SCULPT_CELLS} cells; make it thinner or shorter"
            ));
        }
    }
    let mut out = Vec::new();
    for (cell, (index, t, _)) in nearest {
        let index = index as usize;
        let (a, b) = (&samples[index], &samples[index + 1]);
        let c = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
        let at = add(a.at, scale(sub(b.at, a.at), t));
        let section = a.section.lerp(b.section, t);
        let along = a.along + (b.along - a.along) * t;
        let mut off = sub(c, at);
        // At a joint or an end the nearest point is a sample: measure the
        // section in the plane across the path there.
        let (tangent, normal) = if t <= 0.0 {
            (a.tangent, a.normal)
        } else if t >= 1.0 {
            (b.tangent, b.normal)
        } else {
            let d = normalize(sub(b.at, a.at)).unwrap_or(a.tangent);
            let n = normalize(sub(a.normal, scale(d, dot(a.normal, d)))).unwrap_or(a.normal);
            (d, n)
        };
        let axial = dot(off, tangent);
        let start = index == 0 && t <= 0.0;
        let end = index + 1 == last && t >= 1.0;
        let mut cap = 0.0;
        if start || end {
            let outward = if start { -axial } else { axial };
            if outward > 0.0 {
                match caps {
                    Caps::Flat if outward > 0.5 => continue,
                    Caps::Flat => {}
                    Caps::Round => cap = outward / section.rx.min(section.ry),
                }
            }
        }
        if t <= 0.0 || t >= 1.0 {
            off = sub(off, scale(tangent, axial));
        }
        let binormal = cross(tangent, normal);
        let (sin, cos) = section.roll.sin_cos();
        let up = add(scale(normal, cos), scale(binormal, sin));
        let side = sub(scale(binormal, cos), scale(normal, sin));
        let u = dot(off, side);
        let v = dot(off, up);
        let (nu, nv) = (u / section.rx, v / section.ry);
        let measure = (nu * nu + nv * nv + cap * cap).sqrt();
        let angle = nv.atan2(nu);
        let mut edge = 1.0;
        if section.noise > 0.0 {
            edge += section.noise * outline_noise(angle, along, section, seed);
        }
        if measure > edge + 1.0e-9 {
            continue;
        }
        if section.flat > 0.0 && v > section.ry * (1.0 - section.flat) {
            continue;
        }
        if section.flat < 0.0 && v < -section.ry * (1.0 + section.flat) {
            continue;
        }
        let around = if u.abs() + v.abs() > 1.0e-9 {
            u.atan2(v) / TAU
        } else {
            0.0
        };
        out.push(TubeCell {
            cell,
            along,
            around: around.rem_euclid(1.0),
        });
    }
    if out.len() as i64 > MAX_SCULPT_CELLS {
        return Err(format!(
            "the loft covers more than {MAX_SCULPT_CELLS} cells; make it thinner or shorter"
        ));
    }
    out.sort_by_key(|cell| (cell.cell.2, cell.cell.1, cell.cell.0));
    Ok(out)
}

/// The path length of a loft through `points`.
pub fn loft_length(points: &[LoftPoint], curve: bool) -> f64 {
    samples(points, curve)
        .ok()
        .and_then(|s| s.last().map(|s| s.along))
        .unwrap_or(0.0)
}

/// -1..=1 around the outline at `angle` and `along` the path: about
/// `frequency` lumps around, and lumps about as long as they are wide.
fn outline_noise(angle: f64, along: f64, section: Section, seed: u64) -> f64 {
    let ring = section.frequency / TAU;
    let width = TAU * (section.rx + section.ry) * 0.5 / section.frequency.max(1.0);
    let p = [
        angle.cos() * ring,
        angle.sin() * ring,
        along / width.max(1.0),
    ];
    let n = noise3(p, 1.0, seed) * 0.65 + noise3(scale(p, 2.0), 1.0, seed ^ 0x51ed) * 0.35;
    ((n * 2.0 - 1.0) * 1.6).clamp(-1.0, 1.0)
}

/// The path sampled about once per cell, with sections, the distance along
/// it, and a rotation-minimising frame at each sample.
fn samples(points: &[LoftPoint], curve: bool) -> Result<Vec<Sample>, String> {
    let last = points.len() - 1;
    let mut path: Vec<([f64; 3], Section)> = vec![(points[0].at, points[0].section)];
    for span in 0..last {
        let p0 = points[span.saturating_sub(1)].at;
        let p1 = points[span].at;
        let p2 = points[span + 1].at;
        let p3 = points[(span + 2).min(last)].at;
        let chord = dot(sub(p2, p1), sub(p2, p1)).sqrt();
        let steps = (chord.ceil() as usize).clamp(1, MAX_LOFT_SAMPLES + 1);
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let at = if curve {
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
                at
            } else {
                add(p1, scale(sub(p2, p1), t))
            };
            let section = points[span].section.lerp(points[span + 1].section, t);
            path.push((at, section));
        }
        if path.len() > MAX_LOFT_SAMPLES {
            return Err(format!(
                "the loft's path needs more than {MAX_LOFT_SAMPLES} samples; split it into several calls"
            ));
        }
    }
    path.dedup_by(|b, a| {
        let d = sub(b.0, a.0);
        dot(d, d) < 1.0e-12
    });
    if path.len() < 2 {
        return Err("the loft's points are all in one place".into());
    }
    let n = path.len();
    let mut out: Vec<Sample> = Vec::with_capacity(n);
    let mut along = 0.0;
    for i in 0..n {
        let before = path[i.saturating_sub(1)].0;
        let after = path[(i + 1).min(n - 1)].0;
        let tangent = normalize(sub(after, before)).unwrap_or([1.0, 0.0, 0.0]);
        if i > 0 {
            let d = sub(path[i].0, path[i - 1].0);
            along += dot(d, d).sqrt();
        }
        // Parallel transport: the previous normal, made perpendicular again.
        let carried = out
            .last()
            .and_then(|s| normalize(sub(s.normal, scale(tangent, dot(s.normal, tangent)))));
        let normal = carried.unwrap_or_else(|| {
            let reference = if tangent[2].abs() < 0.9 {
                [0.0, 0.0, 1.0]
            } else {
                [1.0, 0.0, 0.0]
            };
            normalize(sub(reference, scale(tangent, dot(reference, tangent))))
                .unwrap_or([0.0, 1.0, 0.0])
        });
        out.push(Sample {
            at: path[i].0,
            section: path[i].1,
            along,
            tangent,
            normal,
        });
    }
    Ok(out)
}

fn segment_box(a: &Sample, b: &Sample) -> ([i32; 3], [i32; 3]) {
    let r = a.section.reach().max(b.section.reach());
    let mut min = [0; 3];
    let mut max = [0; 3];
    for i in 0..3 {
        min[i] = (a.at[i].min(b.at[i]) - r).floor() as i32;
        max[i] = (a.at[i].max(b.at[i]) + r).ceil() as i32;
    }
    (min, max)
}
