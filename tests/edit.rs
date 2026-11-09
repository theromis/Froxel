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
}
