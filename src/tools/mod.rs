//! The shape, face, generator, and foliage operations.
//!
//! Each one enumerates cells so the session can stamp them onto the same
//! `HashMap` the brush already edits.

use crate::state::Cell;

mod generate;
mod shapes;

pub use generate::{foliage, generate, Gen, MAX_GENERATOR_CELLS};
pub use shapes::{
    bresenham, circle, cube, extrude, flood_empty, flood_face, rounded_inside, rounded_radius,
    square,
};

pub(crate) fn plane_axes(axis: i32) -> (usize, usize) {
    if axis == 0 {
        (1, 2)
    } else if axis == 2 {
        (0, 1)
    } else {
        (0, 2)
    }
}

pub(crate) fn coord(cell: Cell, axis: usize) -> i32 {
    match axis {
        0 => cell.0,
        1 => cell.1,
        _ => cell.2,
    }
}

pub(crate) fn axis_delta(axis: i32, steps: i32) -> Cell {
    match axis {
        0 => (steps, 0, 0),
        1 => (0, steps, 0),
        _ => (0, 0, steps),
    }
}

pub fn add(a: Cell, b: Cell) -> Cell {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

#[cfg(test)]
#[path = "../../tests/tools.rs"]
mod tests;
