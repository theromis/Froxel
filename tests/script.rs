use std::collections::HashMap;

use super::*;
use crate::state::{Cell, Motion, Session};

const ROW: &str = r#"
let n = param_int("count", 4, 1, 8);
for x in 0..n {
    set(x, 0, 0, 1);
}
"#;

fn cells_of(session: &Session) -> HashMap<Cell, u8> {
    session.objects[0].cells.clone()
}

#[test]
fn parameters_build_the_same_voxels_until_the_seed_changes() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].cells.len(), 4);
    let first = cells_of(&session);
    session.regenerate_script(0).unwrap();
    assert_eq!(cells_of(&session), first);

    let source = r#"
        let seed = param_int("seed", 1, 0, 99);
        let roll = rng(seed);
        for i in 0..16 {
            set(roll.int(0, 30), roll.int(0, 30), 0, 1);
        }
    "#;
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    let seed_one = cells_of(&session);
    session.regenerate_script(0).unwrap();
    assert_eq!(cells_of(&session), seed_one);
    session
        .set_script_params(
            0,
            &[(
                "seed".into(),
                ParamValue::Int {
                    value: 2,
                    min: 0,
                    max: 99,
                },
            )],
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_ne!(cells_of(&session), seed_one);
    assert!(!session.objects[0].cells.is_empty());
}

#[test]
fn a_timeout_or_a_missing_file_function_leaves_the_document() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.regenerate_script(0).unwrap();
    let cells = cells_of(&session);
    let palette = session.palette;
    let materials = session.materials;
    let params = session.objects[0].script.params.clone();

    session.objects[0].script.source = "let i = 0;\nloop { i += 1; }\n".into();
    let err = session
        .regenerate_script_within(0, Some(std::time::Duration::from_millis(200)))
        .unwrap_err();
    assert!(err.contains("line") && err.contains("timed out"), "{err}");
    assert_eq!(cells_of(&session), cells);
    assert_eq!(session.palette, palette);
    assert_eq!(session.materials, materials);
    assert_eq!(session.objects[0].script.params, params);
    assert_eq!(session.objects.len(), 1);

    session.objects[0].script.source = "let ready = 1;\nread_file(\"/etc/passwd\");\n".into();
    let err = session.regenerate_script(0).unwrap_err();
    assert!(err.contains("line 2"), "{err}");
    assert_eq!(cells_of(&session), cells);
    assert_eq!(session.objects.len(), 1);
}

#[test]
fn script_round_trips_and_opening_does_not_run_it() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].cells.len(), 4);
    session.objects[0].cells.insert((9, 3, 1), 2);
    let path = std::env::temp_dir().join("froxel-script-roundtrip.vox");
    session.write_vox(&path).unwrap();
    session.open_vox(&path).unwrap();
    assert_eq!(
        session.objects[0].cells.len(),
        5,
        "{:?}",
        session.objects[0].cells
    );
    assert!(
        session.objects[0].cells.values().any(|slot| *slot == 2),
        "opening must keep the hand edit instead of regenerating"
    );
    assert!(session.objects[0].script.source.contains("param_int"));
    assert_eq!(session.objects[0].script.params[0].name, "count");
    assert!(session.status.contains("carries a script"));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(4).any(|window| window == b"MVSC"));
    let _ = std::fs::remove_file(&path);

    let mut plain = Session::new();
    plain.replace_empty();
    plain.objects[0].cells.insert((0, 0, 0), 1);
    let path = std::env::temp_dir().join("froxel-script-plain.vox");
    plain.write_vox(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!bytes.windows(4).any(|window| window == b"MVSC"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn motion_and_a_named_object_are_part_of_one_undo_step() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            r#"
            set_slot(4, [10, 20, 30]);
            set_motion(4, "sway", 1.5);
            set(0, 0, 0, 4);
            object("legs");
            fill_box(0, 0, 0, 1, 0, 0, 4);
            "#,
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].cells.get(&(0, 0, 0)), Some(&4));
    assert_eq!(session.palette[4][0], 10);
    assert_eq!(session.materials[4].motion, Motion::Sway);
    assert_eq!(session.materials[4].motion_amount, 1.5);
    let legs = session
        .objects
        .iter()
        .find(|object| object.name == "legs")
        .unwrap();
    assert_eq!(legs.cells.len(), 2);
    session.undo();
    assert!(session.objects[0].cells.is_empty());
    assert_eq!(session.objects.len(), 1);
    assert_eq!(session.materials[4].motion, Motion::None);
}

#[test]
fn the_lily_pad_is_two_layers_and_responds_to_its_seed() {
    let source = include_str!("../examples/lily_pad.rhai");
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    let pad = cells_of(&session);
    assert!(pad.len() > 20, "{}", pad.len());
    assert!(pad.keys().any(|cell| cell.1 == 0));
    assert!(pad.keys().any(|cell| cell.1 == 1));
    assert!(pad.values().any(|slot| *slot == 1));
    assert!(pad.values().any(|slot| *slot == 3));
    session
        .set_script_params(
            0,
            &[(
                "seed".into(),
                ParamValue::Int {
                    value: 7,
                    min: 0,
                    max: 9999,
                },
            )],
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_ne!(cells_of(&session), pad);
}

fn wait_for_script(session: &mut Session) {
    let start = std::time::Instant::now();
    while !session.poll_script() {
        assert!(start.elapsed().as_secs() < 30, "the worker did not finish");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_worker_run_is_one_undo_step_and_has_no_voxel_cap() {
    let mut session = Session::new();
    session.replace_empty();
    // 80 x 80 x 50 = 320 000 voxels, past the old 250 000 cap.
    session
        .set_script(0, "fill_box(0, 0, 0, 79, 79, 49, 1);")
        .unwrap();
    session.start_script(0, false).unwrap();
    assert!(session.script_running().is_some());
    wait_for_script(&mut session);
    assert!(session.script_running().is_none());
    assert!(session.script_error.is_empty(), "{}", session.script_error);
    assert_eq!(session.objects[0].cells.len(), 320_000);
    session.undo();
    assert!(session.objects[0].cells.is_empty());
}

#[test]
fn cancelling_a_worker_run_leaves_the_document() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.regenerate_script(0).unwrap();
    let cells = cells_of(&session);
    session.objects[0].script.source = "let i = 0;\nloop { i += 1; }\n".into();
    session.start_script(0, false).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    session.cancel_script();
    assert!(session.script_running().is_none());
    assert!(!session.poll_script());
    assert_eq!(cells_of(&session), cells);
}

#[test]
fn a_worker_run_is_dropped_when_the_source_changes() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.start_script(0, false).unwrap();
    session.objects[0].script.source = "set(0, 0, 0, 1);".into();
    wait_for_script(&mut session);
    assert!(session.objects[0].cells.is_empty());
    assert!(
        session.script_error.contains("changed"),
        "{}",
        session.script_error
    );
}

#[test]
fn reopening_preserves_generator_coordinates_and_regeneration_placement() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            r#"
        let wide = param_int("wide", 3, 2, 5);
        fill_box(-wide, -4, -2, 5, 2, 9, 1);
    "#,
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    // Hand edits must also survive opening; loading must not execute code.
    session.objects[0].cells.insert((-7, 8, 3), 2);
    session.objects[0].translation = (19, -13, 6);
    let local = cells_of(&session);
    let path = std::env::temp_dir().join("froxel-script-origin-roundtrip.vox");
    for _ in 0..3 {
        session.write_vox(&path).unwrap();
        session.open_vox(&path).unwrap();
        assert_eq!(cells_of(&session), local);
    }
    let position = session.objects[0].translation;
    session.regenerate_script(0).unwrap();
    let generated = cells_of(&session);
    assert!(!generated.contains_key(&(-7, 8, 3)));
    assert_eq!(session.objects[0].translation, position);
    for _ in 0..3 {
        session.write_vox(&path).unwrap();
        session.open_vox(&path).unwrap();
        let before = session.objects[0].translation;
        assert_eq!(cells_of(&session), generated);
        session.regenerate_script(0).unwrap();
        assert_eq!(cells_of(&session), generated);
        assert_eq!(session.objects[0].translation, before);
    }
    session.write_script_int(0, "wide", 5);
    session.regenerate_script(0).unwrap();
    let changed = cells_of(&session);
    session.write_vox(&path).unwrap();
    session.open_vox(&path).unwrap();
    let before = session.objects[0].translation;
    session.regenerate_script(0).unwrap();
    assert_eq!(cells_of(&session), changed);
    assert_eq!(session.objects[0].translation, before);
    let _ = std::fs::remove_file(path);
}

#[test]
fn version_one_script_metadata_remains_readable() {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_str(&mut bytes, "legacy");
    push_str(&mut bytes, ROW);
    bytes.push(0);
    push_u32(&mut bytes, 0);
    let scripts = decode_chunk(&bytes).unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].name, "legacy");
    assert_eq!(scripts[0].script.source, ROW);
    assert_eq!(scripts[0].local_min, None);
}
