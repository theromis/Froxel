//! MCP `shell_plates`: a plate pattern (a central plate, rings of plates,
//! an outer band of plates, and a rim, split by seams) painted over a
//! dome, as a turtle's shell is, instead of seams drawn one by one.
//!
//! The pattern lives on the surface, not on a view: the cells taken are the
//! surface cells that face up within `facing` degrees (or lie in a height
//! range), and each one's place on the dome is its distance along the
//! surface from the apex, divided by that distance out to the dome's edge
//! in its direction. So rings keep their width down the steep sides of a
//! tall or oval dome, and nothing streaks the way a top projection does.
//! The plan angle picks the plate within a ring (`rings` layout, sectors
//! with jittered, staggered boundaries), or the nearest of seeds laid out
//! in rings (`voronoi` layout). Boundaries and seeds are mirrored across
//! the plane through the centre along y when `symmetric`, so a shell facing
//! −Y has matching left and right plates, slots included. A seam is a cell
//! next to a plate that orders before its own (by kind, ring, and turn from
//! +Y), widened to `seam_width`, painted with the seam slot or cut
//! `seam_depth` cells deep. Cells are only recoloured, or
//! erased for a groove.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use super::decal::smooth_normal;
use crate::state::Cell;

/// Most rings of plates.
pub const MAX_RINGS: usize = 8;
/// Most plates in one ring or in the band.
pub const MAX_PLATES_PER_RING: u32 = 64;
/// Largest seam width in cells.
pub const MAX_SEAM_WIDTH: f64 = 6.0;
/// Most cells a plate or a seam reaches into the solid.
pub const MAX_PLATE_DEPTH: u32 = 8;

/// How plates are laid out within the rings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateLayout {
    /// Each ring is cut into sectors; neighbouring rings are staggered.
    Rings,
    /// Each cell takes the nearest of seeds laid out in rings.
    Voronoi,
}

/// The kind of a plate, which picks its slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlateKind {
    Central,
    Ring,
    Band,
    Rim,
}

/// Slots per kind: each plate picks one of its kind's list by `seed`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateSlots {
    pub central: Vec<u8>,
    pub ring: Vec<u8>,
    pub band: Vec<u8>,
    pub rim: Vec<u8>,
    /// The seam slot; 0 cuts a groove instead.
    pub seam: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlateSpec {
    pub layout: PlateLayout,
    /// Plan centre `[x, y]`; defaults to the middle of the taken cells.
    pub center: Option<[f64; 2]>,
    /// Share of the dome's radius the central plate spans; 0 for none.
    pub central: f64,
    /// Plates per ring, from the centre out.
    pub rings: Vec<u32>,
    /// Share of the radius the outer band spans; 0 for none.
    pub band: f64,
    /// Plates in the band; 0 is one unbroken band.
    pub band_count: u32,
    /// Rim width in cells along the surface at the dome's edge; 0 for none.
    pub rim: f64,
    pub slots: PlateSlots,
    pub seam_width: f64,
    /// Cells a seam reaches into the solid (the groove's depth with seam 0).
    pub seam_depth: u32,
    /// Cells a plate's slot reaches into the solid.
    pub depth: u32,
    /// Degrees from straight up a taken cell's normal may be.
    pub facing: f64,
    /// Inclusive heights of the taken cells.
    pub z_range: Option<(i32, i32)>,
    /// Inclusive box of the taken cells.
    pub region: Option<(Cell, Cell)>,
    /// Only cells of these slots are taken.
    pub only: Option<Vec<u8>>,
    /// Share of a plate's angle (or a seed's spacing) its boundaries move.
    pub jitter: f64,
    pub symmetric: bool,
    pub seed: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plated {
    pub writes: Vec<(i32, i32, i32, i32)>,
    /// Surface cells taken.
    pub taken: usize,
    /// Taken cells not joined along the surface to the apex, left alone.
    pub unreached: usize,
    /// Plates with at least one cell.
    pub plates: usize,
    /// Seam cells on the surface.
    pub seams: usize,
    /// The apex cell.
    pub apex: Option<Cell>,
    /// Cells per kind on the surface: central, ring, band, rim.
    pub kinds: [usize; 4],
}

type V3 = [f64; 3];

const FACES: [Cell; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// SplitMix64: a reproducible number per (seed, key).
fn hash(seed: u64, key: u64) -> u64 {
    let mut z = seed
        .wrapping_add(key.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A number in -1..1 per (seed, key).
fn signed(seed: u64, key: u64) -> f64 {
    (hash(seed, key) >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

const TAU: f64 = std::f64::consts::TAU;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

fn wrap(a: f64) -> f64 {
    a.rem_euclid(TAU)
}

/// Sector boundaries of a ring of `n` plates: one plate centred on the
/// +Y axis when `centred`, else a boundary on it; each moved by up to
/// `jitter` of a plate's angle, mirrored across the y axis when
/// `symmetric` (the boundary at angle a and the one at pi - a move apart
/// alike). Sorted.
fn boundaries(
    n: u32,
    centred: bool,
    jitter: f64,
    symmetric: bool,
    seed: u64,
    ring: u64,
) -> Vec<f64> {
    let step = TAU / f64::from(n);
    let offset = if centred { 0.5 } else { 0.0 };
    let base: Vec<f64> = (0..n)
        .map(|k| wrap(HALF_PI + (f64::from(k) + offset) * step))
        .collect();
    let amount = jitter.clamp(0.0, 0.45) * step;
    let mut out = Vec::with_capacity(base.len());
    for (k, &a) in base.iter().enumerate() {
        let shift = if symmetric {
            // The mirror of a is pi - a; pair them by the smaller index.
            let mirror = wrap(std::f64::consts::PI - a);
            let m = base
                .iter()
                .position(|b| (wrap(b - mirror + 1e-9) < 2e-9) || (wrap(mirror - b + 1e-9) < 2e-9))
                .unwrap_or(k);
            if m == k {
                0.0
            } else {
                let key = ring * 1000 + k.min(m) as u64;
                let s = signed(seed, key) * amount;
                if k < m {
                    s
                } else {
                    -s
                }
            }
        } else {
            signed(seed, ring * 1000 + k as u64) * amount
        };
        out.push(wrap(a + shift));
    }
    out.sort_by(f64::total_cmp);
    out
}

/// The sector of `angle` among sorted `bounds`.
fn sector(angle: f64, bounds: &[f64]) -> usize {
    let a = wrap(angle);
    match bounds.iter().rposition(|b| *b <= a) {
        Some(i) => i,
        // Below the first boundary: the last sector wraps round.
        None => bounds.len() - 1,
    }
}

fn pick(list: &[u8], seed: u64, key: u64) -> u8 {
    list[(hash(seed, key ^ 0x5157) % list.len() as u64) as usize]
}

/// A plate: its kind and its number, unique across the pattern.
type Plate = (PlateKind, u32);

/// Paint the plate pattern of `spec` over `cells`.
pub fn shell_plates(cells: &HashMap<Cell, u8>, spec: &PlateSpec) -> Result<Plated, String> {
    if spec.rings.len() > MAX_RINGS {
        return Err(format!("rings holds at most {MAX_RINGS} rings"));
    }
    if spec
        .rings
        .iter()
        .chain(std::iter::once(&spec.band_count))
        .any(|n| *n > MAX_PLATES_PER_RING)
    {
        return Err(format!(
            "a ring or the band holds at most {MAX_PLATES_PER_RING} plates"
        ));
    }
    if spec.rings.contains(&0) {
        return Err("a ring needs at least 1 plate".into());
    }
    if !(0.0..=0.9).contains(&spec.central) || !(0.0..=0.6).contains(&spec.band) {
        return Err("central must be 0..0.9 and band 0..0.6 of the radius".into());
    }
    if spec.central + spec.band >= 0.98 && !spec.rings.is_empty() {
        return Err("central and band leave no room for the rings".into());
    }
    if !(0.0..=32.0).contains(&spec.rim) {
        return Err("rim must be 0..32 cells".into());
    }
    if !(0.5..=MAX_SEAM_WIDTH).contains(&spec.seam_width) {
        return Err(format!("seamWidth must be 0.5..={MAX_SEAM_WIDTH}"));
    }
    if !(1..=MAX_PLATE_DEPTH).contains(&spec.depth)
        || !(1..=MAX_PLATE_DEPTH).contains(&spec.seam_depth)
    {
        return Err(format!("depth and seamDepth must be 1..={MAX_PLATE_DEPTH}"));
    }
    if !(0.0..=180.0).contains(&spec.facing) {
        return Err("facing must be 0..180 degrees".into());
    }
    if !(0.0..=1.0).contains(&spec.jitter) {
        return Err("jitter must be 0..1".into());
    }
    for (what, list) in [
        ("central", &spec.slots.central),
        ("ring", &spec.slots.ring),
        ("band", &spec.slots.band),
        ("rim", &spec.slots.rim),
    ] {
        if list.is_empty() || list.contains(&0) {
            return Err(format!("palettes {what} must be palette indices 1..255"));
        }
    }
    if let Some(only) = &spec.only {
        if only.is_empty() || only.contains(&0) {
            return Err("only must be palette indices 1..255".into());
        }
    }

    // The taken cells: surface cells facing up, in the box, heights, and
    // slots asked for.
    let cos_facing = spec.facing.to_radians().cos();
    let mut normals: HashMap<Cell, V3> = HashMap::new();
    for (&c, &slot) in cells {
        if !FACES
            .iter()
            .any(|d| !cells.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2)))
        {
            continue;
        }
        if let Some((lo, hi)) = spec.region {
            if c.0 < lo.0 || c.1 < lo.1 || c.2 < lo.2 || c.0 > hi.0 || c.1 > hi.1 || c.2 > hi.2 {
                continue;
            }
        }
        if let Some((z0, z1)) = spec.z_range {
            if c.2 < z0 || c.2 > z1 {
                continue;
            }
        }
        if spec.only.as_ref().is_some_and(|only| !only.contains(&slot)) {
            continue;
        }
        let Some(n) = smooth_normal(cells, c) else {
            continue;
        };
        if n[2] < cos_facing - 1e-9 {
            continue;
        }
        normals.insert(c, n);
    }
    if normals.is_empty() {
        return Err(
            "no surface cell faces up within facing (and the box, zRange, and only)".into(),
        );
    }
    let taken = normals.len();
    // The plan's middle and half extents.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for c in normals.keys() {
        for (a, v) in [f64::from(c.0), f64::from(c.1)].into_iter().enumerate() {
            lo[a] = lo[a].min(v);
            hi[a] = hi[a].max(v);
        }
    }
    let centre = spec
        .center
        .unwrap_or([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
    let half = [
        ((hi[0] - lo[0]) / 2.0).max(0.5),
        ((hi[1] - lo[1]) / 2.0).max(0.5),
    ];
    // The apex: the highest taken cell near the centre in plan.
    let mut keys: Vec<Cell> = normals.keys().copied().collect();
    keys.sort_unstable();
    let plan = |c: &Cell| (f64::from(c.0) - centre[0]).hypot(f64::from(c.1) - centre[1]);
    let nearest = keys.iter().map(plan).fold(f64::MAX, f64::min);
    let near: Vec<Cell> = keys
        .iter()
        .copied()
        .filter(|c| plan(c) <= nearest + 1.5)
        .collect();
    let top = near.iter().map(|c| c.2).max().expect("a taken cell");
    // The highest of them all start at their plan distance from the
    // centre, so a centre between two cells gives both sides the same
    // distances.
    let sources: Vec<Cell> = near.into_iter().filter(|c| c.2 == top).collect();
    let apex = *sources
        .iter()
        .min_by(|a, b| plan(a).total_cmp(&plan(b)).then(a.cmp(b)))
        .expect("a taken cell");

    // Distance along the surface from the apex, over the taken cells.
    let mut dist: HashMap<Cell, f64> = HashMap::new();
    let mut heap = BinaryHeap::new();
    for c in &sources {
        let g = plan(c);
        dist.insert(*c, g);
        heap.push((Reverse((g * 1e6) as i64), *c));
    }
    while let Some((Reverse(key), c)) = heap.pop() {
        let g = dist[&c];
        if (g * 1e6) as i64 != key {
            continue;
        }
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    let n = (c.0 + dx, c.1 + dy, c.2 + dz);
                    if !normals.contains_key(&n) {
                        continue;
                    }
                    let gn = g + f64::from(dx * dx + dy * dy + dz * dz).sqrt();
                    if dist.get(&n).is_none_or(|old| gn < *old - 1e-9) {
                        dist.insert(n, gn);
                        heap.push((Reverse((gn * 1e6) as i64), n));
                    }
                }
            }
        }
    }
    let unreached = taken - dist.len();
    // The plan angle, on the plan's ellipse so an oval dome's sectors stay
    // even, and the edge's distance per direction.
    let angle_of = |c: &Cell| {
        let x = (f64::from(c.0) - centre[0]) / half[0];
        let y = (f64::from(c.1) - centre[1]) / half[1];
        if x == 0.0 && y == 0.0 {
            HALF_PI
        } else {
            wrap(y.atan2(x))
        }
    };
    const SECTORS: usize = 72;
    let bin = |a: f64| ((a / TAU * SECTORS as f64) as usize).min(SECTORS - 1);
    let mut edge = [0.0f64; SECTORS];
    for (c, g) in &dist {
        let b = bin(angle_of(c));
        edge[b] = edge[b].max(*g);
    }
    // Empty sectors (a narrow dome) take their neighbours'; then smooth.
    let fallback = edge.iter().cloned().fold(0.0, f64::max).max(1.0);
    let raw = edge;
    for (b, e) in edge.iter_mut().enumerate() {
        if *e <= 0.0 {
            *e = (1..SECTORS / 2)
                .find_map(|k| {
                    let a = raw[(b + k) % SECTORS];
                    let z = raw[(b + SECTORS - k) % SECTORS];
                    (a > 0.0 || z > 0.0).then(|| a.max(z))
                })
                .unwrap_or(fallback);
        }
    }
    let smoothed: Vec<f64> = (0..SECTORS)
        .map(|b| {
            (0..5)
                .map(|k| edge[(b + SECTORS + k - 2) % SECTORS])
                .sum::<f64>()
                / 5.0
        })
        .collect();
    let edge_at = |a: f64| {
        let f = a / TAU * SECTORS as f64 - 0.5;
        let i = f.floor();
        let t = f - i;
        let i0 = (i as i64).rem_euclid(SECTORS as i64) as usize;
        let i1 = (i0 + 1) % SECTORS;
        (smoothed[i0] * (1.0 - t) + smoothed[i1] * t).max(1.0)
    };

    // The layout.
    let nrings = spec.rings.len();
    let ring_bounds: Vec<Vec<f64>> = spec
        .rings
        .iter()
        .enumerate()
        .map(|(r, n)| {
            boundaries(
                *n,
                r % 2 == 1,
                spec.jitter,
                spec.symmetric,
                spec.seed,
                r as u64 + 1,
            )
        })
        .collect();
    let band_bounds = (spec.band_count > 0).then(|| {
        boundaries(
            spec.band_count,
            true,
            spec.jitter * 0.5,
            spec.symmetric,
            spec.seed,
            99,
        )
    });
    // Voronoi seeds in the unit disc: (x, y, plate).
    let mut seeds: Vec<([f64; 2], Plate, u32, f64)> = Vec::new();
    if spec.layout == PlateLayout::Voronoi {
        if spec.central > 0.0 {
            seeds.push(([0.0, 0.0], (PlateKind::Central, 0), 0, HALF_PI));
        }
        let inner = 1.0 - spec.band;
        let span = (inner - spec.central) / nrings.max(1) as f64;
        let mut number = 1;
        for (r, n) in spec.rings.iter().enumerate() {
            let radius = spec.central + span * (r as f64 + 0.5);
            let step = TAU / f64::from(*n);
            let offset = if r % 2 == 1 { 0.0 } else { 0.5 };
            for k in 0..*n {
                let a = HALF_PI + (f64::from(k) + offset) * step;
                // Mirror pairs share their jitter (the angle's sign flips).
                let mirror = (0..*n)
                    .find(|m| {
                        let b = HALF_PI + (f64::from(*m) + offset) * step;
                        wrap(std::f64::consts::PI - a - b + 1e-9) < 2e-9
                    })
                    .unwrap_or(k);
                let key = (r as u64 + 1) * 1000 + u64::from(k.min(mirror));
                let (ja, jr) = if spec.symmetric {
                    let sign = if k <= mirror { 1.0 } else { -1.0 };
                    let ja = if k == mirror {
                        0.0
                    } else {
                        signed(spec.seed, key) * sign
                    };
                    (ja, signed(spec.seed, key ^ 0xABCD))
                } else {
                    (signed(spec.seed, key), signed(spec.seed, key ^ 0xABCD))
                };
                let a = a + ja * spec.jitter * 0.4 * step;
                let rr = radius + jr * spec.jitter * 0.35 * span;
                seeds.push((
                    [rr * a.cos(), rr * a.sin()],
                    (PlateKind::Ring, number),
                    r as u32 + 1,
                    wrap(a),
                ));
                number += 1;
            }
        }
    }

    // The middle angle of sector `i` among sorted `bounds`.
    let middle = |bounds: &[f64], i: usize| {
        let a = bounds[i];
        let mut b = bounds[(i + 1) % bounds.len()];
        if b <= a {
            b += TAU;
        }
        wrap((a + b) / 2.0)
    };
    // Each reached cell's plate, and each plate's ring and middle angle,
    // which order the seams and pair mirrored plates.
    let mut plate_of: HashMap<Cell, Plate> = HashMap::new();
    let mut info: HashMap<Plate, (u32, f64)> = HashMap::new();
    for (c, g) in &dist {
        let a = angle_of(c);
        let reach = edge_at(a);
        let rho = (g / reach).min(1.0);
        let rim = if spec.rim > 0.0 {
            spec.rim / reach
        } else {
            0.0
        };
        let (plate, ring, mid) = if rho >= 1.0 - rim - 1e-9 {
            ((PlateKind::Rim, 0), 0, HALF_PI)
        } else if spec.band > 0.0 && rho >= 1.0 - rim - spec.band {
            match &band_bounds {
                Some(bounds) => {
                    let i = sector(a, bounds);
                    ((PlateKind::Band, i as u32), 0, middle(bounds, i))
                }
                None => ((PlateKind::Band, 0), 0, HALF_PI),
            }
        } else if spec.layout == PlateLayout::Voronoi && !seeds.is_empty() {
            // Distances in the disc inside the band, stretched to fill it.
            let inner = (1.0 - rim - spec.band).max(1e-6);
            let r = rho / inner * (1.0 - spec.band);
            let p = [r * a.cos(), r * a.sin()];
            seeds
                .iter()
                .min_by(|x, y| {
                    let dx = (x.0[0] - p[0]).hypot(x.0[1] - p[1]);
                    let dy = (y.0[0] - p[0]).hypot(y.0[1] - p[1]);
                    dx.total_cmp(&dy)
                })
                .map(|s| (s.1, s.2, s.3))
                .unwrap_or(((PlateKind::Central, 0), 0, HALF_PI))
        } else if rho < spec.central || nrings == 0 {
            ((PlateKind::Central, 0), 0, HALF_PI)
        } else {
            let inner = 1.0 - rim - spec.band;
            let t = (rho - spec.central) / (inner - spec.central).max(1e-6);
            let r = ((t * nrings as f64) as usize).min(nrings - 1);
            let before: u32 = spec.rings[..r].iter().sum();
            let i = sector(a, &ring_bounds[r]);
            (
                (PlateKind::Ring, 1 + before + i as u32),
                r as u32 + 1,
                middle(&ring_bounds[r], i),
            )
        };
        plate_of.insert(*c, plate);
        info.insert(plate, (ring, mid));
    }

    // Seams: a cell next to another plate that orders before or with its
    // own, then widened. Plates order by kind, ring, and (when symmetric)
    // how far their middle turns from +Y, so mirrored plates order alike:
    // the seams fall on mirrored sides and mirrored plates share a slot.
    let kind_rank = |k: PlateKind| match k {
        PlateKind::Central => 0u8,
        PlateKind::Ring => 1,
        PlateKind::Band => 2,
        PlateKind::Rim => 3,
    };
    let order = |p: &Plate| -> (u8, u32, i64) {
        let (ring, mid) = info[p];
        let turn = if spec.symmetric {
            let d = wrap(mid - HALF_PI);
            (d.min(TAU - d) * 1000.0).round() as i64
        } else {
            i64::from(p.1)
        };
        (kind_rank(p.0), ring, turn)
    };
    let mut cores: Vec<Cell> = Vec::new();
    // The rim is a border of its own colour: it takes no seam and puts none
    // on the band.
    for (c, p) in &plate_of {
        if p.0 == PlateKind::Rim {
            continue;
        }
        let mut seam = false;
        'look: for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(q) = plate_of.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) {
                        if q != p && q.0 != PlateKind::Rim && order(q) <= order(p) {
                            seam = true;
                            break 'look;
                        }
                    }
                }
            }
        }
        if seam {
            cores.push(*c);
        }
    }
    cores.sort_unstable();
    let mut seams: std::collections::HashSet<Cell> = cores.iter().copied().collect();
    let widen = (spec.seam_width - 1.0) / 2.0;
    if widen > 0.0 {
        let r = widen.ceil() as i32;
        for c in &cores {
            for dz in -r..=r {
                for dy in -r..=r {
                    for dx in -r..=r {
                        let d = f64::from(dx * dx + dy * dy + dz * dz).sqrt();
                        let q = (c.0 + dx, c.1 + dy, c.2 + dz);
                        if d <= widen + 0.5 + 1e-9
                            && plate_of.get(&q).is_some_and(|p| p.0 != PlateKind::Rim)
                        {
                            seams.insert(q);
                        }
                    }
                }
            }
        }
    }

    // The writes: plates' slots `depth` cells in, seams over them.
    let slot_of = |p: &Plate| -> u8 {
        let (kind, ring, turn) = order(p);
        let key = u64::from(kind) << 56 | u64::from(ring) << 40 | (turn as u64 & 0xFF_FFFF_FFFF);
        match p.0 {
            PlateKind::Central => pick(&spec.slots.central, spec.seed, key),
            PlateKind::Ring => pick(&spec.slots.ring, spec.seed, key),
            PlateKind::Band => pick(&spec.slots.band, spec.seed, key),
            PlateKind::Rim => pick(&spec.slots.rim, spec.seed, key),
        }
    };
    let mut out: HashMap<Cell, u8> = HashMap::new();
    let mut kinds = [0usize; 4];
    let mut used: std::collections::HashSet<Plate> = std::collections::HashSet::new();
    let mut reached: Vec<&Cell> = plate_of.keys().collect();
    reached.sort_unstable();
    let inward = |c: &Cell, k: u32| -> Cell {
        let n = normals[c];
        let at = [0, 1, 2].map(|a| [c.0, c.1, c.2][a] as f64 - n[a] * f64::from(k));
        (
            at[0].round() as i32,
            at[1].round() as i32,
            at[2].round() as i32,
        )
    };
    for c in &reached {
        let p = plate_of[*c];
        used.insert(p);
        kinds[order(&p).0 as usize] += 1;
        let slot = slot_of(&p);
        for k in 0..spec.depth {
            let q = inward(c, k);
            if cells.contains_key(&q) {
                out.entry(q).or_insert(slot);
            }
        }
    }
    for c in &reached {
        if !seams.contains(*c) {
            continue;
        }
        for k in 0..spec.seam_depth {
            let q = inward(c, k);
            if cells.contains_key(&q) {
                out.insert(q, spec.slots.seam);
            }
        }
    }
    let mut writes: Vec<(i32, i32, i32, i32)> = out
        .into_iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(s)))
        .collect();
    writes.sort_unstable();
    Ok(Plated {
        writes,
        taken,
        unreached,
        plates: used.len(),
        seams: seams.len(),
        apex: Some(apex),
        kinds,
    })
}

#[cfg(test)]
#[path = "../../tests/plates.rs"]
mod tests;
