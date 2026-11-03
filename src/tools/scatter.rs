//! Debris behind MCP `scatter`: small pieces dropped into a region that
//! settle on what is below them, so they pile into a heap.

use std::collections::HashMap;

use super::debris::{hash01, Rng};
use super::sculpt::{tube_shaped, TubePoint, TubeShape, MAX_SCULPT_CELLS};
use super::surface::Exclude;
use crate::state::Cell;

/// Most pieces in one scatter.
pub const MAX_PIECES: u32 = 2000;
/// Most cells one scatter adds.
const MAX_SCATTER_CELLS: usize = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// A flat wedge, like a splinter that broke off.
    Shard,
    /// A thin flat flake one cell thick.
    Chip,
    /// A squashed ellipsoid.
    Pebble,
    /// A small block with chipped corners.
    Chunk,
}

impl PieceKind {
    pub const ALL: [PieceKind; 4] = [
        PieceKind::Shard,
        PieceKind::Chip,
        PieceKind::Pebble,
        PieceKind::Chunk,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Shard => "shard",
            PieceKind::Chip => "chip",
            PieceKind::Pebble => "pebble",
            PieceKind::Chunk => "chunk",
        }
    }

    pub fn parse(text: &str) -> Option<PieceKind> {
        PieceKind::ALL.into_iter().find(|kind| kind.name() == text)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScatterSpec {
    /// Inclusive region: pieces start inside its x and y at its top and
    /// fall no lower than `floor`, or its bottom.
    pub min: Cell,
    pub max: Cell,
    pub count: u32,
    pub kinds: Vec<PieceKind>,
    /// Piece size range in cells, 1..=16.
    pub size: (i32, i32),
    pub palettes: Vec<u8>,
    pub seed: u64,
    pub floor: Option<i32>,
    /// 0 spreads pieces evenly over the region; 1 piles them in its middle.
    pub clump: f64,
    /// Boxes left alone: a piece that would come to rest with a cell inside
    /// one is skipped.
    pub exclude: Exclude,
}

#[derive(Debug, PartialEq)]
pub struct Scattered {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub placed: usize,
    /// Pieces that met a solid cell at the region's top, or would rest in an
    /// excluded box, and were left out.
    pub skipped: usize,
}

/// Drop `count` small pieces into the region, one after another. Each falls
/// straight down from the top of the region until the next step would touch
/// a solid cell (the model or a piece before it) or pass the floor, so they
/// rest on what is below and pile into a heap. Only empty cells are written.
pub fn scatter(cells: &HashMap<Cell, u8>, spec: &ScatterSpec) -> Result<Scattered, String> {
    if spec.count > MAX_PIECES {
        return Err(format!("count must be 0..={MAX_PIECES}"));
    }
    if spec.kinds.is_empty() {
        return Err("kinds must name at least one of shard, chip, pebble, chunk".into());
    }
    if spec.palettes.is_empty() || spec.palettes.contains(&0) {
        return Err("palettes must hold 1..=255 indices".into());
    }
    let (lo, hi) = spec.size;
    if !(1..=16).contains(&lo) || !(lo..=16).contains(&hi) {
        return Err(format!(
            "size must be [min, max] with 1 <= min <= max <= 16, not [{lo}, {hi}]"
        ));
    }
    if !(0.0..=1.0).contains(&spec.clump) {
        return Err("clump must be 0..=1".into());
    }
    let (min, max) = (spec.min, spec.max);
    let volume = (i64::from(max.0 - min.0) + 1)
        * (i64::from(max.1 - min.1) + 1)
        * (i64::from(max.2 - min.2) + 1);
    if volume > MAX_SCULPT_CELLS {
        return Err(format!(
            "region is larger than {MAX_SCULPT_CELLS} cells; scatter a smaller one"
        ));
    }
    let floor = spec.floor.unwrap_or(min.2);
    if floor > max.2 {
        return Err("floor is above the region".into());
    }
    let mut rng = Rng::new(spec.seed);
    let mut added: HashMap<Cell, u8> = HashMap::new();
    let solid = |cell: &Cell, added: &HashMap<Cell, u8>| {
        cells.contains_key(cell) || added.contains_key(cell)
    };
    let half = [
        f64::from(max.0 - min.0) / 2.0,
        f64::from(max.1 - min.1) / 2.0,
    ];
    let mid = [f64::from(min.0) + half[0], f64::from(min.1) + half[1]];
    let mut placed = 0;
    let mut skipped = 0;
    for _ in 0..spec.count {
        let kind = rng.pick(&spec.kinds);
        let size = lo + (rng.next() % (hi - lo + 1) as u64) as i32;
        let slot = rng.pick(&spec.palettes);
        let shape = piece(kind, size, &mut rng)?;
        let mut spot = [0i32; 2];
        for axis in 0..2 {
            let t = rng.range(-1.0, 1.0);
            let t = t.signum() * t.abs().powf(1.0 + 2.0 * spec.clump);
            spot[axis] = (mid[axis] + t * half[axis]).round() as i32;
        }
        let fits = |z: i32, added: &HashMap<Cell, u8>| {
            shape
                .iter()
                .all(|o| !solid(&(spot[0] + o.0, spot[1] + o.1, z + o.2), added))
        };
        let top = max.2;
        if !fits(top, &added) {
            skipped += 1;
            continue;
        }
        let mut z = top;
        while z > floor && fits(z - 1, &added) {
            z -= 1;
        }
        if shape.iter().any(|o| {
            spec.exclude
                .contains((spot[0] + o.0, spot[1] + o.1, z + o.2))
        }) {
            skipped += 1;
            continue;
        }
        for o in &shape {
            added.insert((spot[0] + o.0, spot[1] + o.1, z + o.2), slot);
        }
        placed += 1;
        if added.len() > MAX_SCATTER_CELLS {
            return Err(format!(
                "the pieces add more than {MAX_SCATTER_CELLS} cells; use fewer or smaller pieces"
            ));
        }
    }
    let mut writes: Vec<_> = added
        .into_iter()
        .map(|((x, y, z), slot)| (x, y, z, i32::from(slot)))
        .collect();
    writes.sort();
    Ok(Scattered {
        writes,
        placed,
        skipped,
    })
}

/// The cells of one piece around x = y = 0, its lowest cell at z = 0.
fn piece(kind: PieceKind, size: i32, rng: &mut Rng) -> Result<Vec<Cell>, String> {
    let s = f64::from(size);
    let mut cells: Vec<Cell> = match kind {
        PieceKind::Pebble => {
            let r = [
                s / 2.0,
                s / 2.0 * rng.range(0.7, 1.0),
                (s / 2.0 * rng.range(0.45, 0.75)).max(0.5),
            ];
            let n = size;
            let mut out = Vec::new();
            for z in -n..=n {
                for y in -n..=n {
                    for x in -n..=n {
                        let d = (f64::from(x) / r[0]).powi(2)
                            + (f64::from(y) / r[1]).powi(2)
                            + (f64::from(z) / r[2]).powi(2);
                        if d <= 1.0 {
                            out.push((x, y, z));
                        }
                    }
                }
            }
            out
        }
        PieceKind::Chunk => {
            let dims: Vec<i32> = (0..3)
                .map(|_| ((s * rng.range(0.5, 1.0)).round() as i32).max(1))
                .collect();
            let mut out = Vec::new();
            for z in 0..dims[2] {
                for y in 0..dims[1] {
                    for x in 0..dims[0] {
                        let corner = (x == 0 || x == dims[0] - 1)
                            && (y == 0 || y == dims[1] - 1)
                            && (z == 0 || z == dims[2] - 1);
                        let chipped = corner
                            && dims.iter().all(|d| *d > 1)
                            && hash01(x.into(), y.into(), z.into(), rng.next()) < 0.5;
                        if !chipped {
                            out.push((x - dims[0] / 2, y - dims[1] / 2, z));
                        }
                    }
                }
            }
            out
        }
        PieceKind::Chip => {
            let w = size;
            let d = ((s * rng.range(0.4, 0.8)).round() as i32).max(1);
            let (w, d) = if rng.unit() < 0.5 { (w, d) } else { (d, w) };
            let mut out = Vec::new();
            for y in 0..d {
                for x in 0..w {
                    out.push((x - w / 2, y - d / 2, 0));
                }
            }
            out
        }
        PieceKind::Shard => {
            let length = (s * rng.range(1.0, 1.6)).max(1.0);
            let yaw = rng.range(0.0, std::f64::consts::TAU);
            let rise = rng.range(-0.15, 0.15) * length;
            let points = [
                TubePoint {
                    at: [0.0, 0.0, 0.0],
                    radius: (s * 0.3).max(0.5),
                },
                TubePoint {
                    at: [length * yaw.cos(), length * yaw.sin(), rise],
                    radius: 0.5,
                },
            ];
            let shape = TubeShape {
                aspect: 0.35,
                roll: 0.0,
                blade: true,
            };
            tube_shaped(&points, false, shape)?
                .into_iter()
                .map(|c| c.cell)
                .collect()
        }
    };
    if cells.is_empty() {
        cells.push((0, 0, 0));
    }
    let low = cells.iter().map(|c| c.2).min().unwrap_or(0);
    for cell in &mut cells {
        cell.2 -= low;
    }
    Ok(cells)
}
