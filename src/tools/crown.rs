//! Clustered tree crowns behind MCP `foliage`: a broadleaf crown made of
//! many separate leaf clumps instead of one smooth blob, as pixel-art trees
//! draw them.
//!
//! The crown volume is a union of ellipsoids and boxes, or the existing
//! cells of some slots (to re-cluster a crown already sculpted). Clump
//! centres are packed over its surface with Poisson-like spacing, so no two
//! sit closer than their sizes and `gap` allow. Each clump is a flattened,
//! ragged ellipsoid that bulges out of the surface. The inside of the
//! volume, one cell under its surface, is the dark core, so the gaps
//! between clumps show dark and nothing is see-through. A clump is shaded
//! as one piece: its own height in the crown and the way the crown faces
//! the sun there set its tone, and inside it the top lightens and the
//! underside darkens along the clump's own curve, with a darker seam where
//! two clumps meet. No colour is per-cell noise. `masses` groups the clumps
//! into larger lobes split by deeper grooves.

use std::collections::{HashMap, HashSet, VecDeque};

use super::surface::{
    allowed, cell01, check_slot, check_slots, noise3, step, Exclude, Region, FACES,
};
use crate::state::Cell;

/// Most clumps one call packs.
pub const MAX_CLUMPS: usize = 4096;
/// Most masses (lobes).
pub const MAX_MASSES: u32 = 16;

/// A piece of the crown volume.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CrownShape {
    /// Centre and radii in cells; a whole-number centre is a cell centre.
    Ellipsoid { center: [f64; 3], radii: [f64; 3] },
    /// An inclusive box.
    Box { min: Cell, max: Cell },
}

impl CrownShape {
    /// The inclusive cell bounds.
    pub fn bounds(&self) -> (Cell, Cell) {
        match *self {
            CrownShape::Ellipsoid { center, radii } => {
                let lo = |a: usize| (center[a] - radii[a]).floor() as i32;
                let hi = |a: usize| (center[a] + radii[a]).ceil() as i32;
                ((lo(0), lo(1), lo(2)), (hi(0), hi(1), hi(2)))
            }
            CrownShape::Box { min, max } => (min, max),
        }
    }

    fn contains(&self, c: Cell) -> bool {
        match *self {
            CrownShape::Ellipsoid { center, radii } => {
                let d = [
                    (f64::from(c.0) - center[0]) / radii[0],
                    (f64::from(c.1) - center[1]) / radii[1],
                    (f64::from(c.2) - center[2]) / radii[2],
                ];
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= 1.0
            }
            CrownShape::Box { min, max } => Region::spanning(min, max).contains(c),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoliageSpec {
    /// Writes stay inside it.
    pub region: Region,
    /// The crown volume: these shapes, and the cells of `from`.
    pub shapes: Vec<CrownShape>,
    /// Existing cells of these slots in the region join the volume, and
    /// those no clump or core keeps are removed.
    pub from: Vec<u8>,
    /// Leaf slots from the darkest (shade) to the lightest (sunlit tips).
    pub palettes: Vec<u8>,
    /// The dark core's slot.
    pub core: u8,
    /// Clump radius range in cells.
    pub size: (f64, f64),
    /// Extra cells between neighbouring clumps, 0..=8.
    pub gap: f64,
    /// Clump height over width, 0.2..=1.5.
    pub flatten: f64,
    /// Leafy outline noise, 0..=1.
    pub ragged: f64,
    /// How strongly lit tops lighten, 0..=2.
    pub light: f64,
    /// How strongly undersides darken, 0..=2.
    pub dark: f64,
    /// Toward the sun, Z up.
    pub sun: [f64; 3],
    /// Lobes the clumps are grouped into, 0..=16; 0 is none.
    pub masses: u32,
    /// How many cells deeper the grooves between masses go, 0..=8.
    pub groove: i32,
    /// Slots that are never written, even when they are crown slots.
    pub protect: Vec<u8>,
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foliage {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub clumps: usize,
    pub masses: usize,
    pub added: usize,
    pub recoloured: usize,
    pub removed: usize,
    /// Cells that took the core slot.
    pub core: usize,
}

/// One packed clump.
struct Clump {
    centre: [f64; 3],
    /// Horizontal radii along its own axes, and the top and bottom radius.
    a: f64,
    b: f64,
    top: f64,
    bottom: f64,
    cos: f64,
    sin: f64,
    /// The clump's own light, -1..=1, from its height and facing.
    light: f64,
    /// Noise salt of its outline.
    salt: u64,
}

impl Clump {
    /// Offset from the centre in the clump's frame.
    fn local(&self, c: Cell) -> [f64; 3] {
        let d = [
            f64::from(c.0) - self.centre[0],
            f64::from(c.1) - self.centre[1],
            f64::from(c.2) - self.centre[2],
        ];
        [
            d[0] * self.cos + d[1] * self.sin,
            -d[0] * self.sin + d[1] * self.cos,
            d[2],
        ]
    }

    /// How far out a cell lies: 1 on the plain ellipsoid.
    fn reach(&self, l: [f64; 3]) -> f64 {
        let rz = if l[2] >= 0.0 { self.top } else { self.bottom };
        ((l[0] / self.a).powi(2) + (l[1] / self.b).powi(2) + (l[2] / rz).powi(2)).sqrt()
    }

    /// The outward normal of the ellipsoid through `l`, in world axes.
    fn normal(&self, l: [f64; 3]) -> [f64; 3] {
        let rz = if l[2] >= 0.0 { self.top } else { self.bottom };
        let g = [
            l[0] / (self.a * self.a),
            l[1] / (self.b * self.b),
            l[2] / (rz * rz),
        ];
        let world = [
            g[0] * self.cos - g[1] * self.sin,
            g[0] * self.sin + g[1] * self.cos,
            g[2],
        ];
        unit(world).unwrap_or([0.0, 0.0, 1.0])
    }
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (len > 1.0e-9).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn at(c: Cell) -> [f64; 3] {
    [f64::from(c.0), f64::from(c.1), f64::from(c.2)]
}

fn check_spec(spec: &FoliageSpec) -> Result<(), String> {
    if spec.palettes.is_empty() || spec.palettes.len() > 8 {
        return Err("palettes must hold 1 to 8 leaf slots, darkest first".into());
    }
    check_slots(&spec.palettes, "palettes")?;
    check_slots(&spec.from, "from")?;
    check_slots(&spec.protect, "protect")?;
    check_slot(spec.core, "core")?;
    if spec.shapes.is_empty() && spec.from.is_empty() {
        return Err(
            "the crown needs a volume: ellipsoids, boxes, or from (the slots of an existing crown)"
                .into(),
        );
    }
    let (lo, hi) = spec.size;
    if !(1.0..=32.0).contains(&lo) || !(lo..=32.0).contains(&hi) {
        return Err("size must be [min, max] clump radii with 1 <= min <= max <= 32".into());
    }
    let ranges = [
        (spec.gap, 0.0, 8.0, "gap"),
        (spec.flatten, 0.2, 1.5, "flatten"),
        (spec.ragged, 0.0, 1.0, "ragged"),
        (spec.light, 0.0, 2.0, "light"),
        (spec.dark, 0.0, 2.0, "dark"),
    ];
    for (value, lo, hi, what) in ranges {
        if !(lo..=hi).contains(&value) {
            return Err(format!("{what} must be {lo}..={hi}"));
        }
    }
    if spec.masses > MAX_MASSES {
        return Err(format!("masses must be 0..={MAX_MASSES}"));
    }
    if !(0..=8).contains(&spec.groove) {
        return Err("groove must be 0..=8".into());
    }
    for shape in &spec.shapes {
        if let CrownShape::Ellipsoid { radii, .. } = shape {
            if radii.iter().any(|r| !(0.5..=512.0).contains(r)) {
                return Err("ellipsoid radii must be 0.5..=512".into());
            }
        }
    }
    unit(spec.sun).ok_or("sun must be a direction, not [0, 0, 0]")?;
    Ok(())
}

/// The crown's writes against `cells`.
pub fn foliage(cells: &HashMap<Cell, u8>, spec: &FoliageSpec) -> Result<Foliage, String> {
    check_spec(spec)?;
    let sun = unit(spec.sun).expect("checked");
    let region = spec.region;
    let crown_slot = |slot: u8| {
        !spec.protect.contains(&slot)
            && (spec.from.contains(&slot) || spec.palettes.contains(&slot) || slot == spec.core)
    };

    // The volume, and what else is solid there (the wood): the surface is
    // where the volume meets air, not where it meets a branch.
    let mut volume: HashSet<Cell> = HashSet::new();
    for shape in &spec.shapes {
        let (lo, hi) = shape.bounds();
        for z in lo.2.max(region.min.2)..=hi.2.min(region.max.2) {
            for y in lo.1.max(region.min.1)..=hi.1.min(region.max.1) {
                for x in lo.0.max(region.min.0)..=hi.0.min(region.max.0) {
                    if shape.contains((x, y, z)) {
                        volume.insert((x, y, z));
                    }
                }
            }
        }
    }
    for (cell, slot) in cells {
        if region.contains(*cell) && !spec.from.is_empty() && allowed(&spec.from, *slot) {
            volume.insert(*cell);
        }
    }
    if volume.is_empty() {
        return Err("the crown volume is empty inside the box".into());
    }
    let solid =
        |c: Cell| volume.contains(&c) || cells.get(&c).is_some_and(|slot| !crown_slot(*slot));
    let mut ordered: Vec<Cell> = volume.iter().copied().collect();
    ordered.sort_unstable_by_key(|c| (c.2, c.1, c.0));
    let surface: Vec<Cell> = ordered
        .iter()
        .copied()
        .filter(|c| FACES.iter().any(|d| !solid(step(*c, *d))))
        .collect();
    if surface.is_empty() {
        return Err("the crown volume has no surface open to the air".into());
    }

    // Depth under the surface, 1 on it.
    let mut depth: HashMap<Cell, u32> = HashMap::with_capacity(volume.len());
    let mut queue = VecDeque::new();
    for c in &surface {
        depth.insert(*c, 1);
        queue.push_back(*c);
    }
    while let Some(c) = queue.pop_front() {
        let next = depth[&c] + 1;
        for d in FACES {
            let n = step(c, d);
            if volume.contains(&n) && !depth.contains_key(&n) {
                depth.insert(n, next);
                queue.push_back(n);
            }
        }
    }

    let (zmin, zmax) = ordered.iter().fold((i32::MAX, i32::MIN), |(lo, hi), c| {
        (lo.min(c.2), hi.max(c.2))
    });
    let height = |z: f64| {
        if zmax > zmin {
            ((z - f64::from(zmin)) / f64::from(zmax - zmin)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    };
    let centroid = {
        let mut sum = [0.0; 3];
        for c in &ordered {
            for (axis, value) in at(*c).into_iter().enumerate() {
                sum[axis] += value;
            }
        }
        let n = ordered.len() as f64;
        [sum[0] / n, sum[1] / n, sum[2] / n]
    };
    // The outward direction of the volume near a surface cell: the mean
    // offset of the air within three cells.
    let normal = |c: Cell| {
        let mut sum = [0.0f64; 3];
        for dz in -3..=3 {
            for dy in -3..=3 {
                for dx in -3..=3 {
                    if !solid((c.0 + dx, c.1 + dy, c.2 + dz)) {
                        sum[0] += f64::from(dx);
                        sum[1] += f64::from(dy);
                        sum[2] += f64::from(dz);
                    }
                }
            }
        }
        let p = at(c);
        unit(sum)
            .or_else(|| unit([p[0] - centroid[0], p[1] - centroid[1], p[2] - centroid[2]]))
            .unwrap_or([0.0, 0.0, 1.0])
    };

    // Masses: lobe seeds spread over the surface, farthest point first.
    let mut lobes: Vec<([f64; 3], f64)> = Vec::new();
    if spec.masses > 0 {
        let first = surface
            [(cell01(surface[0], spec.seed, 11) * surface.len() as f64) as usize % surface.len()];
        let mut seeds = vec![first];
        let mut nearest: Vec<f64> = surface.iter().map(|c| dist2(at(*c), at(first))).collect();
        while seeds.len() < spec.masses as usize {
            let (index, far) = nearest.iter().enumerate().fold((0, -1.0), |best, (i, d)| {
                if *d > best.1 {
                    (i, *d)
                } else {
                    best
                }
            });
            if far <= 0.0 {
                break;
            }
            let next = surface[index];
            seeds.push(next);
            for (i, c) in surface.iter().enumerate() {
                nearest[i] = nearest[i].min(dist2(at(*c), at(next)));
            }
        }
        for (i, s) in seeds.iter().enumerate() {
            let lit = 0.6 * dot(normal(*s), sun) + 0.4 * (2.0 * height(f64::from(s.2)) - 1.0);
            let jitter = (cell01(*s, spec.seed, 13 + i as u64) - 0.5) * 0.3;
            lobes.push((at(*s), (lit + jitter).clamp(-1.0, 1.0)));
        }
    }
    // The nearest lobe of a point and how far it is from the next one's
    // border, in cells.
    let lobe_of = |p: [f64; 3]| -> Option<(usize, f64)> {
        if lobes.len() < 2 {
            return lobes.first().map(|_| (0, f64::INFINITY));
        }
        let mut best = (0, f64::INFINITY, f64::INFINITY);
        for (i, (seed, _)) in lobes.iter().enumerate() {
            let d = dist2(p, *seed).sqrt();
            if d < best.1 {
                best = (i, d, best.1);
            } else if d < best.2 {
                best.2 = d;
            }
        }
        Some((best.0, (best.2 - best.1) * 0.5))
    };

    // Clump centres, Poisson-like over the surface: candidates in a seeded
    // order, each kept when it is far enough from every kept one. Distance
    // is measured with height stretched by 1 / flatten, as the clumps are.
    let mut candidates = surface.clone();
    candidates.sort_by(|a, b| {
        cell01(*a, spec.seed, 1)
            .partial_cmp(&cell01(*b, spec.seed, 1))
            .expect("finite")
            .then((a.2, a.1, a.0).cmp(&(b.2, b.1, b.0)))
    });
    let spacing = |ra: f64, rb: f64| (ra + rb) * 0.6 + spec.gap;
    let cell_size = spacing(spec.size.1, spec.size.1).max(1.0);
    let key = |p: [f64; 3]| {
        (
            (p[0] / cell_size).floor() as i64,
            (p[1] / cell_size).floor() as i64,
            (p[2] / spec.flatten / cell_size).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    let mut kept: Vec<(Cell, f64)> = Vec::new();
    for c in candidates {
        let r = spec.size.0 + (spec.size.1 - spec.size.0) * cell01(c, spec.seed, 2);
        let p = at(c);
        let k = key(p);
        let mut free = true;
        'near: for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(list) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) else {
                        continue;
                    };
                    for &j in list {
                        let (other, rj) = kept[j];
                        let q = at(other);
                        let d = [p[0] - q[0], p[1] - q[1], (p[2] - q[2]) / spec.flatten];
                        if dot(d, d).sqrt() < spacing(r, rj) {
                            free = false;
                            break 'near;
                        }
                    }
                }
            }
        }
        if free {
            if kept.len() == MAX_CLUMPS {
                return Err(format!(
                    "more than {MAX_CLUMPS} clumps; pass a larger size or a smaller crown"
                ));
            }
            grid.entry(k).or_default().push(kept.len());
            kept.push((c, r));
        }
    }

    // Each kept point becomes a flattened ellipsoid sunk a third into the
    // volume, so it bulges out of it; near a mass border it sinks deeper.
    let mut clumps = Vec::with_capacity(kept.len());
    for (index, (c, r)) in kept.iter().enumerate() {
        let n = normal(*c);
        let p = at(*c);
        let mut r = *r;
        let mut sink = r * 0.35;
        let mut light = 0.55 * dot(n, sun) + 0.45 * (2.0 * height(p[2]) - 1.0);
        if let Some((lobe, border)) = lobe_of(p) {
            light = 0.6 * light + 0.4 * lobes[lobe].1;
            if border < spec.size.1 {
                let t = 1.0 - border / spec.size.1;
                sink += f64::from(spec.groove) * t;
                r *= 1.0 - 0.2 * t;
                light -= 0.3 * t;
            }
        }
        let salt = index as u64;
        let stretch = (cell01(*c, spec.seed, 3) - 0.5) * 0.3;
        let yaw = cell01(*c, spec.seed, 4) * std::f64::consts::PI;
        // Each clump's own tone varies a little, as a whole.
        light += (cell01(*c, spec.seed, 5) - 0.5) * 0.25;
        clumps.push(Clump {
            centre: [
                p[0] - n[0] * sink,
                p[1] - n[1] * sink,
                p[2] - n[2] * sink * spec.flatten,
            ],
            a: r * (1.0 + stretch),
            b: r * (1.0 - stretch),
            top: r * spec.flatten,
            bottom: r * spec.flatten * 0.75,
            cos: yaw.cos(),
            sin: yaw.sin(),
            light: light.clamp(-1.0, 1.0),
            salt,
        });
    }

    // Rasterise: a cell belongs to the clump it lies deepest in.
    let writable = |c: Cell| {
        region.contains(c)
            && !spec.exclude.contains(c)
            && cells.get(&c).is_none_or(|slot| crown_slot(*slot))
    };
    let mut owner: HashMap<Cell, (f64, usize)> = HashMap::new();
    let wobble = spec.ragged * 0.7;
    for (index, clump) in clumps.iter().enumerate() {
        let reach = clump.a.max(clump.b).max(clump.top) * (1.0 + wobble) + 1.0;
        let lo = |v: f64| (v - reach).floor() as i32;
        let hi = |v: f64| (v + reach).ceil() as i32;
        for z in lo(clump.centre[2]).max(region.min.2)..=hi(clump.centre[2]).min(region.max.2) {
            for y in lo(clump.centre[1]).max(region.min.1)..=hi(clump.centre[1]).min(region.max.1) {
                for x in
                    lo(clump.centre[0]).max(region.min.0)..=hi(clump.centre[0]).min(region.max.0)
                {
                    let c = (x, y, z);
                    let q = clump.reach(clump.local(c));
                    if q > 1.0 + wobble {
                        continue;
                    }
                    // A leafy outline: smooth noise of about two cells moves
                    // the edge in and out, more near the clump's rim.
                    let edge = if wobble > 0.0 {
                        let n = noise3(at(c), 1.7, spec.seed ^ clump.salt.wrapping_mul(0x9e37));
                        1.0 + wobble * (n - 0.5) * 2.0
                    } else {
                        1.0
                    };
                    let ratio = q / edge;
                    if ratio > 1.0 || !writable(c) {
                        continue;
                    }
                    match owner.get(&c) {
                        Some((best, _)) if *best <= ratio => {}
                        _ => {
                            owner.insert(c, (ratio, index));
                        }
                    }
                }
            }
        }
    }

    // The dark core: the volume one cell under its surface, and deeper
    // under a groove between masses.
    let mut core: HashSet<Cell> = HashSet::new();
    for c in &ordered {
        let mut need = 2;
        if let Some((_, border)) = lobe_of(at(*c)) {
            if border < spec.size.0 {
                need += spec.groove as u32;
            }
        }
        if depth.get(c).is_some_and(|d| *d >= need) && writable(*c) {
            core.insert(*c);
        }
    }

    // Tones: the clump's own light and, inside it, the way its surface
    // faces the sun; a seam darkens where two clumps meet.
    let filled =
        |c: Cell| owner.contains_key(&c) || core.contains(&c) || solid_other(cells, c, &crown_slot);
    let tones = spec.palettes.len();
    let mut slot_of: HashMap<Cell, u8> = HashMap::with_capacity(owner.len() + core.len());
    for (c, (_, index)) in &owner {
        if FACES.iter().all(|d| filled(step(*c, *d))) {
            slot_of.insert(*c, spec.core);
            continue;
        }
        let clump = &clumps[*index];
        let lit = dot(clump.normal(clump.local(*c)), sun);
        let mut raw = 0.6 * lit + 0.4 * clump.light;
        let seam = FACES.iter().any(|d| {
            owner
                .get(&step(*c, *d))
                .is_some_and(|(_, other)| other != index)
        });
        if seam {
            raw -= 0.35;
        }
        let gain = if raw >= 0.0 { spec.light } else { spec.dark };
        let t = (0.5 + 0.5 * raw * gain).clamp(0.0, 1.0);
        let tone = ((t * tones as f64) as usize).min(tones - 1);
        slot_of.insert(*c, spec.palettes[tone]);
    }
    for c in &core {
        slot_of.entry(*c).or_insert(spec.core);
    }

    let mut out = Foliage {
        clumps: clumps.len(),
        masses: lobes.len(),
        ..Foliage::default()
    };
    let mut touched: Vec<Cell> = slot_of.keys().copied().collect();
    for (c, slot) in cells {
        if region.contains(*c)
            && !spec.from.is_empty()
            && spec.from.contains(slot)
            && !slot_of.contains_key(c)
        {
            touched.push(*c);
        }
    }
    touched.sort_unstable_by_key(|c| (c.2, c.1, c.0));
    for c in touched {
        if spec.exclude.contains(c) {
            continue;
        }
        let old = cells.get(&c).copied();
        match (slot_of.get(&c), old) {
            (Some(&slot), None) => {
                out.writes.push((c.0, c.1, c.2, i32::from(slot)));
                out.added += 1;
                if slot == spec.core {
                    out.core += 1;
                }
            }
            (Some(&slot), Some(prev)) => {
                if slot == spec.core {
                    out.core += 1;
                }
                if prev != slot && crown_slot(prev) {
                    out.writes.push((c.0, c.1, c.2, i32::from(slot)));
                    out.recoloured += 1;
                }
            }
            (None, Some(prev)) if crown_slot(prev) => {
                out.writes.push((c.0, c.1, c.2, 0));
                out.removed += 1;
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A solid cell that is not part of the crown: wood the leaves wrap.
fn solid_other(cells: &HashMap<Cell, u8>, c: Cell, crown_slot: &impl Fn(u8) -> bool) -> bool {
    cells.get(&c).is_some_and(|slot| !crown_slot(*slot))
}

fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d)
}

#[cfg(test)]
#[path = "../../tests/foliage.rs"]
mod tests;
