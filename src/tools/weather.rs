//! Weathering behind MCP `weather`: chipped edges and corners, pits that
//! dent flat faces, and subtle colour noise on the exposed cells of a
//! region. Faces a chip or a pit uncovers take the `broken` slot.

use std::collections::{HashMap, HashSet};

use super::debris::Rng;
use super::surface::{
    allowed, cell01, check_slot, check_slots, check_unit, noise3, step, Exclude, Region, FACES,
};
use crate::state::Cell;

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherSpec {
    pub region: Region,
    /// Chance that an edge cell starts a chip, 0..=1. Corners are likelier.
    pub chip: f64,
    /// Most cells one chip takes, 1..=8.
    pub chip_size: u32,
    /// Chance that a flat face cell starts a pit, 0..=1.
    pub pits: f64,
    /// Deepest pit in cells, 1..=4.
    pub pit_depth: u32,
    /// Slot for the faces a chip or pit uncovers. `None` keeps their slot.
    pub broken: Option<u8>,
    /// Slots of the colour noise. Empty leaves colours alone.
    pub noise: Vec<u8>,
    /// Share of the exposed cells the noise recolours, 0..=1.
    pub noise_density: f64,
    /// Patch size of the noise in cells, 1..=32.
    pub noise_scale: f64,
    /// Act only on cells of these slots. Empty acts on every slot.
    pub only: Vec<u8>,
    /// Boxes left alone: no chip, pit, broken face, or noise inside them.
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Weathered {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub chips: usize,
    pub pits: usize,
    pub removed: usize,
    pub broken: usize,
    pub recoloured: usize,
}

pub fn weather(cells: &HashMap<Cell, u8>, spec: &WeatherSpec) -> Result<Weathered, String> {
    check_unit(spec.chip, "chip")?;
    check_unit(spec.pits, "pits")?;
    check_unit(spec.noise_density, "noiseDensity")?;
    if !(1..=8).contains(&spec.chip_size) {
        return Err("chipSize must be 1..=8".into());
    }
    if !(1..=4).contains(&spec.pit_depth) {
        return Err("pitDepth must be 1..=4".into());
    }
    if !(1.0..=32.0).contains(&spec.noise_scale) {
        return Err("noiseScale must be 1..=32".into());
    }
    if let Some(broken) = spec.broken {
        check_slot(broken, "broken")?;
    }
    check_slots(&spec.noise, "noise")?;
    let region = spec.region;
    let mut rng = Rng::new(spec.seed);
    let mut gone: HashSet<Cell> = HashSet::new();
    let solid = |c: &Cell, gone: &HashSet<Cell>| cells.contains_key(c) && !gone.contains(c);
    let usable = |c: &Cell, gone: &HashSet<Cell>| {
        region.contains(*c)
            && !spec.exclude.contains(*c)
            && solid(c, gone)
            && allowed(&spec.only, cells[c])
    };
    let open = |c: Cell, gone: &HashSet<Cell>| -> Vec<Cell> {
        FACES
            .iter()
            .copied()
            .filter(|d| !solid(&step(c, *d), gone))
            .collect()
    };
    let list = region.solid(cells);

    // Chips: edges and corners of the shape as it was lose small irregular
    // chunks, so one chip does not expose new edges for the next.
    let before = HashSet::new();
    let mut chips = 0;
    for &cell in &list {
        if !usable(&cell, &gone) {
            continue;
        }
        let faces = open(cell, &before);
        let edge = faces
            .iter()
            .any(|a| faces.iter().any(|b| a.0 * b.0 + a.1 * b.1 + a.2 * b.2 == 0));
        if !edge {
            continue;
        }
        let chance = spec.chip * (1.0 + 0.5 * (faces.len() as f64 - 2.0)).min(2.0);
        if cell01(cell, spec.seed, 1) >= chance {
            continue;
        }
        let size = 1 + (rng.next() % u64::from(spec.chip_size)) as usize;
        let mut chunk = vec![cell];
        gone.insert(cell);
        while chunk.len() < size {
            let from = chunk[(rng.next() % chunk.len() as u64) as usize];
            let d = FACES[(rng.next() % 6) as usize];
            let next = step(from, d);
            // A chip stays at the surface: each cell it takes was exposed.
            if usable(&next, &gone) && !open(next, &before).is_empty() {
                gone.insert(next);
                chunk.push(next);
            } else if rng.unit() < 0.3 {
                break;
            }
        }
        chips += 1;
    }

    // Pits: flat faces take small dents.
    let mut pits = 0;
    for &cell in &list {
        if !usable(&cell, &gone) {
            continue;
        }
        let faces = open(cell, &before);
        if faces.len() != 1 || cell01(cell, spec.seed, 2) >= spec.pits {
            continue;
        }
        let inward = (-faces[0].0, -faces[0].1, -faces[0].2);
        let deep = 1 + (rng.next() % u64::from(spec.pit_depth)) as i32;
        let mut at = cell;
        for _ in 0..deep {
            if !usable(&at, &gone) {
                break;
            }
            gone.insert(at);
            at = step(at, inward);
        }
        // Half the pits are two cells wide.
        if rng.unit() < 0.5 {
            let side = FACES[(rng.next() % 6) as usize];
            let beside = step(cell, side);
            if side != faces[0] && side != inward && usable(&beside, &gone) {
                gone.insert(beside);
            }
        }
        pits += 1;
    }

    let mut writes: Vec<(i32, i32, i32, i32)> = Vec::new();
    let mut removed: Vec<Cell> = gone.iter().copied().collect();
    removed.sort_unstable_by_key(|c| (c.2, c.1, c.0));
    for c in &removed {
        writes.push((c.0, c.1, c.2, 0));
    }

    // Broken faces: what a chip or pit uncovered.
    let mut painted: HashSet<Cell> = HashSet::new();
    if let Some(slot) = spec.broken {
        for c in &removed {
            for d in FACES {
                let n = step(*c, d);
                if usable(&n, &gone) && painted.insert(n) && cells[&n] != slot {
                    writes.push((n.0, n.1, n.2, i32::from(slot)));
                }
            }
        }
    }
    let broken = painted.len();

    // Colour noise: soft patches with a little speckle, on exposed cells.
    let mut recoloured = 0;
    if !spec.noise.is_empty() && spec.noise_density > 0.0 {
        for &cell in &list {
            if painted.contains(&cell) || !usable(&cell, &gone) || open(cell, &gone).is_empty() {
                continue;
            }
            let p = [f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)];
            let patch = noise3(p, spec.noise_scale, spec.seed ^ 21);
            let speckle = cell01(cell, spec.seed, 3);
            let value = 0.7 * ((patch - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0) + 0.3 * speckle;
            if value >= spec.noise_density {
                continue;
            }
            let tone = noise3(p, spec.noise_scale * 1.7, spec.seed ^ 22);
            let index = ((tone * 0.6 + speckle * 0.4) * spec.noise.len() as f64) as usize;
            let slot = spec.noise[index.min(spec.noise.len() - 1)];
            if cells[&cell] != slot {
                writes.push((cell.0, cell.1, cell.2, i32::from(slot)));
                recoloured += 1;
            }
        }
    }
    Ok(Weathered {
        writes,
        chips,
        pits,
        removed: removed.len(),
        broken,
        recoloured,
    })
}
