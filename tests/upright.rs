use super::*;
use crate::tools::orient::{from_euler_xyz, turned};

/// A creature-like cloud: a flat dome body 30 long (along +X), 20 wide,
/// 8 tall, wider at the bottom, and a taller head bump at the +X end.
fn creature() -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for i in 0..40 {
        for j in 0..30 {
            for k in 0..10 {
                let (u, v, w) = (
                    -1.0 + 2.0 * f64::from(i) / 39.0,
                    -1.0 + 2.0 * f64::from(j) / 29.0,
                    f64::from(k) / 9.0,
                );
                // A dome: the cross-section shrinks with height.
                if u * u + v * v <= (1.0 - w * w).max(0.0) {
                    out.push([15.0 * u, 10.0 * v, 8.0 * w * w]);
                }
            }
        }
    }
    for i in 0..10 {
        for k in 0..12 {
            out.push([14.0 + f64::from(i) * 0.4, 0.0, f64::from(k)]);
        }
    }
    out
}

fn eigen_ok(m: [[f64; 3]; 3]) {
    let (values, vectors) = eigen_symmetric(m);
    for col in 0..3 {
        let v = [vectors[0][col], vectors[1][col], vectors[2][col]];
        for row in 0..3 {
            let mv: f64 = (0..3).map(|k| m[row][k] * v[k]).sum();
            assert!((mv - values[col] * v[row]).abs() < 1e-9, "{m:?}");
        }
    }
    assert!(values[0] <= values[1] && values[1] <= values[2]);
}

#[test]
fn jacobi_finds_eigenpairs() {
    eigen_ok([[4.0, 1.0, 2.0], [1.0, 3.0, 0.5], [2.0, 0.5, 6.0]]);
    eigen_ok([[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]]);
    eigen_ok([[2.0, -1.0, 0.0], [-1.0, 2.0, -1.0], [0.0, -1.0, 2.0]]);
}

#[test]
fn a_tilted_creature_stands_up_with_its_head_to_minus_y() {
    let tilt = from_euler_xyz([35.0, -20.0, 70.0]);
    let points: Vec<[f64; 3]> = creature().iter().map(|p| turned(&tilt, *p)).collect();
    let found = upright(&points, 180.0).unwrap();
    // The body's up (+Z before the tilt) ends at +Z.
    let up = turned(&found.turn, turned(&tilt, [0.0, 0.0, 1.0]));
    assert!(up[2] > 0.98, "{up:?}");
    // The head end (+X before the tilt) ends at -Y.
    let head = turned(&found.turn, turned(&tilt, [1.0, 0.0, 0.0]));
    assert!(head[1] < -0.95, "{head:?}");
    assert!(found.skew.abs() > 0.0);
}

#[test]
fn an_upright_creature_only_turns_about_up() {
    let found = upright(&creature(), 180.0).unwrap();
    let up = turned(&found.turn, [0.0, 0.0, 1.0]);
    assert!(up[2] > 0.99, "{up:?}");
    let head = turned(&found.turn, [1.0, 0.0, 0.0]);
    assert!(head[1] < -0.99, "{head:?}");
}

#[test]
fn a_round_cloud_keeps_its_up_and_yaw() {
    // A ball: no thinnest or longest axis.
    let mut points = Vec::new();
    for i in -6i32..=6 {
        for j in -6i32..=6 {
            for k in -6i32..=6 {
                if i * i + j * j + k * k <= 36 {
                    points.push([f64::from(i), f64::from(j), f64::from(k)]);
                }
            }
        }
    }
    let found = upright(&points, 180.0).unwrap();
    assert!(found.kept_up && found.kept_yaw);
    let x = turned(&found.turn, [1.0, 0.0, 0.0]);
    assert!(x[0] > 0.999, "{x:?}");
}

#[test]
fn max_tilt_keeps_a_fish_on_its_belly() {
    // A fish: long along X, tall along Z, thin along Y. Its thinnest axis
    // is Y, flat; maxTilt 40 keeps +Z up.
    let mut points = Vec::new();
    for i in -20i32..=20 {
        for j in -3i32..=3 {
            for k in -8i32..=8 {
                let (u, v, w) = (f64::from(i) / 20.0, f64::from(j) / 3.0, f64::from(k) / 8.0);
                if u * u + v * v + w * w <= 1.0 {
                    points.push([f64::from(i), f64::from(j), f64::from(k)]);
                }
            }
        }
    }
    let kept = upright(&points, 40.0).unwrap();
    assert!(kept.kept_up);
    let up = turned(&kept.turn, [0.0, 0.0, 1.0]);
    assert!(up[2] > 0.999, "{up:?}");
    let free = upright(&points, 180.0).unwrap();
    assert!(!free.kept_up);
}

#[test]
fn the_tilted_slug_example_stands_level_with_its_head_to_minus_y() {
    use crate::state::Session;
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/tilted_slug.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.objects[0].cells;
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    for c in cells.keys() {
        for (a, v) in [c.0, c.1, c.2].into_iter().enumerate() {
            lo[a] = lo[a].min(v);
            hi[a] = hi[a].max(v);
        }
    }
    let span: Vec<i32> = (0..3).map(|a| hi[a] - lo[a] + 1).collect();
    // Long along y, low along z.
    assert!(span[1] > span[0] && span[0] > span[2], "{span:?}");
    // The dark head cells sit at the -Y end.
    let dark = |s: u8| {
        let c = session.palette[s as usize];
        u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]) < 300
    };
    let (mut sum, mut n) = (0i64, 0i64);
    for (c, s) in cells {
        if dark(*s) {
            sum += i64::from(c.1);
            n += 1;
        }
    }
    assert!(n > 0);
    let mid = i64::from(lo[1] + hi[1]) / 2;
    assert!(
        sum / n < mid - 4,
        "head at y {} of {}..{}",
        sum / n,
        lo[1],
        hi[1]
    );
    // clean left a few colours.
    let mut slots: Vec<u8> = cells.values().copied().collect();
    slots.sort_unstable();
    slots.dedup();
    assert!(slots.len() <= 5, "{slots:?}");
}
