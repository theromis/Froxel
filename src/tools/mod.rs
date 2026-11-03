//! The shape, face, generator, and foliage operations.
//!
//! Each one enumerates cells so the session can stamp them onto the same
//! `HashMap` the brush already edits.

use crate::state::Cell;

mod debris;
mod generate;
mod growth;
mod joints;
mod relief;
mod scatter;
mod sculpt;
mod shapes;
mod surface;
mod sweep;
mod weather;

pub use debris::{break_end, BreakSpec, Broken, Splinters};
pub use generate::{foliage, generate, Gen, MAX_GENERATOR_CELLS};
pub use growth::{grow, Face, GrowthKind, GrowthSpec, Layer};
pub use joints::{joints, JointPattern, JointSpec};
pub use relief::{column_relief, relief_facing, ReliefSpec};
pub use scatter::{scatter, PieceKind, ScatterSpec, Scattered, MAX_PIECES};
#[cfg(test)]
pub use sculpt::{smooth, tube};
pub use sculpt::{
    smooth_checked, tube_shaped, Smoothed, TubeCell, TubePattern, TubePoint, TubeShape,
    MAX_SMOOTH_ITERATIONS,
};
pub use shapes::{
    bresenham, circle, cube, extrude, flood_empty, flood_face, rounded_inside, rounded_radius,
    square,
};
pub use surface::{Exclude, Region, MAX_EXCLUDE};
pub use sweep::{sweep, Profile, SweepPoint};
pub use weather::{weather, WeatherSpec};

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

#[cfg(test)]
#[path = "../../tests/stone.rs"]
mod stone_tests;
