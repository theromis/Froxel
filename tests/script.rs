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

const SHORE: &str = r#"
let heights = param_floats("heights", [1.0, 2, 3.0], 0.0, 8.0, 16);
let path = param_vec2s("path", [[0.0, 0.0], [4.0, 2.0]], -32.0, 32.0, 64);
let posts = param_vec3s("posts", [], 0.0, 16.0, 8);
let x = 0;
for h in heights {
    set(x, 0, to_int(h), 1);
    x += 1;
}
for p in path {
    set(to_int(p[0]), to_int(p[1]), 10, 2);
}
set(heights.len(), path.len(), 20 + posts.len(), 3);
"#;

#[test]
fn list_parameters_are_arrays_of_numbers_and_points() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, SHORE).unwrap();
    let params = &session.objects[0].script.params;
    assert_eq!(
        params[0].value,
        ParamValue::Floats {
            values: vec![1.0, 2.0, 3.0],
            width: 1,
            min: 0.0,
            max: 8.0,
            max_len: 16,
        }
    );
    assert_eq!(
        param_json(&params[1]),
        serde_json::json!({
            "name": "path", "kind": "vec2s", "value": [[0, 0], [4, 2]],
            "min": -32, "max": 32, "maxLen": 64,
        })
    );
    assert_eq!(param_json(&params[2])["kind"], "vec3s");
    session.regenerate_script(0).unwrap();
    let cells = cells_of(&session);
    assert_eq!(cells.get(&(2, 0, 3)), Some(&1));
    assert_eq!(cells.get(&(4, 2, 10)), Some(&2));
    assert_eq!(cells.get(&(3, 2, 20)), Some(&3));

    let spec = session.objects[0].script.params[1].value.clone();
    let longer = parse_param(&spec, &serde_json::json!([[1, 1], [2, 5], [-3, 7.5]])).unwrap();
    session
        .set_script_params(0, &[("path".into(), longer)])
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].cells.get(&(2, 5, 10)), Some(&2));
    assert_eq!(session.objects[0].cells.get(&(3, 3, 20)), Some(&3));
}

#[test]
fn a_list_too_long_or_out_of_range_is_refused() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, SHORE).unwrap();
    let floats = session.objects[0].script.params[0].value.clone();
    let path = session.objects[0].script.params[1].value.clone();
    let err = parse_param(&floats, &serde_json::json!(vec![0.0; 17])).unwrap_err();
    assert!(err.contains("17 entries; at most 16"), "{err}");
    let err = parse_param(&floats, &serde_json::json!([1, 9])).unwrap_err();
    assert!(
        err.contains("entry 1") && err.contains("outside 0..=8"),
        "{err}"
    );
    let err = parse_param(&path, &serde_json::json!([[1, 2, 3]])).unwrap_err();
    assert!(err.contains("point of 2 numbers"), "{err}");
    let err = parse_param(&path, &serde_json::json!([1, 2])).unwrap_err();
    assert!(err.contains("entry 0"), "{err}");
    let err = parse_param(&floats, &serde_json::json!(3)).unwrap_err();
    assert!(err.contains("a list of numbers"), "{err}");

    let err = session
        .write_script_floats(0, "heights", vec![9.0])
        .unwrap_err();
    assert!(err.contains("outside"), "{err}");
    session
        .write_script_floats(0, "heights", vec![4.0])
        .unwrap();

    for (source, needle) in [
        (
            r#"param_floats("a", [1.0, 9.0], 0.0, 4.0, 4);"#,
            "default: entry 1",
        ),
        (
            r#"param_floats("a", [], 0.0, 4.0, 0);"#,
            "max_len must be 1..=4096",
        ),
        (
            r#"param_vec3s("a", [], 0.0, 4.0, 2000);"#,
            "max_len must be 1..=1365",
        ),
        (r#"param_vec2s("a", [[1.0]], 0.0, 4.0, 4);"#, "point of 2"),
        (r#"param_floats("a", [], 4.0, 0.0, 4);"#, "empty range"),
    ] {
        let err = session.set_script(0, source).unwrap_err();
        assert!(err.contains(needle), "{source}: {err}");
    }
}

#[test]
fn a_stored_list_is_kept_clamped_and_cut_to_the_new_declaration() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            r#"param_floats("h", [1.0, 2.0, 3.0, 4.0], 0.0, 8.0, 8);"#,
        )
        .unwrap();
    session
        .write_script_floats(0, "h", vec![7.0, 6.0, 5.0])
        .unwrap();
    session
        .set_script(0, r#"param_floats("h", [], 0.0, 5.5, 2);"#)
        .unwrap();
    assert_eq!(
        session.objects[0].script.params[0].value,
        ParamValue::Floats {
            values: vec![5.5, 5.5],
            width: 1,
            min: 0.0,
            max: 5.5,
            max_len: 2,
        }
    );
    // A list of another width starts from the default.
    session
        .set_script(0, r#"param_vec2s("h", [[1.0, 1.0]], 0.0, 5.5, 2);"#)
        .unwrap();
    assert_eq!(
        param_json(&session.objects[0].script.params[0])["value"],
        serde_json::json!([[1, 1]])
    );
}

#[test]
fn list_parameters_round_trip_through_the_chunk() {
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, SHORE).unwrap();
    session
        .write_script_floats(0, "posts", vec![1.5, 2.0, 3.25, 0.0, 16.0, 8.0])
        .unwrap();
    let body = encode_chunk(&session.objects).unwrap();
    let scripts = decode_chunk(&body).unwrap();
    assert_eq!(scripts[0].script.params, session.objects[0].script.params);

    let mut bad = body.clone();
    let at = bad.windows(5).rposition(|w| w == b"posts").unwrap() + 5;
    assert_eq!(bad[at], 4);
    bad[at + 1] = 5;
    let Err(err) = decode_chunk(&bad) else {
        panic!("width 5 was read");
    };
    assert!(err.contains("width"), "{err}");
}

/// Run `source` on one object and return its cells.
fn run_cells(source: &str) -> Result<HashMap<Cell, u8>, String> {
    let view = DocView {
        names: vec!["host".to_string()],
        ..DocView::default()
    };
    let outcome = execute(source, &[], 0, &view, &Stop::after(None))?;
    Ok(outcome.built.into_iter().next().unwrap().1)
}

/// The cells MCP leaves after `calls` on an empty object.
fn mcp_cells(calls: &[(&str, serde_json::Value)]) -> HashMap<Cell, u8> {
    let mut session = Session::new();
    session.replace_empty();
    for (method, params) in calls {
        crate::remote::apply(&mut session, method, params).unwrap();
    }
    cells_of(&session)
}

#[test]
fn shape_fills_build_what_the_mcp_tools_build() {
    use serde_json::json;
    let script = run_cells(
        r#"
        fill_rounded_box(0, 0, 0, 11, 7, 5, 2, 1);
        fill_ellipsoid(2, 2, 2, 9, 5, 9, 2, #{mode: "empty"});
        fill_box(0, 0, 0, 11, 0, 9, 3, #{mode: "paint"});
        let points = [[0, 3, 3, 2.5], [6, 3, 8], [12, 6, 12, 1.0]];
        fill_tube(points, [4, 5], #{
            radius: 1.5, curve: "smooth", aspect: 0.5, roll: 30,
            pattern: "stripes", sectors: 2, mode: "empty"
        });
        fill_tube([[0, 0, 12], [8, 0, 12]], 6, #{radius: 2, profile: "blade"});
        "#,
    )
    .unwrap();
    let mcp = mcp_cells(&[
        (
            "voxel.rounded",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 11, "y1": 7, "z1": 5, "radius": 2, "palette": 1}),
        ),
        (
            "voxel.ellipsoid",
            json!({"x0": 2, "y0": 2, "z0": 2, "x1": 9, "y1": 5, "z1": 9, "palette": 2, "mode": "empty"}),
        ),
        (
            "voxel.fill",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 11, "y1": 0, "z1": 9, "palette": 3, "mode": "paint"}),
        ),
        (
            "voxel.tube",
            json!({"points": [[0, 3, 3, 2.5], [6, 3, 8], [12, 6, 12, 1.0]], "palettes": [4, 5],
                "radius": 1.5, "curve": "smooth", "aspect": 0.5, "roll": 30,
                "pattern": "stripes", "sectors": 2, "mode": "empty"}),
        ),
        (
            "voxel.tube",
            json!({"points": [[0, 0, 12], [8, 0, 12]], "palette": 6, "radius": 2, "profile": "blade"}),
        ),
    ]);
    assert_eq!(script, mcp);
    for slot in 1..=6 {
        assert!(
            script.values().any(|&s| s == slot),
            "slot {slot} is missing"
        );
    }
}

#[test]
fn carve_smooth_break_and_scatter_match_the_mcp_tools() {
    use serde_json::json;
    let script = run_cells(
        r#"
        fill_box(0, 0, 0, 23, 9, 9, 1);
        if carve_box(0, 0, 0, 1, 1, 1) != 8 { throw "carve_box count"; }
        carve_ellipsoid(4, 2, 2, 9, 7, 7);
        carve_rounded_box(12, 2, 2, 17, 7, 7, 1.5);
        carve_tube([[0, 5, 5], [23, 5, 5]], #{radius: 1.5});
        let soft = smooth(0, 0, 0, 23, 9, 9, #{iterations: 2, fillAt: 14, removeAt: 12});
        if soft.iterations < 1 { throw "smooth ran no pass"; }
        let broken = break_end([20, 5, 5], [1, 0, 0], #{
            depth: 2, splinters: 6, length: [3, 6], bark: 2, sapwood: [3, 4], tips: 5, seed: 7
        });
        if broken.removed == 0 { throw "nothing broke off"; }
        let heap = scatter(0, 0, 10, 23, 9, 20, [6, 7], #{count: 12, kinds: ["chip", "pebble"], seed: 3, floor: 10});
        if heap.pieces == 0 { throw "no pieces landed"; }
        "#,
    )
    .unwrap();
    let mcp = mcp_cells(&[
        (
            "voxel.fill",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 9, "z1": 9, "palette": 1}),
        ),
        (
            "voxel.carve",
            json!({"shape": "box", "x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1}),
        ),
        (
            "voxel.carve",
            json!({"shape": "ellipsoid", "x0": 4, "y0": 2, "z0": 2, "x1": 9, "y1": 7, "z1": 7}),
        ),
        (
            "voxel.carve",
            json!({"shape": "rounded_box", "x0": 12, "y0": 2, "z0": 2, "x1": 17, "y1": 7, "z1": 7, "radius": 1.5}),
        ),
        (
            "voxel.carve",
            json!({"shape": "tube", "points": [[0, 5, 5], [23, 5, 5]], "radius": 1.5}),
        ),
        (
            "voxel.smooth",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 9, "z1": 9, "iterations": 2, "fillAt": 14, "removeAt": 12}),
        ),
        (
            "voxel.break",
            json!({"point": [20, 5, 5], "normal": [1, 0, 0], "depth": 2, "splinters": 6,
                "length": [3, 6], "bark": 2, "sapwood": [3, 4], "tips": 5, "seed": 7}),
        ),
        (
            "voxel.scatter",
            json!({"x0": 0, "y0": 0, "z0": 10, "x1": 23, "y1": 9, "z1": 20, "palettes": [6, 7],
                "count": 12, "kinds": ["chip", "pebble"], "seed": 3, "floor": 10}),
        ),
    ]);
    assert_eq!(script, mcp);
}

#[test]
fn shape_calls_refuse_bad_options_and_coordinates() {
    let err = run_cells(r#"fill_ellipsoid(0, 0, 0, 4, 4, 4, 1, #{mod: "empty"});"#).unwrap_err();
    assert!(err.contains("no option `mod`"), "{err}");
    let err = run_cells(r#"fill_box(0, 0, 0, 4, 4, 4, 1, #{mode: "under"});"#).unwrap_err();
    assert!(
        err.contains("mode must be replace, empty, or paint"),
        "{err}"
    );
    let err = run_cells(r#"carve_box(0, 0, 0, 600, 4, 4);"#).unwrap_err();
    assert!(err.contains("coordinate is outside"), "{err}");
    let err = run_cells(r#"fill_tube([[500, 0, 0], [530, 0, 0]], 1, #{radius: 1});"#).unwrap_err();
    assert!(err.contains("coordinate is outside"), "{err}");
    let err = run_cells(r#"fill_tube([[0, 0, 0], [5, 0, 0]], "red");"#).unwrap_err();
    assert!(err.contains("palette must be an index"), "{err}");
    let err = run_cells(r#"smooth(0, 0, 0, 4, 4, 4, #{iterations: 9});"#).unwrap_err();
    assert!(err.contains("iterations must be 1..=8"), "{err}");
    // A tube with no radius anywhere names the point.
    let err = run_cells(r#"fill_tube([[0, 0, 0], [5, 0, 0]], 1);"#).unwrap_err();
    assert!(err.contains("point 0 has no radius"), "{err}");
}

#[test]
fn math_and_array_helpers() {
    run_cells(
        r#"
        fn half_turn() { PI() }
        if abs(PI - 3.141592653589793) > 1e-12 { throw "PI"; }
        if half_turn() != PI { throw "PI()"; }
        if sqrt(16) != 4.0 || sqrt(2.25) != 1.5 { throw "sqrt"; }
        if pow(2, 10) != 1024 || type_of(pow(2, 10)) != "i64" { throw "pow int"; }
        if pow(4.0, 0.5) != 2.0 || pow(2, -1) != 0.5 { throw "pow float"; }
        if min(3, 5) != 3 || max(3, 5.5) != 5.5 || min(-1.5, 2) != -1.5 { throw "min max"; }
        if clamp(12, 0, 10) != 10 || clamp(-0.5, 0.0, 1.0) != 0.0 { throw "clamp"; }
        if ceil(1.2) != 2.0 || round(2.5) != 3.0 || round(-1.4) != -1.0 || ceil(3) != 3 { throw "ceil round"; }
        if abs(tan(PI / 4.0) - 1.0) > 1e-12 { throw "tan"; }
        if to_float(2.5) != 2.5 || to_int(7) != 7 || to_int(round(2.6)) != 3 { throw "to"; }
        if sin(0) != 0.0 || floor(4) != 4 { throw "int forms"; }

        let list = [];
        list.push(1);
        list.push([2, 3]);
        list.insert(0, 0);
        list.insert(-1, 9);
        if list.len() != 4 || list[0] != 0 || list[2] != 9 { throw "push insert"; }
        if list.pop()[1] != 3 { throw "pop"; }
        if list.remove(1) != 1 || list.len() != 2 { throw "remove"; }
        if list.remove(5) != () || [].pop() != () { throw "out of range"; }
        set(0, 0, 0, 1);
        "#,
    )
    .unwrap();
    let err = run_cells(r#"sqrt(-1.0);"#).unwrap_err();
    assert!(err.contains("negative"), "{err}");
    let err = run_cells(r#"clamp(1, 5, 0);"#).unwrap_err();
    assert!(err.contains("empty"), "{err}");
    let err = run_cells(r#"let a = []; for i in 0..5000 { a.push(i); }"#).unwrap_err();
    assert!(err.contains("at most 4096 entries"), "{err}");
    // Arrays inside arrays count toward the same limit.
    let err = run_cells(
        r#"let b = []; for i in 0..4000 { b.push(i); } let a = []; for i in 0..10 { a.push(b); }"#,
    )
    .unwrap_err();
    assert!(err.contains("too large"), "{err}");
}

#[test]
fn the_hollow_log_is_hollow_snapped_and_rooted() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/hollow_log.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let log = cells_of(&session);
    session.regenerate_script(0).unwrap();
    assert_eq!(cells_of(&session), log, "the same seed builds the same log");
    // Hollow along the axis near the root end, solid wall above it.
    assert!(!log.contains_key(&(6, 0, 5)));
    assert!((8..=10).any(|z| log.contains_key(&(6, 0, z))));
    // The break recolours the new face with sapwood and leaves splinter tips.
    for slot in [3, 4, 5, 7] {
        assert!(log.values().any(|&s| s == slot), "slot {slot} is missing");
    }
    // Roots reach behind the root end and down to the ground.
    assert!(log.keys().any(|c| c.0 < -1 && (0..=1).contains(&c.2)));
    assert!(log.values().any(|&s| s == 8), "the ground is missing");
}

#[test]
fn the_fallen_tree_has_a_root_flare_a_forked_limb_and_lumps() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/fallen_tree.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let tree = cells_of(&session);
    session.regenerate_script(0).unwrap();
    assert_eq!(
        cells_of(&session),
        tree,
        "the same seed builds the same tree"
    );
    // The trunk is wider than tall and flattened where it rests.
    let slice: Vec<_> = tree
        .keys()
        .filter(|c| c.0 == 20 && c.2 >= 0 && tree[c] != 8)
        .collect();
    let wide = slice.iter().map(|c| c.1).max().unwrap() - slice.iter().map(|c| c.1).min().unwrap();
    let tall = slice.iter().map(|c| c.2).max().unwrap() - slice.iter().map(|c| c.2).min().unwrap();
    assert!(wide > tall, "wide {wide}, tall {tall}");
    // Root bark flares out past the trunk at the root end.
    let roots: Vec<_> = tree
        .iter()
        .filter(|(_, s)| **s == 5)
        .map(|(c, _)| *c)
        .collect();
    assert!(roots.iter().any(|c| c.1.abs() > 14) && roots.iter().any(|c| c.2 > 18));
    // The limb rises well above the trunk and splits into two tips.
    let high: Vec<_> = tree.keys().filter(|c| c.2 > 28).collect();
    assert!(!high.is_empty(), "no limb above the trunk");
    let far: Vec<_> = tree.keys().filter(|c| c.1 > 22 && c.2 > 12).collect();
    assert!(!far.is_empty(), "no second fork branch");
    assert!(tree.values().any(|&s| s == 8), "the ground is missing");
}

const RIGGED: &str = r#"
fill_box(0, 0, 0, 3, 3, 3, 1);
object("arm");
fill_box(4, 0, 0, 6, 1, 1, 2);
set_palette(#{index: 2, r: 10, g: 20, b: 30, a: 128, motion: "sway", motion_amount: 2.0});
set_palette([#{index: 3, r: 1, g: 2, b: 3}]);
if !has_bone("root") {
    add_bone("root", [0.0, 0.0, 0.0]);
    add_bone("arm", [4.0, 0.5, 0.5], #{parent: "root"});
    add_bone("spare", [1, 1, 1]);
    remove_bone("spare");
}
set_bone("arm", #{rotation: [0.0, 0.0, 0.0, 1.0]});
bind_object("arm", "arm");
if !has_object("arm_l") {
    mirror_object("arm", "x", "arm_l", #{mirrorBones: true});
}
set_object("arm_l", #{hidden: true, position: [0, 0, 2]});
set_pose(#{arm: [0.0, 0.0, 0.3826834, 0.9238795]});
if !has_clip("wave") {
    add_clip("wave");
    add_clip("spare");
    remove_clip("spare");
}
set_clip_keys("wave", "arm", [
    #{time: 0.0, rotation: [0.0, 0.0, 0.0, 1.0]},
    #{time: 1.0, rotation: [0.0, 0.0, 0.3826834, 0.9238795]}
]);
"#;

#[test]
fn object_bone_clip_and_palette_calls_run_after_the_voxels_as_one_step() {
    use serde_json::json;
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, RIGGED).unwrap();
    let before = session.objects.len();
    session.regenerate_script(0).unwrap();
    let after = |session: &Session| {
        (
            session.objects.len(),
            session.skeleton.bones.len(),
            session.clips.len(),
            session.pose.len(),
        )
    };
    let first = after(&session);
    assert_eq!(first, (before + 2, 3, 1, 1), "arm, arm_l, root/arm/mirror");
    let arm_l = session
        .objects
        .iter()
        .position(|o| o.name == "arm_l")
        .unwrap();
    assert!(session.objects[arm_l].hidden);
    assert_eq!(session.objects[arm_l].translation, (0, 0, 2));
    assert!(
        !session.objects[arm_l].cells.is_empty(),
        "mirrored after the voxels"
    );
    assert_eq!(session.palette[2], [10, 20, 30, 128]);
    assert_eq!(session.palette[3][..3], [1, 2, 3]);
    assert_eq!(session.clips[0].name, "wave");

    // The same calls through MCP build the same rig.
    let mut mcp = Session::new();
    mcp.replace_empty();
    let arm = mcp.add_object(Some("arm"), [0, 0, 0], None, false).unwrap();
    let calls = [
        ("rig.add", json!({"name": "root", "pivot": [0, 0, 0]})),
        (
            "rig.add",
            json!({"name": "arm", "pivot": [4, 0.5, 0.5], "parent": "root"}),
        ),
        ("rig.bind", json!({"index": arm, "bone": "arm"})),
        (
            "object.mirror",
            json!({"index": arm, "axis": "x", "name": "arm_l", "mirrorBones": true}),
        ),
    ];
    for (method, params) in calls {
        crate::remote::apply(&mut mcp, method, &params).unwrap();
    }
    let names = |s: &Session| -> Vec<(String, Option<String>)> {
        s.skeleton
            .bones
            .iter()
            .map(|b| (b.name.clone(), b.parent.clone()))
            .collect()
    };
    assert_eq!(names(&session), names(&mcp));

    // A rerun skips what exists and changes nothing, so it adds no undo
    // step; one undo removes the whole first run.
    session.regenerate_script(0).unwrap();
    assert_eq!(after(&session), first);
    session.undo();
    assert_eq!(after(&session), (before, 0, 0, 0));
}

#[test]
fn a_failing_document_call_names_its_line_and_leaves_the_document() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            "fill_box(0, 0, 0, 2, 2, 2, 1);\nbind_object(0, \"none\");",
        )
        .unwrap();
    let err = session.regenerate_script(0).unwrap_err();
    assert!(err.starts_with("line 2: bind_object:"), "{err}");
    assert!(
        session.objects[0].cells.is_empty(),
        "the voxels were put back"
    );
    assert_eq!(session.script_error, err);

    let err = run_cells(r#"remove_object("host");"#).unwrap_err();
    assert!(err.contains("cannot remove its own object"), "{err}");
    let err = run_cells(r#"set_object(0, #{hiden: true});"#).unwrap_err();
    assert!(err.contains("no option `hiden`"), "{err}");
}

#[test]
fn voxel_lists_read_and_write_like_get_and_set_voxels() {
    run_cells(
        r#"
        let n = set_voxels([[0, 0, 0, 1], [1, 0, 0, 2], #{x: 2, y: 0, z: 0, palette: 3}]);
        if n != 3 { throw "set_voxels count"; }
        set_voxels([[1, 0, 0, 0]]);
        let found = get_voxels(0, 0, 0, 5, 5, 5);
        if found.len() != 2 || found[1][0] != 2 || found[1][3] != 3 { throw "get_voxels"; }
        "#,
    )
    .unwrap();
    let err = run_cells(r#"set_voxels([[0, 0, 600, 1]]);"#).unwrap_err();
    assert!(err.contains("coordinate is outside"), "{err}");
}

#[test]
fn list_bones_and_clips_read_the_document_at_the_start() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .add_bone("hip", None, [1.0, 2.0, 3.0], None)
        .unwrap();
    session.add_clip("walk").unwrap();
    session
        .set_script(
            0,
            r#"
            let bones = list_bones();
            if bones.len() != 1 || bones[0].name != "hip" || bones[0].pivot[2] != 3.0 { throw "bones"; }
            if list_clips()[0].name != "walk" || !has_clip("walk") || has_bone("knee") { throw "clips"; }
            add_bone("knee", [1.0, 2.0, 0.0], #{parent: "hip"});
            if list_bones().len() != 1 { throw "a recorded call is not read back"; }
            set(0, 0, 0, 1);
            "#,
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.skeleton.bones.len(), 2);
}

#[test]
fn stone_tools_match_the_mcp_tools() {
    use serde_json::json;
    let script = run_cells(
        r#"
        fill_box(0, 0, 0, 23, 5, 15, 1);
        let cut = joints(0, 0, 0, 23, 5, 15, "columns", #{dirt: 2, depth: 2, seed: 5});
        if cut.removed == 0 { throw "no joints"; }
        let worn = weather(0, 0, 0, 23, 5, 15, #{chip: 0.3, broken: 3, noise: [4, 5], only: [1], seed: 2});
        if worn.chips == 0 { throw "no chips"; }
        moss(-1, -1, -1, 24, 6, 16, [6, 7], #{curtains: 0.5, only: [1, 3, 4, 5], seed: 1});
        algae(-1, -1, -1, 24, 6, 16, [8, 9], #{band: [0, 5]});
        let hung = vines(-1, -1, -1, 24, 6, 16, [10, 11], #{amount: 0.3, blooms: [12], facing: [0, -1, 0]});
        if hung.strands == 0 { throw "no vines"; }
        let wall = sweep([[0, 10, 0], [12, 16, 0, 20], [24, 12, 0, 0, 0.5]], #{width: 4, height: 8, radius: 1},
            [1, 3], #{curve: "smooth", pattern: "bands", period: 2, mode: "empty"});
        if wall.cells == 0 || wall.length < 20.0 { throw "no sweep"; }
        sweep([[0, -6, 0], [10, -6, 0]], [[-2, 0], [2, 0], [0, 3]], 4);
        sweep([[2, -6, 1], [8, -6, 1]], #{width: 2, height: 1}, 0);
        "#,
    )
    .unwrap();
    let mcp = mcp_cells(&[
        (
            "voxel.fill",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 5, "z1": 15, "palette": 1}),
        ),
        (
            "voxel.joints",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 5, "z1": 15, "pattern": "columns",
                "dirt": 2, "depth": 2, "seed": 5}),
        ),
        (
            "voxel.weather",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 5, "z1": 15, "chip": 0.3,
                "broken": 3, "noise": [4, 5], "only": [1], "seed": 2}),
        ),
        (
            "voxel.moss",
            json!({"x0": -1, "y0": -1, "z0": -1, "x1": 24, "y1": 6, "z1": 16, "palettes": [6, 7],
                "curtains": 0.5, "only": [1, 3, 4, 5], "seed": 1}),
        ),
        (
            "voxel.algae",
            json!({"x0": -1, "y0": -1, "z0": -1, "x1": 24, "y1": 6, "z1": 16, "palettes": [8, 9],
                "band": [0, 5]}),
        ),
        (
            "voxel.vines",
            json!({"x0": -1, "y0": -1, "z0": -1, "x1": 24, "y1": 6, "z1": 16, "palettes": [10, 11],
                "amount": 0.3, "blooms": [12], "facing": [0, -1, 0]}),
        ),
        (
            "voxel.sweep",
            json!({"points": [[0, 10, 0], [12, 16, 0, 20], [24, 12, 0, 0, 0.5]], "width": 4,
                "height": 8, "radius": 1, "palettes": [1, 3], "curve": "smooth",
                "pattern": "bands", "period": 2, "mode": "empty"}),
        ),
        (
            "voxel.sweep",
            json!({"points": [[0, -6, 0], [10, -6, 0]], "profile": [[-2, 0], [2, 0], [0, 3]], "palette": 4}),
        ),
        (
            "voxel.sweep",
            json!({"points": [[2, -6, 1], [8, -6, 1]], "width": 2, "height": 1, "palette": 0}),
        ),
    ]);
    assert_eq!(script, mcp);
    let err = run_cells(
        r#"fill_box(0, 0, 0, 4, 4, 4, 1); joints(0, 0, 0, 4, 4, 4, "columns", #{dept: 2});"#,
    )
    .unwrap_err();
    assert!(err.contains("no option `dept`"), "{err}");
}

#[test]
fn script_markers_match_set_marker_and_rerun_replaces_them() {
    use serde_json::json;
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            r#"
            let n = param_int("ledges", 2, 1, 4);
            fill_box(0, 0, 0, 7, 3, 3, 1);
            marker("ledge_top", 3.5, 1.5, 4);
            collision_box(0, 0, 0, 7, 3, 3);
            if n > 1 { collision_box("upper", 0, 0, 4, 3, 3, 5); }
            connector("east", [8, 2, 2], [1, 0, 0]);
            "#,
        )
        .unwrap();
    session
        .set_marker(
            0,
            "hand_placed",
            crate::state::MarkerKind::Point {
                at: [0.0, 0.0, 9.0],
            },
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let names: Vec<&str> = session.markers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["hand_placed", "ledge_top", "collision_1", "upper", "east"]
    );
    // The same markers through MCP, on a fresh document.
    let mut mcp = Session::new();
    mcp.replace_empty();
    for args in [
        json!({"name": "ledge_top", "point": [3.5, 1.5, 4]}),
        json!({"name": "collision_1", "kind": "box", "x0": 0, "y0": 0, "z0": 0, "x1": 7, "y1": 3, "z1": 3}),
        json!({"name": "upper", "kind": "box", "x0": 0, "y0": 0, "z0": 4, "x1": 3, "y1": 3, "z1": 5}),
        json!({"name": "east", "kind": "connector", "point": [8, 2, 2], "direction": [1, 0, 0]}),
    ] {
        crate::remote::apply(&mut mcp, "marker.set", &args).unwrap();
    }
    let kinds = |s: &Session| -> Vec<(String, crate::state::MarkerKind)> {
        s.markers
            .iter()
            .filter(|m| m.name != "hand_placed")
            .map(|m| (m.name.clone(), m.kind.clone()))
            .collect()
    };
    assert_eq!(kinds(&session), kinds(&mcp));
    // A rerun with one ledge drops `upper` and keeps the hand-placed marker.
    session.write_script_int(0, "ledges", 1);
    session.regenerate_script(0).unwrap();
    let names: Vec<&str> = session.markers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["hand_placed", "ledge_top", "collision_1", "east"]);
    let err = run_cells(r#"connector("c", [0, 0, 0], [0, 0, 0]);"#).unwrap_err();
    assert!(err.contains("direction"), "{err}");
}

#[test]
fn the_gorge_wall_is_jointed_overgrown_and_marked() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/gorge_wall.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let wall = cells_of(&session);
    // Joint dirt, broken stone, moss, algae, vines, and blooms are all there.
    for slot in [2, 3, 10, 13, 16, 19] {
        assert!(wall.values().any(|&s| s == slot), "slot {slot} is missing");
    }
    // Algae stays at the waterline (6).
    assert!(wall
        .iter()
        .all(|(c, &s)| !(13..=15).contains(&s) || c.2 <= 7));
    // Three points, four collision boxes, two connectors.
    assert_eq!(session.markers.len(), 9);
    assert!(session.markers.iter().all(|m| m.scripted));
    session.regenerate_script(0).unwrap();
    assert_eq!(
        cells_of(&session),
        wall,
        "the same seed builds the same wall"
    );
    assert_eq!(session.markers.len(), 9);
}

#[test]
fn exclude_keeps_a_walking_surface_in_scripts_as_in_mcp() {
    use serde_json::json;
    let script = run_cells(
        r#"
        fill_box(0, 0, 0, 23, 5, 15, 1);
        let top = [[0, 0, 15, 23, 5, 15], [0, 0, 16, 23, 5, 20]];
        weather(0, 0, 0, 23, 5, 15, #{chip: 0.5, pits: 0.4, broken: 3, exclude: top, seed: 2});
        moss(-1, -1, -1, 24, 6, 20, [6, 7], #{amount: 1.0, exclude: top, seed: 1});
        scatter(0, 0, 16, 23, 5, 20, [8], #{count: 20, exclude: [0, 0, 16, 23, 5, 20]});
        "#,
    )
    .unwrap();
    let top = json!([[0, 0, 15, 23, 5, 15], [0, 0, 16, 23, 5, 20]]);
    let mcp = mcp_cells(&[
        (
            "voxel.fill",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 5, "z1": 15, "palette": 1}),
        ),
        (
            "voxel.weather",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 5, "z1": 15, "chip": 0.5,
                "pits": 0.4, "broken": 3, "exclude": top, "seed": 2}),
        ),
        (
            "voxel.moss",
            json!({"x0": -1, "y0": -1, "z0": -1, "x1": 24, "y1": 6, "z1": 20,
                "palettes": [6, 7], "amount": 1.0, "exclude": top, "seed": 1}),
        ),
        (
            "voxel.scatter",
            json!({"x0": 0, "y0": 0, "z0": 16, "x1": 23, "y1": 5, "z1": 20, "palettes": [8],
                "count": 20, "exclude": [0, 0, 16, 23, 5, 20]}),
        ),
    ]);
    assert_eq!(script, mcp);
    // The top stays a flat slot-1 floor with nothing above it.
    for y in 0..=5 {
        for x in 0..=23 {
            assert_eq!(script.get(&(x, y, 15)), Some(&1), "({x}, {y}, 15)");
            assert!(!script.contains_key(&(x, y, 16)), "({x}, {y}, 16)");
        }
    }
    // Weathering and moss still reach the sides.
    assert!(script
        .values()
        .any(|slot| *slot == 3 || *slot == 6 || *slot == 7));
    let bad = run_cells(
        r#"fill_box(0, 0, 0, 3, 3, 3, 1); weather(0, 0, 0, 3, 3, 3, #{exclude: [1, 2]});"#,
    )
    .unwrap_err();
    assert!(bad.contains("exclude must be a box"), "{bad}");
}

#[test]
fn column_relief_matches_the_mcp_tool() {
    use serde_json::json;
    let script = run_cells(
        r#"
        fill_box(0, 0, 0, 47, 7, 20, 1);
        let r = column_relief(-2, -8, 0, 49, 9, 26, "-y", 1,
            #{width: [5, 9], out: [1, 4], top: [-5, 4], depth: 3, exclude: [10, -8, 0, 15, 9, 26], seed: 3});
        if r.columns < 5 || r.crown != 20 { throw "columns " + r.columns; }
        column_relief(-2, -2, 0, 3, 9, 26, [-1, 0, 0], 1, #{lower: false});
        "#,
    )
    .unwrap();
    let mcp = mcp_cells(&[
        (
            "voxel.fill",
            json!({"x0": 0, "y0": 0, "z0": 0, "x1": 47, "y1": 7, "z1": 20, "palette": 1}),
        ),
        (
            "voxel.column_relief",
            json!({"x0": -2, "y0": -8, "z0": 0, "x1": 49, "y1": 9, "z1": 26, "facing": [0, -1, 0],
                "palette": 1, "width": [5, 9], "out": [1, 4], "top": [-5, 4], "depth": 3,
                "exclude": [10, -8, 0, 15, 9, 26], "seed": 3}),
        ),
        (
            "voxel.column_relief",
            json!({"x0": -2, "y0": -2, "z0": 0, "x1": 3, "y1": 9, "z1": 26, "facing": "-x",
                "palette": 1, "lower": false}),
        ),
    ]);
    assert_eq!(script, mcp);
    let bad =
        run_cells(r#"fill_box(0, 0, 0, 3, 3, 3, 1); column_relief(0, 0, 0, 3, 3, 3, "+z", 1);"#)
            .unwrap_err();
    assert!(bad.contains("facing"), "{bad}");
}

#[test]
fn strings_join_numbers_and_bools() {
    run_cells(
        r#"
        let n = 4;
        let names = ["ledge_" + n, n + "_ledge", "w" + 2.5, "on_" + true, false + "!"];
        let want = ["ledge_4", "4_ledge", "w2.5", "on_true", "false!"];
        for i in 0..names.len() {
            if names[i] != want[i] { throw "got " + names[i] + " for " + want[i]; }
        }
        if `ledge_${n}_${1.5}` != "ledge_4_1.5" { throw "interpolation"; }
        if to_string(7) != "7" || 7.to_string() != "7" || to_string(1.0) != "1.0" { throw "to_string"; }
        if true.to_string() != "true" || "ab".to_string() != "ab" { throw "to_string"; }
        let s = "box_"; s += 3; s += "_"; s += 0.5;
        if s != "box_3_0.5" || s.len() != 9 { throw "+= " + s; }
        marker("ledge_" + n, 0.0, 0.0, 1.0);
        set(0, 0, 0, 1);
        "#,
    )
    .unwrap();
}

#[test]
fn script_loft_deform_and_brush_match_the_mcp_tools() {
    use serde_json::json;
    let script = run_cells(
        r#"
        let trunk = loft([[0, 0, 0, 7, 5], #{x: 18, y: 3, z: 1, rx: 5, flatTop: -0.3}, [36, 0, 3, 3, 3, 20]], [2, 3],
            #{curve: "smooth", noise: 0.2, frequency: 5, seed: 1, pattern: "rings", period: 3});
        if trunk.cells == 0 || trunk.length < 30.0 { throw "no loft"; }
        let bent = deform("bend", #{axis: "x", angle: 30, direction: "+z", origin: 18});
        if bent.removed == 0 { throw "no bend"; }
        deform(0, -10, -10, 12, 10, 10, "displace", #{amplitude: 2, scale: 5, bias: 0.5, seed: 3});
        deform("taper", #{axis: "x", start: 1.0, end: 0.7});
        let dabs = brush("inflate", [[4, 0, 6], [12, 0, 6, 4]], #{strength: 0.8});
        if dabs.dabs < 2 { throw "no dabs"; }
        brush("smooth", [[8, 0, 6]]);
        "#,
    )
    .unwrap();
    let mcp = mcp_cells(&[
        (
            "voxel.loft",
            json!({"points": [[0, 0, 0, 7, 5], {"x": 18, "y": 3, "z": 1, "rx": 5, "flatTop": -0.3},
                [36, 0, 3, 3, 3, 20]], "palettes": [2, 3], "curve": "smooth", "noise": 0.2,
                "frequency": 5, "seed": 1, "pattern": "rings", "period": 3}),
        ),
        (
            "voxel.deform",
            json!({"op": "bend", "axis": "x", "angle": 30, "direction": "+z", "origin": 18}),
        ),
        (
            "voxel.deform",
            json!({"op": "displace", "x0": 0, "y0": -10, "z0": -10, "x1": 12, "y1": 10, "z1": 10,
                "amplitude": 2, "scale": 5, "bias": 0.5, "seed": 3}),
        ),
        (
            "voxel.deform",
            json!({"op": "taper", "axis": "x", "start": 1.0, "end": 0.7}),
        ),
        (
            "voxel.brush",
            json!({"op": "inflate", "points": [[4, 0, 6], [12, 0, 6, 4]], "strength": 0.8}),
        ),
        (
            "voxel.brush",
            json!({"op": "smooth", "points": [[8, 0, 6]]}),
        ),
    ]);
    assert_eq!(script, mcp);
    let err = run_cells(r#"loft([[0, 0, 0, 2], [4, 0, 0, 2]], 1, #{radios: 2});"#).unwrap_err();
    assert!(err.contains("no option `radios`"), "{err}");
}

const CUBE_MESH: &str = r#"
    let positions = [];
    for z in 0..2 { for y in 0..2 { for x in 0..2 { positions.push([x, y, z]); } } }
    let triangles = [[0, 2, 3], [0, 3, 1], [4, 5, 7], [4, 7, 6], [0, 1, 5], [0, 5, 4],
        [2, 6, 7], [2, 7, 3], [0, 4, 6], [0, 6, 2], [1, 3, 7], [1, 7, 5]];
"#;

#[test]
fn voxelize_builds_what_import_mesh_builds() {
    use serde_json::json;
    let script = run_cells(&format!(
        "{CUBE_MESH}
        let done = voxelize(#{{positions: positions, triangles: triangles}},
            #{{size: 6, palette: 5, position: [2, 0, 0], fill: \"shell\"}});
        if done.cells != 216 - 64 || done.bounds.size[0] != 6 {{ throw `bad ${{done}}`; }}
        voxelize(#{{positions: positions, triangles: triangles}}, #{{size: 4, palette: 6, mode: \"paint\"}});"
    ))
    .unwrap();
    let cube = format!(
        "{}/tests/fixtures/mesh/cube.obj",
        env!("CARGO_MANIFEST_DIR")
    );
    let mcp = mcp_cells(&[
        (
            "mesh.import",
            json!({"path": cube, "size": 6, "palette": 5, "position": [2, 0, 0], "fill": "shell"}),
        ),
        (
            "mesh.import",
            json!({"path": cube, "size": 4, "palette": 6, "mode": "paint"}),
        ),
    ]);
    assert_eq!(script, mcp);
    let err = run_cells(&format!(
        "{CUBE_MESH} voxelize(#{{positions: positions, triangles: [[0, 1, 9]]}});"
    ))
    .unwrap_err();
    assert!(err.contains("index positions"), "{err}");
    let err = run_cells(&format!(
        "{CUBE_MESH} voxelize(#{{positions: positions, triangles: triangles}}, #{{path: \"x.glb\"}});"
    ))
    .unwrap_err();
    assert!(err.contains("no option `path`"), "{err}");
}

#[test]
fn voxelize_adds_palette_slots_after_the_run() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(
            0,
            &format!(
                "{CUBE_MESH}
                let colors = [];
                for i in 0..8 {{ colors.push([30, 160, 40]); }}
                let done = voxelize(#{{positions: positions, triangles: triangles, colors: colors}},
                    #{{size: 4, tolerance: 0}});
                if done.newSlots.len() != 1 {{ throw `bad ${{done}}`; }}"
            ),
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.active().cells.len(), 64);
    assert!(session.active().cells.values().all(|&s| s == 255));
    assert_eq!(session.palette[255], [30, 160, 40, 255]);
}

#[test]
fn the_faceted_boulder_example_voxelizes_and_weathers() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/faceted_boulder.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.active().cells;
    assert!(cells.len() > 3000, "{}", cells.len());
    for slot in [1, 2, 3] {
        assert!(cells.values().any(|&s| s == slot), "slot {slot}");
    }
    if let Ok(dir) = std::env::var("FROXEL_EXAMPLE_OUT") {
        session
            .write_vox(&std::path::Path::new(&dir).join("faceted_boulder.vox"))
            .unwrap();
    }
}

#[test]
fn surface_tools_match_the_mcp_tools() {
    use serde_json::json;
    const TOOLS: &str = r#"
        let trunk = [[0, 0, 6, 5], [30, 2, 6, 4]];
        fill_tube(trunk, 1, #{curve: "smooth"});
        let cut = bark(-6, -8, 0, 36, 8, 12, [1, 2, 3], #{points: trunk, curve: "smooth", dirt: 4, seed: 2});
        if cut.plates == 0 || cut.removed == 0 { throw "no bark"; }
        let tint = cavity_tint(-6, -8, 0, 36, 8, 12, #{lighten: true});
        if tint.darkened == 0 || tint.shades.len() == 0 { throw "no shades"; }
        cavity_tint(-6, -8, 0, 36, 8, 12, #{pairs: [[2, 5]], dark: 0.2});
        let grown = moss(-6, -8, 0, 36, 8, 14, [6, 7], #{style: "cushions", amount: 0.6, seed: 3});
        if grown.cushions == 0 { throw "no cushions"; }
    "#;
    let mut script = Session::new();
    script.replace_empty();
    script.set_script(0, TOOLS).unwrap();
    script.regenerate_script(0).unwrap();

    let mut mcp = Session::new();
    mcp.replace_empty();
    let area = json!({"x0": -6, "y0": -8, "z0": 0, "x1": 36, "y1": 8, "z1": 12});
    let with = |extra: serde_json::Value| {
        let mut args = area.clone();
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        args
    };
    let trunk = json!([[0, 0, 6, 5], [30, 2, 6, 4]]);
    for (method, params) in [
        (
            "voxel.tube",
            json!({"points": trunk, "palette": 1, "curve": "smooth"}),
        ),
        (
            "voxel.bark",
            with(
                json!({"points": trunk, "curve": "smooth", "palettes": [1, 2, 3], "dirt": 4, "seed": 2}),
            ),
        ),
        ("voxel.cavity_tint", with(json!({"lighten": true}))),
        (
            "voxel.cavity_tint",
            with(json!({"pairs": [[2, 5]], "dark": 0.2})),
        ),
        (
            "voxel.moss",
            with(
                json!({"z1": 14, "palettes": [6, 7], "style": "cushions", "amount": 0.6, "seed": 3}),
            ),
        ),
    ] {
        crate::remote::apply(&mut mcp, method, &params).unwrap();
    }
    assert_eq!(cells_of(&script), cells_of(&mcp));
    assert_eq!(
        script.palette, mcp.palette,
        "the same shades in the same slots"
    );
    assert_ne!(script.palette, Session::new().palette, "shades were made");
    let err = run_cells(
        r#"fill_box(0, 0, 0, 4, 4, 4, 1); bark(0, 0, 0, 4, 4, 4, [1], #{axis: "x", plate: 2});"#,
    )
    .unwrap_err();
    assert!(err.contains("no option `plate`"), "{err}");
    let err =
        run_cells(r#"fill_box(0, 0, 0, 4, 4, 4, 1); bark(0, 0, 0, 4, 4, 4, [1]);"#).unwrap_err();
    assert!(err.contains("axis or points"), "{err}");
}

#[test]
fn the_swept_cut_removes_what_the_same_sweep_covers() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/swept_cut.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cut = cells_of(&session);
    // The slanted side: the top row starts further in than the bottom row at
    // the same place, and the channel opens the top at the middle.
    let first_y = |z: i32| (0..24).find(|&y| cut.contains_key(&(20, y, z))).unwrap();
    assert!(first_y(14) > first_y(1), "{} {}", first_y(14), first_y(1));
    assert!(!cut.contains_key(&(20, 12, 15)));
    assert!(cut.contains_key(&(20, 12, 8)));
    assert!(cut.values().all(|&slot| slot == 1));
    let err = run_cells(
        r#"fill_box(0, 0, 0, 4, 4, 4, 1); sweep([[0, 0, 0], [4, 0, 0]], #{width: 2, height: 2}, 0, #{mode: "empty"});"#,
    )
    .unwrap_err();
    assert!(err.contains("mode empty"), "{err}");
}

#[test]
fn the_barked_trunk_has_plates_shaded_fissures_and_moss_cushions() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/barked_trunk.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let trunk = cells_of(&session);
    // Plate tones, fissure dirt, and moss are all there.
    for slot in [3, 4, 5, 6, 7] {
        assert!(trunk.values().any(|&s| s == slot), "slot {slot} is missing");
    }
    // cavity_tint made darker shades in free slots above the script's own.
    assert!(trunk.values().any(|&s| s > 9), "no shaded cells");
    session.regenerate_script(0).unwrap();
    assert_eq!(
        cells_of(&session),
        trunk,
        "the same seed builds the same trunk"
    );
}
