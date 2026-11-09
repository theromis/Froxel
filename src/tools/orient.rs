//! MCP `orient_to_views`: the turn of an object (or of a mesh before it is
//! voxelized) that best matches a views sheet.
//!
//! Each candidate turn is scored by projecting the object's surface cells
//! onto every view's image axes, as `model_view` draws them, and fitting
//! the view's mask over that projection with one scale and offset per view
//! (`silhouette::fit_iou`); the score is the weighted mean IoU. The 24
//! axis-aligned turns (or the 4 upright ones) are tried first, then fine
//! steps about up (yaw), and optionally about X and Y, around the best, one
//! axis at a time, twice over. The chosen turn is applied by
//! resampling: each cell of the turned (and scaled) box takes the slot of
//! the nearest source cell.

use std::collections::HashMap;

use super::silhouette::{Mask, Side};
use crate::state::Cell;

/// A 3 x 3 rotation, applied to column vectors: `m * p`.
pub type Turn = [[f64; 3]; 3];

/// Longest side of a model mask, the default.
pub const DEFAULT_ORIENT_RESOLUTION: u32 = 64;
/// Most cells in the box the resampled object fills.
pub const MAX_ORIENT_BOX: i64 = 16_000_000;

pub const IDENTITY: Turn = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// One view of the sheet: the side it looks from and its silhouette.
#[derive(Clone, Debug, PartialEq)]
pub struct OrientView {
    pub side: Side,
    pub mask: Mask,
    pub weight: f32,
}

/// Which turns are tried before the fine yaw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turns {
    /// All 24 axis-aligned turns.
    All,
    /// The 4 turns about up: the object is already upright.
    Upright,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrientSpec {
    pub views: Vec<OrientView>,
    pub turns: Turns,
    /// Degrees between fine yaw steps about up around the best turn; 0 is
    /// none.
    pub yaw_step: f64,
    /// Fine yaw goes this many degrees each way.
    pub yaw_range: f64,
    /// The axes of the fine steps, of X, Y, and Z (up): `[false, false,
    /// true]` is yaw alone.
    pub fine_axes: [bool; 3],
    /// Longest side of a model mask in pixels.
    pub resolution: u32,
}

/// One scored turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub turn: Turn,
    /// Degrees of the fine turns about X, Y, and Z after the axis-aligned
    /// turn: `turn = Rz * Ry * Rx * axis turn`.
    pub fine: [f64; 3],
    pub score: f32,
    /// IoU per view, in the spec's order.
    pub ious: Vec<f32>,
}

/// What `orient` found.
#[derive(Clone, Debug, PartialEq)]
pub struct Oriented {
    /// The object as it is.
    pub before: Candidate,
    pub best: Candidate,
    /// The axis-aligned turns tried, best first.
    pub ranked: Vec<Candidate>,
}

pub fn mul(a: &Turn, b: &Turn) -> Turn {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// `m * p`.
pub fn turned(m: &Turn, p: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
    ]
}

fn transpose(m: &Turn) -> Turn {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in m.iter().enumerate() {
        for (j, value) in row.iter().enumerate() {
            out[j][i] = *value;
        }
    }
    out
}

/// A turn of `degrees` about up (+Z), counter-clockwise seen from above.
pub fn yaw(degrees: f64) -> Turn {
    about(2, degrees)
}

/// A turn of `degrees` about axis `axis` (0 X, 1 Y, 2 Z), right-handed.
pub fn about(axis: usize, degrees: f64) -> Turn {
    let (s, c) = degrees.to_radians().sin_cos();
    match axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// The 24 axis-aligned turns, the identity first; with `Upright`, the 4
/// about up.
pub fn axis_turns(turns: Turns) -> Vec<Turn> {
    if turns == Turns::Upright {
        return [0.0, 90.0, 180.0, 270.0]
            .iter()
            .map(|d| round_turn(yaw(*d)))
            .collect();
    }
    let mut out = vec![IDENTITY];
    let axes = [0usize, 1, 2];
    for perm in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for signs in 0..8 {
            let mut m = [[0.0; 3]; 3];
            for row in axes {
                let sign = if signs >> row & 1 == 1 { -1.0 } else { 1.0 };
                m[row][perm[row]] = sign;
            }
            if det(&m) > 0.0 && m != IDENTITY {
                out.push(m);
            }
        }
    }
    out
}

fn round_turn(m: Turn) -> Turn {
    m.map(|row| row.map(|v| v.round()))
}

fn det(m: &Turn) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Degrees about X, then Y, then Z (`R = Rz * Ry * Rx`), as `import_mesh`
/// `rotation` takes them.
pub fn euler_xyz(m: &Turn) -> [f64; 3] {
    let sy = (-m[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if sy.abs() < 1.0 - 1e-9 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        ((-m[1][2]).atan2(m[1][1]), 0.0)
    };
    let clean = |r: f64| {
        let d = r.to_degrees();
        let d = (d * 1000.0).round() / 1000.0;
        if d == -180.0 || d == 0.0 {
            d.abs()
        } else {
            d
        }
    };
    [clean(x), clean(y), clean(z)]
}

/// The turn of `euler_xyz` angles.
pub fn from_euler_xyz(degrees: [f64; 3]) -> Turn {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mul(&rz, &mul(&ry, &rx))
}

/// Where the object's +X, +Y, and +Z end up, such as `["-y", "x", "z"]`.
pub fn axis_names(m: &Turn) -> [String; 3] {
    let name = |column: usize| {
        let mut best = (0usize, 0.0f64);
        for (row, values) in m.iter().enumerate() {
            if values[column].abs() > best.1.abs() {
                best = (row, values[column]);
            }
        }
        let axis = ["x", "y", "z"][best.0];
        if best.1 < 0.0 {
            format!("-{axis}")
        } else {
            axis.to_string()
        }
    };
    [name(0), name(1), name(2)]
}

/// The cells whose projections make the silhouettes: those with an empty
/// face neighbour, centred on the cells' mean.
pub fn surface_points(cells: &HashMap<Cell, u8>) -> Vec<[f64; 3]> {
    let n = cells.len().max(1) as f64;
    let mut mean = [0.0; 3];
    for c in cells.keys() {
        mean[0] += f64::from(c.0) / n;
        mean[1] += f64::from(c.1) / n;
        mean[2] += f64::from(c.2) / n;
    }
    let faces = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    let mut out: Vec<[f64; 3]> = cells
        .keys()
        .filter(|c| {
            faces
                .iter()
                .any(|f| !cells.contains_key(&(c.0 + f.0, c.1 + f.1, c.2 + f.2)))
        })
        .map(|c| {
            [
                f64::from(c.0) - mean[0],
                f64::from(c.1) - mean[1],
                f64::from(c.2) - mean[2],
            ]
        })
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The silhouette of `points` turned by `m`, seen from `side`, in a mask
/// at most `resolution` pixels on its longer side.
pub fn project(
    points: &[[f64; 3]],
    m: &Turn,
    side: Side,
    resolution: u32,
) -> (Vec<bool>, u32, u32) {
    let [(ra, rs), (da, ds), _] = side.axes();
    let uv: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            let q = turned(m, *p);
            (q[ra] * f64::from(rs), q[da] * f64::from(ds))
        })
        .collect();
    let (mut u0, mut v0, mut u1, mut v1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (u, v) in &uv {
        u0 = u0.min(*u);
        v0 = v0.min(*v);
        u1 = u1.max(*u);
        v1 = v1.max(*v);
    }
    if uv.is_empty() {
        return (Vec::new(), 0, 0);
    }
    let half = 0.45;
    let (u0, v0) = (u0 - 0.5, v0 - 0.5);
    let extent = (u1 + 0.5 - u0).max(v1 + 0.5 - v0);
    let k = (extent / f64::from(resolution.max(8))).max(1.0);
    let w = ((u1 + 0.5 - u0) / k).ceil().max(1.0) as u32;
    let h = ((v1 + 0.5 - v0) / k).ceil().max(1.0) as u32;
    let mut mask = vec![false; (w * h) as usize];
    for (u, v) in uv {
        let x0 = (((u - half - u0) / k).floor().max(0.0) as u32).min(w - 1);
        let x1 = (((u + half - u0) / k).floor().max(0.0) as u32).min(w - 1);
        let y0 = (((v - half - v0) / k).floor().max(0.0) as u32).min(h - 1);
        let y1 = (((v + half - v0) / k).floor().max(0.0) as u32).min(h - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                mask[(y * w + x) as usize] = true;
            }
        }
    }
    (mask, w, h)
}

/// The score of turn `m`: the weighted mean IoU over the views.
pub fn score(points: &[[f64; 3]], m: &Turn, spec: &OrientSpec) -> (f32, Vec<f32>) {
    let mut ious = Vec::with_capacity(spec.views.len());
    let mut sum = 0.0;
    let mut weights = 0.0;
    for view in &spec.views {
        let (model, w, h) = project(points, m, view.side, spec.resolution);
        let iou = crate::silhouette::fit_iou(&view.mask, &model, w, h);
        sum += iou * view.weight;
        weights += view.weight;
        ious.push(iou);
    }
    (if weights > 0.0 { sum / weights } else { 0.0 }, ious)
}

/// How many of the best axis-aligned turns the fine steps start from.
pub const FINE_STARTS: usize = 6;

/// The best turn near `start` by fine steps about each axis of
/// `spec.fine_axes`, one axis at a time, twice over.
fn fine_steps(
    start: &Candidate,
    spec: &OrientSpec,
    scored: &dyn Fn(Turn, [f64; 3]) -> Candidate,
) -> Candidate {
    let base = start.turn;
    let steps = (spec.yaw_range / spec.yaw_step).floor() as i32;
    let compose = |fine: [f64; 3]| {
        mul(
            &about(2, fine[2]),
            &mul(&about(1, fine[1]), &mul(&about(0, fine[0]), &base)),
        )
    };
    let mut best = start.clone();
    for _pass in 0..2 {
        for axis in [2usize, 0, 1] {
            if !spec.fine_axes[axis] {
                continue;
            }
            let from = best.fine;
            for i in -steps..=steps {
                let mut fine = from;
                fine[axis] = f64::from(i) * spec.yaw_step;
                if fine == from {
                    continue;
                }
                let candidate = scored(compose(fine), fine);
                if candidate.score > best.score + 1e-4 {
                    best = candidate;
                }
            }
        }
        if !spec.fine_axes[0] && !spec.fine_axes[1] {
            break;
        }
    }
    best
}

/// Find the turn of `cells` that best matches the views.
pub fn orient(cells: &HashMap<Cell, u8>, spec: &OrientSpec) -> Result<Oriented, String> {
    if cells.is_empty() {
        return Err("the object is empty".into());
    }
    if spec.views.is_empty() {
        return Err("views must hold at least one view".into());
    }
    if spec.views.iter().any(|view| view.mask.count() == 0) {
        return Err("a view's mask is empty: check its crop and background".into());
    }
    if !(spec.yaw_step >= 0.0 && spec.yaw_step <= 90.0) {
        return Err("yawStep must be 0..=90 degrees".into());
    }
    if !(0.0..=180.0).contains(&spec.yaw_range) {
        return Err("yawRange must be 0..=180 degrees".into());
    }
    if !(8..=512).contains(&spec.resolution) {
        return Err("resolution must be 8..=512".into());
    }
    let points = surface_points(cells);
    let scored = |turn: Turn, fine: [f64; 3]| {
        let (score, ious) = score(&points, &turn, spec);
        Candidate {
            turn,
            fine,
            score,
            ious,
        }
    };
    let before = scored(IDENTITY, [0.0; 3]);
    let mut ranked: Vec<Candidate> = axis_turns(spec.turns)
        .into_iter()
        .map(|turn| scored(turn, [0.0; 3]))
        .collect();
    // Best first; ties keep the earlier (simpler) turn.
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut best = ranked[0].clone();
    if spec.yaw_step > 0.0 && spec.yaw_range > 0.0 && spec.fine_axes.contains(&true) {
        // The fine steps start from each of the best few turns, since two
        // turns the views barely tell apart may end in different places.
        for start in ranked.iter().take(FINE_STARTS) {
            let found = fine_steps(start, spec, &scored);
            if found.score > best.score + 1e-4 {
                best = found;
            }
        }
    }
    Ok(Oriented {
        before,
        best,
        ranked,
    })
}

/// The cells of `cells` turned by `m` and scaled by `scale` about their
/// centre, resampled (each new cell takes the nearest source cell's slot),
/// with the bottom centre of the new cells' box on the old one's.
pub fn resample(
    cells: &HashMap<Cell, u8>,
    m: &Turn,
    scale: f64,
) -> Result<HashMap<Cell, u8>, String> {
    if !(scale > 0.0 && scale.is_finite()) {
        return Err("scale must be a positive number".into());
    }
    let Some((min, max)) = bounds(cells.keys().copied()) else {
        return Ok(HashMap::new());
    };
    // A whole cell, so an axis-aligned turn maps cells one to one.
    let centre = [
        f64::from((min.0 + max.0).div_euclid(2)),
        f64::from((min.1 + max.1).div_euclid(2)),
        f64::from((min.2 + max.2).div_euclid(2)),
    ];
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for corner in 0..8 {
        let p = [
            if corner & 1 == 0 {
                f64::from(min.0) - 0.5
            } else {
                f64::from(max.0) + 0.5
            } - centre[0],
            if corner & 2 == 0 {
                f64::from(min.1) - 0.5
            } else {
                f64::from(max.1) + 0.5
            } - centre[1],
            if corner & 4 == 0 {
                f64::from(min.2) - 0.5
            } else {
                f64::from(max.2) + 0.5
            } - centre[2],
        ];
        let q = turned(m, p);
        for axis in 0..3 {
            lo[axis] = lo[axis].min(q[axis] * scale);
            hi[axis] = hi[axis].max(q[axis] * scale);
        }
    }
    let from: [i64; 3] = std::array::from_fn(|a| lo[a].floor() as i64);
    let to: [i64; 3] = std::array::from_fn(|a| hi[a].ceil() as i64);
    let volume: i64 = (0..3).map(|a| to[a] - from[a] + 1).product();
    if volume > MAX_ORIENT_BOX {
        return Err(format!(
            "the turned box would hold {volume} cells; at most {MAX_ORIENT_BOX}"
        ));
    }
    let back = transpose(m);
    let mut out = HashMap::new();
    for z in from[2]..=to[2] {
        for y in from[1]..=to[1] {
            for x in from[0]..=to[0] {
                // The cell centre in the turned frame, back to the source.
                let q = [x as f64 / scale, y as f64 / scale, z as f64 / scale];
                let p = turned(&back, q);
                let source = (
                    (p[0] + centre[0]).round() as i32,
                    (p[1] + centre[1]).round() as i32,
                    (p[2] + centre[2]).round() as i32,
                );
                if let Some(slot) = cells.get(&source) {
                    out.insert((x as i32, y as i32, z as i32), *slot);
                }
            }
        }
    }
    // Bottom centre on the old bottom centre.
    let Some((nmin, nmax)) = bounds(out.keys().copied()) else {
        return Ok(out);
    };
    let shift = (
        (min.0 + max.0).div_euclid(2) - (nmin.0 + nmax.0).div_euclid(2),
        (min.1 + max.1).div_euclid(2) - (nmin.1 + nmax.1).div_euclid(2),
        min.2 - nmin.2,
    );
    Ok(out
        .into_iter()
        .map(|(c, s)| ((c.0 + shift.0, c.1 + shift.1, c.2 + shift.2), s))
        .collect())
}

pub(crate) fn bounds(cells: impl IntoIterator<Item = Cell>) -> Option<(Cell, Cell)> {
    let mut found: Option<(Cell, Cell)> = None;
    for c in cells {
        found = Some(match found {
            None => (c, c),
            Some((a, b)) => (
                (a.0.min(c.0), a.1.min(c.1), a.2.min(c.2)),
                (b.0.max(c.0), b.1.max(c.1), b.2.max(c.2)),
            ),
        });
    }
    found
}

#[cfg(test)]
#[path = "../../tests/orient.rs"]
mod tests;
