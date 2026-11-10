//! The deformers behind MCP `deform`: bend, taper, and twist move the cells
//! of an object or a box along an axis; displace pushes their surface in and
//! out with 3D noise; scale enlarges (or shrinks) a part about a point with
//! a smooth falloff, so a head, an eye, or a fin gets the cells it needs at
//! a small size (the exaggeration step of caricature at low resolution).
//! Every cell keeps its palette slot.

use std::collections::HashMap;

use super::field::{signed, Grid};
use super::sculpt::MAX_SCULPT_CELLS;
use super::surface::noise3;
use super::Region;
use crate::state::Cell;

/// Most cells a bend, taper, or twist may test in its target box.
const MAX_DEFORM_WORK: i64 = 16_000_000;
/// Largest displace amplitude in cells.
pub const MAX_DISPLACE: f64 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeformOp {
    /// Bend toward `toward` (an axis other than the deform axis, and a
    /// sign) by `angle` radians over the length; past it the rest follows
    /// straight.
    Bend {
        angle: f64,
        toward: (usize, f64),
    },
    /// Scale across the axis from `start` at the origin to `end` at the end
    /// of the length.
    Taper {
        start: f64,
        end: f64,
    },
    /// Turn about the axis by `angle` radians over the length.
    Twist {
        angle: f64,
    },
    Displace(Displace),
    Scale(Magnify),
}

/// Most a scale enlarges or shrinks along an axis.
pub const MAX_MAGNIFY: f64 = 4.0;

/// A local scale about `point`: the cells within `radius` are scaled by
/// `factor` (per axis) as one rigid piece, and over the next `falloff`
/// cells (before scaling) the scale eases back to none, so the rest of the
/// object stays where it is and the part stays attached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnify {
    pub point: [f64; 3],
    pub factor: [f64; 3],
    pub radius: f64,
    pub falloff: f64,
}

impl Magnify {
    /// The outer edge in units of `radius`, past which nothing moves.
    fn outer(&self) -> f64 {
        1.0 + self.falloff / self.radius
    }

    /// Where the cell at `p` comes from.
    fn inverse(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [
            p[0] - self.point[0],
            p[1] - self.point[1],
            p[2] - self.point[2],
        ];
        let core = [
            d[0] / self.factor[0],
            d[1] / self.factor[1],
            d[2] / self.factor[2],
        ];
        let e = (core[0] * core[0] + core[1] * core[1] + core[2] * core[2]).sqrt() / self.radius;
        let w = if e <= 1.0 {
            0.0
        } else {
            let t = ((e - 1.0) / (self.outer() - 1.0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        std::array::from_fn(|i| self.point[i] + core[i] * (1.0 - w) + d[i] * w)
    }

    fn check(&self) -> Result<(), String> {
        if self
            .factor
            .iter()
            .any(|k| !(1.0 / MAX_MAGNIFY..=MAX_MAGNIFY).contains(k))
        {
            return Err(format!(
                "factor must be 1/{MAX_MAGNIFY}..={MAX_MAGNIFY} along each axis"
            ));
        }
        if !(0.5..=512.0).contains(&self.radius) {
            return Err("radius must be 0.5..=512 cells".into());
        }
        if !(0.5..=512.0).contains(&self.falloff) {
            return Err("falloff must be 0.5..=512 cells".into());
        }
        if self.point.iter().any(|v| !v.is_finite()) {
            return Err("point must be three finite numbers".into());
        }
        // Along every ray from the point, each axis's offset of the source
        // must grow with the target's, or the scale would fold cells over.
        let reach = self.reach();
        for dx in -1i32..=1 {
            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    let n = f64::from(dx * dx + dy * dy + dz * dz).sqrt();
                    let u = [f64::from(dx) / n, f64::from(dy) / n, f64::from(dz) / n];
                    let mut last = [0.0f64; 3];
                    let steps = 400;
                    for step in 1..=steps {
                        let r = reach * f64::from(step) / f64::from(steps);
                        let q = self.inverse(std::array::from_fn(|i| self.point[i] + u[i] * r));
                        for i in 0..3 {
                            let off = (q[i] - self.point[i]) * u[i].signum();
                            if u[i] != 0.0 && off < last[i] - 1.0e-6 {
                                return Err(format!(
                                    "falloff {} is too short for factor [{}, {}, {}]: the scale would fold cells over; give a longer falloff",
                                    self.falloff, self.factor[0], self.factor[1], self.factor[2]
                                ));
                            }
                            last[i] = off;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// How far from the point a target cell can move.
    fn reach(&self) -> f64 {
        let kmax = self.factor.iter().cloned().fold(1.0, f64::max);
        self.radius * self.outer() * kmax + 2.0
    }
}

/// The noise of a displace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Displace {
    /// Most cells the surface moves, 0.5..=32.
    pub amplitude: f64,
    /// Size of a bump in cells.
    pub scale: f64,
    /// 1..=6; each octave halves the size and the height.
    pub octaves: u32,
    pub seed: u64,
    /// -1 only dents, 0 dents and bulges, 1 only bulges.
    pub bias: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeformSpec {
    pub op: DeformOp,
    /// Only the cells inside; `None` is the whole object.
    pub region: Option<Region>,
    /// 0 x, 1 y, 2 z.
    pub axis: usize,
    /// Where along the axis the deform starts; the cells' lowest by default.
    pub origin: Option<f64>,
    /// How far along the axis it runs; the cells' extent by default.
    pub length: Option<f64>,
    /// The line the bend, taper, and twist work about, as the two other
    /// coordinates in x, y, z order; the cells' mean by default.
    pub center: Option<[f64; 2]>,
}

#[derive(Debug, PartialEq)]
pub struct Deformed {
    /// `(x, y, z, palette)`; 0 removes.
    pub writes: Vec<(i32, i32, i32, i32)>,
    /// Solid cells the deform read.
    pub cells: usize,
    /// Solid cells it left.
    pub result: usize,
    pub added: usize,
    pub removed: usize,
}

/// Deform the solid cells of `cells` in the spec's region.
pub fn deform(cells: &HashMap<Cell, u8>, spec: &DeformSpec) -> Result<Deformed, String> {
    if spec.axis > 2 {
        return Err("axis must be x, y, or z".into());
    }
    let source: HashMap<Cell, u8> = cells
        .iter()
        .filter(|(c, _)| spec.region.is_none_or(|r| r.contains(**c)))
        .map(|(&c, &s)| (c, s))
        .collect();
    if source.is_empty() {
        return Err(match spec.region {
            Some(_) => "the region holds no cells to deform".into(),
            None => "the object holds no cells to deform".into(),
        });
    }
    if let DeformOp::Displace(noise) = spec.op {
        return displace(cells, &source, spec.region, noise);
    }
    if let DeformOp::Scale(m) = spec.op {
        return magnify(cells, &source, m);
    }
    let map = Mapping::new(&source, spec)?;
    // The target box: every source centre carried forward, a cell around.
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &c in source.keys() {
        let p = map.forward(centre(c));
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let min = (
        lo[0].floor() as i32 - 1,
        lo[1].floor() as i32 - 1,
        lo[2].floor() as i32 - 1,
    );
    let max = (
        hi[0].ceil() as i32 + 1,
        hi[1].ceil() as i32 + 1,
        hi[2].ceil() as i32 + 1,
    );
    let work = (i64::from(max.0 - min.0) + 1)
        * (i64::from(max.1 - min.1) + 1)
        * (i64::from(max.2 - min.2) + 1);
    if work > MAX_DEFORM_WORK {
        return Err(format!(
            "the deformed cells span {work} cells, more than {MAX_DEFORM_WORK}; deform a smaller region"
        ));
    }
    let mut out: HashMap<Cell, u8> = HashMap::new();
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                for p in map.inverse(centre((x, y, z))).into_iter().flatten() {
                    let from = (
                        p[0].round() as i32,
                        p[1].round() as i32,
                        p[2].round() as i32,
                    );
                    if let Some(&slot) = source.get(&from) {
                        out.insert((x, y, z), slot);
                        break;
                    }
                }
            }
        }
        if out.len() as i64 > MAX_SCULPT_CELLS {
            return Err(format!(
                "the deform leaves more than {MAX_SCULPT_CELLS} cells; deform a smaller region"
            ));
        }
    }
    Ok(diff(cells, &source, out))
}

/// Writes that take `cells` from `source` to `out`.
fn diff(cells: &HashMap<Cell, u8>, source: &HashMap<Cell, u8>, out: HashMap<Cell, u8>) -> Deformed {
    let mut done = Deformed {
        writes: Vec::new(),
        cells: source.len(),
        result: out.len(),
        added: 0,
        removed: 0,
    };
    for &c in source.keys() {
        if !out.contains_key(&c) {
            done.writes.push((c.0, c.1, c.2, 0));
            done.removed += 1;
        }
    }
    for (&c, &slot) in &out {
        match cells.get(&c) {
            Some(&old) if old == slot => {}
            Some(_) => done.writes.push((c.0, c.1, c.2, i32::from(slot))),
            None => {
                done.writes.push((c.0, c.1, c.2, i32::from(slot)));
                done.added += 1;
            }
        }
    }
    done.writes.sort_unstable();
    done
}

fn centre(c: Cell) -> [f64; 3] {
    [f64::from(c.0), f64::from(c.1), f64::from(c.2)]
}

/// A bend, taper, or twist in the axis frame: `s` along the axis from the
/// origin, and the two other coordinates about the centre line.
struct Mapping {
    op: DeformOp,
    axis: usize,
    across: [usize; 2],
    origin: f64,
    length: f64,
    center: [f64; 2],
}

impl Mapping {
    fn new(source: &HashMap<Cell, u8>, spec: &DeformSpec) -> Result<Mapping, String> {
        let axis = spec.axis;
        let across = match axis {
            0 => [1, 2],
            1 => [0, 2],
            _ => [0, 1],
        };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut sum = [0.0; 2];
        for &c in source.keys() {
            let p = centre(c);
            lo = lo.min(p[axis]);
            hi = hi.max(p[axis]);
            sum[0] += p[across[0]];
            sum[1] += p[across[1]];
        }
        let n = source.len() as f64;
        let origin = spec.origin.unwrap_or(lo);
        let length = spec.length.unwrap_or(hi - lo + 1.0);
        if !origin.is_finite() || !(0.5..=4096.0).contains(&length) {
            return Err("length must be 0.5..=4096 cells and origin a finite number".into());
        }
        let center = spec.center.unwrap_or([sum[0] / n, sum[1] / n]);
        if center.iter().any(|v| !v.is_finite()) {
            return Err("center must be two finite numbers".into());
        }
        match spec.op {
            DeformOp::Bend { angle, toward } => {
                if !(angle.abs() <= std::f64::consts::PI + 1.0e-9) {
                    return Err("bend angle must be -180..=180 degrees".into());
                }
                if toward.0 == axis || toward.0 > 2 {
                    return Err("bend direction must lie across the axis".into());
                }
            }
            DeformOp::Taper { start, end } => {
                if !(0.05..=16.0).contains(&start) || !(0.05..=16.0).contains(&end) {
                    return Err("taper start and end must be 0.05..=16".into());
                }
            }
            DeformOp::Twist { angle } => {
                if !angle.is_finite() || angle.abs() > 8.0 * std::f64::consts::TAU {
                    return Err("twist angle must be within 2880 degrees".into());
                }
            }
            DeformOp::Displace(_) | DeformOp::Scale(_) => {
                unreachable!("displace and scale have no axis mapping")
            }
        }
        Ok(Mapping {
            op: spec.op,
            axis,
            across,
            origin,
            length,
            center,
        })
    }

    /// The fraction of the length at `s`, 0..=1.
    fn part(&self, s: f64) -> f64 {
        (s / self.length).clamp(0.0, 1.0)
    }

    fn forward(&self, p: [f64; 3]) -> [f64; 3] {
        let s = p[self.axis] - self.origin;
        let a = p[self.across[0]] - self.center[0];
        let b = p[self.across[1]] - self.center[1];
        let mut out = p;
        match self.op {
            DeformOp::Bend { angle, toward } => {
                if let Some((s2, u2)) = self.bend_forward(s, self.bend_u(p, toward), angle) {
                    out[self.axis] = self.origin + s2;
                    out[toward.0] = self.bend_back(u2, toward);
                }
            }
            DeformOp::Taper { start, end } => {
                let k = start + (end - start) * self.part(s);
                out[self.across[0]] = self.center[0] + a * k;
                out[self.across[1]] = self.center[1] + b * k;
            }
            DeformOp::Twist { angle } => {
                let (sin, cos) = (angle * self.part(s)).sin_cos();
                out[self.across[0]] = self.center[0] + a * cos - b * sin;
                out[self.across[1]] = self.center[1] + a * sin + b * cos;
            }
            DeformOp::Displace(_) | DeformOp::Scale(_) => {}
        }
        out
    }

    /// Where the cell at `p` may come from, most likely first.
    fn inverse(&self, p: [f64; 3]) -> [Option<[f64; 3]>; 3] {
        let s = p[self.axis] - self.origin;
        let a = p[self.across[0]] - self.center[0];
        let b = p[self.across[1]] - self.center[1];
        let mut out = [None; 3];
        match self.op {
            DeformOp::Bend { angle, toward } => {
                let u = self.bend_u(p, toward);
                for (slot, (s0, u0)) in self.bend_inverse(s, u, angle).into_iter().enumerate() {
                    let mut q = p;
                    q[self.axis] = self.origin + s0;
                    q[toward.0] = self.bend_back(u0, toward);
                    out[slot] = Some(q);
                }
            }
            DeformOp::Taper { start, end } => {
                let k = start + (end - start) * self.part(s);
                let mut q = p;
                q[self.across[0]] = self.center[0] + a / k;
                q[self.across[1]] = self.center[1] + b / k;
                out[0] = Some(q);
            }
            DeformOp::Twist { angle } => {
                let (sin, cos) = (-angle * self.part(s)).sin_cos();
                let mut q = p;
                q[self.across[0]] = self.center[0] + a * cos - b * sin;
                q[self.across[1]] = self.center[1] + a * sin + b * cos;
                out[0] = Some(q);
            }
            DeformOp::Displace(_) | DeformOp::Scale(_) => out[0] = Some(p),
        }
        out
    }

    /// The offset of `p` from the centre line toward the bend.
    fn bend_u(&self, p: [f64; 3], toward: (usize, f64)) -> f64 {
        let slot = if toward.0 == self.across[0] { 0 } else { 1 };
        (p[toward.0] - self.center[slot]) * toward.1
    }

    fn bend_back(&self, u: f64, toward: (usize, f64)) -> f64 {
        let slot = if toward.0 == self.across[0] { 0 } else { 1 };
        self.center[slot] + u * toward.1
    }

    /// A bend along a circle of radius `length / angle` from the origin:
    /// `(s, u)` to the bent `(s, u)`. `None` leaves the point where it is.
    fn bend_forward(&self, s: f64, u: f64, angle: f64) -> Option<(f64, f64)> {
        let (theta, sign) = (angle.abs(), angle.signum());
        if theta < 1.0e-9 || s < 0.0 {
            return None;
        }
        let u = u * sign;
        let r = self.length / theta;
        let out = if s <= self.length {
            let t = s / r;
            ((r - u) * t.sin(), r - (r - u) * t.cos())
        } else {
            let (sin, cos) = theta.sin_cos();
            let rest = s - self.length;
            (
                r * sin + rest * cos - u * sin,
                r - r * cos + rest * sin + u * cos,
            )
        };
        Some((out.0, out.1 * sign))
    }

    /// The unbent `(s, u)` that may land on `(s, u)`: on the arc, past it,
    /// and before the origin.
    fn bend_inverse(&self, s: f64, u: f64, angle: f64) -> Vec<(f64, f64)> {
        let (theta, sign) = (angle.abs(), angle.signum());
        if theta < 1.0e-9 {
            return vec![(s, u)];
        }
        let u = u * sign;
        let r = self.length / theta;
        let mut out = Vec::with_capacity(3);
        let phi = s.atan2(r - u);
        if (0.0..=theta).contains(&phi) {
            let radius = (s * s + (r - u) * (r - u)).sqrt();
            out.push((r * phi, (r - radius) * sign));
        }
        let (sin, cos) = theta.sin_cos();
        let w = (s - r * sin, u - (r - r * cos));
        let along = self.length + w.0 * cos + w.1 * sin;
        if along >= self.length {
            out.push((along, (-w.0 * sin + w.1 * cos) * sign));
        }
        if s < 0.0 {
            out.push((s, u * sign));
        }
        out
    }
}

/// Scale the cells of `source` about `m.point`: every cell of the box the
/// scale reaches takes the slot of the source cell its inverse lands on;
/// cells outside that box stay as they are.
fn magnify(
    cells: &HashMap<Cell, u8>,
    source: &HashMap<Cell, u8>,
    m: Magnify,
) -> Result<Deformed, String> {
    m.check()?;
    let reach = m.reach();
    let lo: [i32; 3] = std::array::from_fn(|i| (m.point[i] - reach).floor() as i32);
    let hi: [i32; 3] = std::array::from_fn(|i| (m.point[i] + reach).ceil() as i32);
    let work = (0..3)
        .map(|i| i64::from(hi[i] - lo[i]) + 1)
        .product::<i64>();
    if work > MAX_DEFORM_WORK {
        return Err(format!(
            "the scale reaches {work} cells, more than {MAX_DEFORM_WORK}; use a smaller radius or falloff"
        ));
    }
    let inside = |c: Cell| {
        (lo[0]..=hi[0]).contains(&c.0)
            && (lo[1]..=hi[1]).contains(&c.1)
            && (lo[2]..=hi[2]).contains(&c.2)
    };
    let mut out: HashMap<Cell, u8> = source
        .iter()
        .filter(|(c, _)| !inside(**c))
        .map(|(&c, &s)| (c, s))
        .collect();
    for z in lo[2]..=hi[2] {
        for y in lo[1]..=hi[1] {
            for x in lo[0]..=hi[0] {
                let q = m.inverse(centre((x, y, z)));
                let from = (
                    q[0].round() as i32,
                    q[1].round() as i32,
                    q[2].round() as i32,
                );
                if let Some(&slot) = source.get(&from) {
                    out.insert((x, y, z), slot);
                }
            }
        }
    }
    if out.len() as i64 > MAX_SCULPT_CELLS {
        return Err(format!(
            "the scale leaves more than {MAX_SCULPT_CELLS} cells; scale a smaller part"
        ));
    }
    Ok(diff(cells, source, out))
}

/// Push the surface of `source` in and out with fractal noise. Only cells in
/// `region` (or near the object) change.
fn displace(
    cells: &HashMap<Cell, u8>,
    source: &HashMap<Cell, u8>,
    region: Option<Region>,
    noise: Displace,
) -> Result<Deformed, String> {
    if !(0.5..=MAX_DISPLACE).contains(&noise.amplitude) {
        return Err(format!("amplitude must be 0.5..={MAX_DISPLACE} cells"));
    }
    if !(1.0..=512.0).contains(&noise.scale) {
        return Err("scale must be 1..=512 cells".into());
    }
    if !(1..=6).contains(&noise.octaves) {
        return Err("octaves must be 1..=6".into());
    }
    if !(-1.0..=1.0).contains(&noise.bias) {
        return Err("bias must be -1..=1".into());
    }
    let pad = noise.amplitude.ceil() as i32 + 2;
    let (mut min, mut max) = bounds(source.keys().copied());
    min = (min.0 - pad, min.1 - pad, min.2 - pad);
    max = (max.0 + pad, max.1 + pad, max.2 + pad);
    if let Some(r) = region {
        min = (min.0.max(r.min.0), min.1.max(r.min.1), min.2.max(r.min.2));
        max = (max.0.min(r.max.0), max.1.min(r.max.1), max.2.min(r.max.2));
    }
    // Read a little past the region so its edge cells see their neighbours.
    let grid = Grid::new(
        cells,
        (min.0 - pad, min.1 - pad, min.2 - pad),
        (max.0 + pad, max.1 + pad, max.2 + pad),
    )?;
    let (sdf, solid) = signed(&grid, noise.amplitude as f32 + 1.0);
    let mut out = source.clone();
    for i in 0..grid.len() {
        let c = grid.cell(i);
        if c.0 < min.0 || c.1 < min.1 || c.2 < min.2 || c.0 > max.0 || c.1 > max.1 || c.2 > max.2 {
            continue;
        }
        if f64::from(sdf[i]).abs() > noise.amplitude + 1.0 {
            continue;
        }
        let push = noise.amplitude * fractal(centre(c), noise);
        let inside = f64::from(sdf[i]) < push;
        let here = grid.slots[i];
        if inside && here == 0 && solid[i] != u32::MAX {
            out.insert(c, grid.slots[solid[i] as usize]);
        } else if !inside && here != 0 {
            out.remove(&c);
        }
    }
    if out.len() as i64 > MAX_SCULPT_CELLS {
        return Err(format!(
            "the displace leaves more than {MAX_SCULPT_CELLS} cells; displace a smaller region"
        ));
    }
    Ok(diff(cells, source, out))
}

/// The displace at `p`, -1..=1, shifted by the bias.
fn fractal(p: [f64; 3], noise: Displace) -> f64 {
    let mut sum = 0.0;
    let mut weight = 1.0;
    let mut total = 0.0;
    let mut size = noise.scale;
    for octave in 0..noise.octaves {
        let n = noise3(
            p,
            size.max(0.5),
            noise.seed.wrapping_add(u64::from(octave) * 7919),
        );
        sum += (n * 2.0 - 1.0) * weight;
        total += weight;
        weight *= 0.5;
        size *= 0.5;
    }
    // Value noise rarely reaches its ends; stretch it so the amplitude does.
    let n = (sum / total * 1.6).clamp(-1.0, 1.0);
    (n + noise.bias) / (1.0 + noise.bias.abs())
}

fn bounds(cells: impl Iterator<Item = Cell>) -> (Cell, Cell) {
    let mut min = (i32::MAX, i32::MAX, i32::MAX);
    let mut max = (i32::MIN, i32::MIN, i32::MIN);
    for c in cells {
        min = (min.0.min(c.0), min.1.min(c.1), min.2.min(c.2));
        max = (max.0.max(c.0), max.1.max(c.1), max.2.max(c.2));
    }
    (min, max)
}
