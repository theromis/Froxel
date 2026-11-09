//! `mirror` of the paint and edit tools: one call writes both sides of a
//! plane.
//!
//! A tool with a mirror runs twice: once on the object, and once on the
//! object reflected across the plane, whose writes are reflected back. So
//! the second pass sees the real surface on the far side (a slightly
//! lopsided head still gets its paint on its own cells), and a picture
//! comes out mirrored, as a left eye mirrors a right one. Where both passes
//! write a cell, the first one's slot stays.

use std::collections::HashMap;

use crate::state::Cell;

/// A plane across one axis. Cell `c` spans `c..c + 1` along it, and the
/// plane lies at `twice / 2`, so `twice` odd keeps the middle cell on
/// itself and even puts the plane between two cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mirror {
    /// 0 x, 1 y, 2 z.
    pub axis: usize,
    /// Twice the plane's coordinate along `axis`.
    pub twice: i32,
}

impl Mirror {
    /// The plane through the middle of `cells`' bounds along `axis`, or
    /// `None` when there are no cells.
    pub fn about_centre(axis: usize, cells: impl IntoIterator<Item = Cell>) -> Option<Self> {
        let mut range: Option<(i32, i32)> = None;
        for cell in cells {
            let c = super::coord(cell, axis);
            range = Some(match range {
                None => (c, c),
                Some((lo, hi)) => (lo.min(c), hi.max(c)),
            });
        }
        range.map(|(lo, hi)| Self {
            axis,
            twice: lo + hi + 1,
        })
    }

    /// The plane at coordinate `at` along `axis`; `at` is a multiple of 0.5.
    pub fn at(axis: usize, at: f64) -> Result<Self, String> {
        let twice = at * 2.0;
        if !twice.is_finite() || twice.fract() != 0.0 || twice.abs() > 8192.0 {
            return Err("mirror at must be a multiple of 0.5 within -4096..4096".into());
        }
        Ok(Self {
            axis,
            twice: twice as i32,
        })
    }

    /// The cell across the plane from `cell`.
    pub fn cell(&self, cell: Cell) -> Cell {
        let flip = |c: i32| self.twice - 1 - c;
        match self.axis {
            0 => (flip(cell.0), cell.1, cell.2),
            1 => (cell.0, flip(cell.1), cell.2),
            _ => (cell.0, cell.1, flip(cell.2)),
        }
    }

    /// Every cell of `cells` reflected, slots kept.
    pub fn cells(&self, cells: &HashMap<Cell, u8>) -> HashMap<Cell, u8> {
        cells.iter().map(|(c, s)| (self.cell(*c), *s)).collect()
    }
}

/// Run `op` on `cells` and, with a mirror, again on the reflected cells,
/// merging the second pass's writes reflected back. Returns the writes, the
/// first pass's reply, and how many cells only the mirror pass wrote.
pub fn mirrored<R>(
    cells: &HashMap<Cell, u8>,
    mirror: Option<Mirror>,
    mut op: impl FnMut(&HashMap<Cell, u8>) -> Result<(Vec<(i32, i32, i32, i32)>, R), String>,
) -> Result<(Vec<(i32, i32, i32, i32)>, R, usize), String> {
    let (mut writes, reply) = op(cells)?;
    let Some(mirror) = mirror else {
        return Ok((writes, reply, 0));
    };
    let reflected = mirror.cells(cells);
    let (second, _) = op(&reflected)?;
    let mut seen: std::collections::HashSet<Cell> =
        writes.iter().map(|w| (w.0, w.1, w.2)).collect();
    let mut added = 0;
    for (x, y, z, slot) in second {
        let cell = mirror.cell((x, y, z));
        if seen.insert(cell) {
            writes.push((cell.0, cell.1, cell.2, slot));
            added += 1;
        }
    }
    Ok((writes, reply, added))
}

#[cfg(test)]
#[path = "../../tests/mirror.rs"]
mod tests;
