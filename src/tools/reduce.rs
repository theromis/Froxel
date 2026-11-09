//! MCP `reduce_palette`: clean up the colours of a voxelized mesh.
//!
//! An image-to-3D import comes out with a score of slots that differ by a
//! shade or two and a per-cell ripple where the texture's noise met the
//! grid. Two passes fix it. The reduction clusters the object's colours
//! into a few (median cut refined by k-means in Oklab, over the surface
//! cells, with the accent guard keeping eyes and mouths), or snaps them to a
//! given palette. The despeckle is a majority filter on the surface: a
//! surface cell whose slot few of its surface neighbours share takes the
//! slot most of them hold. Cells are never added or removed.

use std::collections::HashMap;

use super::colors::{self, Guard, Lab};
use super::Shades;
use crate::state::Cell;

/// Most clusters `colors` asks for.
pub const MAX_COLORS: usize = 64;
/// Most despeckle passes.
pub const MAX_DESPECKLE: u32 = 16;

/// A colour a snap palette names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapTo {
    Slot(u8),
    Rgb([u8; 3]),
}

/// What the reduction does.
#[derive(Clone, Debug, PartialEq)]
pub enum Reduce {
    /// Leave the colours; only despeckle.
    Keep,
    /// Cluster into this many colours.
    Clusters(usize),
    /// Snap every colour to the nearest of these.
    Snap(Vec<SnapTo>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReduceSpec {
    pub reduce: Reduce,
    /// The accent guard of `Clusters`.
    pub guard: Option<Guard>,
    /// Majority filter passes; 0 is none.
    pub despeckle: u32,
    /// A surface cell keeps its slot when at least this many of its
    /// surface neighbours share it.
    pub min_same: u32,
    /// Slots left exactly as they are by both passes.
    pub protect: Vec<u8>,
    /// Only cells in this inclusive box change.
    pub region: Option<(Cell, Cell)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reduced {
    /// Every cell whose slot changes, with its new slot.
    pub writes: Vec<(Cell, u8)>,
    /// Slots whose colour changes in place (a slot only these cells used
    /// takes its cluster's colour), with the colour.
    pub recoloured: Vec<(u8, [u8; 3])>,
    /// Distinct slots of the cells in the region, before and after.
    pub slots: [usize; 2],
    /// Each cluster or snap target: its slot, colour, and cells after.
    pub clusters: Vec<(u8, [u8; 3], usize)>,
    pub accents: usize,
    /// Cells the despeckle recoloured.
    pub despeckled: usize,
}

const FACES: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

fn add(a: Cell, d: (i32, i32, i32)) -> Cell {
    (a.0 + d.0, a.1 + d.1, a.2 + d.2)
}

/// Reduce and despeckle `cells`. `palette` gives slot colours; `shades`
/// allocates new slots; `exclusive` marks slots no cell outside the region
/// (of this object or another) uses, which a cluster may recolour in place.
pub fn reduce_palette(
    cells: &HashMap<Cell, u8>,
    spec: &ReduceSpec,
    palette: &[[u8; 3]; 256],
    shades: &mut Shades,
    exclusive: &[bool; 256],
) -> Result<Reduced, String> {
    match &spec.reduce {
        Reduce::Clusters(n) if *n == 0 || *n > MAX_COLORS => {
            return Err(format!("colors must be 1..={MAX_COLORS}"))
        }
        Reduce::Snap(list) if list.is_empty() || list.len() > 255 => {
            return Err("palette must list 1 to 255 slots or #rrggbb colours".into())
        }
        Reduce::Snap(list) if list.contains(&SnapTo::Slot(0)) => {
            return Err("palette slots must be 1..255".into())
        }
        _ => {}
    }
    if spec.despeckle > MAX_DESPECKLE {
        return Err(format!("despeckle must be 0..={MAX_DESPECKLE} passes"));
    }
    if spec.min_same > 26 {
        return Err("minSame must be 0..26".into());
    }
    if spec.reduce == Reduce::Keep && spec.despeckle == 0 {
        return Err("pass colors, palette, or despeckle: there is nothing to do".into());
    }
    let in_region = |c: Cell| {
        spec.region.is_none_or(|(a, b)| {
            c.0 >= a.0 && c.0 <= b.0 && c.1 >= a.1 && c.1 <= b.1 && c.2 >= a.2 && c.2 <= b.2
        })
    };
    let protected = |slot: u8| spec.protect.contains(&slot);
    let surface = |c: Cell| FACES.iter().any(|d| !cells.contains_key(&add(c, *d)));
    let mut keys: Vec<Cell> = cells.keys().copied().filter(|c| in_region(*c)).collect();
    keys.sort_unstable();
    let distinct = |map: &HashMap<Cell, u8>| {
        let mut seen = [false; 256];
        for c in &keys {
            seen[map[c] as usize] = true;
        }
        seen.iter().filter(|s| **s).count()
    };
    let before = distinct(cells);
    let mut next = cells.clone();
    let mut clusters: Vec<(u8, [u8; 3], usize)> = Vec::new();
    let mut recoloured: Vec<(u8, [u8; 3])> = Vec::new();
    let mut accents = 0;
    // The reduction.
    let targets: Vec<(Lab, u8, [u8; 3])> = match &spec.reduce {
        Reduce::Keep => Vec::new(),
        Reduce::Snap(list) => {
            let mut out = Vec::new();
            for snap in list {
                match *snap {
                    SnapTo::Slot(slot) => {
                        let rgb = palette[slot as usize];
                        out.push((colors::oklab(rgb), slot, rgb));
                    }
                    SnapTo::Rgb(rgb) => {
                        let slot = shades.colour(rgb, 1)?;
                        out.push((colors::oklab(rgb), slot, rgb));
                    }
                }
            }
            out
        }
        Reduce::Clusters(n) => {
            // Surface cells carry the look; one sample each.
            let mut weight: HashMap<u8, f32> = HashMap::new();
            for c in &keys {
                let slot = cells[c];
                if !protected(slot) && surface(*c) {
                    *weight.entry(slot).or_default() += 1.0;
                }
            }
            if weight.is_empty() {
                for c in &keys {
                    let slot = cells[c];
                    if !protected(slot) {
                        *weight.entry(slot).or_default() += 1.0;
                    }
                }
            }
            let mut samples: Vec<(Lab, f32, u8)> = weight
                .iter()
                .map(|(slot, w)| (colors::oklab(palette[*slot as usize]), *w, *slot))
                .collect();
            samples.sort_by_key(|s| s.2);
            let plain: Vec<(Lab, f32)> = samples.iter().map(|(l, w, _)| (*l, *w)).collect();
            let found = colors::cluster(&plain, *n, spec.guard);
            accents = found.accents;
            // Each centre's slot: the commonest member slot no other cell
            // uses, recoloured in place, or else a free slot.
            let mut members: Vec<Vec<(u8, f32)>> = vec![Vec::new(); found.centres.len()];
            for (lab, w, slot) in &samples {
                members[colors::nearest(*lab, &found.centres)].push((*slot, *w));
            }
            let mut order: Vec<usize> = (0..found.centres.len()).collect();
            order.sort_by(|a, b| {
                let wa: f32 = members[*a].iter().map(|m| m.1).sum();
                let wb: f32 = members[*b].iter().map(|m| m.1).sum();
                wb.total_cmp(&wa).then(a.cmp(b))
            });
            let mut claimed = [false; 256];
            let mut slot_of = vec![0u8; found.centres.len()];
            for i in order {
                let rgb = colors::srgb(found.centres[i]);
                let mut list = members[i].clone();
                list.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                let reuse = list
                    .iter()
                    .map(|m| m.0)
                    .find(|s| exclusive[*s as usize] && !claimed[*s as usize]);
                let slot = match reuse {
                    Some(slot) => {
                        if palette[slot as usize] != rgb {
                            recoloured.push((slot, rgb));
                        }
                        shades.colors[slot as usize] = [rgb[0], rgb[1], rgb[2], 255];
                        slot
                    }
                    None => {
                        let from = list.first().map_or(1, |m| m.0);
                        shades.colour(rgb, from)?
                    }
                };
                claimed[slot as usize] = true;
                slot_of[i] = slot;
            }
            found
                .centres
                .iter()
                .zip(&slot_of)
                .map(|(lab, slot)| (*lab, *slot, colors::srgb(*lab)))
                .collect()
        }
    };
    if !targets.is_empty() {
        let labs: Vec<Lab> = targets.iter().map(|t| t.0).collect();
        let mut memo: HashMap<u8, u8> = HashMap::new();
        for c in &keys {
            let slot = cells[c];
            if protected(slot) {
                continue;
            }
            let to = *memo.entry(slot).or_insert_with(|| {
                targets[colors::nearest(colors::oklab(palette[slot as usize]), &labs)].1
            });
            next.insert(*c, to);
        }
    }
    // The despeckle: a majority filter over surface neighbours.
    let mut despeckled = 0;
    let shell: Vec<Cell> = keys.iter().copied().filter(|c| surface(*c)).collect();
    let is_surface: std::collections::HashSet<Cell> =
        cells.keys().copied().filter(|c| surface(*c)).collect();
    for _ in 0..spec.despeckle {
        let mut changes: Vec<(Cell, u8)> = Vec::new();
        for c in &shell {
            let slot = next[c];
            if protected(slot) {
                continue;
            }
            let mut counts: Vec<(u8, u32)> = Vec::new();
            let mut total = 0u32;
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let n = add(*c, (dx, dy, dz));
                        if !is_surface.contains(&n) {
                            continue;
                        }
                        let s = next[&n];
                        total += 1;
                        match counts.iter_mut().find(|(k, _)| *k == s) {
                            Some(found) => found.1 += 1,
                            None => counts.push((s, 1)),
                        }
                    }
                }
            }
            let same = counts.iter().find(|(k, _)| *k == slot).map_or(0, |k| k.1);
            if same >= spec.min_same {
                continue;
            }
            let Some(&(best, count)) = counts
                .iter()
                .filter(|(k, _)| *k != slot)
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            else {
                continue;
            };
            if count * 2 > total {
                changes.push((*c, best));
            }
        }
        if changes.is_empty() {
            break;
        }
        despeckled += changes.len();
        for (c, s) in changes {
            next.insert(c, s);
        }
    }
    let after = distinct(&next);
    let mut per: HashMap<u8, usize> = HashMap::new();
    for c in &keys {
        *per.entry(next[c]).or_default() += 1;
    }
    for (_, slot, rgb) in &targets {
        if !clusters.iter().any(|(s, _, _)| s == slot) {
            clusters.push((*slot, *rgb, per.get(slot).copied().unwrap_or(0)));
        }
    }
    clusters.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let writes: Vec<(Cell, u8)> = keys
        .iter()
        .filter(|c| next[*c] != cells[*c])
        .map(|c| (*c, next[c]))
        .collect();
    Ok(Reduced {
        writes,
        recoloured,
        slots: [before, after],
        clusters,
        accents,
        despeckled,
    })
}

#[cfg(test)]
#[path = "../../tests/reduce.rs"]
mod tests;
