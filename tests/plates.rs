use serde_json::{json, Value};

use crate::remote::apply;
use crate::state::Session;

/// A session whose active object is the top half of an ellipsoid of slot
/// 2, 34 x 44 cells in plan and 15 high: an oval dome.
fn dome() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": -14, "x1": 33, "y1": 43, "z1": 14, "palette": 2}),
    )
    .unwrap();
    apply(
        &mut session,
        "voxel.carve",
        &json!({"shape": "box", "x0": 0, "y0": 0, "z0": -14, "x1": 33, "y1": 43, "z1": -1}),
    )
    .unwrap();
    session
}

/// Distinct slots per kind: central 7, ring 3, band 4, rim 8, seam 6.
fn kinds() -> Value {
    json!({"central": 7, "ring": 3, "band": 4, "rim": 8, "seam": 6})
}

fn count(session: &Session, slot: u8) -> usize {
    session
        .active()
        .cells
        .values()
        .filter(|s| **s == slot)
        .count()
}

/// The kind rank of a slot (central 0, ring 1, band 2, rim 3), seams none.
fn rank(slot: u8) -> Option<u8> {
    match slot {
        7 => Some(0),
        3 => Some(1),
        4 => Some(2),
        8 => Some(3),
        _ => None,
    }
}

/// Going down the dome from the apex along each of 16 plan directions, the
/// kinds come in order: central, rings, band, rim, never back toward the
/// centre, which a streak down a slope would do. The cells of a narrow
/// sector of the taken surface are sorted by their elevation seen from the
/// dome's middle.
fn rings_in_order(session: &Session) {
    let cells = &session.active().cells;
    let (cx, cy) = (16.5f64, 21.5f64);
    for k in 0..16 {
        let a = f64::from(k) * std::f64::consts::TAU / 16.0;
        let mut found: Vec<(f64, u8)> = Vec::new();
        for (c, slot) in cells {
            let Some(r) = rank(*slot) else {
                continue;
            };
            let (x, y) = ((f64::from(c.0) - cx) / 17.0, (f64::from(c.1) - cy) / 22.0);
            // Within two cells of the ray in plan (a seam may run along
            // it), on its side.
            let dir = [a.cos() * 17.0, a.sin() * 22.0];
            let len = dir[0].hypot(dir[1]);
            let (px, py) = (f64::from(c.0) - cx, f64::from(c.1) - cy);
            let along = (px * dir[0] + py * dir[1]) / len;
            let across = (px * dir[1] - py * dir[0]).abs() / len;
            if across > 2.0 || along < -0.5 {
                continue;
            }
            let elevation = (f64::from(c.2) / 15.0).atan2(x.hypot(y));
            found.push((elevation, r));
        }
        found.sort_by(|p, q| q.0.total_cmp(&p.0));
        let mut seen = Vec::new();
        let mut back = 0;
        let mut last = 0u8;
        for (_, r) in &found {
            if *r < last {
                back += 1;
            }
            last = last.max(*r);
            if !seen.contains(r) {
                seen.push(*r);
            }
        }
        assert_eq!(seen, vec![0, 1, 2, 3], "direction {k}");
        assert!(
            back * 100 <= found.len() * 3,
            "direction {k}: {back} of {} out of order",
            found.len()
        );
    }
}

#[test]
fn an_oval_dome_takes_a_clean_ring_pattern_with_no_streaks() {
    let mut session = dome();
    let reply = apply(
        &mut session,
        "voxel.shell_plates",
        &json!({"palettes": kinds(), "rings": [5, 9], "bandCount": 14}),
    )
    .unwrap();
    // Central, 5 + 9 ring plates, 14 band plates, and the rim.
    assert_eq!(reply["plates"], 30, "{reply}");
    assert_eq!(reply["unreached"], 0);
    let taken = reply["taken"].as_u64().unwrap() as usize;
    let seams = reply["seams"].as_u64().unwrap() as usize;
    assert!(seams * 100 < taken * 45, "{seams} seams of {taken}");
    assert_eq!(count(&session, 6), seams);
    rings_in_order(&session);
    // Nothing below the dome's floor or under it changes.
    assert!(session
        .active()
        .cells
        .iter()
        .all(|(c, s)| c.2 < 0 || *s != 0));
    // One undo step.
    session.undo();
    assert!(session.active().cells.values().all(|s| *s == 2));
}

#[test]
fn a_symmetric_pattern_mirrors_plates_seams_and_slots() {
    for layout in ["rings", "voronoi"] {
        let mut session = dome();
        apply(
            &mut session,
            "voxel.shell_plates",
            &json!({"palettes": {"ring": [3, 4, 5], "central": 7, "rim": 8, "seam": 6}, "layout": layout, "rings": [6, 10], "jitter": 0.5, "seed": 9}),
        )
        .unwrap();
        let cells = &session.active().cells;
        let plane = crate::tools::Mirror { axis: 0, twice: 34 };
        let off = cells
            .iter()
            .filter(|(c, s)| cells.get(&plane.cell(**c)) != Some(s))
            .count();
        assert!(
            off * 100 <= cells.len(),
            "{layout}: {off} cells differ from their mirror"
        );
        for slot in [3, 4, 5, 6, 7, 8] {
            assert!(count(&session, slot) > 0, "{layout} slot {slot}");
        }
    }
}

#[test]
fn voronoi_plates_keep_the_kinds_in_order() {
    let mut session = dome();
    let reply = apply(
        &mut session,
        "voxel.shell_plates",
        &json!({"palettes": kinds(), "layout": "voronoi", "rings": [6, 10], "jitter": 0.4}),
    )
    .unwrap();
    assert!(reply["plates"].as_u64().unwrap() >= 25, "{reply}");
    rings_in_order(&session);
}

#[test]
fn grooves_facing_the_box_and_only_limit_what_changes() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 23, "z1": 23, "palette": 2}),
    )
    .unwrap();
    let before = session.active().cells.len();
    // A groove: seam 0 erases two cells deep; the underside is left alone.
    let reply = apply(
        &mut session,
        "voxel.shell_plates",
        &json!({"palettes": {"ring": 3, "seam": 0}, "rings": [6], "seamDepth": 2, "facing": 60}),
    )
    .unwrap();
    let after = session.active().cells.len();
    assert!(after < before, "{reply}");
    assert!(before - after >= reply["seams"].as_u64().unwrap() as usize);
    let cells = &session.active().cells;
    assert!(
        cells.iter().all(|(c, s)| c.2 >= 8 || *s == 2),
        "the underside changed"
    );
    // zRange and a box keep it to the top cap's +x half.
    let mut session = dome();
    apply(
        &mut session,
        "voxel.shell_plates",
        &json!({"palettes": kinds(), "zRange": [8, 14], "x0": 17, "y0": 0, "z0": 0, "x1": 33, "y1": 43, "z1": 14, "center": [16.5, 21.5]}),
    )
    .unwrap();
    assert!(session
        .active()
        .cells
        .iter()
        .all(|(c, s)| *s == 2 || (c.0 >= 17 && c.2 >= 8)));
    // only skips other slots.
    let mut session = dome();
    apply(&mut session, "voxel.fill", &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 33, "y1": 10, "z1": 14, "palette": 9, "mode": "paint"})).unwrap();
    let nines = count(&session, 9);
    apply(
        &mut session,
        "voxel.shell_plates",
        &json!({"palettes": kinds(), "only": [2]}),
    )
    .unwrap();
    assert_eq!(count(&session, 9), nines);
}

#[test]
fn bad_plate_calls_are_explained() {
    let mut session = dome();
    for (args, text) in [
        (json!({}), "palettes is required"),
        (json!({"palettes": {"seam": 6}}), "needs ring"),
        (json!({"palettes": {"ring": 3}}), "needs seam"),
        (
            json!({"palettes": {"ring": 3, "seam": 6, "belly": 2}}),
            "not `belly`",
        ),
        (json!({"palettes": {"ring": 0, "seam": 6}}), "palettes ring"),
        (
            json!({"palettes": kinds(), "layout": "hex"}),
            "rings or voronoi",
        ),
        (
            json!({"palettes": kinds(), "rings": [3, 0]}),
            "at least 1 plate",
        ),
        (
            json!({"palettes": kinds(), "rings": [1, 1, 1, 1, 1, 1, 1, 1, 1]}),
            "at most 8",
        ),
        (
            json!({"palettes": kinds(), "facing": 0, "zRange": [100, 120]}),
            "no surface cell",
        ),
        (json!({"palettes": kinds(), "x0": 1}), "all of x0"),
    ] {
        let err = apply(&mut session, "voxel.shell_plates", &args).unwrap_err();
        assert!(err.contains(text), "{args}: {err}");
    }
}

#[test]
fn plates_run_from_a_script_a_batch_the_panel_and_world_cells() {
    let mut session = dome();
    session
        .set_script(
            0,
            "fill_ellipsoid(0, 0, -14, 33, 43, 14, 2);\n\
             carve_box(0, 0, -14, 33, 43, -1);\n\
             let r = shell_plates(#{palettes: #{ring: [3, 4], central: 7, seam: 6}, rings: [5, 9], layout: \"voronoi\"});\n\
             if r.plates < 10 || r.seams == 0 || r.unreached != 0 { throw \"plates\"; }",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert!(count(&session, 6) > 0 && count(&session, 7) > 0);

    let mut session = dome();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "position": [100, 50, 0]}),
    )
    .unwrap();
    let reply = apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [{"tool": "shell_plates", "arguments": {"palettes": kinds(), "space": "world", "center": [116.5, 71.5], "zRange": [0, 14]}}]}),
    )
    .unwrap();
    assert!(reply.to_string().contains("plates"), "{reply}");
    assert!(count(&session, 7) > 0);

    let mut session = dome();
    session.active_slot = 5;
    session.edit.plates.rings = "5 9".into();
    session.edit.plates.seam = 6;
    session.plates_from_panel();
    assert!(session.status.starts_with("Painted"), "{}", session.status);
    assert!(count(&session, 5) > 0 && count(&session, 6) > 0);
    session.edit.plates.rings = "5 x".into();
    let err = session.edit.plates.args(1, None).unwrap_err();
    assert!(err.contains("plate count"));
}

#[test]
fn the_plated_dome_example_paints_every_kind() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/plated_dome.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    for slot in [3, 4, 5, 6, 7, 8] {
        assert!(count(&session, slot) > 0, "slot {slot}");
    }
}
