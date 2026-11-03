//! Joint gaps behind MCP `joints`: the solid cells of a region are split
//! into stones (columns, running bond courses, or a crack network), and the
//! cells on the boundary between two stones are removed down to a depth
//! under the surface, with an optional dirt slot at the bottom of each gap.

use std::collections::HashMap;

use super::debris::hash01;
use super::surface::{allowed, check_slot, depths_from, noise3, surface_normal, Exclude, Region};
use crate::state::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointPattern {
    /// Vertical columns of random widths, seen from above as cells of a
    /// jittered grid, with a bed joint now and then.
    Columns,
    /// Horizontal courses of blocks whose vertical joints shift from one
    /// course to the next (running bond).
    Courses,
    /// An irregular network of cracks between jagged pieces.
    Cracks,
}

impl JointPattern {
    pub const NAMES: &'static str = "columns, courses, or cracks";

    pub fn parse(text: &str) -> Option<JointPattern> {
        match text {
            "columns" => Some(JointPattern::Columns),
            "courses" => Some(JointPattern::Courses),
            "cracks" => Some(JointPattern::Cracks),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            JointPattern::Columns => "columns",
            JointPattern::Courses => "courses",
            JointPattern::Cracks => "cracks",
        }
    }

    /// The `spacing` and `course` defaults of this pattern.
    pub fn defaults(self) -> ((f64, f64), (i32, i32)) {
        match self {
            JointPattern::Columns => ((4.0, 9.0), (10, 24)),
            JointPattern::Courses => ((6.0, 11.0), (3, 5)),
            JointPattern::Cracks => ((6.0, 11.0), (0, 0)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JointSpec {
    pub region: Region,
    pub pattern: JointPattern,
    /// Stone width range in cells: column width, block length, or crack
    /// piece size. 2..=128.
    pub spacing: (f64, f64),
    /// Course height range in cells (courses), or the spacing of the bed
    /// joints across a column (columns). 1..=128. Cracks ignore it.
    pub course: (i32, i32),
    /// Gap width in cells, 1..=3.
    pub width: i32,
    /// Gap depth under the surface in cells, 1..=16.
    pub depth: u32,
    /// Slot for the solid cell at the bottom of each gap. `None` keeps it.
    pub dirt: Option<u8>,
    /// Cut only cells of these slots. Empty cuts every slot.
    pub only: Vec<u8>,
    /// Cracks: the share of piece boundaries that crack, 0..=1.
    pub density: f64,
    /// Boxes left alone: no gap or dirt inside them.
    pub exclude: Exclude,
    pub seed: u64,
}

#[derive(Debug, PartialEq)]
pub struct Jointed {
    pub writes: Vec<(i32, i32, i32, i32)>,
    pub removed: usize,
    pub dirtied: usize,
}

pub fn joints(cells: &HashMap<Cell, u8>, spec: &JointSpec) -> Result<Jointed, String> {
    let (lo, hi) = spec.spacing;
    if !(2.0..=128.0).contains(&lo) || !(lo..=128.0).contains(&hi) {
        return Err(format!(
            "spacing must be [min, max] with 2 <= min <= max <= 128, not [{lo}, {hi}]"
        ));
    }
    if spec.pattern != JointPattern::Cracks {
        let (lo, hi) = spec.course;
        if !(1..=128).contains(&lo) || !(lo..=128).contains(&hi) {
            return Err(format!(
                "course must be [min, max] with 1 <= min <= max <= 128, not [{lo}, {hi}]"
            ));
        }
    }
    if !(1..=3).contains(&spec.width) {
        return Err("width must be 1..=3".into());
    }
    if !(1..=16).contains(&spec.depth) {
        return Err("depth must be 1..=16".into());
    }
    if !(0.0..=1.0).contains(&spec.density) {
        return Err("density must be 0..=1".into());
    }
    if let Some(dirt) = spec.dirt {
        check_slot(dirt, "dirt")?;
    }
    let stones = Stones::new(spec);
    let depth = depths_from(cells, &spec.region, spec.depth + 1);
    // Whether each surface cell lies on a joint. A boundary that runs along
    // the surface would peel a layer off instead of cutting a line, so it
    // only counts where it crosses the surface.
    let mut joint: HashMap<Cell, bool> = HashMap::new();
    let mut on_joint = |surface: Cell| -> bool {
        *joint.entry(surface).or_insert_with(|| {
            let normal = surface_normal(cells, surface);
            stones
                .crossings(surface)
                .iter()
                .any(|across| normal.is_none_or(|n| dot(n, *across).abs() <= 0.7))
        })
    };
    let mut writes = Vec::new();
    let mut removed = 0;
    let mut bottom: Vec<Cell> = Vec::new();
    for cell in spec.region.solid(cells) {
        let slot = cells[&cell];
        let Some(&(d, surface)) = depth.get(&cell) else {
            continue;
        };
        if !allowed(&spec.only, slot) || spec.exclude.contains(cell) || !on_joint(surface) {
            continue;
        }
        // The cut runs straight in from the surface cell, so every gap is
        // `depth` deep with its bottom behind it.
        if d <= spec.depth {
            writes.push((cell.0, cell.1, cell.2, 0));
            removed += 1;
        } else if spec.dirt.is_some_and(|dirt| dirt != slot) {
            bottom.push(cell);
        }
    }
    let dirtied = bottom.len();
    if let Some(dirt) = spec.dirt {
        for n in &bottom {
            writes.push((n.0, n.1, n.2, i32::from(dirt)));
        }
    }
    Ok(Jointed {
        writes,
        removed,
        dirtied,
    })
}

/// Which stone a point belongs to, as a pure function of its position.
struct Stones {
    pattern: JointPattern,
    grid: f64,
    jitter: f64,
    width: i32,
    density: f64,
    seed: u64,
    /// Courses: the bottom z of each course, from the region's bottom up.
    courses: Vec<i32>,
    course: (i32, i32),
    base: i32,
}

impl Stones {
    fn new(spec: &JointSpec) -> Stones {
        let (lo, hi) = spec.spacing;
        let grid = (lo + hi) / 2.0;
        let mut courses = Vec::new();
        if spec.pattern == JointPattern::Courses {
            let mut z = spec.region.min.2;
            let mut k = 0u64;
            while z <= spec.region.max.2 + spec.course.1 {
                courses.push(z);
                z += pick(spec.course, hash01(k as i64, 7, 0, spec.seed));
                k += 1;
            }
        }
        Stones {
            pattern: spec.pattern,
            grid,
            jitter: ((hi - lo) / grid).clamp(0.2, 1.0),
            width: spec.width,
            density: spec.density,
            seed: spec.seed,
            courses,
            course: spec.course,
            base: spec.region.min.2,
        }
    }

    /// The directions across the boundaries when a neighbour up to `width`
    /// cells away along +x, +y, or +z belongs to another stone, so every
    /// boundary is `width` cells wide.
    fn crossings(&self, c: Cell) -> Vec<[f64; 3]> {
        let (here, at) = self.id(c);
        let mut found = Vec::new();
        for k in 1..=self.width {
            for n in [
                (c.0 + k, c.1, c.2),
                (c.0, c.1 + k, c.2),
                (c.0, c.1, c.2 + k),
            ] {
                let (there, other) = self.id(n);
                if there != here && self.cracks(here, there) {
                    let d = [other[0] - at[0], other[1] - at[1], other[2] - at[2]];
                    let len = dot(d, d).sqrt().max(1.0e-9);
                    found.push([d[0] / len, d[1] / len, d[2] / len]);
                }
            }
        }
        found
    }

    fn cracks(&self, a: u64, b: u64) -> bool {
        if self.pattern != JointPattern::Cracks {
            return true;
        }
        let (a, b) = (a.min(b), a.max(b));
        hash01(a as i64, b as i64, 3, self.seed) < self.density
    }

    /// The stone's id, and for cracks the point of its piece.
    fn id(&self, c: Cell) -> (u64, [f64; 3]) {
        let p = [f64::from(c.0), f64::from(c.1), f64::from(c.2)];
        match self.pattern {
            JointPattern::Columns => {
                let column = self.cell2(p[0], p[1], 1);
                // Bed joints: each column breaks at its own heights.
                let rise = pick(self.course, hash01(column as i64, 1, 0, self.seed));
                let offset = (hash01(column as i64, 2, 0, self.seed) * f64::from(rise)) as i32;
                let band = (c.2 - self.base + offset).div_euclid(rise);
                (
                    column ^ (band as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15),
                    p,
                )
            }
            JointPattern::Courses => {
                let k = self.courses.partition_point(|z| *z <= c.2).max(1) - 1;
                let shift = if k % 2 == 1 { self.grid / 2.0 } else { 0.0 };
                let block = self.cell2(p[0] + shift, p[1] + shift, 10 + k as u64);
                (
                    block ^ (k as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15),
                    p,
                )
            }
            JointPattern::Cracks => {
                // Warp the space so the boundaries between pieces wander.
                let warp = self.grid * 0.18;
                let q = [
                    p[0] + (noise3(p, self.grid, self.seed ^ 11) - 0.5) * 2.0 * warp,
                    p[1] + (noise3(p, self.grid, self.seed ^ 12) - 0.5) * 2.0 * warp,
                    p[2] + (noise3(p, self.grid, self.seed ^ 13) - 0.5) * 2.0 * warp,
                ];
                self.cell3(q)
            }
        }
    }

    /// The nearest point of a jittered grid in x and y.
    fn cell2(&self, x: f64, y: f64, salt: u64) -> u64 {
        let (gx, gy) = (
            (x / self.grid).floor() as i64,
            (y / self.grid).floor() as i64,
        );
        let seed = self.seed ^ salt.wrapping_mul(0xc2b2_ae35);
        let mut best = (f64::MAX, 0u64);
        for i in gx - 1..=gx + 1 {
            for j in gy - 1..=gy + 1 {
                let px = (i as f64 + 0.5 + self.jitter * (hash01(i, j, 1, seed) - 0.5)) * self.grid;
                let py = (j as f64 + 0.5 + self.jitter * (hash01(i, j, 2, seed) - 0.5)) * self.grid;
                let d = (px - x).powi(2) + (py - y).powi(2);
                if d < best.0 {
                    best = (d, key(i, j, 0, salt));
                }
            }
        }
        best.1
    }

    /// The nearest point of a jittered 3D grid.
    fn cell3(&self, p: [f64; 3]) -> (u64, [f64; 3]) {
        let g = p.map(|v| (v / self.grid).floor() as i64);
        let mut best = (f64::MAX, 0u64, [0.0; 3]);
        for i in g[0] - 1..=g[0] + 1 {
            for j in g[1] - 1..=g[1] + 1 {
                for k in g[2] - 1..=g[2] + 1 {
                    let mut d = 0.0;
                    let mut point = [0.0; 3];
                    for (axis, at) in [i, j, k].into_iter().enumerate() {
                        let jit = hash01(i, j, k ^ ((axis as i64 + 1) << 40), self.seed);
                        point[axis] = (at as f64 + 0.5 + 0.9 * (jit - 0.5)) * self.grid;
                        d += (point[axis] - p[axis]).powi(2);
                    }
                    if d < best.0 {
                        best = (d, key(i, j, k, 99), point);
                    }
                }
            }
        }
        (best.1, best.2)
    }
}

fn key(i: i64, j: i64, k: i64, salt: u64) -> u64 {
    (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (j as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ (k as u64).wrapping_mul(0x94d0_49bb_1331_11eb)
        ^ salt.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

/// A whole number in `range` from a 0..1 value.
fn pick(range: (i32, i32), unit: f64) -> i32 {
    let (lo, hi) = range;
    (lo + (unit * f64::from(hi - lo + 1)) as i32).min(hi).max(1)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
