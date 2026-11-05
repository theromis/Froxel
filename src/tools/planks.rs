//! Boards behind MCP `planks`: a deck, floor, or dock of separate boards.
//!
//! Rows of random width tile the rectangle across the axis, with a gap between
//! neighbours. Each row is split along the axis into boards of random length
//! with a gap at each end joint, and a row's joints keep clear of those of the
//! row before it. A gap runs down through the boards to a `dirt` cell. Each
//! board takes one tone, never that of the board before it in the row, and as
//! few as it can of the boards beside it. So no layout runs as one uniform
//! stripe: a row long enough for a joint always has one, and it always changes
//! tone across that joint.

use std::collections::{HashMap, HashSet};

use super::debris::Rng;
use super::surface::{allowed, check_slot, check_slots, check_unit, Exclude, Region};
use crate::state::Cell;

/// The shortest piece a row is sawn to at a joint.
const END_MIN: i32 = 2;

/// The way the boards run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlankAxis {
    X,
    Y,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlankSpec {
    /// Inclusive corners of the rectangle, in x and in y.
    pub x: (i32, i32),
    pub y: (i32, i32),
    /// The top layer of the boards.
    pub z: i32,
    pub axis: PlankAxis,
    /// Board tones; each board takes one. At least two different slots.
    pub palettes: Vec<u8>,
    /// Slot of the cell under a gap. `None` leaves that cell as it is.
    pub dirt: Option<u8>,
    /// Slot of nails and knots. `None` lays neither.
    pub dark: Option<u8>,
    /// Board width range across the axis, 1..=64.
    pub width: (i32, i32),
    /// Board length range along the axis, 2..=256.
    pub length: (i32, i32),
    /// Gap range between boards, 1..=8.
    pub gap: (i32, i32),
    /// Layers of board under `z`, top layer included, 1..=16.
    pub thickness: i32,
    /// Chance per cell across a board's end that it is cut back, 0..=1.
    pub ragged: f64,
    /// Share of boards with a nail at each end, 0..=1.
    pub nails: f64,
    /// Share of boards with a knot, 0..=1.
    pub knots: f64,
    /// Act only on cells of these slots. Empty lays boards everywhere.
    pub only: Vec<u8>,
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Planked {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub rows: usize,
    /// Boards with at least one cell.
    pub boards: usize,
    /// End joints inside the rows.
    pub joints: usize,
    pub added: usize,
    pub painted: usize,
    pub removed: usize,
    pub dirtied: usize,
    pub nails: usize,
    pub knots: usize,
}

/// A board, `from..to` along the axis, and its index into `palettes`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Board {
    pub from: i32,
    pub to: i32,
    pub tone: usize,
}

/// A row, `from..to` across the axis.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub from: i32,
    pub to: i32,
    pub boards: Vec<Board>,
}

fn draw(rng: &mut Rng, lo: i32, hi: i32) -> i32 {
    lo + (rng.next() % (hi - lo + 1) as u64) as i32
}

/// The rows across `across` cells. A row narrower than `width` only when the
/// rectangle is; the last may run over by what no further row has room for.
fn lay_rows(rng: &mut Rng, across: i32, width: (i32, i32), gap: (i32, i32)) -> Vec<(i32, i32)> {
    let mut rows = Vec::new();
    let mut at = 0;
    loop {
        let rest = across - at;
        let space = draw(rng, gap.0, gap.1);
        let top = width.1.min(rest - space - width.0);
        if top < width.0 || (rest <= width.1 && rng.unit() < 0.5) {
            rows.push((at, across));
            return rows;
        }
        let wide = draw(rng, width.0, top);
        rows.push((at, at + wide));
        at += wide + space;
    }
}

/// How far the gap `at..at + wide` lies from the gaps of the row before: 0
/// touching at a corner, negative when they line up.
fn clearance(at: i32, wide: i32, above: &[(i32, i32)]) -> i32 {
    above
        .iter()
        .map(|(from, to)| (from - (at + wide)).max(at - to))
        .min()
        .unwrap_or(i32::MAX)
}

/// The boards of a row `len` cells long, as `from..to`. A row with room for a
/// joint gets one, however long `length` is: its first board is sawn to fit.
/// Its first and last boards are sawn pieces, so they may be shorter than
/// `length`, and the last may run over by what no joint has room for. A board
/// gives way by a few cells when its joint would otherwise line up with one
/// in `above`.
fn lay_boards(
    rng: &mut Rng,
    len: i32,
    length: (i32, i32),
    gap: (i32, i32),
    above: &[(i32, i32)],
) -> Vec<(i32, i32)> {
    let clear = (length.0 / 4).clamp(2, 6);
    let mut boards = Vec::new();
    let mut at = 0;
    loop {
        let first = boards.is_empty();
        let rest = len - at;
        let space = draw(rng, gap.0, gap.1);
        // The longest board that leaves a joint and a piece after it.
        let room = rest - space - END_MIN;
        let top = length.1.min(room);
        let shortest = if first { END_MIN } else { length.0 };
        if top < shortest || (!first && rest <= length.1 && rng.unit() < 0.5) {
            boards.push((at, len));
            return boards;
        }
        let lo = if first {
            (length.0 / 2).max(END_MIN).min(top)
        } else {
            shortest
        };
        let mut best = (i32::MIN, lo);
        for _ in 0..16 {
            let long = draw(rng, lo, top);
            let found = clearance(at + long, space, above);
            if found > best.0 {
                best = (found, long);
            }
            if found >= clear {
                break;
            }
        }
        if best.0 < clear && !first && rest <= length.1 {
            // No joint keeps clear this near the end, and one board reaches it.
            boards.push((at, len));
            return boards;
        }
        if best.0 < clear {
            // Nothing in range keeps clear: the board nearest the range that
            // does, or that at least does not line up.
            let give = clear + 1;
            let wide_lo = (lo - give).max(END_MIN);
            let wide_hi = (top + give).min(room);
            let beyond = |long: &i32| (lo - *long).max(*long - top).max(0);
            let mut sizes: Vec<i32> = (wide_lo..=wide_hi).collect();
            sizes.sort_by_key(beyond);
            for need in [clear, 1, 0] {
                if best.0 >= need {
                    break;
                }
                if let Some(long) = sizes
                    .iter()
                    .find(|long| clearance(at + **long, space, above) >= need)
                {
                    best = (need, *long);
                    break;
                }
            }
        }
        boards.push((at, at + best.1));
        at += best.1 + space;
    }
}

/// A tone per board. It is never the tone of the board before it in the row,
/// and of the rest, the one that lies least beside itself in the row before.
fn tone_rows(rng: &mut Rng, rows: &mut [Row], tones: &[u8]) {
    let n = tones.len();
    for index in 0..rows.len() {
        let (done, rest) = rows.split_at_mut(index);
        let before = done.last().map_or(&[][..], |row| row.boards.as_slice());
        let mut previous: Option<u8> = None;
        for board in &mut rest[0].boards {
            let start = (rng.next() % n as u64) as usize;
            let mut best = (u64::MAX, start);
            for step in 0..n {
                let tone = (start + step) % n;
                let beside: i32 = before
                    .iter()
                    .filter(|other| tones[other.tone] == tones[tone])
                    .map(|other| (board.to.min(other.to) - board.from.max(other.from)).max(0))
                    .sum();
                let same = if previous == Some(tones[tone]) {
                    u64::MAX / 2
                } else {
                    0
                };
                let score = same + beside as u64;
                if score < best.0 {
                    best = (score, tone);
                }
            }
            board.tone = best.1;
            previous = Some(tones[best.1]);
        }
    }
}

/// The rows and boards of a spec, tones chosen.
pub(super) fn layout(spec: &PlankSpec, rng: &mut Rng) -> Vec<Row> {
    let (along, across) = extents(spec);
    let spans = lay_rows(rng, across, spec.width, spec.gap);
    let mut rows: Vec<Row> = Vec::with_capacity(spans.len());
    for (from, to) in spans {
        let above: Vec<(i32, i32)> = rows.last().map_or(Vec::new(), |row| {
            row.boards
                .windows(2)
                .map(|pair| (pair[0].to, pair[1].from))
                .collect()
        });
        let boards = lay_boards(rng, along, spec.length, spec.gap, &above)
            .into_iter()
            .map(|(from, to)| Board { from, to, tone: 0 })
            .collect();
        rows.push(Row { from, to, boards });
    }
    tone_rows(rng, &mut rows, &spec.palettes);
    rows
}

/// The rectangle's cells along the axis and across it.
fn extents(spec: &PlankSpec) -> (i32, i32) {
    let dx = spec.x.1 - spec.x.0 + 1;
    let dy = spec.y.1 - spec.y.0 + 1;
    match spec.axis {
        PlankAxis::X => (dx, dy),
        PlankAxis::Y => (dy, dx),
    }
}

fn check(spec: &PlankSpec) -> Result<(), String> {
    let mut tones = spec.palettes.clone();
    tones.sort_unstable();
    tones.dedup();
    if tones.len() < 2 {
        return Err(
            "palettes must hold at least two different slots: a tone per board, so the boards never read as one stripe"
                .into(),
        );
    }
    check_slots(&spec.palettes, "palettes")?;
    if let Some(dirt) = spec.dirt {
        check_slot(dirt, "dirt")?;
    }
    if let Some(dark) = spec.dark {
        check_slot(dark, "dark")?;
    }
    check_unit(spec.ragged, "ragged")?;
    check_unit(spec.nails, "nails")?;
    check_unit(spec.knots, "knots")?;
    let ranges = [
        ("width", spec.width, 1, 64),
        ("length", spec.length, 2, 256),
        ("gap", spec.gap, 1, 8),
    ];
    for (name, (lo, hi), least, most) in ranges {
        if !(least..=most).contains(&lo) || !(lo..=most).contains(&hi) {
            return Err(format!(
                "{name} must be [min, max] with {least} <= min <= max <= {most}, not [{lo}, {hi}]"
            ));
        }
    }
    if !(1..=16).contains(&spec.thickness) {
        return Err(format!("thickness must be 1..=16, not {}", spec.thickness));
    }
    Ok(())
}

pub fn planks(cells: &HashMap<Cell, u8>, spec: &PlankSpec) -> Result<Planked, String> {
    check(spec)?;
    let (x0, y0) = (spec.x.0.min(spec.x.1), spec.y.0.min(spec.y.1));
    let spec = PlankSpec {
        x: (x0, spec.x.0.max(spec.x.1)),
        y: (y0, spec.y.0.max(spec.y.1)),
        ..spec.clone()
    };
    let spec = &spec;
    let top = spec.z;
    let under = top - spec.thickness;
    Region::new(
        (spec.x.0, spec.y.0, under),
        (spec.x.1, spec.y.1, top),
        "planks",
    )?;

    let mut rng = Rng::new(spec.seed ^ 0x706c_616e_6b73);
    let rows = layout(spec, &mut rng);
    let (along, across) = extents(spec);
    let at = |u: i32, v: i32| (v as usize) * (along as usize) + u as usize;

    // Per column, its board, numbered through the rows; none in a gap.
    let mut grid: Vec<Option<(u32, u8)>> = vec![None; (along as usize) * (across as usize)];
    let mut count = 0u32;
    for row in &rows {
        for board in &row.boards {
            let slot = spec.palettes[board.tone];
            for v in row.from..row.to {
                for u in board.from..board.to {
                    grid[at(u, v)] = Some((count, slot));
                }
            }
            count += 1;
        }
    }
    // A ragged end is cut back cell by cell, into the gap.
    if spec.ragged > 0.0 {
        for row in &rows {
            for board in &row.boards {
                let most = ((board.to - board.from - 1) / 2).min(2);
                for v in row.from..row.to {
                    for end in [false, true] {
                        let hit = rng.unit() < spec.ragged;
                        let cut = 1 + (rng.next() % 2) as i32;
                        if !hit {
                            continue;
                        }
                        for step in 0..cut.min(most) {
                            let u = if end {
                                board.to - 1 - step
                            } else {
                                board.from + step
                            };
                            grid[at(u, v)] = None;
                        }
                    }
                }
            }
        }
    }
    // Nails near a board's ends and knots along its middle, in the top layer.
    let mut marks: HashSet<(i32, i32)> = HashSet::new();
    let (mut nails, mut knots) = (0, 0);
    if spec.dark.is_some() {
        for row in &rows {
            let wide = row.to - row.from;
            for board in &row.boards {
                let long = board.to - board.from;
                let across_at = |rng: &mut Rng| row.from + (rng.next() % wide.max(1) as u64) as i32;
                if rng.unit() < spec.nails {
                    let ends: &[i32] = if long >= 4 {
                        &[board.from + 1, board.to - 2]
                    } else {
                        &[board.from + long / 2]
                    };
                    for u in ends {
                        let v = across_at(&mut rng);
                        if grid[at(*u, v)].is_some() && marks.insert((*u, v)) {
                            nails += 1;
                        }
                    }
                }
                if rng.unit() < spec.knots && long >= 5 {
                    let u = board.from + long / 4 + (rng.next() % (long / 2) as u64) as i32;
                    let v = across_at(&mut rng);
                    if grid[at(u, v)].is_some() && marks.insert((u, v)) {
                        knots += 1;
                        // A larger knot takes the cell beside it on a wide board.
                        if wide >= 3
                            && rng.unit() < 0.5
                            && u + 1 < board.to
                            && grid[at(u + 1, v)].is_some()
                        {
                            marks.insert((u + 1, v));
                        }
                    }
                }
            }
        }
    }

    let usable = |c: Cell| {
        !spec.exclude.contains(c)
            && (spec.only.is_empty() || cells.get(&c).is_some_and(|s| allowed(&spec.only, *s)))
    };
    let mut writes = Vec::new();
    let mut seen = vec![false; count as usize];
    let (mut added, mut painted, mut removed, mut dirtied) = (0, 0, 0, 0);
    for v in 0..across {
        for u in 0..along {
            let (x, y) = match spec.axis {
                PlankAxis::X => (spec.x.0 + u, spec.y.0 + v),
                PlankAxis::Y => (spec.x.0 + v, spec.y.0 + u),
            };
            let board = grid[at(u, v)];
            let mut active = spec.only.is_empty();
            for z in (under + 1)..=top {
                let cell = (x, y, z);
                if !usable(cell) {
                    continue;
                }
                active = true;
                let old = cells.get(&cell).copied();
                match board {
                    Some((id, tone)) => {
                        seen[id as usize] = true;
                        let slot = match spec.dark {
                            Some(dark) if z == top && marks.contains(&(u, v)) => dark,
                            _ => tone,
                        };
                        if old != Some(slot) {
                            writes.push((x, y, z, i32::from(slot)));
                            if old.is_some() {
                                painted += 1;
                            } else {
                                added += 1;
                            }
                        }
                    }
                    None => {
                        if old.is_some() {
                            writes.push((x, y, z, 0));
                            removed += 1;
                        }
                    }
                }
            }
            if let (None, Some(dirt), true) = (board, spec.dirt, active) {
                let cell = (x, y, under);
                if !spec.exclude.contains(cell) {
                    if cells.get(&cell) != Some(&dirt) {
                        writes.push((x, y, under, i32::from(dirt)));
                    }
                    dirtied += 1;
                }
            }
        }
    }
    Ok(Planked {
        writes,
        rows: rows.len(),
        boards: seen.iter().filter(|shown| **shown).count(),
        joints: rows.iter().map(|row| row.boards.len() - 1).sum(),
        added,
        painted,
        removed,
        dirtied,
        nails,
        knots,
    })
}

#[cfg(test)]
#[path = "../../tests/planks.rs"]
mod tests;
