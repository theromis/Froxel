use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::{Cell, Session};

fn spec(kind: RadialKind, count: u32) -> RadialSpec {
    RadialSpec {
        kind,
        axis: 2,
        center: None,
        count,
        phase: 0.0,
        width: None,
        from: None,
        to: None,
        bounds: [None; 6],
        ends: RecessEnds::Round,
        fade: None,
        profile: RecessProfile::U,
        depth: 1.0,
        rib_width: 2.0,
        rib_height: 0,
        rib_slot: None,
        only: Vec::new(),
        exclude: Exclude::default(),
        jitter: 0.0,
        seed: 0,
    }
}

/// A square shaft of `side` cells, `height` high, of slot 2.
fn shaft(side: i32, height: i32) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in 0..height {
        for y in 0..side {
            for x in 0..side {
                cells.insert((x, y, z), 2);
            }
        }
    }
    cells
}

/// The top half of a sphere of `radius` on z 0, of slot 2.
fn dome(radius: i32) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    let r2 = f64::from(radius) * f64::from(radius);
    for z in 0..=radius {
        for y in -radius..=radius {
            for x in -radius..=radius {
                let d2 = f64::from(x * x + y * y + z * z);
                if d2 <= r2 {
                    cells.insert((x, y, z), 2);
                }
            }
        }
    }
    cells
}

#[test]
fn flutes_alternate_round_a_square_shaft() {
    // Side 8: 28 surface cells round, so 14 flutes alternate flute, ridge.
    let cells = shaft(8, 40);
    let mut s = spec(RadialKind::Flutes, 14);
    s.from = Some(4);
    s.to = Some(35);
    let done = radial_relief(&cells, &s).unwrap();
    assert_eq!(done.recesses, 14);
    assert!(done.warning.is_none());
    let cut: std::collections::HashSet<Cell> = done.removed.iter().copied().collect();
    // Only the outer layer is cut, and only inside the range.
    for c in &cut {
        let edge = [c.0, 7 - c.0, c.1, 7 - c.1].into_iter().min().unwrap();
        assert_eq!(edge, 0, "{c:?} cut below the surface");
        assert!((4..=35).contains(&c.2), "{c:?} cut outside the range");
    }
    // Mid height, every face alternates: four of its eight cells cut.
    for face in 0..4 {
        let row: Vec<bool> = (0..8)
            .map(|i| {
                let c = match face {
                    0 => (i, 0, 20),
                    1 => (7, i, 20),
                    2 => (i, 7, 20),
                    _ => (0, i, 20),
                };
                cut.contains(&c)
            })
            .collect();
        assert_eq!(
            row.iter().filter(|c| **c).count(),
            4,
            "face {face}: {row:?}"
        );
        assert!(row.windows(2).all(|w| w[0] != w[1]), "face {face}: {row:?}");
    }
    // Rounded ends: the flutes' first and last cells are not cut.
    assert!(cut.iter().all(|c| c.2 != 4 && c.2 != 35));
    assert_eq!(done, radial_relief(&cells, &s).unwrap());
}

#[test]
fn gores_dish_the_dome_between_proud_ribs() {
    let cells = dome(24);
    let mut s = spec(RadialKind::Gores, 12);
    s.depth = 2.0;
    s.from = Some(0);
    s.rib_slot = Some(3);
    s.rib_height = 1;
    let done = radial_relief(&cells, &s).unwrap();
    assert_eq!(done.recesses, 12);
    assert!(done.removed.len() > 200, "removed {}", done.removed.len());
    assert!(!done.added.is_empty());
    assert!(done.added.iter().all(|(_, slot)| *slot == 3));
    let deep = done.tones.iter().filter(|t| t.2 == Tone::Deep).count();
    let ribs = done.tones.iter().filter(|t| t.2 == Tone::Rib).count();
    assert!(deep > 100 && ribs > 50, "deep {deep}, ribs {ribs}");
    for &(x, y, z) in &done.removed {
        // Never at the foot, never at the crown, never deeper than the depth.
        assert!(z > 0, "({x}, {y}, {z}) cut at the foot");
        assert!(x * x + y * y > 4, "({x}, {y}, {z}) cut at the crown");
        let d = 24.0 - f64::from(x * x + y * y + z * z).sqrt();
        assert!(d < 2.6, "({x}, {y}, {z}) cut {d} deep");
    }
    // A cut lies in the middle of its gore, away from the meridians at
    // every 30 degrees.
    for &(x, y, _) in &done.removed {
        let angle = f64::from(y)
            .atan2(f64::from(x))
            .to_degrees()
            .rem_euclid(30.0);
        let r = f64::from(x * x + y * y).sqrt();
        let off = (angle.min(30.0 - angle)).to_radians() * r;
        assert!(off > 0.4, "({x}, {y}) cut on a rib");
    }
}

#[test]
fn only_exclude_and_the_box_keep_cells_whole() {
    let mut cells = shaft(8, 30);
    for z in 0..15 {
        for y in 0..8 {
            for x in 0..8 {
                cells.insert((x, y, z), 4);
            }
        }
    }
    let mut s = spec(RadialKind::Flutes, 14);
    s.only = vec![2];
    s.exclude = Exclude(vec![Region::spanning((0, 0, 20), (7, 7, 22))]);
    s.bounds[5] = Some(26);
    let done = radial_relief(&cells, &s).unwrap();
    assert!(!done.removed.is_empty());
    for c in done.removed.iter().chain(done.tones.iter().map(|t| &t.0)) {
        assert_eq!(cells[c], 2, "{c:?} of another slot");
        assert!(
            !(20..=22).contains(&c.2) && c.2 <= 26,
            "{c:?} out of bounds"
        );
    }
    // Nothing in the box: a warning, no writes.
    s.bounds = [Some(100), None, None, None, None, None];
    let none = radial_relief(&cells, &s).unwrap();
    assert!(none.removed.is_empty() && none.warning.is_some());
}

#[test]
fn bad_arguments_are_named() {
    let cells = shaft(8, 10);
    let mut s = spec(RadialKind::Flutes, 0);
    assert!(radial_relief(&cells, &s).unwrap_err().contains("count"));
    s.count = 8;
    s.depth = 9.0;
    assert!(radial_relief(&cells, &s).unwrap_err().contains("depth"));
}

fn count(session: &Session, slot: u8) -> usize {
    session
        .active()
        .cells
        .values()
        .filter(|s| **s == slot)
        .count()
}

/// A session whose active object is a round shaft of slot 2 at a world
/// position, radius 6, z 0 to 29.
fn round_shaft() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": -6, "y0": -6, "z0": 0, "x1": 6, "y1": 6, "z1": 29, "palette": 2}),
    )
    .unwrap();
    apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": -6, "y0": -6, "z0": -200, "x1": 6, "y1": 6, "z1": 229, "palette": 0, "mode": "intersect"}),
    )
    .unwrap();
    session
}

#[test]
fn the_mcp_tool_makes_shades_and_counts_in_one_undo_step() {
    let mut session = round_shaft();
    let before = session.active().cells.clone();
    let reply = apply(
        &mut session,
        "voxel.radial_relief",
        &json!({"kind": "flutes", "count": 12, "depth": 1.5, "light": true, "from": 3, "to": 26}),
    )
    .unwrap();
    assert_eq!(reply["recesses"], 12, "{reply}");
    assert!(reply["removed"].as_u64().unwrap() > 50, "{reply}");
    assert!(reply["painted"].as_u64().unwrap() > 50, "{reply}");
    let shades = reply["shades"].as_array().unwrap();
    assert_eq!(shades.len(), 2, "a darker and a lighter shade: {reply}");
    for shade in shades {
        let slot = shade["slot"].as_u64().unwrap() as u8;
        assert!(count(&session, slot) > 0);
    }
    session.undo();
    assert_eq!(session.active().cells, before);
    // A second run reuses the shades it made.
    for _ in 0..2 {
        let again = apply(
            &mut session,
            "voxel.radial_relief",
            &json!({"kind": "flutes", "count": 12, "shade": 9, "ribs": 7}),
        )
        .unwrap();
        assert!(again["shades"].as_array().unwrap().is_empty(), "{again}");
    }
    assert!(count(&session, 9) > 0 && count(&session, 7) > 0);
}

#[test]
fn world_space_moves_the_center_and_range() {
    let mut local = round_shaft();
    let mut world = round_shaft();
    world.objects[0].translation = (10, -4, 7);
    let a = apply(
        &mut local,
        "voxel.radial_relief",
        &json!({"kind": "flutes", "count": 10, "center": [0, 0], "from": 2, "to": 20}),
    )
    .unwrap();
    let b = apply(
        &mut world,
        "voxel.radial_relief",
        &json!({"kind": "flutes", "count": 10, "center": [10, -4], "from": 9, "to": 27, "space": "world"}),
    )
    .unwrap();
    assert_eq!(a["removed"], b["removed"]);
    assert_eq!(local.active().cells, world.active().cells);
}

#[test]
fn the_examples_cut_every_recess() {
    for (source, slots) in [
        (
            include_str!("fixtures/examples/fluted_column.rhai"),
            &[2u8, 3][..],
        ),
        (
            include_str!("fixtures/examples/gored_dome.rhai"),
            &[2u8, 3, 4][..],
        ),
    ] {
        let mut session = Session::new();
        session.replace_empty();
        session.set_script(0, source).unwrap();
        session.regenerate_script(0).unwrap();
        for slot in slots {
            assert!(count(&session, *slot) > 0, "slot {slot}");
        }
        // The floors took a shade the run made.
        assert!(
            session.active().cells.values().any(|s| *s > 200),
            "no shade made"
        );
    }
}
