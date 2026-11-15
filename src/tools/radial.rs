//! Radial relief behind MCP `radial_relief`: regular, shallow, sloped
//! recesses repeated evenly round an axis. Flutes are grooves along a
//! column, vase, or barrel side; gores are the dished segments of a dome
//! between proud meridian ribs.
//!
//! Every solid cell near the outer surface gets a place in the pattern:
//! which recess it lies in, how far across it from the recess's middle, and
//! how deep it lies under the outer surface along the outward direction
//! (straight out from the axis for flutes, out from the dome's centre for
//! gores). The recess profile (`u`, `v`, or `flat`) times the end fade gives
//! the recess depth there; a cell above it is removed. Because cells can
//! only step, the slope is carried by colour as well: the cells left at the
//! floor take the shade tone, those on the slopes by the ridges the light
//! tone, and the lines between recesses the rib slot, raised when asked.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use super::debris::hash01;
use super::surface::{allowed, depths_from, step, Exclude, Region, FACES};
use super::{coord, plane_axes};
use crate::state::Cell;

/// Most recesses round the axis.
pub const MAX_RECESSES: u32 = 256;
/// Deepest recess, in cells.
pub const MAX_RECESS_DEPTH: f64 = 4.0;
/// Highest raised rib, in cells.
pub const MAX_RIB_HEIGHT: i32 = 4;

/// Rays round the axis that trace a flute slice's outline.
const OUTLINE_RAYS: usize = 360;
/// March step along a ray, in cells.
const STEP: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialKind {
    /// Grooves along the side of a shaft, spaced evenly along its outline.
    Flutes,
    /// Dished segments of a dome between meridian ribs, spaced by angle.
    Gores,
}

impl RadialKind {
    pub const NAMES: &'static str = "flutes or gores";

    pub fn parse(name: &str) -> Option<RadialKind> {
        match name {
            "flutes" | "flute" => Some(RadialKind::Flutes),
            "gores" | "gore" => Some(RadialKind::Gores),
            _ => None,
        }
    }
}

/// The section across one recess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecessProfile {
    /// Round-bottomed: deepest in the middle, steepening toward the ridges.
    U,
    /// Straight sides meeting in the middle.
    V,
    /// A flat floor over the middle half with sloped sides.
    Flat,
}

impl RecessProfile {
    pub const NAMES: &'static str = "u, v, or flat";

    pub fn parse(name: &str) -> Option<RecessProfile> {
        match name {
            "u" | "U" => Some(RecessProfile::U),
            "v" | "V" => Some(RecessProfile::V),
            "flat" => Some(RecessProfile::Flat),
            _ => None,
        }
    }

    /// Depth share at `x`, the distance from the middle as a share of the
    /// half width (0 the middle, 1 the ridge).
    fn depth(self, x: f64) -> f64 {
        if !(0.0..1.0).contains(&x) {
            return 0.0;
        }
        match self {
            RecessProfile::U => (1.0 - x * x).sqrt(),
            RecessProfile::V => 1.0 - x,
            RecessProfile::Flat if x <= 0.5 => 1.0,
            RecessProfile::Flat => 1.0 - (x - 0.5) / 0.5,
        }
    }
}

/// How a recess ends over its fade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecessEnds {
    /// A quarter circle: a flute ends in a rounded cup.
    Round,
    /// A straight ramp.
    Taper,
    /// Full depth up to the end.
    Square,
}

impl RecessEnds {
    pub const NAMES: &'static str = "round, taper, or square";

    pub fn parse(name: &str) -> Option<RecessEnds> {
        match name {
            "round" | "rounded" => Some(RecessEnds::Round),
            "taper" | "tapered" => Some(RecessEnds::Taper),
            "square" => Some(RecessEnds::Square),
            _ => None,
        }
    }

    /// The share of full depth (and, for flutes, width) `t` cells from the
    /// end over a fade of `fade` cells.
    fn factor(self, t: f64, fade: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }
        if fade <= 0.0 || self == RecessEnds::Square {
            return 1.0;
        }
        let s = (t / fade).min(1.0);
        match self {
            RecessEnds::Round => (1.0 - (1.0 - s) * (1.0 - s)).sqrt(),
            _ => s,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialSpec {
    pub kind: RadialKind,
    /// 0, 1, or 2: x, y, or z.
    pub axis: usize,
    /// The axis in the plane across it, as the two other coordinates in x,
    /// y, z order. `None` takes the middle of the solid's extent.
    pub center: Option<[f64; 2]>,
    pub count: u32,
    /// Turns the pattern, in degrees: for flutes a share of the outline's
    /// length (360 a whole turn), for gores an angle.
    pub phase: f64,
    /// Flutes: cells across each flute. `None` takes 0.6 of the pitch.
    pub width: Option<f64>,
    /// Inclusive range along the axis. `None` takes the solid's extent.
    pub from: Option<i32>,
    pub to: Option<i32>,
    /// Inclusive box corners the tool keeps inside, min x, y, z then max
    /// x, y, z; a missing corner does not bound that side.
    pub bounds: [Option<i32>; 6],
    pub ends: RecessEnds,
    /// Cells over which a recess fades in from its ends (flutes) or from
    /// the foot and the crown (gores). `None` takes the width (at least
    /// 2) for flutes and 6 for gores.
    pub fade: Option<f64>,
    pub profile: RecessProfile,
    /// Deepest recess depth, 0.5..=4 cells.
    pub depth: f64,
    /// Width of the rib line between gores, in cells.
    pub rib_width: f64,
    /// Cells a rib is raised by, 0..=4.
    pub rib_height: i32,
    /// The slot of raised rib cells; `None` takes the slot under them.
    pub rib_slot: Option<u8>,
    pub only: Vec<u8>,
    pub exclude: Exclude,
    /// Share of random change in each recess's depth and place, 0..=1.
    pub jitter: f64,
    pub seed: u64,
}

/// What colour a cell left on a recess takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// The floor in the middle of a recess.
    Deep,
    /// A slope by the ridges.
    Slope,
    /// A rib or ridge between recesses.
    Rib,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadialDone {
    /// Cells cut out of the recesses.
    pub removed: Vec<Cell>,
    /// Cells added to raise the ribs, with their slot.
    pub added: Vec<(Cell, u8)>,
    /// Cells left on the surface to tone, with their slot, in z, y, x order.
    pub tones: Vec<(Cell, u8, Tone)>,
    /// Recesses that cut or toned at least one cell.
    pub recesses: usize,
    /// Surface cells the recesses and ribs covered.
    pub cells: usize,
    pub warning: Option<String>,
}

fn inside(bounds: &[Option<i32>; 6], c: Cell) -> bool {
    let v = [c.0, c.1, c.2];
    (0..3)
        .all(|a| bounds[a].is_none_or(|lo| v[a] >= lo) && bounds[a + 3].is_none_or(|hi| v[a] <= hi))
}

fn solid_at(cells: &HashMap<Cell, u8>, p: [f64; 3]) -> bool {
    let c = (
        (p[0] + 0.5).floor() as i32,
        (p[1] + 0.5).floor() as i32,
        (p[2] + 0.5).floor() as i32,
    );
    cells.contains_key(&c)
}

/// One flute slice's outline round the axis, traced by rays: the centre of
/// the outermost solid cell on ray `b`, and the length along the outline from
/// ray 0 to it (`cum[OUTLINE_RAYS]` is the whole perimeter).
struct Outline {
    points: Vec<(f64, f64)>,
    cum: Vec<f64>,
}

impl Outline {
    fn perimeter(&self) -> f64 {
        self.cum[OUTLINE_RAYS]
    }

    /// The outline length from ray 0 to the outline point nearest `(p,
    /// q)`, searched over the rays within an eighth of a turn of its angle,
    /// so a square shaft's flutes keep their place on the face rather than
    /// drifting toward the corners as a ray from the axis would.
    fn length_at(&self, p: f64, q: f64) -> f64 {
        let theta = q.atan2(p).rem_euclid(TAU);
        let around = (theta / TAU * OUTLINE_RAYS as f64).floor() as isize;
        let window = (OUTLINE_RAYS / 8) as isize;
        let mut best = (f64::MAX, 0.0);
        for d in -window..=window {
            let b = (around + d).rem_euclid(OUTLINE_RAYS as isize) as usize;
            let a = self.points[b];
            let z = self.points[(b + 1) % OUTLINE_RAYS];
            let (dx, dy) = (z.0 - a.0, z.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 1.0e-12 {
                (((p - a.0) * dx + (q - a.1) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
            let dist = (p - cx).hypot(q - cy);
            if dist < best.0 {
                best = (dist, self.cum[b] + (self.cum[b + 1] - self.cum[b]) * t);
            }
        }
        best.1.rem_euclid(self.perimeter())
    }
}

/// The recesses round the axis: cells to cut, ribs to raise, and cells to
/// tone.
pub fn radial_relief(cells: &HashMap<Cell, u8>, spec: &RadialSpec) -> Result<RadialDone, String> {
    if !(1..=MAX_RECESSES).contains(&spec.count) {
        return Err(format!("count must be 1..={MAX_RECESSES}"));
    }
    if !(0.5..=MAX_RECESS_DEPTH).contains(&spec.depth) {
        return Err(format!("depth must be 0.5..={MAX_RECESS_DEPTH} cells"));
    }
    if spec.width.is_some_and(|w| !(0.5..=64.0).contains(&w)) {
        return Err("width must be 0.5..=64 cells".into());
    }
    if spec.fade.is_some_and(|f| !(0.0..=128.0).contains(&f)) {
        return Err("fade must be 0..=128 cells".into());
    }
    if !(0.0..=16.0).contains(&spec.rib_width) {
        return Err("ribWidth must be 0..=16 cells".into());
    }
    if !(0..=MAX_RIB_HEIGHT).contains(&spec.rib_height) {
        return Err(format!("ribHeight must be 0..={MAX_RIB_HEIGHT}"));
    }
    if !(0.0..=1.0).contains(&spec.jitter) {
        return Err("jitter must be 0..=1".into());
    }
    if !spec.phase.is_finite() {
        return Err("phase must be a number of degrees".into());
    }
    let axis = spec.axis.min(2);
    let (pa, pb) = plane_axes(axis as i32);
    let mut done = RadialDone::default();

    // The solid in the box and the range along the axis.
    let boxed: Vec<Cell> = cells
        .keys()
        .copied()
        .filter(|c| inside(&spec.bounds, *c))
        .collect();
    let Some(lo_h) = boxed.iter().map(|c| coord(*c, axis)).min() else {
        done.warning = Some("no solid cells in the box: nothing to relieve".into());
        return Ok(done);
    };
    let hi_h = boxed.iter().map(|c| coord(*c, axis)).max().expect("a cell");
    let from = spec.from.unwrap_or(lo_h);
    let to = spec.to.unwrap_or(hi_h);
    let (from, to) = (from.min(to), from.max(to));
    let ranged: Vec<Cell> = boxed
        .into_iter()
        .filter(|c| (from..=to).contains(&coord(*c, axis)))
        .collect();
    if ranged.is_empty() {
        done.warning = Some(format!(
            "no solid cells between {from} and {to} along the axis: nothing to relieve"
        ));
        return Ok(done);
    }
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    for c in &ranged {
        for (a, v) in [c.0, c.1, c.2].into_iter().enumerate() {
            min[a] = min[a].min(v);
            max[a] = max[a].max(v);
        }
    }
    let center = spec.center.unwrap_or([
        f64::from(min[pa] + max[pa]) / 2.0,
        f64::from(min[pb] + max[pb]) / 2.0,
    ]);
    if center.iter().any(|v| !v.is_finite()) {
        return Err("center must be two finite numbers".into());
    }
    let region = Region::new(
        (min[0], min[1], min[2]),
        (max[0], max[1], max[2]),
        "radial_relief",
    )?;
    let across = |c: Cell| {
        (
            f64::from(coord(c, pa)) - center[0],
            f64::from(coord(c, pb)) - center[1],
        )
    };
    let radius = |c: Cell| {
        let (p, q) = across(c);
        p.hypot(q)
    };
    let n = f64::from(spec.count);

    // Gores: the dome's centre on the axis, from a sphere through its foot
    // circle and its crown, never above its middle height.
    let foot_radius = ranged
        .iter()
        .filter(|c| coord(**c, axis) == from)
        .map(|c| radius(*c) + 0.5)
        .fold(0.0, f64::max);
    let rise = f64::from(to - from) + 0.5;
    let dome_centre = {
        let sphere = (foot_radius * foot_radius + rise * rise) / (2.0 * rise);
        (f64::from(to) + 0.5 - sphere).min(f64::from(from) + rise / 2.0)
    };
    // The outward direction at a point, in x, y, z order.
    let outward = |p: f64, q: f64, h: f64| -> Option<[f64; 3]> {
        let mut v = [0.0; 3];
        v[pa] = p;
        v[pb] = q;
        if spec.kind == RadialKind::Gores {
            v[axis] = h - dome_centre;
        }
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (len > 1.0e-6).then(|| [v[0] / len, v[1] / len, v[2] / len])
    };

    // Flutes: each slice's outline, traced by rays from the axis.
    let mut outlines: HashMap<i32, Outline> = HashMap::new();
    if spec.kind == RadialKind::Flutes {
        let mut reach: HashMap<i32, f64> = HashMap::new();
        for c in &ranged {
            let r = reach.entry(coord(*c, axis)).or_insert(0.0);
            *r = r.max(radius(*c) + 1.5);
        }
        let set: HashSet<Cell> = ranged.iter().copied().collect();
        for (&h, &far) in &reach {
            let mut ends: Vec<Option<(f64, f64)>> = Vec::with_capacity(OUTLINE_RAYS);
            for b in 0..OUTLINE_RAYS {
                let theta = (b as f64 + 0.5) / OUTLINE_RAYS as f64 * TAU;
                let (cos, sin) = (theta.cos(), theta.sin());
                let mut r = far;
                let mut found = None;
                while r >= 0.0 {
                    let mut at = [0.0; 3];
                    at[pa] = center[0] + r * cos;
                    at[pb] = center[1] + r * sin;
                    at[axis] = f64::from(h);
                    let c = (
                        (at[0] + 0.5).floor() as i32,
                        (at[1] + 0.5).floor() as i32,
                        (at[2] + 0.5).floor() as i32,
                    );
                    if set.contains(&c) {
                        // The outermost cell's centre: the outline then
                        // runs through the surface cells, one step each.
                        found = Some(across(c));
                        break;
                    }
                    r -= STEP;
                }
                ends.push(found);
            }
            if ends.iter().all(Option::is_none) {
                continue;
            }
            // A ray that missed takes its nearest neighbour's cell.
            let points: Vec<(f64, f64)> = (0..OUTLINE_RAYS)
                .map(|b| {
                    (0..OUTLINE_RAYS)
                        .find_map(|d| {
                            ends[(b + d) % OUTLINE_RAYS]
                                .or(ends[(b + OUTLINE_RAYS - d) % OUTLINE_RAYS])
                        })
                        .unwrap_or((0.0, 0.0))
                })
                .collect();
            let mut cum = vec![0.0; OUTLINE_RAYS + 1];
            for b in 1..=OUTLINE_RAYS {
                let (a, z) = (points[b - 1], points[b % OUTLINE_RAYS]);
                cum[b] = cum[b - 1] + (z.0 - a.0).hypot(z.1 - a.1);
            }
            if cum[OUTLINE_RAYS] > 1.0e-6 {
                outlines.insert(h, Outline { points, cum });
            }
        }
    }
    // Flutes: the width, by default 0.6 of the median pitch.
    let width = match spec.kind {
        RadialKind::Flutes => {
            let mut pitches: Vec<f64> = outlines.values().map(|o| o.perimeter() / n).collect();
            pitches.sort_by(f64::total_cmp);
            let median = pitches.get(pitches.len() / 2).copied().unwrap_or(1.0);
            spec.width.unwrap_or((0.6 * median).max(1.0))
        }
        RadialKind::Gores => 0.0,
    };
    let fade = spec.fade.unwrap_or(match spec.kind {
        RadialKind::Flutes => width.max(2.0),
        RadialKind::Gores => 6.0,
    });

    // Cells near enough the surface for the deepest recess.
    let limit = spec.depth.ceil() as u32 + 2;
    let near = depths_from(cells, &region, limit);
    let mut near: Vec<Cell> = near.into_keys().collect();
    near.sort_unstable_by_key(|c| (c.2, c.1, c.0));

    let mut touched: HashSet<u32> = HashSet::new();
    let mut ribs: Vec<(Cell, [f64; 3])> = Vec::new();
    let mut floors: Vec<(Cell, u8, Option<Tone>, u32)> = Vec::new();
    for cell in near {
        let h = coord(cell, axis);
        if !(from..=to).contains(&h) || !inside(&spec.bounds, cell) {
            continue;
        }
        let slot = cells[&cell];
        if !allowed(&spec.only, slot) || spec.exclude.contains(cell) {
            continue;
        }
        let (p, q) = across(cell);
        let r = p.hypot(q);
        let Some(out) = outward(p, q, f64::from(h)) else {
            continue;
        };
        // How deep the cell's centre lies under the outer surface.
        let base = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
        let reach = spec.depth + 1.5;
        let mut t = STEP;
        let mut under = None;
        while t <= reach {
            let at = [
                base[0] + out[0] * t,
                base[1] + out[1] * t,
                base[2] + out[2] * t,
            ];
            if !solid_at(cells, at) {
                under = Some(t - STEP / 2.0);
                break;
            }
            t += STEP;
        }
        let Some(under) = under else {
            continue;
        };
        let theta = q.atan2(p).rem_euclid(TAU);
        // Which recess, and how far across it from its middle.
        let (k, u, half, end) = match spec.kind {
            RadialKind::Flutes => {
                let Some(outline) = outlines.get(&h) else {
                    continue;
                };
                let pitch = outline.perimeter() / n;
                let v = (outline.length_at(p, q) / outline.perimeter() - spec.phase / 360.0)
                    .rem_euclid(1.0)
                    * n;
                let k = (v.floor() as u32).min(spec.count - 1);
                let u = (v - v.floor() - 0.5) * pitch;
                let half = (width / 2.0).min(pitch / 2.0);
                let t_end = f64::from((h - from).min(to - h)) + 0.5;
                (k, u, half, t_end)
            }
            RadialKind::Gores => {
                let v = ((theta - spec.phase.to_radians()) / TAU).rem_euclid(1.0) * n;
                let k = (v.floor() as u32).min(spec.count - 1);
                let seg = TAU / n * r;
                let u = (v - v.floor() - 0.5) * seg;
                let half = seg / 2.0 - spec.rib_width / 2.0;
                let t_end = (f64::from(h - from) + 0.5).min(r);
                (k, u, half, t_end)
            }
        };
        let (mut deep, mut shift) = (1.0, 0.0);
        if spec.jitter > 0.0 {
            let j = |salt: i64| hash01(i64::from(k), salt, 7, spec.seed) * 2.0 - 1.0;
            deep = (1.0 + 0.5 * spec.jitter * j(1)).max(0.1);
            shift = 0.3 * spec.jitter * half.max(0.0) * j(2);
        }
        let e = spec.ends.factor(end, fade);
        let half_eff = match spec.kind {
            RadialKind::Flutes => half * e,
            RadialKind::Gores => half,
        };
        let x = if half_eff > 0.25 {
            (u - shift).abs() / half_eff
        } else {
            f64::INFINITY
        };
        let recess = spec.depth * deep * e * spec.profile.depth(x);
        if x < 1.0 && e > 0.0 {
            if recess >= under + 0.25 {
                done.removed.push(cell);
                touched.insert(k);
                done.cells += 1;
            } else {
                // Toned below once the cuts are known, if it is left open.
                let tone = if x <= 0.5 {
                    Some(Tone::Deep)
                } else if x > 0.55 {
                    Some(Tone::Slope)
                } else {
                    None
                };
                floors.push((cell, slot, tone, k));
            }
        } else if under < 1.0 {
            // A rib or ridge between recesses.
            done.tones.push((cell, slot, Tone::Rib));
            done.cells += 1;
            if spec.rib_height > 0 {
                ribs.push((cell, out));
            }
        }
    }
    // The cells a recess leaves open: its floor and slopes.
    let cut: HashSet<Cell> = done.removed.iter().copied().collect();
    for (cell, slot, tone, k) in floors {
        let open = FACES.iter().any(|d| {
            let next = step(cell, *d);
            !cells.contains_key(&next) || cut.contains(&next)
        });
        if !open {
            continue;
        }
        done.cells += 1;
        if let Some(tone) = tone {
            done.tones.push((cell, slot, tone));
            touched.insert(k);
        }
    }
    done.tones.sort_unstable_by_key(|(c, _, _)| (c.2, c.1, c.0));
    // Raised ribs: cells straight out from the rib's surface cells.
    let mut added: HashMap<Cell, u8> = HashMap::new();
    for (cell, out) in ribs {
        let base = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
        for i in 1..=spec.rib_height {
            let t = f64::from(i);
            let at = (
                (base[0] + out[0] * t + 0.5).floor() as i32,
                (base[1] + out[1] * t + 0.5).floor() as i32,
                (base[2] + out[2] * t + 0.5).floor() as i32,
            );
            if cells.contains_key(&at)
                || !inside(&spec.bounds, at)
                || !(from..=to).contains(&coord(at, axis))
                || spec.exclude.contains(at)
            {
                continue;
            }
            added
                .entry(at)
                .or_insert(spec.rib_slot.unwrap_or(cells[&cell]));
        }
    }
    let mut added: Vec<(Cell, u8)> = added.into_iter().collect();
    added.sort_unstable_by_key(|(c, _)| (c.2, c.1, c.0));
    done.added = added;
    done.recesses = touched.len();
    if done.removed.is_empty() && done.tones.is_empty() && done.added.is_empty() {
        done.warning = Some(match spec.kind {
            RadialKind::Flutes => "no surface cells matched: check the axis, center, range, and only, or give a larger width or depth".into(),
            RadialKind::Gores => "no surface cells matched: check the axis, center, range (from is the dome's foot), and only".into(),
        });
    }
    Ok(done)
}

#[cfg(test)]
#[path = "../../tests/radial.rs"]
mod tests;
