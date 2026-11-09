//! `import_mesh` `orient: "upright"`: stand a tilted image-to-3D mesh level
//! with its head toward −Y, from the shape alone.
//!
//! Up is the thinnest principal axis of the vertices (a creature is wider
//! and longer than it is tall), signed so the form narrows upward: the
//! third moment along it is positive, as for a dome. Then, level, the long
//! horizontal principal axis is the body, and the end whose cross-section
//! stands taller (the vertices beyond the 92nd percentile along it against
//! those below the 8th) is the head, turned to −Y.

use super::orient::Turn;

/// The turn found and what it was found from.
#[derive(Clone, Debug, PartialEq)]
pub struct Upright {
    /// Applied to the points, stands them up with the head toward −Y.
    pub turn: Turn,
    /// The up axis in the points' own frame.
    pub up: [f64; 3],
    /// The principal spreads (eigenvalues, smallest first).
    pub spread: [f64; 3],
    /// The third moment along `up`, before the sign was chosen.
    pub skew: f64,
    /// Mean height of the head end over the tail end, after standing up.
    pub head_rise: f64,
    /// The thinnest axis was too close to the next to trust (their spreads
    /// within [`AMBIGUOUS`]): the points' own +Z stayed up.
    pub kept_up: bool,
    /// The two level axes were too close to tell the long one: no yaw.
    pub kept_yaw: bool,
}

/// Two principal spreads (standard deviations) whose ratio is above this
/// do not pick an axis: a round toad has no thinnest or longest axis.
pub const AMBIGUOUS: f64 = 0.92;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn apply(m: &Turn, p: [f64; 3]) -> [f64; 3] {
    [dot(m[0], p), dot(m[1], p), dot(m[2], p)]
}

fn mul(a: &Turn, b: &Turn) -> Turn {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Eigenvalues (ascending) and unit eigenvectors (as columns, matching) of
/// a symmetric 3 x 3 matrix, by Jacobi rotations.
pub fn eigen_symmetric(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-15 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-18 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let mut next = a;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                next[k][p] = c * akp - s * akq;
                next[k][q] = s * akp + c * akq;
            }
            let b = next;
            for k in 0..3 {
                let (bpk, bqk) = (b[p][k], b[q][k]);
                next[p][k] = c * bpk - s * bqk;
                next[q][k] = s * bpk + c * bqk;
            }
            a = next;
            for row in v.iter_mut() {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|i, j| a[*i][*i].total_cmp(&a[*j][*j]));
    let values = order.map(|i| a[i][i]);
    let vectors = [
        order.map(|i| v[0][i]),
        order.map(|i| v[1][i]),
        order.map(|i| v[2][i]),
    ];
    (values, vectors)
}

/// The smallest rotation taking unit `from` to unit `to`.
fn rotation_between(from: [f64; 3], to: [f64; 3]) -> Turn {
    let c = dot(from, to);
    let axis = cross(from, to);
    let s = dot(axis, axis).sqrt();
    if s < 1e-12 {
        if c > 0.0 {
            return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        }
        // Half a turn about any axis square to `from`.
        let pick = if from[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let k = cross(from, pick);
        let n = dot(k, k).sqrt();
        let k = k.map(|x| x / n);
        return std::array::from_fn(|i| {
            std::array::from_fn(|j| 2.0 * k[i] * k[j] - if i == j { 1.0 } else { 0.0 })
        });
    }
    let k = axis.map(|x| x / s);
    let t = 1.0 - c;
    [
        [
            t * k[0] * k[0] + c,
            t * k[0] * k[1] - s * k[2],
            t * k[0] * k[2] + s * k[1],
        ],
        [
            t * k[0] * k[1] + s * k[2],
            t * k[1] * k[1] + c,
            t * k[1] * k[2] - s * k[0],
        ],
        [
            t * k[0] * k[2] - s * k[1],
            t * k[1] * k[2] + s * k[0],
            t * k[2] * k[2] + c,
        ],
    ]
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// The upright turn of `points` (Z up).
///
/// `max_tilt` (degrees) bounds how far up may move from the points' own +Z:
/// a fish is thinner side to side than top to bottom, so its thinnest axis
/// lies flat, and an up further off than this keeps +Z instead. When the
/// third moment is too small to say which way the form narrows (under 0.1
/// of the spread cubed), up takes the sign nearer +Z.
pub fn upright(points: &[[f64; 3]], max_tilt: f64) -> Result<Upright, String> {
    if !(0.0..=180.0).contains(&max_tilt) {
        return Err("maxTilt must be 0..180 degrees".into());
    }
    if points.len() < 4 {
        return Err("upright needs a mesh of at least 4 vertices".into());
    }
    let n = points.len() as f64;
    let mut c = [0.0; 3];
    for p in points {
        for a in 0..3 {
            c[a] += p[a] / n;
        }
    }
    let mut cov = [[0.0; 3]; 3];
    for p in points {
        let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    let (spread, vectors) = eigen_symmetric(cov);
    if !(spread[2] > 0.0) {
        return Err("upright needs a mesh with extent".into());
    }
    let ratio = |a: f64, b: f64| (a.max(0.0) / b.max(1e-30)).sqrt();
    let mut kept_up = ratio(spread[0], spread[1]) > AMBIGUOUS;
    let mut up = if kept_up {
        [0.0, 0.0, 1.0]
    } else {
        [vectors[0][0], vectors[1][0], vectors[2][0]]
    };
    let skew: f64 = points
        .iter()
        .map(|p| dot([p[0] - c[0], p[1] - c[1], p[2] - c[2]], up).powi(3))
        .sum::<f64>()
        / n;
    if !kept_up {
        let sure = skew.abs() >= 0.1 * spread[0].max(0.0).powf(1.5);
        if (sure && skew < 0.0) || (!sure && up[2] < 0.0) {
            up = up.map(|x| -x);
        }
        if up[2].clamp(-1.0, 1.0).acos().to_degrees() > max_tilt + 1e-9 {
            kept_up = true;
            up = [0.0, 0.0, 1.0];
        }
    }
    let stand = rotation_between(up, [0.0, 0.0, 1.0]);
    let level: Vec<[f64; 3]> = points
        .iter()
        .map(|p| apply(&stand, [p[0] - c[0], p[1] - c[1], p[2] - c[2]]))
        .collect();
    // The long horizontal axis.
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in &level {
        sxx += p[0] * p[0];
        sxy += p[0] * p[1];
        syy += p[1] * p[1];
    }
    let (w2, v2) = eigen_symmetric([[sxx, sxy, 0.0], [sxy, syy, 0.0], [0.0, 0.0, -1.0]]);
    let kept_yaw = ratio(w2[1], w2[2]) > AMBIGUOUS;
    let axis = [v2[0][2], v2[1][2]];
    let t: Vec<f64> = level
        .iter()
        .map(|p| p[0] * axis[0] + p[1] * axis[1])
        .collect();
    let mut sorted = t.clone();
    sorted.sort_by(f64::total_cmp);
    let (lo, hi) = (quantile(&sorted, 0.08), quantile(&sorted, 0.92));
    let mean = |pick: &dyn Fn(f64) -> bool| -> f64 {
        let (sum, count) = level
            .iter()
            .zip(&t)
            .filter(|(_, t)| pick(**t))
            .fold((0.0, 0usize), |(s, n), (p, _)| (s + p[2], n + 1));
        if count == 0 {
            0.0
        } else {
            sum / count as f64
        }
    };
    let (front, back) = (mean(&|t| t > hi), mean(&|t| t < lo));
    let head = if front > back {
        axis
    } else {
        [-axis[0], -axis[1]]
    };
    // Turn the head to −Y.
    let angle = if kept_yaw {
        0.0
    } else {
        (-1.0f64).atan2(0.0) - head[1].atan2(head[0])
    };
    let (s, co) = angle.sin_cos();
    let yaw = [[co, -s, 0.0], [s, co, 0.0], [0.0, 0.0, 1.0]];
    Ok(Upright {
        turn: mul(&yaw, &stand),
        up,
        spread,
        skew,
        head_rise: (front - back).abs(),
        kept_up,
        kept_yaw,
    })
}

#[cfg(test)]
#[path = "../../tests/upright.rs"]
mod tests;
