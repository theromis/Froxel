//! Broken wood behind MCP `break_end`: a jagged fracture with splinters.

use std::collections::{HashMap, HashSet};

use super::sculpt::{tube_shaped, TubePoint, TubeShape, MAX_SCULPT_CELLS};
use crate::state::Cell;

/// Most splinters in one break.
pub const MAX_SPLINTERS: u32 = 512;

/// A deterministic stream of numbers from one seed (SplitMix64).
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Rng(seed ^ 0x2545_f491_4f6c_dd1d)
    }

    pub(super) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 0..1.
    pub(super) fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(super) fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    pub(super) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() % items.len() as u64) as usize]
    }
}

/// A stable 0..1 value for integer coordinates and a salt.
pub(super) fn hash01(a: i64, b: i64, c: i64, seed: u64) -> f64 {
    let mut rng = Rng::new(
        seed ^ (a as u64).wrapping_mul(0x9e37_79b9)
            ^ (b as u64).wrapping_mul(0x85eb_ca6b_0000_0001)
            ^ (c as u64).wrapping_mul(0xc2b2_ae35_0000_0003),
    );
    rng.unit()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Splinters {
    Count(u32),
    /// Splinters per cell of the new face, 0..=1.
    Density(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakSpec {
    /// A point on the break plane, in local cells.
    pub point: [f64; 3],
    /// Points out of the wood, toward the part that breaks off.
    pub normal: [f64; 3],
    /// How far pits and the uneven face reach into the wood, 0..=32 cells.
    pub depth: f64,
    pub splinters: Splinters,
    /// Splinter length range in cells, 1..=64.
    pub length: (f64, f64),
    /// Splinter radius at its base, 0.5..=4.
    pub thickness: f64,
    /// 0 keeps the base radius to the tip; 1 thins it to one cell.
    pub taper: f64,
    /// 0..=1: how much more often splinters start near the rim, and how
    /// much longer they are there.
    pub rim_bias: f64,
    /// Colour of the rim of the new face; `None` keeps the cell's colour.
    pub bark: Option<u8>,
    /// Colours of the new face and the splinters, mixed per cell; empty
    /// keeps the cell's colour.
    pub sapwood: Vec<u8>,
    /// Colour of the outer part of each splinter; `None` uses the sapwood.
    pub tips: Option<u8>,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Broken {
    /// `(x, y, z, palette)`; 0 removes.
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub removed: usize,
    pub recoloured: usize,
    pub splinters: usize,
}

/// Cut `cells` along a jagged surface around the plane and grow tapered
/// splinters out of the new face along the normal.
///
/// Every cell on the outer side of the surface goes. The surface sits on the
/// plane and dips up to `depth` into the wood: blocks of 2 by 3 cells across
/// the face, a quarter of them deep pits, plus a per-cell jitter, as wood
/// tears along its grain. The cells left facing the cut take `bark` on the
/// rim of the section and `sapwood` inside. Splinters are thin flat tubes
/// from cells of the new face, out along the normal with a little lean,
/// sapwood at the base and `tips` beyond. They fill only empty cells.
pub fn break_end(cells: &HashMap<Cell, u8>, spec: &BreakSpec) -> Result<Broken, String> {
    let normal = normalize(spec.normal).ok_or("normal must not be zero")?;
    if spec.point.iter().any(|v| !v.is_finite()) {
        return Err("point must be finite".into());
    }
    if !(0.0..=32.0).contains(&spec.depth) {
        return Err(format!("depth must be 0..=32, not {}", spec.depth));
    }
    let (lo, hi) = spec.length;
    if !(1.0..=64.0).contains(&lo) || !(lo..=64.0).contains(&hi) {
        return Err(format!(
            "length must be [min, max] with 1 <= min <= max <= 64, not [{lo}, {hi}]"
        ));
    }
    if !(0.5..=4.0).contains(&spec.thickness) {
        return Err(format!("thickness must be 0.5..=4, not {}", spec.thickness));
    }
    if !(0.0..=1.0).contains(&spec.taper) || !(0.0..=1.0).contains(&spec.rim_bias) {
        return Err("taper and rimBias must be 0..=1".into());
    }
    match spec.splinters {
        Splinters::Count(count) if count > MAX_SPLINTERS => {
            return Err(format!("splinters must be 0..={MAX_SPLINTERS}"));
        }
        Splinters::Density(density) if !(0.0..=1.0).contains(&density) => {
            return Err("density must be 0..=1".into());
        }
        _ => {}
    }
    let u = normalize(perpendicular(normal)).unwrap_or([0.0, 1.0, 0.0]);
    let v = cross(normal, u);
    let local = |cell: &Cell| {
        let d = sub(center(*cell), spec.point);
        (dot(d, normal), dot(d, u), dot(d, v))
    };
    let depth = spec.depth;
    let seed = spec.seed;
    let dip = |uu: f64, vv: f64| {
        let block = hash01(
            (uu / 2.0).floor() as i64,
            (vv / 3.0).floor() as i64,
            1,
            seed,
        );
        let jitter = hash01(uu.round() as i64, vv.round() as i64, 2, seed);
        if block < 0.25 {
            depth * (0.65 + 0.35 * jitter)
        } else {
            depth * (0.45 * block + 0.2 * jitter)
        }
    };

    let mut removed = HashSet::new();
    let mut section: HashSet<(i64, i64)> = HashSet::new();
    for cell in cells.keys() {
        let (s, uu, vv) = local(cell);
        if s > -dip(uu, vv) + 1.0e-9 {
            removed.insert(*cell);
        }
        if s.abs() <= depth + 1.0 {
            section.insert((uu.round() as i64, vv.round() as i64));
        }
    }
    const FACES: [Cell; 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    let mut exposed: Vec<Cell> = cells
        .keys()
        .filter(|cell| !removed.contains(*cell))
        .filter(|cell| {
            FACES
                .iter()
                .any(|f| removed.contains(&(cell.0 + f.0, cell.1 + f.1, cell.2 + f.2)))
        })
        .copied()
        .collect();
    exposed.sort();
    let key_of = |cell: &Cell| {
        let (_, uu, vv) = local(cell);
        (uu.round() as i64, vv.round() as i64)
    };
    let rim = |key: (i64, i64)| {
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|(du, dv)| !section.contains(&(key.0 + du, key.1 + dv)))
    };
    let (mut cu, mut cv) = (0.0, 0.0);
    for key in &section {
        cu += key.0 as f64;
        cv += key.1 as f64;
    }
    let count = section.len().max(1) as f64;
    let (cu, cv) = (cu / count, cv / count);
    let reach = section
        .iter()
        .map(|k| ((k.0 as f64 - cu).powi(2) + (k.1 as f64 - cv).powi(2)).sqrt())
        .fold(1.0f64, f64::max);
    let radial = |key: (i64, i64)| {
        (((key.0 as f64 - cu).powi(2) + (key.1 as f64 - cv).powi(2)).sqrt() / reach).min(1.0)
    };
    let sap = |cell: Cell, fallback: u8| -> u8 {
        if spec.sapwood.is_empty() {
            fallback
        } else {
            let pick = hash01(cell.0.into(), cell.1.into(), cell.2.into(), seed ^ 7);
            spec.sapwood[(pick * spec.sapwood.len() as f64) as usize % spec.sapwood.len()]
        }
    };

    let mut out: HashMap<Cell, u8> = HashMap::new();
    for cell in &removed {
        out.insert(*cell, 0);
    }
    let mut recoloured = 0;
    for cell in &exposed {
        let old = cells[cell];
        let next = if rim(key_of(cell)) {
            spec.bark.unwrap_or(old)
        } else {
            sap(*cell, old)
        };
        if next != old {
            out.insert(*cell, next);
            recoloured += 1;
        }
    }

    let wanted = match spec.splinters {
        Splinters::Count(count) => count as usize,
        Splinters::Density(density) => {
            ((density * exposed.len() as f64).round() as usize).min(MAX_SPLINTERS as usize)
        }
    };
    let mut rng = Rng::new(seed);
    let weights: Vec<f64> = exposed
        .iter()
        .map(|cell| 1.0 + 4.0 * spec.rim_bias * radial(key_of(cell)).powi(2))
        .collect();
    let total: f64 = weights.iter().sum();
    let mut splinters = 0;
    let mut used = HashSet::new();
    for _ in 0..wanted * 4 {
        if splinters >= wanted || exposed.is_empty() {
            break;
        }
        let mut target = rng.unit() * total;
        let mut index = 0;
        while index + 1 < exposed.len() && target > weights[index] {
            target -= weights[index];
            index += 1;
        }
        if !used.insert(index) {
            continue;
        }
        let base = exposed[index];
        let key = key_of(&base);
        let on_rim = rim(key);
        let near = radial(key);
        let t = rng.unit() * (1.0 - spec.rim_bias) + spec.rim_bias * near * rng.range(0.5, 1.0);
        let length = lo + (hi - lo) * t.clamp(0.0, 1.0);
        let lean = add(
            scale(u, rng.range(-0.12, 0.12)),
            scale(v, rng.range(-0.12, 0.12)),
        );
        let direction = normalize(add(normal, lean)).unwrap_or(normal);
        let start = center(base);
        let tip_radius = (spec.thickness * (1.0 - spec.taper)).max(0.5);
        let points = [
            TubePoint {
                at: start,
                radius: spec.thickness,
            },
            TubePoint {
                at: add(start, scale(direction, length)),
                radius: tip_radius,
            },
        ];
        let shape = TubeShape {
            aspect: if spec.thickness >= 1.0 { 0.6 } else { 1.0 },
            roll: rng.range(0.0, std::f64::consts::PI),
            blade: false,
        };
        let body = tube_shaped(&points, false, shape)?;
        let base_colour = cells[&base];
        for piece in body {
            let cell = piece.cell;
            let free = match out.get(&cell) {
                Some(slot) => *slot == 0,
                None => !cells.contains_key(&cell),
            };
            if !free {
                continue;
            }
            let fraction = piece.along / length.max(1.0);
            let slot = if on_rim && piece.along < 2.0 {
                spec.bark.unwrap_or(base_colour)
            } else if fraction < 0.55 {
                sap(cell, base_colour)
            } else {
                spec.tips.unwrap_or_else(|| sap(cell, base_colour))
            };
            out.insert(cell, slot);
        }
        if out.len() > MAX_SCULPT_CELLS as usize {
            return Err(format!(
                "the break touches more than {MAX_SCULPT_CELLS} cells; use fewer or shorter splinters"
            ));
        }
        splinters += 1;
    }
    let mut writes: Vec<_> = out
        .into_iter()
        .filter(|(cell, slot)| cells.get(cell).copied().unwrap_or(0) != *slot)
        .map(|((x, y, z), slot)| (x, y, z, i32::from(slot)))
        .collect();
    writes.sort();
    Ok(Broken {
        removed: writes
            .iter()
            .filter(|w| w.3 == 0 && removed.contains(&(w.0, w.1, w.2)))
            .count(),
        writes,
        recoloured,
        splinters,
    })
}

fn center(cell: Cell) -> [f64; 3] {
    [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)]
}

fn perpendicular(n: [f64; 3]) -> [f64; 3] {
    let reference = if n[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    sub(reference, scale(n, dot(reference, n)))
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
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

fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let len = dot(a, a).sqrt();
    (len > 1.0e-9 && len.is_finite()).then(|| scale(a, 1.0 / len))
}
