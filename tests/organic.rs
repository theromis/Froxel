use std::collections::HashMap;

use super::*;
use crate::state::Cell;

/// A solid slab of `slot` from `min` to `max`, inclusive.
fn slab(min: Cell, max: Cell, slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                cells.insert((x, y, z), slot);
            }
        }
    }
    cells
}

fn apply(cells: &HashMap<Cell, u8>, writes: &[(i32, i32, i32, i32)]) -> HashMap<Cell, u8> {
    let mut out = cells.clone();
    for &(x, y, z, slot) in writes {
        if slot == 0 {
            out.remove(&(x, y, z));
        } else {
            out.insert((x, y, z), slot as u8);
        }
    }
    out
}

fn spec(op: DeformOp) -> DeformSpec {
    DeformSpec {
        op,
        region: None,
        axis: 2,
        origin: None,
        length: None,
        center: None,
    }
}

fn ball(c: Cell, r: i32, slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in -r..=r {
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y + z * z <= r * r {
                    cells.insert((c.0 + x, c.1 + y, c.2 + z), slot);
                }
            }
        }
    }
    cells
}

#[test]
fn a_bend_curls_a_column_toward_its_direction_and_keeps_slots() {
    let mut column = slab((-1, -1, 0), (1, 1, 29), 3);
    for z in 15..30 {
        column.insert((0, 0, z), 7);
    }
    let bent = deform(
        &column,
        &spec(DeformOp::Bend {
            angle: 90f64.to_radians(),
            toward: (0, 1.0),
        }),
    )
    .unwrap();
    let out = apply(&column, &bent.writes);
    // The base stays; the top now lies along +x near the bend's radius.
    assert_eq!(out.get(&(0, 0, 0)), Some(&3));
    let top = out.keys().map(|c| c.2).max().unwrap();
    let reach = out.keys().map(|c| c.0).max().unwrap();
    assert!(top < 24 && reach > 14, "top {top}, reach {reach}");
    let count = out.len() as f64;
    assert!((count / column.len() as f64 - 1.0).abs() < 0.25, "{count}");
    assert!(out.values().all(|&s| s == 3 || s == 7));
    assert!(out.values().filter(|&&s| s == 7).count() >= 8);
    // A negative angle bends the other way.
    let back = deform(
        &column,
        &spec(DeformOp::Bend {
            angle: -(90f64.to_radians()),
            toward: (0, 1.0),
        }),
    )
    .unwrap();
    let out = apply(&column, &back.writes);
    assert!(out.keys().map(|c| c.0).min().unwrap() < -14);
}

#[test]
fn taper_narrows_and_twist_turns_a_plank() {
    let column = slab((-4, -4, 0), (4, 4, 19), 2);
    let tapered = deform(
        &column,
        &spec(DeformOp::Taper {
            start: 1.0,
            end: 0.3,
        }),
    )
    .unwrap();
    let out = apply(&column, &tapered.writes);
    let width = |z: i32| out.keys().filter(|c| c.2 == z).map(|c| c.0).max().unwrap();
    assert_eq!(width(0), 4);
    assert!(width(19) <= 2, "{}", width(19));
    assert!(tapered.removed > 0 && tapered.added == 0);

    let plank = slab((-6, 0, 0), (6, 0, 19), 4);
    let mut twist = spec(DeformOp::Twist {
        angle: 90f64.to_radians(),
    });
    twist.center = Some([0.0, 0.0]);
    let twisted = deform(&plank, &twist).unwrap();
    let out = apply(&plank, &twisted.writes);
    let top: Vec<&Cell> = out.keys().filter(|c| c.2 == 19).collect();
    assert!(top.iter().all(|c| c.0.abs() <= 1), "{top:?}");
    assert!(top.iter().any(|c| c.1.abs() >= 5), "{top:?}");
    assert_eq!(twisted.cells, plank.len());
}

#[test]
fn deform_acts_only_in_its_region_and_refuses_an_empty_one() {
    let mut cells = slab((-1, -1, 0), (1, 1, 19), 3);
    cells.extend(slab((20, 0, 0), (22, 2, 4), 5));
    let mut bend = spec(DeformOp::Bend {
        angle: 60f64.to_radians(),
        toward: (1, -1.0),
    });
    bend.region = Some(Region::new((-2, -2, 0), (2, 2, 19), "deform").unwrap());
    let done = deform(&cells, &bend).unwrap();
    assert!(done.writes.iter().all(|w| w.0 < 20));
    bend.region = Some(Region::new((40, 40, 40), (44, 44, 44), "deform").unwrap());
    assert!(deform(&cells, &bend).unwrap_err().contains("no cells"));
    bend.axis = 1;
    bend.region = None;
    assert!(deform(&cells, &bend).unwrap_err().contains("across"));
}

#[test]
fn displace_bulges_dents_and_is_stable_for_a_seed() {
    let sphere = ball((0, 0, 0), 10, 6);
    let noise = |bias: f64, seed: u64| Displace {
        amplitude: 3.0,
        scale: 6.0,
        octaves: 2,
        seed,
        bias,
    };
    let out = deform(&sphere, &spec(DeformOp::Displace(noise(1.0, 1)))).unwrap();
    assert!(
        out.added > 50 && out.removed == 0,
        "{} {}",
        out.added,
        out.removed
    );
    assert!(apply(&sphere, &out.writes).values().all(|&s| s == 6));
    let dents = deform(&sphere, &spec(DeformOp::Displace(noise(-1.0, 1)))).unwrap();
    assert!(dents.removed > 50 && dents.added == 0);
    let both = deform(&sphere, &spec(DeformOp::Displace(noise(0.0, 1)))).unwrap();
    assert!(both.added > 0 && both.removed > 0);
    assert_eq!(
        both,
        deform(&sphere, &spec(DeformOp::Displace(noise(0.0, 1)))).unwrap()
    );
    assert_ne!(
        both.writes,
        deform(&sphere, &spec(DeformOp::Displace(noise(0.0, 2))))
            .unwrap()
            .writes
    );
    // No cell moves farther than the amplitude from the surface.
    let grown = apply(&sphere, &out.writes);
    assert!(grown
        .keys()
        .all(|c| ((c.0 * c.0 + c.1 * c.1 + c.2 * c.2) as f64).sqrt() <= 10.0 + 3.0 + 1.0));
    let mut half = spec(DeformOp::Displace(noise(0.0, 1)));
    half.region = Some(Region::new((0, -14, -14), (14, 14, 14), "deform").unwrap());
    let done = deform(&sphere, &half).unwrap();
    assert!(done.writes.iter().all(|w| w.0 >= 0));
    let mut bad = spec(DeformOp::Displace(noise(0.0, 1)));
    bad.op = DeformOp::Displace(Displace {
        amplitude: 99.0,
        ..noise(0.0, 1)
    });
    assert!(deform(&sphere, &bad).unwrap_err().contains("amplitude"));
}

fn section(rx: f64, ry: f64) -> Section {
    Section {
        rx,
        ry,
        roll: 0.0,
        flat: 0.0,
        noise: 0.0,
        frequency: 5.0,
    }
}

fn point(x: f64, y: f64, z: f64, s: Section) -> LoftPoint {
    LoftPoint {
        at: [x, y, z],
        section: s,
    }
}

#[test]
fn a_loft_has_an_elliptical_section_that_changes_along_it() {
    let points = [
        point(0.0, 0.0, 0.0, section(6.0, 3.0)),
        point(30.0, 0.0, 0.0, section(2.0, 2.0)),
    ];
    let cells = loft(&points, false, Caps::Flat, 0).unwrap();
    let at = |x: i32| cells.iter().filter(move |c| c.cell.0 == x);
    // Wide and low at the start: rx across (y), ry up (z).
    assert_eq!(at(0).map(|c| c.cell.1).max(), Some(6));
    assert_eq!(at(0).map(|c| c.cell.2).max(), Some(3));
    assert_eq!(at(30).map(|c| c.cell.1).max(), Some(2));
    // Flat caps stop at the end points.
    assert_eq!(cells.iter().map(|c| c.cell.0).min(), Some(0));
    assert_eq!(cells.iter().map(|c| c.cell.0).max(), Some(30));
    let round = loft(&points, false, Caps::Round, 0).unwrap();
    assert!(round.iter().map(|c| c.cell.0).min().unwrap() < -1);
    assert!((loft_length(&points, false) - 30.0).abs() < 1.0e-9);
}

#[test]
fn flat_top_roll_and_noise_shape_the_outline() {
    let mut s = section(5.0, 5.0);
    s.flat = 1.0;
    let points = [point(0.0, 0.0, 0.0, s), point(20.0, 0.0, 0.0, s)];
    let half = loft(&points, false, Caps::Flat, 0).unwrap();
    assert!(half.iter().all(|c| c.cell.2 <= 0));
    assert!(half.iter().any(|c| c.cell.2 == -5));
    s.flat = -0.5;
    let points = [point(0.0, 0.0, 0.0, s), point(20.0, 0.0, 0.0, s)];
    let bottom = loft(&points, false, Caps::Flat, 0).unwrap();
    assert!(bottom.iter().all(|c| f64::from(c.cell.2) >= -2.5));
    let mut rolled = section(6.0, 2.0);
    rolled.roll = 90f64.to_radians();
    let points = [point(0.0, 0.0, 0.0, rolled), point(10.0, 0.0, 0.0, rolled)];
    let turned = loft(&points, false, Caps::Flat, 0).unwrap();
    assert_eq!(turned.iter().map(|c| c.cell.2).max(), Some(6));
    assert_eq!(turned.iter().map(|c| c.cell.1).max(), Some(2));
    let mut knobbly = section(6.0, 6.0);
    knobbly.noise = 0.4;
    knobbly.frequency = 6.0;
    let points = [
        point(0.0, 0.0, 0.0, knobbly),
        point(40.0, 0.0, 0.0, knobbly),
    ];
    let lumpy = loft(&points, false, Caps::Flat, 3).unwrap();
    let plain = loft(
        &[
            point(0.0, 0.0, 0.0, section(6.0, 6.0)),
            point(40.0, 0.0, 0.0, section(6.0, 6.0)),
        ],
        false,
        Caps::Flat,
        3,
    )
    .unwrap();
    let reach = |cells: &[TubeCell]| {
        cells
            .iter()
            .map(|c| f64::from(c.cell.1 * c.cell.1 + c.cell.2 * c.cell.2).sqrt())
            .fold(0.0, f64::max)
    };
    assert!(reach(&lumpy) > 7.0 && reach(&lumpy) <= 6.0 * 1.4 + 0.5);
    assert_ne!(lumpy.len(), plain.len());
    assert_eq!(lumpy, loft(&points, false, Caps::Flat, 3).unwrap());
    assert_ne!(lumpy, loft(&points, false, Caps::Flat, 4).unwrap());
}

#[test]
fn a_curved_loft_has_no_holes_and_bad_input_is_refused() {
    let points = [
        point(0.0, 0.0, 0.0, section(3.0, 3.0)),
        point(15.0, 10.0, 4.0, section(3.0, 3.0)),
        point(30.0, 0.0, 8.0, section(3.0, 3.0)),
    ];
    let cells = loft(&points, true, Caps::Flat, 0).unwrap();
    let set: std::collections::HashSet<Cell> = cells.iter().map(|c| c.cell).collect();
    // Every slice across x is solid, so the bend leaves no gap.
    for x in 0..=30 {
        assert!(set.iter().filter(|c| c.0 == x).count() >= 20, "slice {x}");
    }
    assert!(set.contains(&(15, 10, 4)) && set.contains(&(0, 0, 0)));
    assert!(cells.iter().all(|c| (0.0..=1.0).contains(&c.around)));
    let thin = [
        point(0.0, 0.0, 0.0, section(0.2, 1.0)),
        point(4.0, 0.0, 0.0, section(1.0, 1.0)),
    ];
    assert!(loft(&thin, false, Caps::Flat, 0)
        .unwrap_err()
        .contains("rx"));
    assert!(loft(&points[..1], false, Caps::Flat, 0).is_err());
}

fn dab(at: [f64; 3], radius: f64, strength: f64) -> Dab {
    Dab {
        at,
        radius,
        strength,
    }
}

fn stroke(op: BrushOp, points: Vec<Dab>) -> BrushSpec {
    BrushSpec {
        op,
        points,
        falloff: Falloff::Smooth,
        only: Vec::new(),
        palette: None,
        vector: [0.0; 3],
        spacing: None,
    }
}

#[test]
fn inflate_and_deflate_push_the_surface_near_the_stroke() {
    let ground = slab((-12, -12, -4), (12, 12, 0), 2);
    let path = vec![
        dab([-6.0, 0.0, 0.0], 4.0, 1.0),
        dab([6.0, 0.0, 0.0], 4.0, 1.0),
    ];
    let up = brush(&ground, &stroke(BrushOp::Inflate, path.clone())).unwrap();
    assert!(up.added > 20 && up.removed == 0 && up.dabs >= 7);
    assert!(up
        .writes
        .iter()
        .all(|w| w.2 >= 1 && w.0.abs() <= 10 && w.1.abs() <= 4));
    assert!(up.writes.iter().all(|w| w.3 == 2));
    let down = brush(&ground, &stroke(BrushOp::Deflate, path.clone())).unwrap();
    assert!(down.removed > 20 && down.added == 0);
    assert!(down.writes.iter().all(|w| w.2 <= 0 && w.3 == 0));
    // only keeps other slots out of the brush.
    let mut stone = ground.clone();
    for x in -12..=12 {
        stone.insert((x, 0, 0), 9);
    }
    let mut spec = stroke(BrushOp::Deflate, path);
    spec.only = vec![2];
    let done = brush(&stone, &spec).unwrap();
    assert!(done
        .writes
        .iter()
        .all(|w| (w.0, w.1, w.2) != (0, 0, 0) || w.1 != 0));
    assert!(done.writes.iter().all(|w| stone[&(w.0, w.1, w.2)] == 2));
    assert!(BrushOp::parse("dig") == Some(BrushOp::Deflate));
}

#[test]
fn smooth_melts_a_spike_and_flatten_cuts_a_bump() {
    let mut ground = slab((-10, -10, -3), (10, 10, 0), 2);
    for z in 1..=4 {
        ground.insert((0, 0, z), 2);
    }
    let soft = brush(
        &ground,
        &stroke(BrushOp::Smooth, vec![dab([0.0, 0.0, 2.0], 4.0, 1.0)]),
    )
    .unwrap();
    let out = apply(&ground, &soft.writes);
    assert!(!out.contains_key(&(0, 0, 4)), "the spike's tip is gone");
    let mut bumpy = slab((-10, -10, -3), (10, 10, 0), 2);
    bumpy.extend(ball((0, 0, 0), 3, 5));
    let flat = brush(
        &bumpy,
        &stroke(BrushOp::Flatten, vec![dab([0.0, 0.0, 1.0], 6.0, 1.0)]),
    )
    .unwrap();
    let out = apply(&bumpy, &flat.writes);
    assert!(flat.removed > 0);
    assert!(out
        .keys()
        .filter(|c| c.0.abs() <= 1 && c.1.abs() <= 1)
        .all(|c| c.2 <= 1));
}

#[test]
fn grab_moves_the_cells_under_the_dab_along_its_vector() {
    let mut ground = slab((-10, -10, -3), (10, 10, 0), 2);
    ground.extend(ball((0, 0, 2), 2, 5));
    let mut spec = stroke(BrushOp::Grab, vec![dab([0.0, 0.0, 2.0], 5.0, 1.0)]);
    spec.vector = [0.0, 0.0, 2.0];
    let done = brush(&ground, &spec).unwrap();
    let out = apply(&ground, &done.writes);
    assert_eq!(out.get(&(0, 0, 5)), Some(&5), "the bump rose");
    assert_eq!(out.get(&(0, 0, 6)), None);
    assert!(out.contains_key(&(8, 8, 0)), "far cells stay");
    spec.vector = [0.0, 0.0, 9.0];
    assert!(brush(&ground, &spec).unwrap_err().contains("radius"));
    spec.vector = [0.0; 3];
    assert!(brush(&ground, &spec).unwrap_err().contains("vector"));
}

#[test]
fn scale_enlarges_a_head_and_leaves_the_body_alone() {
    // A body slab and a head ball with a one-cell eye on its front.
    let mut cells = slab((0, 10, 0), (9, 29, 5), 1);
    cells.extend(ball((5, 5, 5), 4, 2));
    cells.insert((5, 1, 5), 3);
    let m = Magnify {
        point: [5.5, 5.5, 5.5],
        factor: [2.0; 3],
        radius: 5.0,
        falloff: 3.0,
    };
    let done = deform(&cells, &spec(DeformOp::Scale(m))).unwrap();
    let after = apply(&cells, &done.writes);
    let head = |c: &HashMap<Cell, u8>| c.values().filter(|s| **s == 2).count();
    assert!(head(&after) as f64 > head(&cells) as f64 * 3.0);
    // The eye grows from one cell to a patch, slots kept.
    let eye = after.values().filter(|s| **s == 3).count();
    assert!((2..=8).contains(&eye), "{eye}");
    // The far end of the body is untouched.
    for y in 26..=29 {
        assert_eq!(after.get(&(0, y, 0)), Some(&1));
    }
    assert!(after.values().all(|s| [1, 2, 3].contains(s)));
}

#[test]
fn scale_refuses_a_fold() {
    let cells = ball((0, 0, 0), 5, 1);
    let m = Magnify {
        point: [0.0; 3],
        factor: [0.3; 3],
        radius: 5.0,
        falloff: 0.5,
    };
    let err = deform(&cells, &spec(DeformOp::Scale(m))).unwrap_err();
    assert!(err.contains("falloff"), "{err}");
    let m = Magnify {
        factor: [5.0; 3],
        ..m
    };
    assert!(deform(&cells, &spec(DeformOp::Scale(m))).is_err());
}

#[test]
fn deform_over_mcp_scales_and_reports_thin_parts() {
    use serde_json::json;
    let mut session = crate::state::Session::new();
    crate::remote::apply(&mut session, "object.clear", &json!({})).unwrap();
    crate::remote::apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 7, "z1": 7, "palette": 1}),
    )
    .unwrap();
    // A fin one cell thick.
    crate::remote::apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 3, "y0": 8, "z0": 2, "x1": 3, "y1": 12, "z1": 6, "palette": 2}),
    )
    .unwrap();
    let reply = crate::remote::apply(
        &mut session,
        "voxel.deform",
        &json!({"op": "scale", "point": [4, 4, 4], "factor": 1.25, "radius": 3, "minFeature": 2}),
    )
    .unwrap();
    assert!(reply["added"].as_u64().unwrap() > 0);
    let thin = &reply["thin"];
    assert_eq!(thin["count"], json!(1));
    assert_eq!(thin["parts"][0]["thickness"], json!(1));
    assert!(thin["parts"][0]["min"][1].as_i64().unwrap() >= 8);
}

#[test]
fn the_big_head_frog_example_grows_its_eyes() {
    let mut session = crate::state::Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/big_head_frog.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let eyes = session.objects[0]
        .cells
        .values()
        .filter(|s| **s == 4)
        .count();
    assert!(eyes >= 4, "{eyes} eye cells");
}
