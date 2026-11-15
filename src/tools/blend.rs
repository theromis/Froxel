//! MCP `blend_shapes`: simple shapes as signed distance fields joined by a
//! smooth minimum, so they flow into each other like clay.
//!
//! Each shape is a signed distance field (negative inside, in cells). The
//! shapes are combined in order: a union takes the smooth minimum of the
//! result so far and the shape, a subtract the smooth maximum of the result
//! and the shape's negation, and an intersect the smooth maximum of both.
//! `k` is the width of the fillet in cells: 0 is the hard union, a larger
//! `k` lets two shapes melt into one another over about that many cells. A
//! cell is filled when the blended distance at its centre is at most `grow`.
//! A whole-number point is the centre of that cell, as for `fill_tube`.

use std::collections::HashMap;

use crate::state::Cell;

/// At most this many shapes in one call.
pub const MAX_BLEND_SHAPES: usize = 256;
/// The widest smoothing radius, in cells.
pub const MAX_BLEND_K: f64 = 64.0;
/// At most this many cells evaluated (the shapes' box grown by `k`).
pub const MAX_BLEND_BOX: i64 = 16_000_000;
/// At most this many cells filled.
pub const MAX_BLEND_CELLS: usize = 2_000_000;
/// A cutter's palette paints the cells within this many cells of its
/// surface, plus `k / 8`: the walls and floor of a groove, as a thin line.
pub const CUT_LINE: f64 = 0.6;
/// At most this many points on one capsule chain.
pub const MAX_CAPSULE_POINTS: usize = 64;

/// The geometry of one shape, in cells.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Ellipsoid {
        center: [f64; 3],
        radii: [f64; 3],
    },
    /// A box of full extent `2 * half`, its edges rounded by `round`.
    Box {
        center: [f64; 3],
        half: [f64; 3],
        round: f64,
    },
    /// Round cones through the points, each with its own radius: a capsule,
    /// tapered when the radii differ, or a bent limb of several segments.
    Capsule {
        points: Vec<([f64; 3], f64)>,
    },
    /// A ring about the shape's z axis: `major` to the middle of the tube,
    /// `minor` the tube's radius.
    Torus {
        center: [f64; 3],
        major: f64,
        minor: f64,
    },
}

/// How a shape joins the result of the shapes before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendOp {
    Union,
    Subtract,
    Intersect,
}

impl BlendOp {
    pub const NAMES: &'static str = "union, subtract, or intersect";

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "union" | "add" => Some(Self::Union),
            "subtract" | "sub" => Some(Self::Subtract),
            "intersect" => Some(Self::Intersect),
            _ => None,
        }
    }
}

/// One shape of a blend.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendShape {
    pub form: Form,
    /// Degrees about x, then y, then z, about the shape's centre.
    pub rotate: [f64; 3],
    pub op: BlendOp,
    /// Smoothing radius in cells for joining this shape; 0 is hard.
    pub k: f64,
    /// A union's colour; a subtract's or intersect's paints its cut.
    pub palette: Option<u8>,
}

/// Which union shape colours a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendColors {
    /// The shape whose own surface the cell is deepest inside (smallest
    /// distance): seams fall halfway through the fillets.
    Nearest,
    /// The last shape in the list whose own body holds the cell; cells only
    /// the fillet fills take the nearest. Eyes, buttons, and spots listed
    /// after the body keep their colour where they sink into it.
    Last,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlendSpec {
    pub shapes: Vec<BlendShape>,
    /// Fill where the blended distance is at most this, in cells.
    pub grow: f64,
    pub colors: BlendColors,
    /// Dither the colour boundary between two shapes over this many cells;
    /// 0 keeps it sharp.
    pub seam: f64,
    pub seed: u64,
    /// Evaluate only inside this inclusive box.
    pub bounds: Option<(Cell, Cell)>,
}

/// The filled cells with their slots, and how many cells were evaluated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blended {
    pub cells: Vec<(Cell, u8)>,
    pub evaluated: usize,
}

/// A shape ready to evaluate: the inverse turn and a box around it.
struct Prepared<'a> {
    shape: &'a BlendShape,
    /// World to shape frame, when the shape is turned.
    inverse: Option<[[f64; 3]; 3]>,
    center: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
}

/// The filled cells of a blend of `spec.shapes`.
pub fn blend_shapes(spec: &BlendSpec) -> Result<Blended, String> {
    if spec.shapes.is_empty() {
        return Err("shapes must hold at least one shape".into());
    }
    if spec.shapes.len() > MAX_BLEND_SHAPES {
        return Err(format!("at most {MAX_BLEND_SHAPES} shapes"));
    }
    if spec.shapes[0].op != BlendOp::Union {
        return Err("the first shape must be a union: there is nothing to cut yet".into());
    }
    let prepared: Vec<Prepared> = spec.shapes.iter().map(prepare).collect::<Result<_, _>>()?;
    // The box the result can reach: a smooth union lies within k / 4 of the
    // hard one, a subtract only removes, and an intersect only keeps what is
    // inside its shape.
    let grow = spec.grow.max(0.0);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for item in &prepared {
        match item.shape.op {
            BlendOp::Union => {
                let pad = item.shape.k / 4.0 + grow + 1.0;
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(item.min[axis] - pad);
                    hi[axis] = hi[axis].max(item.max[axis] + pad);
                }
            }
            BlendOp::Intersect => {
                let pad = grow + 1.0;
                for axis in 0..3 {
                    lo[axis] = lo[axis].max(item.min[axis] - pad);
                    hi[axis] = hi[axis].min(item.max[axis] + pad);
                }
            }
            BlendOp::Subtract => {}
        }
    }
    let mut min = [0i64; 3];
    let mut max = [0i64; 3];
    for axis in 0..3 {
        min[axis] = lo[axis].floor() as i64;
        max[axis] = hi[axis].ceil() as i64;
    }
    if let Some((a, b)) = spec.bounds {
        let (a, b) = ([a.0, a.1, a.2], [b.0, b.1, b.2]);
        for axis in 0..3 {
            min[axis] = min[axis].max(i64::from(a[axis].min(b[axis])));
            max[axis] = max[axis].min(i64::from(a[axis].max(b[axis])));
        }
    }
    if (0..3).any(|axis| min[axis] > max[axis]) {
        return Ok(Blended::default());
    }
    let count: i64 = (0..3).map(|axis| max[axis] - min[axis] + 1).product();
    if count > MAX_BLEND_BOX {
        return Err(format!(
            "the shapes' box grown by k is {count} cells, more than {MAX_BLEND_BOX}: pass a box (x0..z1) or blend the parts in several calls"
        ));
    }
    for axis in 0..3 {
        if min[axis] < i64::from(i32::MIN / 2) || max[axis] > i64::from(i32::MAX / 2) {
            return Err("shapes lie too far from the origin".into());
        }
    }
    let mut out = Blended {
        evaluated: count as usize,
        ..Blended::default()
    };
    let mut distances = vec![0.0; prepared.len()];
    for z in min[2]..=max[2] {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                let p = [x as f64, y as f64, z as f64];
                let mut d = 0.0;
                for (index, item) in prepared.iter().enumerate() {
                    let di = item.distance(p);
                    distances[index] = di;
                    let k = item.shape.k;
                    d = if index == 0 {
                        di
                    } else {
                        match item.shape.op {
                            BlendOp::Union => smooth_min(d, di, k),
                            BlendOp::Subtract => smooth_max(d, -di, k),
                            BlendOp::Intersect => smooth_max(d, di, k),
                        }
                    };
                }
                if d > spec.grow + 1.0e-9 {
                    continue;
                }
                let cell = (x as i32, y as i32, z as i32);
                if let Some(slot) = colour(spec, &prepared, &distances, cell) {
                    out.cells.push((cell, slot));
                    if out.cells.len() > MAX_BLEND_CELLS {
                        return Err(format!(
                            "the blend fills more than {MAX_BLEND_CELLS} cells: pass a box or blend the parts in several calls"
                        ));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The slot of a filled cell: the union colour by `spec.colors`, then each
/// cutter with a palette over the cells along its cut.
fn colour(spec: &BlendSpec, prepared: &[Prepared], distances: &[f64], cell: Cell) -> Option<u8> {
    let unions = || {
        prepared
            .iter()
            .zip(distances)
            .filter(|(item, _)| item.shape.op == BlendOp::Union)
            .filter_map(|(item, &d)| item.shape.palette.map(|slot| (d, slot)))
    };
    let best = unions().min_by(|a, b| a.0.total_cmp(&b.0));
    let second = best.and_then(|(_, first)| {
        unions()
            .filter(|&(_, slot)| slot != first)
            .min_by(|a, b| a.0.total_cmp(&b.0))
    });
    let last = unions().filter(|&(d, _)| d <= 1.0e-9).last().map(|u| u.1);
    let mut slot = match spec.colors {
        BlendColors::Last => last.or(best.map(|b| b.1)),
        BlendColors::Nearest => {
            let mut slot = best.map(|b| b.1);
            if let (Some((bd, _)), Some((sd, sslot)), true) = (best, second, spec.seam > 0.0) {
                let share = 0.5 * (1.0 - (sd - bd) / spec.seam);
                if share > 0.0 && unit_hash(cell, spec.seed) < share {
                    slot = Some(sslot);
                }
            }
            slot
        }
    };
    for (item, &d) in prepared.iter().zip(distances) {
        if item.shape.op == BlendOp::Union {
            continue;
        }
        if let Some(cut) = item.shape.palette {
            if d.abs() <= CUT_LINE + item.shape.k / 8.0 {
                slot = Some(cut);
            }
        }
    }
    slot
}

/// A number in `0..1` from a cell and a seed.
fn unit_hash(cell: Cell, seed: u64) -> f64 {
    let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
    for v in [cell.0, cell.1, cell.2] {
        h ^= (v as i64 as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
        h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 29;
    }
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The polynomial smooth minimum: `min` when `k` is 0, otherwise at most
/// `k / 4` below it where `a` and `b` are within `k` of each other.
pub fn smooth_min(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// The smooth maximum, `-smooth_min(-a, -b, k)`.
pub fn smooth_max(a: f64, b: f64, k: f64) -> f64 {
    -smooth_min(-a, -b, k)
}

fn prepare(shape: &BlendShape) -> Result<Prepared<'_>, String> {
    let finite = |v: f64| v.is_finite();
    if !(0.0..=MAX_BLEND_K).contains(&shape.k) {
        return Err(format!("k must be 0..={MAX_BLEND_K} cells"));
    }
    if !shape.rotate.iter().copied().all(finite) {
        return Err("rotate must be finite degrees".into());
    }
    let turned = shape.rotate.iter().any(|a| *a != 0.0);
    let matrix = rotation(shape.rotate);
    let positive = |v: f64, what: &str| -> Result<(), String> {
        if v.is_finite() && v > 0.0 && v <= 4096.0 {
            Ok(())
        } else {
            Err(format!("{what} must be above 0 and at most 4096 cells"))
        }
    };
    let point_ok = |p: &[f64; 3], what: &str| -> Result<(), String> {
        if p.iter().all(|v| v.is_finite() && v.abs() < 1.0e8) {
            Ok(())
        } else {
            Err(format!("{what} must be finite"))
        }
    };
    // The half extent of the shape's box in its own frame, about `center`.
    let (center, half): ([f64; 3], [f64; 3]) = match &shape.form {
        Form::Sphere { center, radius } => {
            point_ok(center, "center")?;
            positive(*radius, "radius")?;
            (*center, [*radius; 3])
        }
        Form::Ellipsoid { center, radii } => {
            point_ok(center, "center")?;
            for r in radii {
                positive(*r, "radii")?;
            }
            (*center, *radii)
        }
        Form::Box {
            center,
            half,
            round,
        } => {
            point_ok(center, "center")?;
            for h in half {
                positive(*h, "size")?;
            }
            if !round.is_finite() || *round < 0.0 {
                return Err("round must be at least 0".into());
            }
            (*center, *half)
        }
        Form::Torus {
            center,
            major,
            minor,
        } => {
            point_ok(center, "center")?;
            positive(*major, "radius")?;
            positive(*minor, "thickness")?;
            let across = major + minor;
            (*center, [across, across, *minor])
        }
        Form::Capsule { points } => {
            if !(1..=MAX_CAPSULE_POINTS).contains(&points.len()) {
                return Err(format!("a capsule has 1..={MAX_CAPSULE_POINTS} points"));
            }
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for (p, r) in points {
                point_ok(p, "points")?;
                positive(*r, "radius")?;
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(p[axis] - r);
                    hi[axis] = hi[axis].max(p[axis] + r);
                }
            }
            let center = [0, 1, 2].map(|a| (lo[a] + hi[a]) / 2.0);
            return Ok(Prepared {
                shape,
                inverse: None,
                center,
                min: lo,
                max: hi,
            });
        }
    };
    // A turned box's extent along each world axis.
    let extent = if turned {
        [0, 1, 2].map(|row| {
            (0..3)
                .map(|col| matrix[row][col].abs() * half[col])
                .sum::<f64>()
        })
    } else {
        half
    };
    Ok(Prepared {
        shape,
        inverse: turned.then(|| transpose(matrix)),
        center,
        min: [0, 1, 2].map(|a| center[a] - extent[a]),
        max: [0, 1, 2].map(|a| center[a] + extent[a]),
    })
}

impl Prepared<'_> {
    /// The signed distance of the shape at `p`, in cells.
    fn distance(&self, p: [f64; 3]) -> f64 {
        let mut q = [0, 1, 2].map(|a| p[a] - self.center[a]);
        if let Some(m) = &self.inverse {
            q = mul(m, q);
        }
        match &self.shape.form {
            Form::Sphere { radius, .. } => length(q) - radius,
            Form::Ellipsoid { radii, .. } => ellipsoid(q, *radii),
            Form::Box { half, round, .. } => rounded_box(q, *half, *round),
            Form::Torus { major, minor, .. } => {
                let ring = (q[0] * q[0] + q[1] * q[1]).sqrt() - major;
                (ring * ring + q[2] * q[2]).sqrt() - minor
            }
            Form::Capsule { points } => {
                if points.len() == 1 {
                    return length(sub(p, points[0].0)) - points[0].1;
                }
                points
                    .windows(2)
                    .map(|w| round_cone(p, w[0].0, w[1].0, w[0].1, w[1].1))
                    .fold(f64::INFINITY, f64::min)
            }
        }
    }
}

/// An ellipsoid's distance, the bound of Inigo Quilez: exact on the axes
/// and close elsewhere for the radii a model uses.
fn ellipsoid(p: [f64; 3], r: [f64; 3]) -> f64 {
    let k0 = length([p[0] / r[0], p[1] / r[1], p[2] / r[2]]);
    let k1 = length([
        p[0] / (r[0] * r[0]),
        p[1] / (r[1] * r[1]),
        p[2] / (r[2] * r[2]),
    ]);
    if k1 < 1.0e-12 {
        return -r[0].min(r[1]).min(r[2]);
    }
    k0 * (k0 - 1.0) / k1
}

fn rounded_box(p: [f64; 3], half: [f64; 3], round: f64) -> f64 {
    let round = round.min(half[0]).min(half[1]).min(half[2]);
    let q = [0, 1, 2].map(|a| p[a].abs() - (half[a] - round));
    let outside = length(q.map(|v| v.max(0.0)));
    let inside = q[0].max(q[1]).max(q[2]).min(0.0);
    outside + inside - round
}

/// The round cone between spheres `r1` at `a` and `r2` at `b`, after Inigo
/// Quilez. When one sphere holds the other it is that sphere.
fn round_cone(p: [f64; 3], a: [f64; 3], b: [f64; 3], r1: f64, r2: f64) -> f64 {
    let ba = sub(b, a);
    let l2 = dot(ba, ba);
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    if a2 <= 1.0e-9 {
        return if r1 >= r2 {
            length(sub(p, a)) - r1
        } else {
            length(sub(p, b)) - r2
        };
    }
    let il2 = 1.0 / l2;
    let pa = sub(p, a);
    let y = dot(pa, ba);
    let z = y - l2;
    let side = [0, 1, 2].map(|i| pa[i] * l2 - ba[i] * y);
    let x2 = dot(side, side);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - r2;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - r1;
    }
    ((x2 * a2 * il2).sqrt() + y * rr) * il2 - r1
}

/// The turn about x, then y, then z, by degrees.
fn rotation(degrees: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    matmul(&rz, &matmul(&ry, &rx))
}

fn matmul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for (row, line) in out.iter_mut().enumerate() {
        for (col, value) in line.iter_mut().enumerate() {
            *value = (0..3).map(|i| a[row][i] * b[i][col]).sum();
        }
    }
    out
}

fn transpose(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|row| [0, 1, 2].map(|col| m[col][row]))
}

fn mul(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| (0..3).map(|i| m[row][i] * v[i]).sum())
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The cells of `blended` kept by mode against `cells`, as writes.
pub fn blend_writes(
    cells: &HashMap<Cell, u8>,
    blended: &Blended,
    empty_only: bool,
    paint_only: bool,
) -> Vec<(i32, i32, i32, i32)> {
    blended
        .cells
        .iter()
        .filter(|(cell, _)| {
            let solid = cells.contains_key(cell);
            !(empty_only && solid) && !(paint_only && !solid)
        })
        .map(|&((x, y, z), slot)| (x, y, z, i32::from(slot)))
        .collect()
}

#[cfg(test)]
#[path = "../../tests/blend.rs"]
mod tests;
