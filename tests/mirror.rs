use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

/// A session whose active object holds a ball of slot 2, 16 cells across
/// (x, y, z 0..15), so its centre plane along x lies at 8.
fn ball() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 15, "y1": 15, "z1": 15, "palette": 2}),
    )
    .unwrap();
    session
}

fn of_slot(session: &Session, slot: u8) -> Vec<Cell> {
    let mut cells: Vec<Cell> = session
        .active()
        .cells
        .iter()
        .filter(|(_, s)| **s == slot)
        .map(|(c, _)| *c)
        .collect();
    cells.sort_unstable();
    cells
}

/// Every cell of `cells` has its mirror across x = 8 in `cells`.
fn symmetric(cells: &[Cell]) -> bool {
    let plane = Mirror { axis: 0, twice: 16 };
    cells.iter().all(|c| cells.contains(&plane.cell(*c)))
}

#[test]
fn a_plane_reflects_cells_about_the_centre_or_a_coordinate() {
    let plane = Mirror::about_centre(0, [(0, 0, 0), (15, 3, 3)]).unwrap();
    assert_eq!(plane.twice, 16);
    assert_eq!(plane.cell((0, 1, 2)), (15, 1, 2));
    assert_eq!(plane.cell((7, 1, 2)), (8, 1, 2));
    // An odd width keeps its middle cell on itself.
    let odd = Mirror::about_centre(2, [(0, 0, 0), (0, 0, 4)]).unwrap();
    assert_eq!(odd.cell((0, 0, 2)), (0, 0, 2));
    assert_eq!(Mirror::at(1, 4.5).unwrap().cell((0, 4, 0)), (0, 4, 0));
    assert!(Mirror::at(0, 4.25).is_err());
    assert!(Mirror::about_centre(0, []).is_none());
}

#[test]
fn a_stroke_and_a_projected_mark_land_on_both_sides() {
    let mut session = ball();
    let reply = apply(
        &mut session,
        "voxel.surface_stroke",
        &json!({"points": [[2, 3, 8], [4, 2, 12]], "palette": 5, "width": 2, "mirror": true}),
    )
    .unwrap();
    let painted = of_slot(&session, 5);
    assert!(reply["mirrored"].as_u64().unwrap() > 0, "{reply}");
    assert_eq!(reply["cells"].as_u64().unwrap() as usize, painted.len());
    assert!(painted.iter().any(|c| c.0 < 8) && painted.iter().any(|c| c.0 >= 8));
    assert!(symmetric(&painted));
    // One undo step takes both sides away.
    session.undo();
    assert!(of_slot(&session, 5).is_empty());
    // A mark on the image's left only (front view: image right is +X).
    let reply = apply(
        &mut session,
        "voxel.project_paint",
        &json!({"view": "front", "rows": ["x...", "....", "....", "...."], "legend": {"x": 6}, "mirror": "x"}),
    )
    .unwrap();
    let marked = of_slot(&session, 6);
    assert!(reply["mirrored"].as_u64().unwrap() > 0);
    assert!(marked.iter().any(|c| c.0 < 4) && marked.iter().any(|c| c.0 > 11));
    assert!(symmetric(&marked));
}

#[test]
fn the_far_side_is_painted_on_its_own_surface_when_the_object_is_lopsided() {
    let mut session = ball();
    // A bump on the +x side only: its cells are the far side's surface.
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 16, "y0": 6, "z0": 6, "x1": 17, "y1": 9, "z1": 9, "palette": 2}),
    )
    .unwrap();
    // Plane at 8, not the new centre 9.
    apply(
        &mut session,
        "voxel.surface_stroke",
        &json!({"points": [[0, 7, 7], [0, 8, 8]], "palette": 5, "mirror": {"axis": "x", "at": 8}}),
    )
    .unwrap();
    let cells = &session.active().cells;
    let painted = of_slot(&session, 5);
    let right: Vec<&Cell> = painted.iter().filter(|c| c.0 >= 8).collect();
    assert!(!right.is_empty());
    for c in &painted {
        // On the surface: never the ball's old face at x 15, now inside.
        let open = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .iter()
        .any(|d| !cells.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2)));
        assert!(open, "{c:?}");
    }
    assert!(right.iter().all(|c| c.0 >= 16), "{right:?}");
}

#[test]
fn set_voxels_fills_and_recolor_mirror_their_cells() {
    let mut session = ball();
    let reply = apply(
        &mut session,
        "voxel.set_many",
        &json!({"cells": [[1, 8, 8, 7], [3, 3, 20, 7]], "mirror": {"at": 8}}),
    )
    .unwrap();
    assert_eq!(reply["mirrored"], 2);
    let cells = &session.active().cells;
    assert_eq!(cells[&(14, 8, 8)], 7);
    assert_eq!(cells[&(12, 3, 20)], 7);
    // Paint mode only touches solid cells, on both sides.
    let reply = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 6, "z0": 6, "x1": 2, "y1": 9, "z1": 9, "palette": 3, "mode": "paint", "mirror": true}),
    )
    .unwrap();
    let painted = of_slot(&session, 3);
    assert!(reply["mirrored"].as_u64().unwrap() > 0);
    assert!(symmetric(&painted));
    assert!(painted.iter().all(|c| c.0 <= 2 || c.0 >= 13));
    // A recolor box on one side recolours its mirror too.
    let reply = apply(
        &mut session,
        "voxel.recolor",
        &json!({"from": 3, "to": 4, "x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 15, "z1": 15, "mirror": "x"}),
    )
    .unwrap();
    assert_eq!(
        reply["mirrored"].as_u64().unwrap(),
        reply["matched"].as_u64().unwrap()
    );
    assert!(of_slot(&session, 3).is_empty());
    assert_eq!(of_slot(&session, 4), painted);
    // A fill_tube in replace mode adds the second limb.
    apply(
        &mut session,
        "voxel.tube",
        &json!({"points": [[1, 8, 8], [-4, 8, 4]], "radius": 1, "palette": 9, "mirror": true}),
    )
    .unwrap();
    assert_eq!(session.active().cells.get(&(-4, 8, 4)), Some(&9));
}

#[test]
fn bad_mirrors_are_explained() {
    let mut session = ball();
    for (mirror, text) in [
        (json!("w"), "x, y, or z"),
        (json!({"at": 3.3}), "multiple of 0.5"),
        (json!({"plane": 3}), "axis and at"),
        (json!(4), "true, an axis"),
    ] {
        let err = apply(
            &mut session,
            "voxel.set_many",
            &json!({"cells": [[0, 0, 0, 1]], "mirror": mirror}),
        )
        .unwrap_err();
        assert!(err.contains(text), "{err}");
    }
    let err = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 3, "y1": 3, "z1": 3, "palette": 1, "mode": "intersect", "mirror": true}),
    )
    .unwrap_err();
    assert!(err.contains("intersect"), "{err}");
    let mut empty = Session::new();
    empty.replace_empty();
    let err = apply(
        &mut empty,
        "voxel.set_many",
        &json!({"cells": [[0, 0, 0, 1]], "mirror": true}),
    )
    .unwrap_err();
    assert!(err.contains("needs cells"), "{err}");
}

#[test]
fn a_world_plane_moves_with_the_object() {
    let mut session = ball();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "position": [100, 0, 0]}),
    )
    .unwrap();
    apply(
        &mut session,
        "voxel.set_many",
        &json!({"cells": [[101, 8, 20, 7]], "space": "world", "mirror": {"axis": "x", "at": 108}}),
    )
    .unwrap();
    let cells = &session.active().cells;
    assert_eq!(cells[&(1, 8, 20)], 7);
    assert_eq!(cells[&(14, 8, 20)], 7);
}

#[test]
fn scripts_batches_and_panels_take_the_mirror() {
    let mut session = ball();
    session
        .set_script(
            0,
            "fill_ellipsoid(0, 0, 0, 15, 15, 15, 2);\n\
             set_voxels([[1, 20, 8, 7]], #{mirror: true});\n\
             fill_box(0, 6, 6, 1, 9, 9, 3, #{mode: \"paint\", mirror: \"x\"});\n\
             let s = surface_stroke([[2, 3, 8], [4, 2, 12]], 5, #{mirror: #{axis: \"x\"}});\n\
             if s.cells == 0 || s.mirrored == 0 { throw \"stroke\"; }\n\
             recolor(3, 4, #{x0: 0, y0: 0, z0: 0, x1: 7, y1: 15, z1: 15, mirror: true});\n\
             project_paint(\"front\", [\"x...\", \"....\"], #{legend: #{x: 6}, mirror: true});",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.active().cells;
    assert_eq!(cells[&(14, 20, 8)], 7);
    assert!(symmetric(&of_slot(&session, 4)) && !of_slot(&session, 4).is_empty());
    assert!(of_slot(&session, 3).is_empty());
    assert!(symmetric(&of_slot(&session, 5)) && !of_slot(&session, 5).is_empty());
    assert!(symmetric(&of_slot(&session, 6)) && !of_slot(&session, 6).is_empty());

    let mut session = ball();
    let reply = apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [{"tool": "set_voxels", "arguments": {"cells": [[1, 8, 20, 7]], "mirror": true}}]}),
    )
    .unwrap();
    assert!(reply.to_string().contains("mirrored"), "{reply}");
    assert_eq!(session.active().cells[&(14, 8, 20)], 7);

    // The panels write the argument.
    let mut session = ball();
    session.edit.stroke.points = "2 3 8\n4 2 12".into();
    session.edit.stroke.mirror.on = true;
    session.stroke_from_panel();
    assert!(
        session.status.contains("mirrored side"),
        "{}",
        session.status
    );
    assert!(symmetric(&of_slot(&session, session.active_slot)));
    session.edit.recolor.mirror = crate::state::MirrorField {
        on: true,
        axis: 0,
        at: "8.5".into(),
    };
    assert_eq!(
        session.edit.recolor.args(None).unwrap_or_default()["mirror"],
        json!(null)
    );
    session.edit.recolor.map = "2>3".into();
    assert_eq!(
        session.edit.recolor.args(None).unwrap()["mirror"],
        json!({"axis": "x", "at": 8.5})
    );
    session.edit.recolor.mirror.at = "8.2".into();
    assert!(session.edit.recolor.args(None).is_err());
}

#[test]
fn the_viewport_brush_writes_both_sides() {
    let mut session = ball();
    session.edit.viewport_mirror.on = true;
    let base: HashMap<Cell, u8> = session.active().cells.clone();
    session.write_preview(base, vec![((-1, 8, 8), 3)]);
    let cells = &session.active().cells;
    assert_eq!(cells.get(&(-1, 8, 8)), Some(&3));
    assert_eq!(cells.get(&(16, 8, 8)), Some(&3));
}

#[test]
fn the_mirrored_markings_example_is_symmetric() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("fixtures/examples/mirrored_markings.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let plane = Mirror::about_centre(0, session.active().cells.keys().copied()).unwrap();
    assert_eq!(plane.twice, 24);
    for slot in [3, 4, 5, 6] {
        let cells = of_slot(&session, slot);
        assert!(!cells.is_empty(), "slot {slot}");
        let missing = cells
            .iter()
            .filter(|c| !cells.contains(&plane.cell(**c)))
            .count();
        assert_eq!(missing, 0, "slot {slot}");
    }
}
