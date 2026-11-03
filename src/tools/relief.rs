//! Column relief behind MCP `column_relief`: a fractured cliff face broken
//! into columns of random width that stand out of the face by a random
//! amount and end at uneven heights around a crown.
//!
//! The face is found in the cells, not given: for each step along it, the
//! frontmost solid cell of every row up to the crown, and the median of
//! those, so a ledge does not pull the face forward and a swept, curved
//! wall gives a curved face.

use std::collections::HashMap;

use super::debris::Rng;
use super::surface::{check_slot, Exclude, Region};
use crate::state::Cell;

#[derive(Clone, Debug, PartialEq)]
pub struct ReliefSpec {
    pub region: Region,
    /// The face's outward direction: axis 0 (x) or 1 (y), and its sign.
    pub facing: (usize, i32),
    /// Column width range along the face in cells, 1..=64.
    pub width: (i32, i32),
    /// How far a column stands out of the face in cells, 0..=16.
    pub out: (i32, i32),
    /// The crown: the height the column tops scatter around. `None` is the
    /// highest solid cell in the box.
    pub crown: Option<i32>,
    /// Column top range relative to the crown in cells, -256..=256.
    pub top: (i32, i32),
    /// A column that ends below the crown cuts the wall above it, so the
    /// crown is uneven. Off, columns only add cells.
    pub lower: bool,
    /// How far a column reaches back into the face, and how deep a cut
    /// above a short column goes, in cells, 1..=16.
    pub depth: i32,
    pub palette: u8,
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Relief {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub columns: usize,
    pub added: usize,
    pub removed: usize,
    /// The crown used.
    pub crown: i32,
}

/// A direction name or vector as `(axis, sign)`: `+x`, `-x`, `+y`, `-y`.
pub fn relief_facing(x: f64, y: f64, z: f64) -> Option<(usize, i32)> {
    match (x, y, z) {
        (x, y, z) if y == 0.0 && z == 0.0 && x != 0.0 => Some((0, x.signum() as i32)),
        (x, y, z) if x == 0.0 && z == 0.0 && y != 0.0 => Some((1, y.signum() as i32)),
        _ => None,
    }
}

fn check_range(range: (i32, i32), min: i32, max: i32, what: &str) -> Result<(), String> {
    let (lo, hi) = range;
    if !(min..=max).contains(&lo) || !(lo..=max).contains(&hi) {
        return Err(format!(
            "{what} must be [min, max] with {min} <= min <= max <= {max}, not [{lo}, {hi}]"
        ));
    }
    Ok(())
}

pub fn column_relief(cells: &HashMap<Cell, u8>, spec: &ReliefSpec) -> Result<Relief, String> {
    check_range(spec.width, 1, 64, "width")?;
    check_range(spec.out, 0, 16, "out")?;
    check_range(spec.top, -256, 256, "top")?;
    if !(1..=16).contains(&spec.depth) {
        return Err("depth must be 1..=16".into());
    }
    check_slot(spec.palette, "palette")?;
    let (axis, sign) = spec.facing;
    if axis > 1 || sign.abs() != 1 {
        return Err("facing must be +x, -x, +y, or -y".into());
    }
    let region = spec.region;
    let along = 1 - axis;
    let get = |c: Cell, i: usize| [c.0, c.1, c.2][i];
    let at = |a: i32, o: i32, z: i32| -> Cell {
        let d = o * sign;
        if axis == 0 {
            (d, a, z)
        } else {
            (a, d, z)
        }
    };
    let base = region.min.2;
    let solid = region.solid(cells);
    let crown = match spec.crown {
        Some(crown) => crown,
        None => solid
            .last()
            .map(|c| c.2)
            .ok_or("no solid cells in the box to raise columns on")?,
    };
    // The frontmost solid cell of each row (a, z), as an outward distance.
    let mut front: HashMap<(i32, i32), i32> = HashMap::new();
    for c in solid {
        if c.2 > crown {
            continue;
        }
        let o = get(c, axis) * sign;
        let row = front.entry((get(c, along), c.2)).or_insert(o);
        *row = (*row).max(o);
    }
    let (a0, a1) = (get(region.min, along), get(region.max, along));
    let face: Vec<Option<i32>> = (a0..=a1)
        .map(|a| {
            let mut rows: Vec<i32> = (base..=crown.min(region.max.2))
                .filter_map(|z| front.get(&(a, z)).copied())
                .collect();
            rows.sort_unstable();
            (!rows.is_empty()).then(|| rows[rows.len() / 2])
        })
        .collect();
    if face.iter().all(Option::is_none) {
        return Err("no solid face in the box at or below the crown".into());
    }
    let (o_lo, o_hi) = if sign > 0 {
        (get(region.min, axis), get(region.max, axis))
    } else {
        (-get(region.max, axis), -get(region.min, axis))
    };
    let mut rng = Rng::new(spec.seed ^ 0x636f_6c75_6d6e);
    let pick =
        |rng: &mut Rng, (lo, hi): (i32, i32)| lo + (rng.next() % (hi - lo + 1) as u64) as i32;
    let mut added: HashMap<Cell, u8> = HashMap::new();
    let mut removed: Vec<Cell> = Vec::new();
    let mut columns = 0;
    let mut a = a0;
    while a <= a1 {
        let w = pick(&mut rng, spec.width);
        let end = (a + w - 1).min(a1);
        let out = pick(&mut rng, spec.out);
        let top = (crown + pick(&mut rng, spec.top)).min(region.max.2);
        // A step with no wall, past an end of it, gets no column.
        for step in a..=end {
            let Some(f) = face[(step - a0) as usize] else {
                continue;
            };
            let back = (f - spec.depth + 1).max(o_lo);
            for z in base..=top {
                for o in back..=(f + out).min(o_hi) {
                    let c = at(step, o, z);
                    if !cells.contains_key(&c) && !spec.exclude.contains(c) {
                        added.insert(c, spec.palette);
                    }
                }
            }
            if spec.lower && top < crown {
                for z in top + 1..=region.max.2 {
                    for o in back..=o_hi {
                        let c = at(step, o, z);
                        if cells.contains_key(&c) && !spec.exclude.contains(c) {
                            removed.push(c);
                        }
                    }
                }
            }
        }
        columns += 1;
        a = end + 1;
    }
    let mut writes: Vec<(i32, i32, i32, i32)> = removed
        .iter()
        .map(|c| (c.0, c.1, c.2, 0))
        .chain(added.iter().map(|(c, s)| (c.0, c.1, c.2, i32::from(*s))))
        .collect();
    writes.sort_unstable();
    Ok(Relief {
        writes,
        columns,
        added: added.len(),
        removed: removed.len(),
        crown,
    })
}
