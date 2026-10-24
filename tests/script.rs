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
fn a_limit_or_a_missing_file_function_leaves_the_document() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, ROW).unwrap();
    session.regenerate_script(0).unwrap();
    let cells = cells_of(&session);
    let palette = session.palette;
    let materials = session.materials;
    let params = session.objects[0].script.params.clone();

    session.objects[0].script.source = "let mut i = 0;\nloop { i += 1; }\n".into();
    let err = session.regenerate_script(0).unwrap_err();
    assert!(err.contains("line"), "{err}");
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
