use super::*;
use crate::remote::apply;

/// A session whose active object 0 sits at world `position` and holds a
/// 4 x 2 x 2 bar: slot 3 at x 0..1 and slot 5 at x 2..3.
fn bar(position: [i32; 3]) -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "position": position, "name": "body"}),
    )
    .unwrap();
    for (x, slot) in [(0, 3), (1, 3), (2, 5), (3, 5)] {
        apply(
            &mut session,
            "voxel.fill",
            &json!({"x0": x, "y0": 0, "z0": 0, "x1": x, "y1": 1, "z1": 1, "palette": slot}),
        )
        .unwrap();
    }
    session
}

fn world(session: &Session, index: usize) -> Vec<(i32, i32, i32, u8)> {
    let object = &session.objects[index];
    let t = object.translation;
    let mut cells: Vec<_> = object
        .cells
        .iter()
        .map(|(c, slot)| (c.0 + t.0, c.1 + t.1, c.2 + t.2, *slot))
        .collect();
    cells.sort_unstable();
    cells
}

#[test]
fn split_object_moves_a_box_into_a_new_object_in_place() {
    let mut session = bar([10, -4, 2]);
    let before = world(&session, 0);
    let reply = apply(
        &mut session,
        "object.split",
        &json!({"boxes": [2, 0, 0, 3, 1, 1], "name": "tail"}),
    )
    .unwrap();
    assert_eq!(reply["created"], true);
    assert_eq!(reply["cells"], 8);
    assert_eq!(reply["position"], json!([10, -4, 2]));
    assert_eq!(reply["sourceVoxels"], 8);
    assert_eq!(session.objects.len(), 2);
    assert_eq!(session.objects[1].name, "tail");
    // Every cell keeps its world position; the two objects hold all of them.
    let mut after = world(&session, 0);
    after.extend(world(&session, 1));
    after.sort_unstable();
    assert_eq!(after, before);
    assert!(world(&session, 1).iter().all(|cell| cell.3 == 5));
    // One undo step puts the source back and removes the part.
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.objects.len(), 1);
    assert_eq!(world(&session, 0), before);
}

#[test]
fn split_object_by_slot_copy_and_invert() {
    let mut session = bar([0, 0, 0]);
    let reply = apply(
        &mut session,
        "object.split",
        &json!({"slots": 3, "copy": true, "name": "copy"}),
    )
    .unwrap();
    assert_eq!(reply["cells"], 8);
    assert_eq!(session.objects[0].cells.len(), 16);
    let reply = apply(
        &mut session,
        "object.split",
        &json!({"index": "body", "slots": [3], "invert": true, "name": "rest"}),
    )
    .unwrap();
    assert_eq!(reply["cells"], 8);
    assert!(session.objects[0].cells.values().all(|slot| *slot == 3));
    assert!(session.objects[2].cells.values().all(|slot| *slot == 5));
}

#[test]
fn split_object_into_an_existing_object_at_its_own_offset() {
    let mut session = bar([5, 5, 5]);
    apply(
        &mut session,
        "object.add",
        &json!({"name": "head", "position": [0, 0, 0], "make_active": false}),
    )
    .unwrap();
    let reply = apply(
        &mut session,
        "object.split",
        &json!({"boxes": [[0, 0, 0, 0, 1, 1], [3, 0, 0, 3, 1, 1]], "into": "head", "makeActive": true}),
    )
    .unwrap();
    assert_eq!(reply["created"], false);
    assert_eq!(reply["target"], 1);
    assert_eq!(reply["cells"], 8);
    assert_eq!(session.selected, 1);
    // Local cells of the target are the world cells, since it sits at 0.
    assert_eq!(session.objects[1].cells.get(&(5, 5, 5)), Some(&3));
    assert_eq!(session.objects[1].cells.get(&(8, 6, 6)), Some(&5));
    assert_eq!(session.objects[0].cells.len(), 8);
}

#[test]
fn split_object_refuses_an_empty_selection_and_itself() {
    let mut session = bar([0, 0, 0]);
    let err = apply(
        &mut session,
        "object.split",
        &json!({"boxes": [20, 20, 20, 21, 21, 21]}),
    )
    .unwrap_err();
    assert!(err.contains("select no cell"), "{err}");
    let err = apply(
        &mut session,
        "object.split",
        &json!({"into": 0, "slots": 3}),
    )
    .unwrap_err();
    assert!(err.contains("itself"), "{err}");
    let err = apply(&mut session, "object.split", &json!({})).unwrap_err();
    assert!(err.contains("boxes, slots"), "{err}");
    assert_eq!(session.objects.len(), 1);
    assert!(!session.can_undo() || session.objects[0].cells.len() == 16);
}

#[test]
fn split_object_runs_from_a_script() {
    let mut session = bar([0, 0, 0]);
    session
        .set_script(
            0,
            "fill_box(0, 0, 0, 3, 1, 1, 3);\nsplit_object(0, #{boxes: [2, 0, 0, 3, 1, 1], name: \"tail\"});",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects.len(), 2);
    assert_eq!(session.objects[0].cells.len(), 8);
    assert_eq!(session.objects[1].cells.len(), 8);
}

#[test]
fn split_panel_builds_the_mcp_arguments() {
    let mut session = bar([0, 0, 0]);
    session.edit.split.boxes = "2 0 0 3 1 1".into();
    session.edit.split.name = "tail".into();
    session.split_from_panel();
    assert_eq!(session.objects.len(), 2, "{}", session.status);
    assert_eq!(session.objects[1].name, "tail");
}

#[test]
fn the_turtle_parts_example_cuts_the_same_parts_on_a_rerun() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/turtle_parts.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let names: Vec<&str> = session.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names[1..],
        ["head", "flipper_l", "flipper_r", "tail"],
        "{names:?}"
    );
    let first: Vec<usize> = session.objects.iter().map(|o| o.cells.len()).collect();
    assert!(first.iter().all(|count| *count > 0), "{first:?}");
    // The body keeps only the shell's span.
    let body = &session.objects[0].cells;
    assert!(body
        .keys()
        .all(|c| (0..22).contains(&c.0) && (0..28).contains(&c.1)));
    session.regenerate_script(0).unwrap();
    let again: Vec<usize> = session.objects.iter().map(|o| o.cells.len()).collect();
    assert_eq!(again, first);
    // A shorter turtle leaves no stale cells in its parts.
    let index = 0;
    session
        .set_script_params(
            index,
            &[(
                "length".into(),
                crate::script::ParamValue::Int {
                    value: 20,
                    min: 16,
                    max: 64,
                },
            )],
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let head = &session.objects[1].cells;
    assert!(
        head.keys().all(|c| c.1 >= 20),
        "a head cell is left from the longer turtle"
    );
    assert_eq!(slots(&session, 0)[4] > 0, true);
}

#[test]
fn world_space_fills_carves_and_reads_at_world_cells() {
    let mut session = bar([10, -4, 2]);
    let reply = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 10, "y0": -4, "z0": 4, "x1": 11, "y1": -3, "z1": 4, "palette": 7, "space": "world"}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 4);
    assert_eq!(reply["space"], "world");
    assert_eq!(session.objects[0].cells.get(&(0, 0, 2)), Some(&7));
    let got = apply(
        &mut session,
        "voxel.get",
        &json!({"x": 11, "y": -3, "z": 4, "space": "world"}),
    )
    .unwrap();
    assert_eq!(
        (got["x"].clone(), got["palette"].clone()),
        (json!(11), json!(7))
    );
    let got = apply(
        &mut session,
        "voxel.get_box",
        &json!({"x0": 10, "y0": -4, "z0": 4, "x1": 10, "y1": -4, "z1": 4, "space": "world"}),
    )
    .unwrap();
    assert_eq!(
        got["cells"],
        json!([{"x": 10, "y": -4, "z": 4, "palette": 7}])
    );
    let reply = apply(
        &mut session,
        "voxel.carve",
        &json!({"shape": "box", "x0": 10, "y0": -4, "z0": 4, "x1": 11, "y1": -3, "z1": 4, "space": "world"}),
    )
    .unwrap();
    assert_eq!(reply["removed"], 4);
    apply(
        &mut session,
        "voxel.tube",
        &json!({"points": [[10, -4, 6], [13, -4, 6]], "radius": 0.5, "palette": 9, "space": "world"}),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.get(&(3, 0, 4)), Some(&9));
    apply(
        &mut session,
        "voxel.set_many",
        &json!({"cells": [[20, 0, 2, 4], {"x": 21, "y": 0, "z": 2, "palette": 4}], "space": "world"}),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.get(&(10, 4, 0)), Some(&4));
    assert_eq!(session.objects[0].cells.get(&(11, 4, 0)), Some(&4));
    // A local call is unchanged, and a tool without space refuses world.
    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 0, "y": 0, "z": 9, "palette": 2, "space": "local"}),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.get(&(0, 0, 9)), Some(&2));
    let err = apply(
        &mut session,
        "voxel.deform",
        &json!({"op": "taper", "space": "world"}),
    )
    .unwrap_err();
    assert!(err.contains("space world"), "{err}");
}

#[test]
fn world_space_survives_a_save_and_reopen_that_moves_the_objects() {
    let mut session = bar([0, 0, 0]);
    apply(
        &mut session,
        "object.split",
        &json!({"boxes": [2, 0, 0, 3, 1, 1], "name": "tail"}),
    )
    .unwrap();
    let path = std::env::temp_dir().join("froxel-world-space.vox");
    apply(
        &mut session,
        "document.save",
        &json!({"path": path.display().to_string()}),
    )
    .unwrap();
    apply(
        &mut session,
        "document.open",
        &json!({"path": path.display().to_string()}),
    )
    .unwrap();
    // The same world box paints the same cells of whichever part they are in.
    let mut painted = 0;
    for index in 0..session.objects.len() {
        session.select_object(index);
        let reply = apply(
            &mut session,
            "voxel.fill",
            &json!({"x0": 1, "y0": 0, "z0": 0, "x1": 2, "y1": 1, "z1": 1, "palette": 8, "mode": "paint", "space": "world"}),
        )
        .unwrap();
        painted += reply["changed"].as_i64().unwrap();
    }
    assert_eq!(painted, 8);
    let mut cells: Vec<_> = (0..session.objects.len())
        .flat_map(|index| world(&session, index))
        .filter(|cell| cell.3 == 8)
        .map(|cell| cell.0)
        .collect();
    cells.sort_unstable();
    cells.dedup();
    assert_eq!(cells, [1, 2]);
}

#[test]
fn a_mirrored_copy_is_filled_at_its_mirrored_world_cells() {
    let mut session = bar([4, 0, 0]);
    let reply = apply(
        &mut session,
        "object.mirror",
        &json!({"index": 0, "axis": "x", "name": "body_r"}),
    )
    .unwrap();
    // World x 4..7 mirrors to -8..-5; the copy's origin is that minimum corner.
    assert_eq!(reply["position"], json!([-8, 0, 0]));
    session.select_object(1);
    let reply = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": -8, "y0": 0, "z0": 0, "x1": -5, "y1": 1, "z1": 1, "palette": 6, "mode": "paint", "space": "world"}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 16);
}

#[test]
fn world_space_splits_and_runs_in_scripts() {
    let mut session = bar([10, 0, 0]);
    let reply = apply(
        &mut session,
        "object.split",
        &json!({"boxes": [12, 0, 0, 13, 1, 1], "name": "tail", "space": "world"}),
    )
    .unwrap();
    assert_eq!(reply["cells"], 8);
    assert_eq!(
        reply["bounds"],
        json!({"min": [12, 0, 0], "max": [13, 1, 1]})
    );
    session
        .set_script(
            0,
            "fill_box(10, 0, 0, 11, 1, 1, 3, #{space: \"world\"});\nset_voxels([[10, 0, 5, 4]], #{space: \"world\"});\nlet got = get_voxels(10, 0, 5, 10, 0, 5, #{space: \"world\"});\nif got[0][0] != 10 { throw \"get_voxels is not in world cells\"; }\ncarve_box(11, 0, 0, 11, 1, 1, #{space: \"world\"});",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.objects[0].cells;
    assert_eq!(cells.len(), 5, "{cells:?}");
    assert_eq!(cells.get(&(0, 0, 5)), Some(&4));
    let source = "object(\"fresh\");\nfill_box(0, 0, 0, 1, 1, 1, 3, #{space: \"world\"});";
    let err = session
        .set_script(0, source)
        .and_then(|_| session.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("local cells"), "{err}");
}

fn slots(session: &Session, index: usize) -> [usize; 10] {
    let mut counts = [0; 10];
    for slot in session.objects[index].cells.values() {
        counts[*slot as usize] += 1;
    }
    counts
}

#[test]
fn recolor_swaps_slots_in_a_box_or_everywhere_without_a_shape_change() {
    let mut session = bar([3, 0, 0]);
    let reply = apply(&mut session, "voxel.recolor", &json!({"from": 3, "to": 7})).unwrap();
    assert_eq!(
        (reply["matched"].clone(), reply["changed"].clone()),
        (json!(8), json!(8))
    );
    assert_eq!(slots(&session, 0)[7], 8);
    // A map applies to the cells as they were, so it swaps.
    apply(
        &mut session,
        "voxel.recolor",
        &json!({"map": {"7": 5, "5": 7}}),
    )
    .unwrap();
    assert_eq!((slots(&session, 0)[5], slots(&session, 0)[7]), (8, 8));
    assert_eq!(session.objects[0].cells.get(&(0, 0, 0)), Some(&5));
    // In a box (here in world cells) only.
    let reply = apply(
        &mut session,
        "voxel.recolor",
        &json!({"map": [[5, 9], [7, 9]], "x0": 3, "y0": 0, "z0": 0, "x1": 3, "y1": 1, "z1": 1, "space": "world"}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 4);
    assert_eq!(session.voxel_count(), 16);
    // One undo step.
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(slots(&session, 0)[9], 0);
    // 0 erases; a slot mapped twice and a partial box are refused.
    apply(&mut session, "voxel.recolor", &json!({"from": 7, "to": 0})).unwrap();
    assert_eq!(session.voxel_count(), 8);
    let err = apply(
        &mut session,
        "voxel.recolor",
        &json!({"map": [[5, 1], [5, 2]]}),
    )
    .unwrap_err();
    assert!(err.contains("twice"), "{err}");
    let err = apply(
        &mut session,
        "voxel.recolor",
        &json!({"from": 5, "to": 1, "x0": 0}),
    )
    .unwrap_err();
    assert!(err.contains("all of x0"), "{err}");
}

#[test]
fn recolor_runs_from_a_script_a_batch_and_the_panel() {
    let mut session = bar([0, 0, 0]);
    session
        .set_script(
            0,
            "fill_box(0, 0, 0, 3, 0, 0, 3);\nlet r = recolor(3, 4, #{x0: 0, y0: 0, z0: 0, x1: 1, y1: 0, z1: 0});\nif r.changed != 2 { throw \"recolor\"; }\nrecolor([[4, 6]]);",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(slots(&session, 0)[6], 2);
    assert_eq!(slots(&session, 0)[3], 2);
    let reply = apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [{"tool": "recolor", "arguments": {"from": 6, "to": 2}}]}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 2);
    session.edit.recolor.map = "2>8, 3>8".into();
    session.recolor_from_panel();
    assert_eq!(slots(&session, 0)[8], 4, "{}", session.status);
}

#[test]
fn clear_object_empties_any_object_with_no_box_limit() {
    let mut session = Session::new();
    session.replace_empty();
    // 256 x 256 x 40 cells is more than one fill_box may erase.
    for z in 0..40 {
        apply(
            &mut session,
            "voxel.fill",
            &json!({"x0": 0, "y0": 0, "z0": z, "x1": 255, "y1": 255, "z1": z, "palette": 2}),
        )
        .unwrap();
    }
    let err = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 255, "y1": 255, "z1": 39, "palette": 0}),
    )
    .unwrap_err();
    assert!(err.contains("2000000"), "{err}");
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "name": "big", "position": [3, 4, 5]}),
    )
    .unwrap();
    let reply = apply(&mut session, "object.clear", &json!({"index": "big"})).unwrap();
    assert_eq!(reply["removed"], 256 * 256 * 40);
    assert_eq!(reply["voxels"], 0);
    assert_eq!(session.objects[0].translation, (3, 4, 5));
    assert_eq!(session.objects[0].name, "big");
    // One undo step brings every cell back; clearing an empty object is none.
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.voxel_count(), 256 * 256 * 40);
    apply(&mut session, "edit.redo", &json!({})).unwrap();
    let reply = apply(&mut session, "object.clear", &json!({})).unwrap();
    assert_eq!(reply["removed"], 0);
}

#[test]
fn clear_object_runs_from_a_script_a_batch_and_the_panel() {
    let mut session = bar([0, 0, 0]);
    apply(
        &mut session,
        "object.split",
        &json!({"slots": 5, "name": "tail"}),
    )
    .unwrap();
    session
        .set_script(
            0,
            "fill_box(0, 0, 0, 3, 0, 0, 3);\nif clear_object() != 4 { throw \"clear\"; }\nset(0, 0, 0, 2);\nclear_object(\"tail\");",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].cells.len(), 1);
    assert!(session.objects[1].cells.is_empty());
    let err = session
        .set_script(0, "clear_object(0);")
        .and_then(|_| session.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("object being written"), "{err}");
    session.set_script(0, "").unwrap();
    let reply = apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [{"tool": "clear_object", "arguments": {}}]}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 1);
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    session.clear_from_panel();
    assert!(session.objects[0].cells.is_empty(), "{}", session.status);
}

/// A solid 8 x 8 x 8 cube of slot 3 in a fresh session.
fn cube() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 7, "z1": 7, "palette": 3}),
    )
    .unwrap();
    session
}

#[test]
fn intersect_keeps_only_the_cells_inside_the_shape() {
    let mut session = cube();
    // A dome: the ellipsoid of a box twice the cube's height, sunk to its base.
    let reply = apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": -8, "x1": 7, "y1": 7, "z1": 7, "mode": "intersect"}),
    )
    .unwrap();
    let removed = reply["changed"].as_i64().unwrap();
    assert!(removed > 0 && removed < 512, "{removed}");
    let cells = &session.objects[0].cells;
    assert!(cells.contains_key(&(3, 3, 7)));
    assert!(!cells.contains_key(&(0, 0, 7)));
    assert!(
        cells.values().all(|slot| *slot == 3),
        "no cell is repainted"
    );
    // Nothing is added: a box larger than the object keeps all of it.
    let before = cells.len();
    let reply = apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": -4, "y0": -4, "z0": -4, "x1": 12, "y1": 12, "z1": 12, "mode": "intersect"}),
    )
    .unwrap();
    assert_eq!(reply["changed"], 0);
    assert_eq!(session.objects[0].cells.len(), before);
    // A rounded box and a tube, one undo step each.
    apply(
        &mut session,
        "voxel.rounded",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 7, "z1": 3, "radius": 2, "mode": "intersect"}),
    )
    .unwrap();
    assert!(session.objects[0].cells.keys().all(|c| c.2 <= 3));
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.objects[0].cells.len(), before);
    apply(
        &mut session,
        "voxel.tube",
        &json!({"points": [[3.5, 3.5, 0], [3.5, 3.5, 7]], "radius": 2, "mode": "intersect"}),
    )
    .unwrap();
    let cells = &session.objects[0].cells;
    assert!(cells
        .keys()
        .all(|c| (c.0 - 3).abs() <= 2 && (c.1 - 3).abs() <= 2));
    // Tools that only add refuse it.
    let err = apply(
        &mut session,
        "voxel.loft",
        &json!({"points": [[0, 0, 0], [4, 0, 0]], "radius": 1, "palette": 2, "mode": "intersect"}),
    )
    .unwrap_err();
    assert!(err.contains("intersect"), "{err}");
}

#[test]
fn carve_from_views_intersects_a_blockout_with_its_hull() {
    let mut session = cube();
    // Seen from the front, a triangle; from the side, a square.
    let views = json!([
        {"view": "front", "rows": ["...xx...", "..xxxx..", ".xxxxxx.", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx"]},
        {"view": "side", "rows": vec!["xxxxxxxx"; 8]}
    ]);
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        &json!({"views": views, "cellsPerPixel": 1, "palette": 5, "mode": "intersect"}),
    )
    .unwrap();
    assert!(reply["changed"].as_i64().unwrap() > 0);
    let cells = &session.objects[0].cells;
    assert!(!cells.contains_key(&(0, 0, 7)));
    assert!(cells.contains_key(&(3, 0, 7)));
    assert!(cells.values().all(|slot| *slot == 3));
}

#[test]
fn intersect_runs_from_a_script_and_the_panel() {
    let mut session = cube();
    session
        .set_script(
            0,
            "fill_box(0, 0, 0, 7, 7, 7, 3);\nfill_ellipsoid(0, 0, 0, 7, 7, 7, 0, #{mode: \"intersect\"});",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let ellipsoid = session.objects[0].cells.len();
    assert!(ellipsoid < 512 && ellipsoid > 200, "{ellipsoid}");
    session.set_script(0, "").unwrap();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 7, "z1": 7, "palette": 3}),
    )
    .unwrap();
    session.picked = [(0, 0, 0), (7, 7, 7)].into_iter().collect();
    session.edit.intersect.shape = 1;
    session.intersect_from_panel();
    assert_eq!(
        session.objects[0].cells.len(),
        ellipsoid,
        "{}",
        session.status
    );
}
