use super::*;

#[test]
fn headless_tool_call_edits_and_refuses_a_screenshot() {
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let reply = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "set_voxel", "arguments": {"x": 0, "y": 0, "z": 0, "palette": 9}}}),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false);
    assert_eq!(session.active().cells.get(&(0, 0, 0)), Some(&9));
    let reply = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "screenshot_viewport", "arguments": {}}}),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let reply = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "ui_inspect", "arguments": {}}}),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], true);
    let reply = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "ui_click", "arguments": {"x": 1, "y": 1}}}),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], true);
}

#[test]
fn headless_render_writes_a_png_and_leaves_the_camera() {
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    session.objects[0].cells.insert((0, 0, 0), 4);
    session.yaw = 1.25;
    let path = std::env::temp_dir().join("froxel-mcp-render.png");
    let _ = std::fs::remove_file(&path);
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let reply = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "render", "arguments": {
            "path": path.to_string_lossy(),
            "width": 2,
            "height": 2,
            "samples": 1,
            "bounces": 1,
            "denoise": false
        }}}),
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    assert!((session.yaw - 1.25).abs() < 1e-5);
    let _ = std::fs::remove_file(&path);

    let empty = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "render", "arguments": {
            "path": path.to_string_lossy(),
            "pitch": 120
        }}}),
    )
    .unwrap();
    assert_eq!(empty["result"]["isError"], true);
    let text = empty["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("pitch"), "{text}");
}

#[test]
fn set_palette_motion_exports_texcoord_1() {
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "set_voxel", "arguments": {"x": 0, "y": 0, "z": 0, "palette": 4}}}),
    );
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "set_voxel", "arguments": {"x": 0, "y": 1, "z": 0, "palette": 4}}}),
    );
    let painted = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "set_palette", "arguments": {
            "index": 4, "r": 40, "g": 160, "b": 60, "motion": "sway", "motion_amount": 1.5
        }}}),
    )
    .unwrap();
    assert_eq!(painted["result"]["isError"], false, "{painted}");
    let inspected = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "inspect_document", "arguments": {}}}),
    )
    .unwrap();
    let body: serde_json::Value =
        serde_json::from_str(inspected["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["motion"]["4"]["kind"], "sway");
    assert_eq!(body["motion"]["4"]["amount"], 1.5);
    let path = std::env::temp_dir().join("froxel-mcp-sway.glb");
    let _ = std::fs::remove_file(&path);
    let exported = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "export_mesh", "arguments": {
            "path": path.to_string_lossy(),
            "merge": true
        }}}),
    )
    .unwrap();
    assert_eq!(exported["result"]["isError"], false, "{exported}");
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes
        .windows(b"TEXCOORD_1".len())
        .any(|window| window == b"TEXCOORD_1"));
    let _ = std::fs::remove_file(&path);
    session.undo();
    assert_eq!(session.materials[4].motion, crate::state::Motion::None);
}

#[test]
fn set_script_params_regenerate_and_export() {
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let source = "let n = param_int(\"count\", 1, 1, 6);\nfor x in 0..n { set(x, 0, 0, 2); }\n";
    let stored = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "set_script", "arguments": {"source": source}}}),
    )
    .unwrap();
    assert_eq!(stored["result"]["isError"], false, "{stored}");
    let tuned = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "set_params", "arguments": {"values": {"count": 3}}}}),
    )
    .unwrap();
    assert_eq!(tuned["result"]["isError"], false, "{tuned}");
    let built = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "regenerate", "arguments": {}}}),
    )
    .unwrap();
    assert_eq!(built["result"]["isError"], false, "{built}");
    assert_eq!(session.objects[0].cells.len(), 3);
    let path = std::env::temp_dir().join("froxel-mcp-script.glb");
    let _ = std::fs::remove_file(&path);
    let exported = dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "export_mesh", "arguments": {"path": path.to_string_lossy(), "merge": true}}}),
    )
    .unwrap();
    assert_eq!(exported["result"]["isError"], false, "{exported}");
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"glTF"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn set_params_takes_a_point_list() {
    let mut state = Proto::default();
    let mut session = Session::new();
    session.replace_empty();
    dispatch(
        &mut state,
        &mut session,
        None,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    let source = "let p = param_vec3s(\"posts\", [], 0.0, 9.0, 2);\nfor q in p { set(to_int(q[0]), to_int(q[1]), to_int(q[2]), 2); }\n";
    let call = |state: &mut Proto, session: &mut Session, id: i64, name: &str, arguments: Value| {
        dispatch(
            state,
            session,
            None,
            &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}}),
        )
        .unwrap()
    };
    let stored = call(
        &mut state,
        &mut session,
        2,
        "set_script",
        json!({"source": source}),
    );
    assert_eq!(stored["result"]["isError"], false, "{stored}");
    let refused = call(
        &mut state,
        &mut session,
        3,
        "set_params",
        json!({"values": {"posts": [[1, 1, 1], [2, 2, 2], [3, 3, 3]]}}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(refused.to_string().contains("at most 2"), "{refused}");
    let tuned = call(
        &mut state,
        &mut session,
        4,
        "set_params",
        json!({"values": {"posts": [[1, 2, 3], [4, 5, 6]]}}),
    );
    assert_eq!(tuned["result"]["isError"], false, "{tuned}");
    let built = call(&mut state, &mut session, 5, "regenerate", json!({}));
    assert_eq!(built["result"]["isError"], false, "{built}");
    assert_eq!(session.objects[0].cells.get(&(4, 5, 6)), Some(&2));
}

struct Headless {
    proto: Proto,
    session: Session,
    id: i64,
}

impl Headless {
    fn new() -> Self {
        let mut proto = Proto::default();
        let mut session = Session::new();
        session.replace_empty();
        dispatch(
            &mut proto,
            &mut session,
            None,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        );
        Self {
            proto,
            session,
            id: 1,
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.id += 1;
        dispatch(
            &mut self.proto,
            &mut self.session,
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": self.id,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            }),
        )
        .unwrap()
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.call(name, arguments);
        assert_eq!(reply["result"]["isError"], false, "{name}: {reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

fn world_span(object: &crate::state::Volume) -> ([i32; 3], [i32; 3]) {
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    for (cell, slot) in &object.cells {
        if *slot == 0 {
            continue;
        }
        let world = [
            cell.0 + object.translation.0,
            cell.1 + object.translation.1,
            cell.2 + object.translation.2,
        ];
        for axis in 0..3 {
            min[axis] = min[axis].min(world[axis]);
            max[axis] = max[axis].max(world[axis]);
        }
    }
    (min, max)
}

#[test]
fn add_object_then_set_voxels_lists_names_and_positions() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    let added = head.ok(
        "add_object",
        json!({"name": "leg", "position": [2, 0, 0], "size": [1, 3, 1]}),
    );
    assert_eq!(added["index"], 1);
    assert_eq!(added["activeObject"], 1);
    head.ok("set_voxels", json!({"cells": [[0, 0, 0, 2], [0, 1, 0, 2]]}));
    let doc = head.ok("inspect_document", json!({}));
    let objects = doc["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0]["name"], "Object");
    assert_eq!(objects[0]["position"], json!([0, 0, 0]));
    assert_eq!(objects[1]["name"], "leg");
    assert_eq!(objects[1]["position"], json!([2, 0, 0]));
    assert_eq!(objects[1]["voxels"], 2);
    assert_eq!(head.session.objects[1].cells.get(&(0, 0, 0)), Some(&2));
    head.ok("undo", json!({}));
    assert_eq!(head.session.objects.len(), 2);
    assert!(head.session.objects[1].cells.is_empty());
    head.ok("undo", json!({}));
    assert_eq!(head.session.objects.len(), 1);
}

#[test]
fn set_object_moves_world_bounds_by_the_same_offset() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 1, "y": 0, "z": 2, "palette": 4}));
    let before = world_span(&head.session.objects[0]);
    head.ok("set_object", json!({"index": 0, "position": [5, 3, 1]}));
    let after = world_span(&head.session.objects[0]);
    assert_eq!(
        [
            after.0[0] - before.0[0],
            after.0[1] - before.0[1],
            after.0[2] - before.0[2]
        ],
        [5, 3, 1]
    );
    assert_eq!(
        [
            after.1[0] - before.1[0],
            after.1[1] - before.1[1],
            after.1[2] - before.1[2]
        ],
        [5, 3, 1]
    );
    assert_eq!(head.session.objects[0].cells.get(&(1, 0, 2)), Some(&4));
}

#[test]
fn remove_object_shifts_later_indices_and_refuses_the_last() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    head.ok("add_object", json!({"name": "leg", "position": [2, 0, 0]}));
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 2}));
    head.ok("add_object", json!({"name": "eye", "position": [0, 0, 3]}));
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 3}));
    head.ok("add_bone", json!({"name": "root", "pivot": [0, 0, 0]}));
    head.ok(
        "add_bone",
        json!({"name": "leg", "parent": "root", "pivot": [2, 0, 0]}),
    );
    head.ok(
        "add_bone",
        json!({"name": "eye", "parent": "root", "pivot": [0, 0, 3]}),
    );
    head.ok("bind_object", json!({"index": 0, "bone": "root"}));
    head.ok("bind_object", json!({"index": 1, "bone": "leg"}));
    head.ok("bind_object", json!({"index": 2, "bone": "eye"}));
    let removed = head.ok("remove_object", json!({"index": 1}));
    assert_eq!(removed["indicesShifted"], true);
    assert_eq!(removed["note"], "object indices after 1 decreased by 1");
    assert_eq!(head.session.objects.len(), 2);
    assert_eq!(head.session.objects[1].name, "eye");
    assert_eq!(head.session.objects[1].bone.as_deref(), Some("eye"));
    assert!(head
        .session
        .objects
        .iter()
        .all(|object| object.bone.as_deref() != Some("leg")));
    let bones = head.ok("list_bones", json!({}));
    let leg = bones["bones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bone| bone["name"] == "leg")
        .unwrap();
    assert_eq!(leg["objects"].as_array().unwrap().len(), 0);
    head.ok("remove_object", json!({"index": 0}));
    let refused = head.call("remove_object", json!({"index": 0}));
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let text = refused["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("last object"), "{text}");
    assert_eq!(head.session.objects.len(), 1);
    assert_eq!(head.session.objects[0].bone.as_deref(), Some("eye"));
}

#[test]
fn add_object_round_trips_names_and_positions() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    head.ok("add_object", json!({"name": "leg", "position": [3, 1, 0]}));
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 2}));
    let path = std::env::temp_dir().join("froxel-mcp-objects.vox");
    let _ = std::fs::remove_file(&path);
    head.ok("save_file", json!({"path": path.to_string_lossy()}));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(4).any(|chunk| chunk == b"nTRN"));
    assert!(bytes.windows(4).any(|chunk| chunk == b"nSHP"));
    assert!(bytes.windows(5).any(|chunk| chunk == b"_name"));
    assert!(bytes.windows(3).any(|chunk| chunk == b"leg"));
    head.ok("open_file", json!({"path": path.to_string_lossy()}));
    let doc = head.ok("inspect_document", json!({}));
    let objects = doc["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0]["name"], "Object");
    assert_eq!(objects[0]["position"], json!([0, 0, 0]));
    assert_eq!(objects[1]["name"], "leg");
    assert_eq!(objects[1]["position"], json!([3, 1, 0]));
    assert_eq!(world_span(&head.session.objects[1]).0, [3, 1, 0]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn worked_example_exports_a_glb_with_two_joints() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    head.ok("add_object", json!({"name": "leg", "position": [2, 0, 0]}));
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 0, "y1": 2, "z1": 0, "palette": 2}),
    );
    head.ok("add_bone", json!({"name": "root", "pivot": [0, 0, 0]}));
    head.ok(
        "add_bone",
        json!({"name": "leg", "parent": "root", "pivot": [2, 0, 0]}),
    );
    head.ok("bind_object", json!({"index": 0, "bone": "root"}));
    head.ok("bind_object", json!({"index": 1, "bone": "leg"}));
    let path = std::env::temp_dir().join("froxel-mcp-character.glb");
    let _ = std::fs::remove_file(&path);
    head.ok(
        "export_mesh",
        json!({"path": path.to_string_lossy(), "merge": true}),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"glTF"));
    assert!(bytes.windows(8).any(|chunk| chunk == b"JOINTS_0"));
    assert!(bytes.windows(9).any(|chunk| chunk == b"WEIGHTS_0"));
    assert!(bytes
        .windows(19)
        .any(|chunk| chunk == b"inverseBindMatrices"));
    assert!(bytes.windows(6).any(|chunk| chunk == b"\"root\""));
    assert!(bytes.windows(5).any(|chunk| chunk == b"\"leg\""));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn open_file_replies_with_a_summary_unless_detail_is_full() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    for index in 0..24 {
        head.ok(
            "add_object",
            json!({"name": format!("block{index}"), "position": [index * 2, 0, 0]}),
        );
        head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 2}));
    }
    let path = std::env::temp_dir().join("froxel-mcp-summary.vox");
    let _ = std::fs::remove_file(&path);
    head.ok("save_file", json!({"path": path.to_string_lossy()}));
    let short = head.ok("open_file", json!({"path": path.to_string_lossy()}));
    assert_eq!(short["objectCount"], 25);
    assert_eq!(short["voxels"], 25);
    assert_eq!(short["paletteSlots"], 2);
    assert_eq!(short["objectNames"].as_array().unwrap().len(), 20);
    assert_eq!(short["objectNames"][1], "block0");
    assert_eq!(short["moreObjects"], 5);
    assert_eq!(short["bounds"]["max"][0], 46);
    assert!(short.get("objects").is_none());
    assert!(short.get("palette").is_none());
    let full = head.ok(
        "open_file",
        json!({"path": path.to_string_lossy(), "detail": "full"}),
    );
    assert_eq!(full, head.ok("inspect_document", json!({})));
    assert_eq!(full["objects"].as_array().unwrap().len(), 25);
    let refused = head.call(
        "open_file",
        json!({"path": path.to_string_lossy(), "detail": "all"}),
    );
    assert_eq!(refused["result"]["isError"], true);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_file_counts_the_scene_and_reopen_keeps_the_corner() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    head.ok("add_object", json!({"name": "leg", "position": [2, 0, 0]}));
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 0, "y1": 2, "z1": 0, "palette": 2}),
    );
    let path = std::env::temp_dir().join("froxel-mcp-char.vox");
    let _ = std::fs::remove_file(&path);
    let saved = head.ok("save_file", json!({"path": path.to_string_lossy()}));
    assert_eq!(saved["voxels"], 4);
    assert_eq!(saved["objects"], 2);
    head.ok("open_file", json!({"path": path.to_string_lossy()}));
    let doc = head.ok("inspect_document", json!({}));
    let objects = doc["objects"].as_array().unwrap();
    assert_eq!(objects[1]["name"], "leg");
    assert_eq!(objects[1]["position"], json!([2, 0, 0]));
    assert_eq!(objects[1]["bounds"]["min"][1], 0);
    assert_eq!(objects[1]["bounds"]["max"][1], 2);
    let leg = &head.session.objects[1];
    let world_y: Vec<i32> = leg
        .cells
        .keys()
        .map(|cell| cell.1 + leg.translation.1)
        .collect();
    assert_eq!(world_y.iter().copied().min(), Some(0));
    assert_eq!(world_y.iter().copied().max(), Some(2));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn set_pose_is_undoable_and_render_pose_does_not_stick() {
    let mut head = Headless::new();
    head.ok(
        "add_bone",
        json!({"name": "root", "pivot": [0.0, 0.0, 0.0]}),
    );
    head.ok(
        "add_bone",
        json!({"name": "arm", "parent": "root", "pivot": [2.0, 0.0, 0.0]}),
    );
    let bind = head.session.skeleton.bones[1].rotation;
    head.ok(
        "set_pose",
        json!({"pose": {"arm": [0.0, 0.0, 0.70710678, 0.70710678]}}),
    );
    assert_eq!(head.session.skeleton.bones[1].rotation, bind);
    assert!(head.session.pose.contains_key("arm"));
    head.ok("undo", json!({}));
    assert!(head.session.pose.is_empty());
    assert_eq!(head.session.skeleton.bones[1].rotation, bind);
    head.ok(
        "set_pose",
        json!({"pose": {"root": [0.0, 0.0, 0.70710678, 0.70710678]}}),
    );
    let window = head.session.pose.clone();
    head.session.objects[0].cells.insert((0, 0, 0), 4);
    head.session.yaw = 1.25;
    let yaw = head.session.yaw;
    let path = std::env::temp_dir().join("froxel-pose-render.png");
    let _ = std::fs::remove_file(&path);
    head.ok(
        "render",
        json!({
            "path": path.to_string_lossy(),
            "width": 2,
            "height": 2,
            "samples": 1,
            "bounces": 0,
            "denoise": false,
            "pose": {"arm": [0.0, 0.0, 0.70710678, 0.70710678]}
        }),
    );
    assert_eq!(head.session.pose, window);
    assert!((head.session.yaw - yaw).abs() < 1e-6);
    assert!(!head.session.pose.contains_key("arm"));
    head.ok("clear_pose", json!({}));
    assert!(head.session.pose.is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_pose_entry_may_scale_and_a_negative_scale_is_refused() {
    let mut head = Headless::new();
    head.ok("add_bone", json!({"name": "lid", "pivot": [0.0, 0.0, 0.0]}));
    head.ok(
        "set_pose",
        json!({"pose": {"lid": {"scale": [1.0, 1.0, 0.0]}}}),
    );
    let lid = head.session.pose["lid"];
    assert_eq!(lid.scale, [1.0, 1.0, 0.0]);
    assert_eq!(lid.rotation, crate::rig::IDENTITY_ROTATION);
    let reply = head.call(
        "set_pose",
        json!({"pose": {"lid": {"scale": [1.0, -1.0, 1.0]}}}),
    );
    assert_eq!(reply["result"]["isError"], true);
    // An entry that changes nothing is dropped.
    head.ok(
        "set_pose",
        json!({"pose": {"lid": {"rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]}}}),
    );
    assert!(head.session.pose.is_empty());
}

#[test]
fn clips_save_in_vox_and_mirror_and_shapes_follow_the_box_rules() {
    let mut head = Headless::new();
    head.ok(
        "add_bone",
        json!({"name": "spine", "pivot": [0.0, 0.0, 0.0]}),
    );
    head.ok(
        "add_bone",
        json!({"name": "arm_l", "parent": "spine", "pivot": [1.0, 2.0, 0.5]}),
    );
    head.ok("bind_object", json!({"index": 0, "bone": "arm_l"}));
    head.session.objects[0].cells.insert((2, 0, 0), 3);
    head.ok("add_clip", json!({"name": "wave"}));
    head.ok(
        "set_clip_keys",
        json!({
            "name": "wave",
            "bone": "arm_l",
            "keys": [
                {"time": 0.0, "rotation": [0.0, 0.0, 0.0, 1.0], "translation": [1.0, 2.0, 0.0], "scale": [1.0, 1.0, 1.0]},
                {"time": 0.5, "rotation": [0.0, 0.0, 0.0, 1.0]}
            ]
        }),
    );
    let listed = head.ok("list_clips", json!({}));
    assert_eq!(listed["clips"][0]["name"], "wave");
    assert_eq!(listed["clips"][0]["bones"][0]["keys"][1]["time"], 0.5);
    let bytes = head.session.export_bytes().unwrap();
    assert!(bytes.windows(4).any(|window| window == b"MVAN"));
    let mut loaded = Session::new();
    loaded.import_bytes(&bytes).unwrap();
    assert_eq!(loaded.clips, head.session.clips);
    head.ok("remove_clip", json!({"name": "wave"}));
    assert!(head.session.clips.is_empty());

    let copied = head.ok(
        "mirror_object",
        json!({"index": 0, "axis": "x", "name": "arm", "mirrorBones": true}),
    );
    assert_eq!(copied["bones"][0], "arm_r");
    let copy = &head.session.objects[copied["index"].as_u64().unwrap() as usize];
    assert_eq!(copy.bone.as_deref(), Some("arm_r"));
    let world_x = copy
        .cells
        .iter()
        .find(|(_, slot)| **slot == 3)
        .map(|((x, _, _), _)| x + copy.translation.0)
        .unwrap();
    assert_eq!(world_x, -3);
    let bone = head
        .session
        .skeleton
        .bones
        .iter()
        .find(|bone| bone.name == "arm_r")
        .unwrap();
    assert!((bone.pivot[0] + 1.0).abs() < 1e-5, "{:?}", bone.pivot);
    assert!((bone.pivot[1] - 2.0).abs() < 1e-5);
    assert!((bone.pivot[2] - 0.5).abs() < 1e-5);

    head.ok("set_active_object", json!({"index": 0}));
    head.session.objects[0].cells.clear();
    let ball = head.ok(
        "fill_ellipsoid",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 4, "y1": 4, "z1": 4, "palette": 7}),
    );
    assert!(ball["changed"].as_i64().unwrap() > 0);
    assert!(head.session.objects[0].cells.contains_key(&(2, 2, 2)));
    assert!(!head.session.objects[0].cells.contains_key(&(0, 0, 0)));
    head.session.objects[0].cells.clear();
    let sharp = head.ok(
        "fill_rounded_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1, "palette": 4, "radius": 0}),
    );
    assert_eq!(sharp["changed"], 8);
    assert_eq!(head.session.objects[0].cells.len(), 8);
    head.session.objects[0].cells.clear();
    let dot = head.ok(
        "fill_ellipsoid",
        json!({"x0": 3, "y0": 3, "z0": 3, "x1": 3, "y1": 3, "z1": 3, "palette": 1}),
    );
    assert_eq!(dot["changed"], 1);
    assert!(head.session.objects[0].cells.contains_key(&(3, 3, 3)));
}

#[test]
fn every_export_tool_takes_a_bake() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 3}));
    let dir = std::env::temp_dir().join(format!("froxel-mcp-bake-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("one.vox");
    head.ok("save_file", json!({"path": source.to_string_lossy()}));
    let bake = json!({"preset": "old_pond", "distance": 8, "rays": 8});
    let glb = dir.join("one.glb");
    let reply = head.ok(
        "export_mesh",
        json!({"path": glb.to_string_lossy(), "colorAtlas": true, "bake": bake}),
    );
    assert_eq!(reply["bake"]["rays"], 8);
    let batch = dir.join("batch.glb");
    let report = head.ok(
        "export_models",
        json!({"models": [{
            "source": source.to_string_lossy(),
            "path": batch.to_string_lossy(),
            "colorAtlas": true,
            "bake": bake
        }]}),
    );
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(std::fs::read(&glb).unwrap(), std::fs::read(&batch).unwrap());
    // A file drawn back through preview.
    let png = dir.join("one.png");
    let drawn = head.ok(
        "preview",
        json!({"path": png.to_string_lossy(), "glb": glb.to_string_lossy(), "width": 32, "height": 32}),
    );
    assert_eq!(drawn["previewTriangles"], 12);
    assert!(png.is_file());
    // The counts of a preview name it, and a bad one fails.
    let counted = head.ok(
        "preview",
        json!({"path": png.to_string_lossy(), "width": 32, "height": 32,
               "export": {"colorAtlas": true, "bake": bake}}),
    );
    assert_eq!(counted["export"]["triangles"], 12);
    assert_eq!(counted["export"]["bake"]["rays"], 8);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_models_checks_structure_and_leaves_the_open_document() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 3}));
    let dir = std::env::temp_dir().join(format!("froxel-mcp-batch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("one.vox");
    head.ok("save_file", json!({"path": source.to_string_lossy()}));
    head.ok("set_voxel", json!({"x": 1, "y": 0, "z": 0, "palette": 3}));
    let from_file = dir.join("from-file.glb");
    let from_session = dir.join("from-session.glb");
    let report = head.ok(
        "export_models",
        json!({"models": [
            {"source": source.to_string_lossy(), "path": from_file.to_string_lossy()},
            {"path": from_session.to_string_lossy()}
        ]}),
    );
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(
        report["models"][0]["structure"]["nodes"][0]["name"],
        "voxels"
    );
    assert!(
        report["models"][0]["structure"]["meshes"][0]["primitives"][0]["vertices"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        report["models"][0]["structure"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{report}"
    );
    let file_quads = report["models"][0]["quads"].as_u64().unwrap();
    let session_quads = report["models"][1]["quads"].as_u64().unwrap();
    assert!(session_quads > file_quads, "{report}");
    assert_eq!(head.session.objects[0].cells.len(), 2);
    let mixed = head.ok(
        "export_models",
        json!({"models": [
            {"path": from_session.to_string_lossy()},
            {"path": dir.join("notes.txt").to_string_lossy()}
        ]}),
    );
    assert_eq!(mixed["ok"], false, "{mixed}");
    assert_eq!(mixed["failed"], 1, "{mixed}");
    assert_eq!(mixed["models"][0]["ok"], true, "{mixed}");
    assert!(
        mixed["models"][1]["error"]
            .as_str()
            .unwrap()
            .contains(".glb"),
        "{mixed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn batch_rejects_destination_aliases_before_writing() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x":0,"y":0,"z":0,"palette":3}));
    let dir = std::env::temp_dir().join(format!("froxel-duplicate-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    let path = dir.join("same.glb");
    std::fs::write(&path, b"preserve existing output").unwrap();
    let reply = head.call(
        "export_models",
        json!({"models":[
            {"path":path}, {"path":dir.join("nested/../same.glb")}
        ]}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(reply.to_string().contains("Duplicate"), "{reply}");
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve existing output");
    let reply = head.call(
        "export_models",
        json!({"models":[
            {"path":dir.join("new.glb")}, {"path":dir.join("NEW.glb")}
        ]}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(!dir.join("new.glb").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn batch_rejects_hard_linked_outputs_before_writing() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x":0,"y":0,"z":0,"palette":3}));
    let dir = std::env::temp_dir().join(format!("froxel-hardlink-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.glb");
    let second = dir.join("second.glb");
    let _ = std::fs::remove_file(&second);
    std::fs::write(&first, b"preserved").unwrap();
    std::fs::hard_link(&first, &second).unwrap();
    let reply = head.call(
        "export_models",
        json!({"models":[{"path":first},{"path":second}]}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(reply.to_string().contains("Duplicate"));
    assert_eq!(std::fs::read(first).unwrap(), b"preserved");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_instructions_point_to_hand_sculpting() {
    let mut head = Headless::new();
    let reply = dispatch(
        &mut head.proto,
        &mut head.session,
        None,
        &json!({"jsonrpc": "2.0", "id": 99, "method": "initialize", "params": {}}),
    )
    .unwrap();
    let instructions = reply["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("docs/hand-sculpting.md"));
}

/// A 3x3x3 block of slot 1 with a different slot at the centre of each face.
fn marked_block(head: &mut Headless) {
    let cells = &mut head.session.objects[0].cells;
    cells.clear();
    for x in 0..3 {
        for y in 0..3 {
            for z in 0..3 {
                cells.insert((x, y, z), 1);
            }
        }
    }
    for (cell, slot) in [
        ((1, 0, 1), 2),
        ((1, 2, 1), 3),
        ((0, 1, 1), 4),
        ((2, 1, 1), 5),
        ((1, 1, 2), 6),
        ((1, 1, 0), 7),
    ] {
        cells.insert(cell, slot);
    }
    for slot in 1..8u8 {
        head.session.palette[slot as usize] = [slot * 30, 200 - slot * 20, slot * 7, 255];
    }
    head.session.mesh_epoch += 1;
}

fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

#[test]
fn model_view_pixels_are_palette_entries_and_named_views_face_their_side() {
    let mut head = Headless::new();
    marked_block(&mut head);
    head.session.yaw = 1.25;
    let yaw = head.session.yaw;
    let dir = std::env::temp_dir().join(format!("froxel-model-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let names = ["front", "back", "right", "left", "top", "bottom"];
    let paths: Vec<_> = names
        .iter()
        .map(|name| dir.join(format!("{name}.png")))
        .collect();
    let views: Vec<Value> = names[1..]
        .iter()
        .zip(&paths[1..])
        .map(|(name, path)| json!({"view": name, "path": path}))
        .collect();
    let reply = head.ok(
        "model_view",
        json!({
            "path": paths[0],
            "view": "front",
            "width": 33,
            "height": 33,
            "background": "#102030",
            "views": views
        }),
    );
    assert_eq!(reply["orthographic"], true);
    assert_eq!(reply["views"].as_array().unwrap().len(), 5);
    assert!(reply["pixelsPerVoxel"].as_f64().unwrap() > 1.0);
    assert!((head.session.yaw - yaw).abs() < 1e-6);
    assert!(!head.session.orthographic);
    let palette = head.session.palette;
    for (path, slot) in paths.iter().zip([2u8, 3, 4, 5, 6, 7]) {
        let (w, h, rgba) = read_png(path);
        assert_eq!((w, h), (33, 33));
        let centre = ((16 * w + 16) * 4) as usize;
        assert_eq!(
            &rgba[centre..centre + 4],
            &palette[slot as usize],
            "{path:?}"
        );
        assert_eq!(&rgba[..4], &[0x10, 0x20, 0x30, 255], "{path:?}");
        for pixel in rgba.chunks(4) {
            assert!(
                pixel == [0x10, 0x20, 0x30, 255] || palette[1..8].iter().any(|p| p == pixel),
                "{path:?} has {pixel:?}"
            );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn model_view_refuses_a_view_with_yaw_and_a_bad_background() {
    let mut head = Headless::new();
    marked_block(&mut head);
    let path = std::env::temp_dir().join("froxel-model-view-refused.png");
    let reply = head.call(
        "model_view",
        json!({"path": path, "view": "top", "yaw": 10.0}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let reply = head.call("model_view", json!({"path": path, "background": "#12"}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let reply = head.call("model_view", json!({"path": path, "view": "side"}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(!path.exists());
}

/// A bar 60 cells along x, 4 across, 4 tall: a long thin model.
fn long_bar(head: &mut Headless) {
    let cells = &mut head.session.objects[0].cells;
    cells.clear();
    for x in 0..60 {
        for y in 0..4 {
            for z in 0..4 {
                cells.insert((x, y, z), 1);
            }
        }
    }
}

/// Bounding box `(x0, y0, x1, y1)` of the pixels that are not transparent.
fn solid_box(width: u32, rgba: &[u8]) -> (u32, u32, u32, u32) {
    let mut found = (u32::MAX, u32::MAX, 0, 0);
    for (index, pixel) in rgba.chunks(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let (x, y) = (index as u32 % width, index as u32 / width);
        found = (
            found.0.min(x),
            found.1.min(y),
            found.2.max(x),
            found.3.max(y),
        );
    }
    found
}

#[test]
fn a_long_model_fills_the_frame_at_any_angle() {
    let mut head = Headless::new();
    long_bar(&mut head);
    let path = std::env::temp_dir().join(format!("froxel-fill-{}.png", std::process::id()));
    for (yaw, pitch, orthographic) in [
        (-35.0, 30.0, true),
        (-35.0, 30.0, false),
        (0.0, 10.0, false),
        (-90.0, 0.0, true),
        (60.0, 80.0, false),
    ] {
        head.ok(
            "model_view",
            json!({"path": path, "width": 400, "height": 200, "yaw": yaw, "pitch": pitch, "orthographic": orthographic}),
        );
        let (w, h, rgba) = read_png(&path);
        let (x0, y0, x1, y1) = solid_box(w, &rgba);
        let wide = (x1 - x0 + 1) as f32 / w as f32;
        let tall = (y1 - y0 + 1) as f32 / h as f32;
        assert!(
            wide.max(tall) > 0.84 && wide <= 0.95 && tall <= 0.95,
            "yaw {yaw} pitch {pitch} ortho {orthographic}: {wide} x {tall}"
        );
        assert!(x0 > 0 && y0 > 0 && x1 < w - 1 && y1 < h - 1);
        let (left, right) = (x0 as i32, (w - 1 - x1) as i32);
        let (top, bottom) = (y0 as i32, (h - 1 - y1) as i32);
        assert!(
            (left - right).abs() <= 3 && (top - bottom).abs() <= 3,
            "yaw {yaw} pitch {pitch} ortho {orthographic}: off centre {left} {right} {top} {bottom}"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn zoom_margin_and_pixels_per_voxel_scale_the_flat_view() {
    let mut head = Headless::new();
    long_bar(&mut head);
    let path = std::env::temp_dir().join(format!("froxel-zoom-{}.png", std::process::id()));
    let base = head.ok(
        "model_view",
        json!({"path": path, "view": "front", "width": 300, "height": 300}),
    );
    let zoomed = head.ok(
        "model_view",
        json!({"path": path, "view": "front", "width": 300, "height": 300, "zoom": 2.0}),
    );
    let ppv = base["pixelsPerVoxel"].as_f64().unwrap();
    assert!((ppv - 300.0 * 0.9 / 60.0).abs() < 0.1, "{base}");
    assert!((zoomed["pixelsPerVoxel"].as_f64().unwrap() - 2.0 * ppv).abs() < 1e-3);
    let tight = head.ok(
        "model_view",
        json!({"path": path, "view": "front", "width": 300, "height": 300, "margin": 0.0}),
    );
    assert!(
        (tight["pixelsPerVoxel"].as_f64().unwrap() - 5.0).abs() < 1e-3,
        "{tight}"
    );

    // One side given: the other follows the model's shape.
    let wide = head.ok(
        "model_view",
        json!({"path": path, "view": "front", "width": 600}),
    );
    assert_eq!(wide["width"], 600);
    assert_eq!(wide["height"], 40, "{wide}");

    // pixelsPerVoxel alone sizes the picture around the model.
    let sized = head.ok(
        "model_view",
        json!({"path": path, "view": "front", "pixelsPerVoxel": 3.0}),
    );
    assert!((sized["pixelsPerVoxel"].as_f64().unwrap() - 3.0).abs() < 1e-4);
    assert_eq!(sized["width"], 200);
    assert_eq!(read_png(&path).0, 200);
    let refused = head.call(
        "model_view",
        json!({"path": path, "pixelsPerVoxel": 3.0, "zoom": 2.0}),
    );
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let refused = head.call("model_view", json!({"path": path, "pixelsPerVoxel": 200.0}));
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_target_frames_one_object_or_a_box() {
    let mut head = Headless::new();
    long_bar(&mut head);
    head.ok(
        "add_object",
        json!({"name": "pebble", "position": [100, 0, 0]}),
    );
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1, "palette": 2}),
    );
    let path = std::env::temp_dir().join(format!("froxel-target-{}.png", std::process::id()));
    let reply = head.ok(
        "model_view",
        json!({"path": path, "target": 1, "width": 64, "height": 64}),
    );
    assert_eq!(
        reply["framed"],
        json!({"min": [100.0, 0.0, 0.0], "max": [102.0, 2.0, 2.0]})
    );
    let active = head.ok(
        "model_view",
        json!({"path": path, "target": "active", "width": 64, "height": 64}),
    );
    assert_eq!(active["framed"], reply["framed"]);
    let all = head.ok(
        "model_view",
        json!({"path": path, "width": 64, "height": 64}),
    );
    assert_eq!(
        all["framed"],
        json!({"min": [0.0, 0.0, 0.0], "max": [102.0, 4.0, 4.0]})
    );
    let boxed = head.ok(
        "render",
        json!({"path": path, "target": {"min": [0, 0, 0], "max": [9, 3, 3]}, "width": 8, "height": 8, "samples": 1, "denoise": false}),
    );
    assert_eq!(
        boxed["framed"],
        json!({"min": [0.0, 0.0, 0.0], "max": [10.0, 4.0, 4.0]})
    );
    for target in [
        json!(7),
        json!("most"),
        json!({"min": [0, 0, 0]}),
        json!({"min": [2, 0, 0], "max": [1, 1, 1]}),
    ] {
        let reply = head.call("model_view", json!({"path": path, "target": target}));
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn render_takes_large_and_wide_sizes_and_refuses_unknown_arguments() {
    let mut head = Headless::new();
    long_bar(&mut head);
    let path = std::env::temp_dir().join(format!("froxel-large-{}.png", std::process::id()));
    let reply = head.ok(
        "render",
        json!({"path": path, "width": 3000, "height": 2, "samples": 1, "bounces": 0, "denoise": false}),
    );
    assert_eq!(reply["width"], 3000);
    let reply = head.call(
        "render",
        json!({"path": path, "width": 4096, "height": 4096}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let reply = head.call("render", json!({"path": path, "width": 5000}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");

    let reply = head.call("render", json!({"path": path, "zom": 2.0}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("`zom`") && text.contains("zoom"), "{text}");
    let reply = head.call(
        "model_view",
        json!({"path": path, "views": [{"view": "top", "path": path, "zoom": 2}]}),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let reply = head.call("get_voxel", json!({"x": 0, "y": 0, "z": 0, "w": 1}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn preview_writes_a_lit_framed_png_and_keeps_the_camera() {
    let mut head = Headless::new();
    marked_block(&mut head);
    let yaw = head.session.yaw;
    let dir = std::env::temp_dir().join(format!("froxel-preview-{}", std::process::id()));
    let path = dir.join("nested").join("block.png");
    let reply = head.ok(
        "preview",
        json!({
            "path": path,
            "width": 96,
            "height": 64,
            "view": "front",
            "orthographic": true,
            "object": 0,
            "background": "#102030"
        }),
    );
    assert_eq!(reply["orthographic"], true);
    assert_eq!(reply["object"]["index"], 0);
    assert!(reply["previewTriangles"].as_u64().unwrap() > 0);
    assert!(
        reply.get("triangles").is_none(),
        "only named counts: {reply}"
    );
    assert!(reply["pixelsPerVoxel"].as_f64().unwrap() > 10.0);
    assert!((head.session.yaw - yaw).abs() < 1e-6);
    let (w, h, rgba) = read_png(&path);
    assert_eq!((w, h), (96, 64));
    assert_eq!(&rgba[..4], &[0x10, 0x20, 0x30, 255]);
    let centre = ((32 * w + 48) * 4) as usize;
    let flat = head.session.palette[2];
    assert_ne!(&rgba[centre..centre + 4], &[0x10, 0x20, 0x30, 255]);
    assert_ne!(
        &rgba[centre..centre + 3],
        &flat[..3],
        "the face is lit, not flat"
    );
    let reply = head.call("preview", json!({"path": path, "object": "nope"}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let reply = head.call("preview", json!({"path": path, "isolate": true}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn set_view_frames_the_model_and_hides_the_bones() {
    let mut head = Headless::new();
    marked_block(&mut head);
    let reply = head.ok(
        "set_view",
        json!({"view": "top", "orthographic": true, "bones": false, "distance": 2.0}),
    );
    assert_eq!(reply["pitch"].as_f64().unwrap().round(), 90.0);
    assert_eq!(reply["target"], json!([1.5, 1.5, 1.5]));
    assert!(head.session.orthographic);
    assert!(!head.session.view_bones);
    assert!((head.session.distance - 2.0 * 7.2).abs() < 1e-4);
    head.ok("set_view", json!({"yaw": 180.0, "frame": false}));
    assert!((head.session.yaw - std::f32::consts::PI).abs() < 1e-5);
    assert!((head.session.pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    let reply = head.call("set_view", json!({"frame": false, "distance": 2.0}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
}

#[test]
fn every_tool_declares_all_four_hints() {
    let tools = tools();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            map_tool(name, &json!({})).is_ok() || name.starts_with("screenshot"),
            "{name}"
        );
        for hint in [
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        ] {
            assert!(tool["annotations"][hint].is_boolean(), "{name} {hint}");
        }
        if tool["annotations"]["readOnlyHint"] == true {
            assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
        }
    }
    assert_eq!(
        tools.iter().find(|t| t["name"] == "get_voxels").unwrap()["annotations"]["readOnlyHint"],
        true
    );
    assert_eq!(
        tools.iter().find(|t| t["name"] == "save_file").unwrap()["annotations"]["destructiveHint"],
        true
    );
}

fn error_text(reply: &Value) -> String {
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn fill_modes_keep_or_recolour_existing_cells() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 1, "y": 0, "z": 0, "palette": 9}));
    let corners = json!({"x0": 0, "y0": 0, "z0": 0, "x1": 2, "y1": 0, "z1": 0});
    let mut empty = corners.clone();
    empty["palette"] = json!(3);
    empty["mode"] = json!("empty");
    assert_eq!(head.ok("fill_box", empty)["changed"], 2);
    assert_eq!(head.session.active().cells.get(&(1, 0, 0)), Some(&9));
    head.session.objects[0].cells.remove(&(2, 0, 0));
    let mut paint = corners.clone();
    paint["palette"] = json!(4);
    paint["mode"] = json!("paint");
    assert_eq!(head.ok("fill_box", paint)["changed"], 2);
    assert_eq!(head.session.active().cells.get(&(1, 0, 0)), Some(&4));
    assert!(!head.session.active().cells.contains_key(&(2, 0, 0)));
    let mut bad = corners;
    bad["palette"] = json!(4);
    bad["mode"] = json!("add");
    assert!(error_text(&head.call("fill_ellipsoid", bad)).contains("mode must be"));
}

#[test]
fn fill_tube_tapers_patterns_and_undoes_in_one_step() {
    let mut head = Headless::new();
    let reply = head.ok(
        "fill_tube",
        json!({"points": [[0, 0, 0, 3], [12, 0, 0, 1], {"x": 20, "y": 4, "z": 2}], "radius": 0.5, "palette": 6}),
    );
    let first = reply["changed"].as_i64().unwrap();
    assert!(first > 50, "{reply}");
    let cells = &head.session.active().cells;
    assert!(cells.contains_key(&(0, 0, 3)) && cells.contains_key(&(20, 4, 2)));
    assert!(!cells.contains_key(&(12, 0, 2)));
    head.ok("undo", json!({}));
    assert!(head.session.active().cells.is_empty());

    head.ok(
        "fill_tube",
        json!({"points": [[0, 0, 0], [10, 0, 0]], "radius": 4, "palettes": [2, 3], "pattern": "stripes", "sectors": 8}),
    );
    let mut used: Vec<u8> = head.session.active().cells.values().copied().collect();
    used.sort();
    used.dedup();
    assert_eq!(used, vec![2, 3]);
    head.ok(
        "fill_tube",
        json!({"points": [[0, 0, 0], [10, 0, 0]], "radius": 5, "palette": 7, "mode": "empty", "curve": "smooth"}),
    );
    assert!(matches!(
        head.session.active().cells.get(&(5, 0, 0)),
        Some(2 | 3)
    ));
    assert_eq!(head.session.active().cells.get(&(5, 0, 5)), Some(&7));

    let none = error_text(&head.call(
        "fill_tube",
        json!({"points": [[0, 0, 0], [4, 0, 0]], "palette": 1}),
    ));
    assert!(none.contains("point 0 has no radius"), "{none}");
    let both = error_text(&head.call(
        "fill_tube",
        json!({"points": [[0, 0, 0], [4, 0, 0]], "radius": 1, "palette": 1, "palettes": [1]}),
    ));
    assert!(both.contains("not both"), "{both}");
    let before = head.session.active().cells.clone();
    let huge = error_text(&head.call(
        "fill_tube",
        json!({"points": [[-500, 0, 0], [500, 0, 0]], "radius": 100, "palette": 1}),
    ));
    assert!(
        huge.contains("too large") || huge.contains("2000000"),
        "{huge}"
    );
    assert_eq!(head.session.active().cells, before);
}

#[test]
fn carve_removes_each_shape_and_reports_it() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 15, "y1": 9, "z1": 9, "palette": 5}),
    );
    let full = head.session.voxel_count() as i64;
    let tunnel = head.ok(
        "carve",
        json!({"shape": "tube", "points": [[-1, 4, 4], [16, 4, 4]], "radius": 2}),
    );
    let removed = tunnel["removed"].as_i64().unwrap();
    assert!(removed > 0);
    assert_eq!(tunnel["voxels"].as_i64().unwrap(), full - removed);
    assert!(!head.session.active().cells.contains_key(&(0, 4, 4)));
    assert!(!head.session.active().cells.contains_key(&(15, 4, 4)));
    let pit = head.ok(
        "carve",
        json!({"shape": "ellipsoid", "x0": 2, "y0": 0, "z0": 7, "x1": 4, "y1": 2, "z1": 9}),
    );
    assert!(pit["removed"].as_i64().unwrap() > 0);
    head.ok(
        "carve",
        json!({"shape": "rounded_box", "x0": 10, "y0": 0, "z0": 7, "x1": 13, "y1": 3, "z1": 9, "radius": 1}),
    );
    let corner = head.ok(
        "carve",
        json!({"shape": "box", "x0": 15, "y0": 9, "z0": 9, "x1": 20, "y1": 20, "z1": 20}),
    );
    assert_eq!(corner["removed"], 1);
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells.get(&(15, 9, 9)), Some(&5));
    let shape = error_text(&head.call("carve", json!({"shape": "cone"})));
    assert!(
        shape.contains("box, ellipsoid, rounded_box, or tube"),
        "{shape}"
    );
}

#[test]
fn smooth_softens_a_region_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 8, "y1": 8, "z1": 2, "palette": 3}),
    );
    head.ok("set_voxels", json!({"cells": [[4, 4, 2, 0], [2, 2, 3, 8]]}));
    let reply = head.ok(
        "smooth",
        json!({"x0": 1, "y0": 1, "z0": 1, "x1": 7, "y1": 7, "z1": 4}),
    );
    assert_eq!(reply["filled"], 1, "{reply}");
    assert_eq!(reply["removed"], 1, "{reply}");
    assert_eq!(head.session.active().cells.get(&(4, 4, 2)), Some(&3));
    assert!(!head.session.active().cells.contains_key(&(2, 2, 3)));
    head.ok("undo", json!({}));
    assert!(!head.session.active().cells.contains_key(&(4, 4, 2)));
    assert_eq!(head.session.active().cells.get(&(2, 2, 3)), Some(&8));
    let big = error_text(&head.call(
        "smooth",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 200, "y1": 200, "z1": 200}),
    ));
    assert!(big.contains("2000000"), "{big}");
    let passes = error_text(&head.call(
        "smooth",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1, "iterations": 9}),
    ));
    assert!(passes.contains("iterations must be 1..=8"), "{passes}");
}

#[test]
fn fill_tube_takes_a_flat_blade_profile() {
    let mut head = Headless::new();
    head.ok(
        "fill_tube",
        json!({"points": [[0, 0, 0], [10, 0, 0]], "radius": 3, "aspect": 0.3, "roll": 90, "profile": "blade", "palette": 5}),
    );
    let cells = &head.session.active().cells;
    assert!(cells.contains_key(&(5, 0, 3)) && !cells.contains_key(&(5, 2, 0)));
    let bad = error_text(&head.call(
        "fill_tube",
        json!({"points": [[0, 0, 0], [4, 0, 0]], "radius": 1, "palette": 1, "profile": "knife"}),
    ));
    assert!(bad.contains("profile must be round or blade"), "{bad}");
}

#[test]
fn break_end_breaks_the_active_object_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_tube",
        json!({"points": [[0, 0, 0], [24, 0, 0]], "radius": 4, "palette": 6}),
    );
    let before = head.session.active().cells.clone();
    let reply = head.ok(
        "break_end",
        json!({"point": [16, 0, 0], "normal": [1, 0, 0], "density": 0.2, "length": [3, 8], "bark": 6, "sapwood": [7, 8], "tips": 9, "seed": 2}),
    );
    assert!(reply["removed"].as_i64().unwrap() > 0, "{reply}");
    assert!(reply["splinters"].as_i64().unwrap() > 0, "{reply}");
    assert!(!head.session.active().cells.contains_key(&(24, 0, 0)));
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells, before);
    let both = error_text(&head.call(
        "break_end",
        json!({"point": [16, 0, 0], "normal": [1, 0, 0], "density": 0.2, "splinters": 3}),
    ));
    assert!(both.contains("not both"), "{both}");
    let none = error_text(&head.call("break_end", json!({"point": [16, 0, 0]})));
    assert!(none.contains("normal"), "{none}");
}

#[test]
fn scatter_piles_pieces_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 12, "y1": 12, "z1": 0, "palette": 2}),
    );
    let before = head.session.active().cells.clone();
    let reply = head.ok(
        "scatter",
        json!({"x0": 2, "y0": 2, "z0": 1, "x1": 10, "y1": 10, "z1": 20, "palettes": [3, 4], "count": 25, "kinds": ["chip", "chunk"], "size": [1, 2], "seed": 4}),
    );
    assert!(reply["pieces"].as_i64().unwrap() > 0, "{reply}");
    assert!(head.session.active().cells.len() > before.len());
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells, before);
    let kind = error_text(&head.call(
        "scatter",
        json!({"x0": 0, "y0": 0, "z0": 1, "x1": 4, "y1": 4, "z1": 9, "palettes": 3, "kinds": ["leaf"]}),
    ));
    assert!(kind.contains("shard, chip, pebble, or chunk"), "{kind}");
    let none = error_text(&head.call(
        "scatter",
        json!({"x0": 0, "y0": 0, "z0": 1, "x1": 4, "y1": 4, "z1": 9}),
    ));
    assert!(none.contains("palettes"), "{none}");
}

#[test]
fn joints_cut_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 4, "z1": 15, "palette": 2}),
    );
    let before = head.session.active().cells.clone();
    let reply = head.ok(
        "joints",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 23, "y1": 4, "z1": 15, "pattern": "courses", "dirt": 3, "seed": 2}),
    );
    assert!(reply["removed"].as_i64().unwrap() > 0, "{reply}");
    assert!(reply["dirtied"].as_i64().unwrap() > 0, "{reply}");
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells, before);
    let bad = error_text(&head.call(
        "joints",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 4, "y1": 4, "z1": 4, "pattern": "tiles"}),
    ));
    assert!(bad.contains("columns, courses, or cracks"), "{bad}");
}

#[test]
fn weather_chips_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 15, "y1": 5, "z1": 9, "palette": 2}),
    );
    let before = head.session.active().cells.clone();
    let reply = head.ok(
        "weather",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 15, "y1": 5, "z1": 9, "chip": 0.4, "broken": 3, "noise": [4, 5], "seed": 1}),
    );
    assert!(reply["chips"].as_i64().unwrap() > 0, "{reply}");
    assert!(reply["recoloured"].as_i64().unwrap() > 0, "{reply}");
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells, before);
    let bad = error_text(&head.call(
        "weather",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 4, "y1": 4, "z1": 4, "pitDepth": 9}),
    ));
    assert!(bad.contains("pitDepth"), "{bad}");
}

#[test]
fn moss_algae_and_vines_grow_as_one_undo_step_each() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 19, "y1": 5, "z1": 15, "palette": 2}),
    );
    let before = head.session.active().cells.clone();
    let corners = json!({"x0": -1, "y0": -1, "z0": -1, "x1": 20, "y1": 6, "z1": 16});
    let with = |extra: Value| {
        let mut args = corners.clone();
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        args
    };
    let moss = head.ok("moss", with(json!({"palettes": [4, 5], "seed": 1})));
    assert!(moss["added"].as_i64().unwrap() > 0, "{moss}");
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells, before);
    let algae = head.ok("algae", with(json!({"palettes": [6, 7], "band": [0, 5]})));
    assert!(algae["painted"].as_i64().unwrap() > 0, "{algae}");
    let vines = head.ok(
        "vines",
        with(json!({"palettes": [8, 9], "blooms": 10, "amount": 0.3, "facing": [0, -1, 0]})),
    );
    assert!(vines["strands"].as_i64().unwrap() > 0, "{vines}");
    let faces = error_text(&head.call("moss", with(json!({"palettes": 4, "faces": "front"}))));
    assert!(faces.contains("top, side, bottom, all"), "{faces}");
    let none = error_text(&head.call("vines", corners.clone()));
    assert!(none.contains("palettes"), "{none}");
}

#[test]
fn sweep_fills_a_curved_wall_as_one_undo_step() {
    let mut head = Headless::new();
    let reply = head.ok(
        "sweep",
        json!({"points": [[0, 0, 0], [12, 8, 0], [24, 4, 0, 0, 0.5]], "curve": "smooth",
            "width": 4, "height": 10, "radius": 1, "palettes": [2, 3], "pattern": "bands", "period": 2}),
    );
    assert!(reply["cells"].as_i64().unwrap() > 300, "{reply}");
    let cells = &head.session.active().cells;
    assert!(cells.values().any(|&s| s == 2) && cells.values().any(|&s| s == 3));
    head.ok("undo", json!({}));
    assert!(head.session.active().cells.is_empty());
    let none = error_text(&head.call(
        "sweep",
        json!({"points": [[0, 0, 0], [5, 0, 0]], "palette": 2}),
    ));
    assert!(none.contains("width"), "{none}");
    let both = error_text(&head.call(
        "sweep",
        json!({"points": [[0, 0, 0], [5, 0, 0]], "width": 2, "height": 2, "palette": 2, "palettes": [3]}),
    ));
    assert!(both.contains("not both"), "{both}");
}

#[test]
fn sweep_with_palette_0_erases_what_a_fill_would_cover() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": -6, "z0": 0, "x1": 19, "y1": 6, "z1": 9, "palette": 2}),
    );
    let solid = head.session.active().cells.len();
    // A closed outline inside the block, with a slanted top.
    let path = json!([[4, -3, 0], [15, -3, 0], [15, 3, 0], [4, 3, 0], [4, -3, 0]]);
    let wedge = json!([[-1, 0], [1, 0], [1, 8], [-1, 4]]);
    let mut filled = Headless::new();
    let would = filled.ok(
        "sweep",
        json!({"points": path, "profile": wedge, "palette": 3}),
    );
    let reply = head.ok(
        "sweep",
        json!({"points": path, "profile": wedge, "palette": 0}),
    );
    assert_eq!(reply["cells"], would["cells"], "{reply}");
    let removed = reply["removed"].as_u64().unwrap() as usize;
    assert!(removed > 100, "{reply}");
    let cells = &head.session.active().cells;
    assert_eq!(cells.len(), solid - removed);
    assert!(filled
        .session
        .active()
        .cells
        .keys()
        .all(|cell| !cells.contains_key(cell)));
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells.len(), solid);
    // Nothing solid under it, so nothing is removed.
    let air = head.ok(
        "sweep",
        json!({"points": [[0, 0, 40], [5, 0, 40]], "width": 2, "height": 2, "palette": 0}),
    );
    assert_eq!(air["removed"], 0, "{air}");
    assert_eq!(head.session.active().cells.len(), solid);
    let empty = error_text(&head.call(
        "sweep",
        json!({"points": path, "profile": wedge, "palette": 0, "mode": "empty"}),
    ));
    assert!(empty.contains("mode empty"), "{empty}");
    let bad = error_text(&head.call(
        "sweep",
        json!({"points": path, "profile": wedge, "palette": 256}),
    ));
    assert!(bad.contains("0..255"), "{bad}");
    let shape = error_text(&head.call(
        "carve",
        json!({"shape": "sweep", "points": path, "profile": wedge}),
    ));
    assert!(shape.contains("palette 0"), "{shape}");
}

#[test]
fn markers_save_export_and_follow_their_object() {
    let mut head = Headless::new();
    head.ok("set_voxel", json!({"x": 0, "y": 0, "z": 0, "palette": 1}));
    head.ok(
        "add_object",
        json!({"name": "ledge", "position": [10, 0, 0]}),
    );
    head.ok(
        "fill_box",
        json!({"x0": 2, "y0": 0, "z0": 0, "x1": 5, "y1": 3, "z1": 3, "palette": 2}),
    );
    let set = head.ok(
        "set_marker",
        json!({"name": "ledge_top", "point": [3.5, 1.5, 4]}),
    );
    assert_eq!(set["marker"]["world"]["point"], json!([13.5, 1.5, 4.0]));
    assert_eq!(
        set["marker"]["gltf"]["translation"],
        json!([13.5, 4.0, -1.5])
    );
    head.ok(
        "set_marker",
        json!({"name": "hit", "kind": "box", "x0": 2, "y0": 0, "z0": 0, "x1": 5, "y1": 3, "z1": 3}),
    );
    head.ok(
        "set_marker",
        json!({"name": "east", "kind": "connector", "point": [6, 2, 2], "direction": [2, 0, 0], "object": "ledge"}),
    );
    let listed = head.ok("list_markers", json!({}));
    let all = listed["markers"].as_array().unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[1]["world"]["min"], json!([12.0, 0.0, 0.0]));
    assert_eq!(all[1]["world"]["max"], json!([16.0, 4.0, 4.0]));
    assert_eq!(all[2]["local"]["direction"], json!([1.0, 0.0, 0.0]));
    // Moving the object moves its markers.
    head.ok(
        "set_object",
        json!({"index": 1, "position": [20, 0, 0], "name": "shelf"}),
    );
    let moved = head.ok("list_markers", json!({}));
    assert_eq!(
        moved["markers"][0]["world"]["point"],
        json!([23.5, 1.5, 4.0])
    );
    assert_eq!(moved["markers"][0]["object"], "shelf");
    // Saved and reopened.
    let path = std::env::temp_dir().join("froxel-mcp-markers.vox");
    head.ok("save_file", json!({"path": path.to_string_lossy()}));
    head.ok("open_file", json!({"path": path.to_string_lossy()}));
    // A load may move an object's local origin; the world stays.
    let reopened = head.ok("list_markers", json!({}));
    let placed = |list: &Value| -> Vec<(Value, Value)> {
        list["markers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| (m["world"].clone(), m["gltf"].clone()))
            .collect()
    };
    assert_eq!(placed(&reopened), placed(&moved));
    let _ = std::fs::remove_file(&path);
    // Exported as nodes with extras, and listed in the reply.
    let glb = std::env::temp_dir().join("froxel-mcp-markers.glb");
    let reply = head.ok("export_mesh", json!({"path": glb.to_string_lossy()}));
    assert_eq!(reply["markers"].as_array().unwrap().len(), 3);
    let bytes = std::fs::read(&glb).unwrap();
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let root: Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
    let nodes = root["nodes"].as_array().unwrap();
    let hit = nodes.iter().find(|n| n["name"] == "hit").unwrap();
    assert_eq!(hit["extras"]["froxel"]["kind"], "box");
    assert_eq!(hit["extras"]["froxel"]["size"], json!([4.0, 4.0, 4.0]));
    let east = nodes.iter().find(|n| n["name"] == "east").unwrap();
    assert_eq!(
        east["extras"]["froxel"]["direction"],
        json!([1.0, 0.0, -0.0])
    );
    assert!(east["rotation"].is_array());
    assert_eq!(
        root["scenes"][0]["extras"]["froxelMarkers"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let _ = std::fs::remove_file(&glb);
    // Removing is one undo step.
    head.ok("remove_marker", json!({"name": "hit"}));
    assert_eq!(head.session.markers.len(), 2);
    head.ok("undo", json!({}));
    assert_eq!(head.session.markers.len(), 3);
    let bad = error_text(&head.call(
        "set_marker",
        json!({"name": "x", "kind": "spawn", "point": [0, 0, 0]}),
    ));
    assert!(bad.contains("point, box, or connector"), "{bad}");
    let none = error_text(&head.call("remove_marker", json!({"name": "nope"})));
    assert!(none.contains("no marker"), "{none}");
}

#[test]
fn export_variants_writes_each_set_and_leaves_the_document() {
    let mut head = Headless::new();
    let source = r#"
        let h = param_int("height", 4, 1, 20);
        fill_box(0, 0, 0, 5, 3, h - 1, 2);
        marker("top", 3, 2, h);
        collision_box(0, 0, 0, 5, 3, h - 1);
    "#;
    head.ok("set_script", json!({"source": source}));
    head.ok("regenerate", json!({}));
    let before = head.session.active().cells.clone();
    let undo = head.session.can_undo();
    let dir = std::env::temp_dir().join("froxel-mcp-variants");
    let _ = std::fs::remove_dir_all(&dir);
    let reply = head.ok(
        "export_variants",
        json!({"dir": dir.to_string_lossy(), "merge": true, "colorAtlas": true, "aoStrength": 0.5,
            "variants": [{"name": "low", "params": {"height": 3}}, {"name": "tall", "params": {"height": 12}}]}),
    );
    let variants = reply["variants"].as_array().unwrap();
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0]["params"]["Object/height"], 3);
    assert_eq!(variants[1]["bounds"]["voxel"]["max"], json!([6, 4, 12]));
    assert_eq!(
        variants[1]["markers"][0]["world"]["point"],
        json!([3.0, 2.0, 12.0])
    );
    assert_eq!(reply["options"]["colorAtlas"], true);
    for name in [
        "low.vox",
        "low.glb",
        "tall.vox",
        "tall.glb",
        "variants.json",
    ] {
        assert!(dir.join(name).is_file(), "{name} is missing");
    }
    // A written variant reopens with its own cells and markers.
    let mut tall = Session::new();
    tall.open_vox(&dir.join("tall.vox")).unwrap();
    assert_eq!(tall.voxel_count(), 6 * 4 * 12);
    assert_eq!(tall.markers.len(), 2);
    // The open document is as it was.
    assert_eq!(head.session.active().cells, before);
    assert_eq!(head.session.can_undo(), undo);
    let twice = error_text(&head.call(
        "export_variants",
        json!({"dir": dir.to_string_lossy(), "variants": [{"name": "a"}, {"name": "A"}]}),
    ));
    assert!(twice.contains("used twice"), "{twice}");
    let unknown = error_text(&head.call(
        "export_variants",
        json!({"dir": dir.to_string_lossy(), "variants": [{"params": {"width": 2}}]}),
    ));
    assert!(unknown.contains("unknown parameter"), "{unknown}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_variants_merges_into_the_manifest_by_name() {
    let mut head = Headless::new();
    head.ok(
        "set_script",
        json!({"source": "let h = param_int(\"height\", 4, 1, 20); fill_box(0, 0, 0, 2, 2, h - 1, 2);"}),
    );
    head.ok("regenerate", json!({}));
    let dir = std::env::temp_dir().join(format!("froxel-variants-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let dir_text = dir.to_string_lossy().into_owned();
    let names = |manifest: &Value| -> Vec<String> {
        manifest["variants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_string())
            .collect()
    };
    let first = head.ok(
        "export_variants",
        json!({"dir": dir_text, "vox": false, "variants": [
            {"name": "a", "params": {"height": 2}},
            {"name": "b", "params": {"height": 3}},
            {"name": "c", "params": {"height": 4}}]}),
    );
    assert_eq!(first["added"], json!(["a", "b", "c"]));
    assert_eq!(first["kept"], json!([]));
    // Redo one, add one: b is replaced in place, a and c are kept.
    let second = head.ok(
        "export_variants",
        json!({"dir": dir_text, "vox": false, "variants": [
            {"name": "d", "params": {"height": 5}},
            {"name": "B", "params": {"height": 9}}]}),
    );
    assert_eq!(second["added"], json!(["d"]));
    assert_eq!(second["replaced"], json!(["B"]));
    assert_eq!(second["kept"], json!(["a", "c"]));
    let text = std::fs::read_to_string(dir.join("variants.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(names(&on_disk), ["a", "B", "c", "d"]);
    assert_eq!(on_disk["variants"][1]["params"]["Object/height"], 9);
    assert_eq!(on_disk["variants"][0]["params"]["Object/height"], 2);
    assert_eq!(on_disk["generator"], "Froxel");
    assert_eq!(on_disk["options"]["merge"], false);
    assert!(on_disk.get("added").is_none());
    // Other options: recorded per entry, and the top level drops them.
    let third = head.ok(
        "export_variants",
        json!({"dir": dir_text, "vox": false, "merge": true, "variants": [{"name": "c"}]}),
    );
    assert!(third.get("options").is_none(), "{third}");
    assert_eq!(third["variants"][2]["options"]["merge"], true);
    assert_eq!(third["variants"][0]["options"]["merge"], false);
    // replace starts over with this call's sets.
    let fresh = head.ok(
        "export_variants",
        json!({"dir": dir_text, "vox": false, "replace": true, "variants": [{"name": "e"}]}),
    );
    assert_eq!(names(&fresh), ["e"]);
    assert_eq!(fresh["added"], json!(["e"]));
    assert_eq!(fresh["options"]["merge"], false);
    // A file that is not a manifest is not overwritten without replace.
    std::fs::write(dir.join("variants.json"), "[1, 2]").unwrap();
    let bad = error_text(&head.call(
        "export_variants",
        json!({"dir": dir_text, "vox": false, "variants": [{"name": "e"}]}),
    ));
    assert!(bad.contains("replace: true"), "{bad}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_manifest_fills_kept_options_from_an_old_manifest() {
    let old = json!({"generator": "Froxel", "options": {"merge": true}, "note": "kept",
        "variants": [{"name": "x"}, {"name": "y"}]});
    let (manifest, changes) = crate::variants::merge_manifest(
        Some(old),
        vec![json!({"name": "y"})],
        &json!({"merge": true}),
    );
    assert_eq!(changes.kept, ["x"]);
    assert_eq!(changes.replaced, ["y"]);
    assert_eq!(manifest["variants"][0]["options"], json!({"merge": true}));
    assert_eq!(manifest["options"], json!({"merge": true}));
    assert_eq!(manifest["note"], "kept");
}

#[test]
fn preview_counts_the_export_apart_from_its_own_unmerged_mesh() {
    let mut head = Headless::new();
    let cells = &mut head.session.objects[0].cells;
    cells.clear();
    for x in 0..12 {
        for y in 0..6 {
            for z in 0..3 {
                cells.insert((x, y, z), 1);
            }
        }
    }
    head.session.mesh_epoch += 1;
    let dir = std::env::temp_dir().join(format!("froxel-preview-counts-{}", std::process::id()));
    let png = dir.join("slab.png");
    let glb = dir.join("slab.glb");
    let unmerged = head.ok("preview", json!({"path": png, "width": 64, "height": 64}));
    // Twelve by six by three: every exposed face is two triangles.
    let faces = 2 * (12 * 6 + 12 * 3 + 6 * 3);
    assert_eq!(unmerged["export"]["triangles"], 2 * faces, "{unmerged}");
    assert_eq!(unmerged["export"]["quads"], faces);
    assert_eq!(unmerged["export"]["merge"], false);
    let preview = unmerged["previewTriangles"].as_u64().unwrap();
    assert!(preview > 0 && preview <= 2 * faces as u64);
    for options in [json!({"merge": true}), json!({"colorAtlas": true})] {
        let reply = head.ok(
            "preview",
            json!({"path": png, "width": 64, "height": 64, "export": options, "countsLabel": true}),
        );
        let written = head.ok(
            "export_mesh",
            json!({"path": glb, "merge": options["merge"], "colorAtlas": options["colorAtlas"]}),
        );
        assert_eq!(
            reply["export"]["triangles"], written["triangles"],
            "{options}"
        );
        assert_eq!(reply["export"]["quads"], written["quads"], "{options}");
        assert_eq!(
            reply["previewTriangles"], preview,
            "the preview mesh is the same"
        );
        assert!(
            written["triangles"].as_u64().unwrap() < preview,
            "{written}"
        );
    }
    let off = head.ok(
        "preview",
        json!({"path": png, "width": 64, "height": 64, "export": false}),
    );
    assert!(off.get("export").is_none());
    let bad = head.call("preview", json!({"path": png, "export": {"merged": true}}));
    assert!(error_text(&bad).contains("merged"), "{bad}");
    let failing = head.ok(
        "preview",
        json!({"path": png, "width": 64, "height": 64, "export": {"aoStrength": 0.5, "aoTarget": "texture"}}),
    );
    assert!(failing["export"]["error"]
        .as_str()
        .unwrap()
        .contains("colorAtlas"));
    std::fs::remove_dir_all(&dir).ok();
}

/// An asset placed turned by `placementYaw` from the game camera looks like
/// the asset really built turned that way, sun included.
#[test]
fn placement_yaw_shows_the_asset_as_it_stands_in_the_level() {
    let mut head = Headless::new();
    // A long bar along +X with a stub toward +Y at its far end.
    let shape: Vec<(i32, i32, i32)> = (0..12)
        .flat_map(|x| [(x, 0, 0), (x, 1, 0), (x, 0, 1), (x, 1, 1)])
        .chain([(11, 2, 0), (11, 3, 0), (11, 2, 1), (11, 3, 1)])
        .collect();
    let dir = std::env::temp_dir().join(format!("froxel-placement-{}", std::process::id()));
    let png = dir.join("placed.png");
    let base = json!({"path": png, "width": 128, "height": 128, "preset": "game",
        "lighting": "old_pond", "antialias": false});
    let build = |head: &mut Headless, turned: bool| {
        let cells = &mut head.session.objects[0].cells;
        cells.clear();
        for &(x, y, z) in &shape {
            // Counter-clockwise from above by 90 degrees: +X goes to +Y.
            let cell = if turned { (3 - y, x, z) } else { (x, y, z) };
            cells.insert(cell, 1);
        }
        head.session.mesh_epoch += 1;
    };
    build(&mut head, true);
    let built = head.ok("preview", base.clone());
    assert_eq!(built["placementYaw"], 0.0);
    let (_, _, built) = read_png(&png);

    build(&mut head, false);
    let shown = |head: &mut Headless, turn: f64| {
        let mut args = base.clone();
        args["placementYaw"] = json!(turn);
        let reply = head.ok("preview", args);
        assert_eq!(reply["yaw"].as_f64(), Some(-135.0 - turn));
        assert_eq!(reply["placementYaw"].as_f64(), Some(turn));
        read_png(&png).2
    };
    let differing = |a: &[u8], b: &[u8]| {
        a.chunks(4)
            .zip(b.chunks(4))
            .filter(|(p, q)| p.iter().zip(*q).any(|(u, v)| u.abs_diff(*v) > 6))
            .count()
    };
    let placed = shown(&mut head, 90.0);
    let wrong_way = shown(&mut head, -90.0);
    let same = differing(&built, &placed);
    let opposite = differing(&built, &wrong_way);
    assert!(same < 300, "{same} pixels differ from the turned asset");
    assert!(opposite > same * 5 + 300, "{opposite} against {same}");

    // set_view takes it too, and refuses a turn with no angle to turn from.
    let view = head.ok("set_view", json!({"preset": "game", "placementYaw": 45}));
    assert!(
        (view["yaw"].as_f64().unwrap() + 180.0).abs() < 1e-3,
        "{view}"
    );
    let refused = head.call("set_view", json!({"placementYaw": 45}));
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(error_text(&refused).contains("placementYaw"), "{refused}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn preview_lighting_takes_a_preset_or_fields_and_reports_what_it_used() {
    let mut head = Headless::new();
    marked_block(&mut head);
    let dir = std::env::temp_dir().join(format!("froxel-preview-light-{}", std::process::id()));
    let png = dir.join("pond.png");
    let base = json!({"path": png, "width": 48, "height": 48, "antialias": false});
    let with = |extra: Value| {
        let mut args = base.clone();
        for (key, value) in extra.as_object().unwrap() {
            args[key] = value.clone();
        }
        args
    };
    let viewport = head.ok("preview", base.clone());
    assert!(viewport.get("lighting").is_none());
    let (_, _, plain) = read_png(&png);
    let pond = head.ok("preview", with(json!({"lighting": "old_pond"})));
    assert_eq!(pond["lighting"]["name"], "old_pond");
    assert_eq!(pond["lighting"]["sunEnergy"], 1.45);
    assert_eq!(pond["lighting"]["background"], "#0a3f57ff");
    let sun = pond["lighting"]["sunDirection"].as_array().unwrap();
    assert!((sun[1].as_f64().unwrap() - 0.7036).abs() < 1e-3, "{sun:?}");
    let (_, _, lit) = read_png(&png);
    assert_eq!(
        &lit[..4],
        &[0x0a, 0x3f, 0x57, 255],
        "the preset's background"
    );
    assert_ne!(plain, lit);
    // The call's background wins over the preset's.
    head.ok(
        "preview",
        with(json!({"lighting": "old_pond", "background": "#000000"})),
    );
    assert_eq!(&read_png(&png).2[..4], &[0, 0, 0, 255]);
    let custom = head.ok(
        "preview",
        with(json!({
            "lighting": "noon",
            "lightingPresets": {"noon": {"preset": "old_pond", "sunDirection": [0, 0, 1], "sunColor": "#ffffff"}}
        })),
    );
    assert_eq!(custom["lighting"]["sunDirection"], json!([0.0, 0.0, 1.0]));
    assert_eq!(custom["lighting"]["ambientEnergy"], 1.1);
    let inline = head.ok(
        "preview",
        with(json!({"lighting": {"sunEnergy": 0.0, "ambientEnergy": 1.0, "ambientColor": [1, 1, 1]}})),
    );
    assert_eq!(inline["lighting"]["name"], "custom");
    for bad in [
        json!({"lighting": "dusk"}),
        json!({"lighting": {"sunEnergy": "high"}}),
        json!({"lightingPresets": {"a": {}}}),
        json!({"lighting": "a", "lightingPresets": "/nonexistent/presets.json"}),
    ] {
        let reply = head.call("preview", with(bad.clone()));
        assert_eq!(reply["result"]["isError"], true, "{bad} {reply}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn preview_lays_variants_files_and_a_reference_out_in_a_contact_sheet() {
    let mut head = Headless::new();
    head.ok(
        "set_script",
        json!({"source": "let h = param_int(\"h\", 3, 1, 12); fill_box(0, 0, 0, 3, 3, h, 2);"}),
    );
    head.ok("regenerate", json!({}));
    let before = head.session.active().cells.clone();
    let dir = std::env::temp_dir().join(format!("froxel-sheet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A reference: a 4x2 PNG, red on the left and blue on the right.
    let concept = dir.join("concept.png");
    let mut rgba = Vec::new();
    for _ in 0..2 {
        rgba.extend([
            255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ]);
    }
    crate::trace::write_rgba_png(&concept, 4, 2, &rgba).unwrap();
    let path = dir.join("sheet.png");
    let args = json!({
        "path": path,
        "width": 64,
        "height": 48,
        "preset": "game",
        "variants": [{"name": "low", "params": {"h": 2}}, {"name": "tall", "params": {"h": 11}}],
        "reference": {"path": concept, "label": "concept"},
        "columns": 3,
        "background": "#101010"
    });
    let reply = head.ok("preview", args.clone());
    assert_eq!(
        (reply["yaw"].as_f64(), reply["orthographic"].as_bool()),
        (Some(-135.0), Some(true))
    );
    let tiles = reply["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 3);
    assert_eq!(tiles[0]["label"], "concept");
    assert_eq!(tiles[2]["params"], json!({"h": 11}));
    assert!(tiles[2]["voxels"].as_u64() > tiles[1]["voxels"].as_u64());
    // The document is as it was.
    assert_eq!(head.session.active().cells, before);
    let (w, h, image) = read_png(&path);
    assert_eq!(
        (w, h),
        (
            reply["width"].as_u64().unwrap() as u32,
            reply["height"].as_u64().unwrap() as u32
        )
    );
    assert_eq!(w, 8 + 3 * (64 + 8));
    let px = |x: u32, y: u32| &image[((y * w + x) * 4) as usize..((y * w + x) * 4 + 3) as usize];
    // The reference keeps its aspect: red left, blue right, background above.
    assert_eq!(px(8 + 16, 8 + 24), &[255, 0, 0]);
    assert_eq!(px(8 + 48, 8 + 24), &[0, 0, 255]);
    assert_eq!(px(8 + 32, 8 + 2), &[16, 16, 16]);
    // Each model tile draws something, and labels sit under the tiles.
    for tile in &tiles[1..] {
        let (x, y) = (
            tile["x"].as_u64().unwrap() as u32,
            tile["y"].as_u64().unwrap() as u32,
        );
        assert_ne!(px(x + 32, y + 24), &[16, 16, 16]);
    }
    assert!((8 + 48..h).any(|y| (8..w).any(|x| px(x, y) != [16, 16, 16])));
    // Deterministic.
    let first = std::fs::read(&path).unwrap();
    head.ok("preview", args);
    assert_eq!(std::fs::read(&path).unwrap(), first);
    // Files on disk, labelled by their names.
    let vox = dir.join("block.vox");
    head.ok("save_file", json!({"path": vox}));
    let reply = head.ok(
        "preview",
        json!({"path": path, "width": 32, "height": 32, "inputs": [vox, {"path": vox, "label": "again"}]}),
    );
    assert_eq!(reply["tiles"][0]["label"], "block");
    assert_eq!(reply["tiles"][1]["label"], "again");
    let bad = error_text(&head.call(
        "preview",
        json!({"path": path, "preset": "nope", "inputs": [vox]}),
    ));
    assert!(bad.contains("game"), "{bad}");
    let many: Vec<_> = (0..70).map(|_| json!(vox)).collect();
    let bad = error_text(&head.call("preview", json!({"path": path, "inputs": many})));
    assert!(bad.contains("at most"), "{bad}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preview_overlay_draws_markers_and_fades_what_the_model_hides() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 11, "y1": 11, "z1": 3, "palette": 2}),
    );
    head.ok(
        "set_marker",
        json!({"name": "slab", "kind": "box", "x0": 0, "y0": 0, "z0": 0, "x1": 11, "y1": 11, "z1": 3}),
    );
    head.ok(
        "set_marker",
        json!({"name": "top", "point": [6.0, 6.0, 4.0]}),
    );
    head.ok(
        "set_marker",
        json!({"name": "under", "point": [6.0, 6.0, 1.0]}),
    );
    head.ok(
        "set_marker",
        json!({"name": "east", "kind": "connector", "point": [12.0, 6.0, 2.0], "direction": [1, 0, 0]}),
    );
    let dir = std::env::temp_dir().join(format!("froxel-overlay-{}", std::process::id()));
    let plain = dir.join("plain.png");
    let marked = dir.join("marked.png");
    let base = json!({"width": 160, "height": 120, "preset": "game", "background": "#000000"});
    let mut args = base.clone();
    args["path"] = json!(plain);
    head.ok("preview", args);
    let mut args = base.clone();
    args["path"] = json!(marked);
    args["overlay"] = json!(true);
    let reply = head.ok("preview", args.clone());
    let drawn = reply["overlay"].as_array().unwrap();
    let names: Vec<&str> = drawn.iter().map(|m| m["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["slab", "top", "under", "east"]);
    let hidden = |name: &str| drawn.iter().find(|m| m["name"] == name).unwrap()["hidden"].as_bool();
    assert_eq!(hidden("top"), Some(false));
    assert_eq!(
        hidden("under"),
        Some(true),
        "a point inside the slab is behind it"
    );
    let (_, _, a) = read_png(&plain);
    let (_, _, b) = read_png(&marked);
    let changed = a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count();
    assert!(changed > 300, "{changed} pixels changed");
    // The box outline is orange where it is drawn.
    let orange = b
        .chunks(4)
        .filter(|p| p[0] > 200 && (120..180).contains(&p[1]) && p[2] < 80)
        .count();
    assert!(orange > 50, "{orange} orange pixels");
    // Same request, same bytes.
    let first = std::fs::read(&marked).unwrap();
    head.ok("preview", args.clone());
    assert_eq!(std::fs::read(&marked).unwrap(), first);
    // Only boxes, no labels: fewer changed pixels than everything.
    args["overlay"] = json!(["boxes"]);
    args["overlayLabels"] = json!(false);
    let reply = head.ok("preview", args.clone());
    assert_eq!(reply["overlay"].as_array().unwrap().len(), 1);
    let (_, _, c) = read_png(&marked);
    let boxes = a.chunks(4).zip(c.chunks(4)).filter(|(p, q)| p != q).count();
    assert!(boxes > 0 && boxes < changed, "{boxes} vs {changed}");
    args["overlay"] = json!("lines");
    let bad = error_text(&head.call("preview", args));
    assert!(bad.contains("overlay must be"), "{bad}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn loft_deform_and_brush_shape_a_trunk_one_undo_step_each() {
    let mut head = Headless::new();
    let loft = head.ok(
        "loft",
        json!({"points": [[0, 0, 0, 8, 6], {"x": 20, "y": 4, "z": 2, "rx": 5, "flatTop": -0.3},
            [40, 0, 4, 3, 3, 30]], "curve": "smooth", "noise": 0.25, "frequency": 6, "seed": 2,
            "palettes": [2, 3], "pattern": "rings", "period": 3}),
    );
    assert!(loft["cells"].as_i64().unwrap() > 1500, "{loft}");
    assert!(loft["length"].as_f64().unwrap() > 39.0, "{loft}");
    let lofted = head.session.active().cells.clone();
    let bend = head.ok(
        "deform",
        json!({"op": "bend", "axis": "x", "angle": 40, "direction": "+z", "origin": 20}),
    );
    assert!(bend["removed"].as_i64().unwrap() > 0, "{bend}");
    let bent = head.session.active().cells.clone();
    assert!(bent.keys().map(|c| c.2).max() > lofted.keys().map(|c| c.2).max());
    let lumps = head.ok(
        "deform",
        json!({"op": "displace", "amplitude": 2, "scale": 5, "bias": 0.5, "seed": 4,
            "x0": 0, "y0": -12, "z0": -12, "x1": 20, "y1": 12, "z1": 12}),
    );
    assert!(lumps["added"].as_i64().unwrap() > 0, "{lumps}");
    let brushed = head.ok(
        "brush",
        json!({"op": "inflate", "points": [[5, 0, 6], [15, 0, 6, 4, 1]], "radius": 3}),
    );
    assert!(brushed["dabs"].as_i64().unwrap() >= 3, "{brushed}");
    let grab = head.ok(
        "brush",
        json!({"op": "grab", "points": [[40, 0, 4, 4]], "vector": [2, 0, 3]}),
    );
    assert!(grab["changed"].as_i64().unwrap() > 0, "{grab}");
    for _ in 0..4 {
        head.ok("undo", json!({}));
    }
    assert_eq!(head.session.active().cells, lofted);
    let op = error_text(&head.call("deform", json!({"op": "squash"})));
    assert!(op.contains("bend, taper, twist, or displace"), "{op}");
    let partial = error_text(&head.call("deform", json!({"op": "taper", "x0": 0})));
    assert!(partial.contains("all of x0"), "{partial}");
    let vector = error_text(&head.call(
        "brush",
        json!({"op": "smooth", "points": [[0, 0, 0]], "vector": [1, 0, 0]}),
    ));
    assert!(vector.contains("grab only"), "{vector}");
    let radius = error_text(&head.call(
        "loft",
        json!({"points": [[0, 0, 0], [4, 0, 0]], "palette": 2}),
    ));
    assert!(radius.contains("radius"), "{radius}");
}

fn mesh_fixture(name: &str) -> String {
    format!("{}/tests/fixtures/mesh/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn import_mesh_voxelizes_into_the_palette_as_one_undo_step() {
    let mut head = Headless::new();
    let before = head.session.palette;
    let reply = head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("cube.obj"), "size": 8, "tolerance": 0}),
    );
    assert_eq!(reply["cells"], 512, "{reply}");
    assert_eq!(reply["changed"], 512, "{reply}");
    assert_eq!(reply["interior"], 216, "{reply}");
    assert_eq!(reply["bounds"]["size"], json!([8, 8, 8]), "{reply}");
    assert_eq!(
        reply["newSlots"],
        json!([{"index": 255, "color": [204, 26, 26]}])
    );
    assert_eq!(head.session.palette[255], [204, 26, 26, 255]);
    assert!(head.session.active().cells.values().all(|&s| s == 255));
    head.ok("undo", json!({}));
    assert!(head.session.active().cells.is_empty());
    assert_eq!(head.session.palette, before);

    // The same red again reuses slot 255 rather than adding one.
    head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("cube.obj"), "size": 4, "tolerance": 0}),
    );
    let again = head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("cube.obj"), "size": 4, "position": [10, 0, 0], "tolerance": 0}),
    );
    assert_eq!(again["slots"], json!([255]));
    assert_eq!(again["newSlots"], json!([]));
}

#[test]
fn import_mesh_into_a_new_object_maps_slots_and_paints() {
    let mut head = Headless::new();
    let reply = head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("parts.glb"), "voxelSize": 0.5, "newObject": true,
            "name": "parts", "position": [4, 0, 0], "anchor": "bottom", "fill": "shell"}),
    );
    assert_eq!(reply["name"], "parts", "{reply}");
    assert_eq!(reply["primitives"], 2);
    assert_eq!(reply["bounds"]["size"], json!([12, 6, 4]));
    assert_eq!(reply["interior"], 0);
    let object = head.session.active();
    assert_eq!(object.translation, (4, 0, 0));
    assert_eq!(reply["slots"].as_array().unwrap().len(), 2, "{reply}");
    // slots maps every colour to the given slot; the palette stays.
    let palette = head.session.palette;
    let mapped = head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("quad.gltf"), "size": 4, "slots": [7], "position": [0, 0, 8]}),
    );
    assert_eq!(mapped["slots"], json!([7]));
    assert_eq!(head.session.palette, palette);
    // paint recolours only solid cells: of the quad's 16 on z 0, the parts'
    // shell (x -6..=5, y -3..=2) holds 6.
    let voxels = head.session.voxel_count();
    let painted = head.ok(
        "import_mesh",
        json!({"path": mesh_fixture("quad.gltf"), "size": 4, "palette": 9, "mode": "paint"}),
    );
    assert_eq!(
        (painted["cells"].as_i64(), painted["written"].as_i64()),
        (Some(16), Some(6))
    );
    assert_eq!(head.session.voxel_count(), voxels);
    let both = error_text(&head.call(
        "import_mesh",
        json!({"path": mesh_fixture("quad.gltf"), "palette": 2, "slots": [3]}),
    ));
    assert!(both.contains("not both"), "{both}");
    let fit = error_text(&head.call(
        "import_mesh",
        json!({"path": mesh_fixture("quad.gltf"), "size": 8, "scale": 2}),
    ));
    assert!(fit.contains("one of size"), "{fit}");
    let missing = error_text(&head.call("import_mesh", json!({"path": mesh_fixture("nope.glb")})));
    assert!(missing.contains("Could not read"), "{missing}");
}

#[test]
fn manual_negative_cells_and_markers_keep_world_origin_across_saves() {
    let mut head = Headless::new();
    head.ok(
        "set_object",
        json!({"index": 0, "name": "manual", "position": [13, -7, 5]}),
    );
    head.ok(
        "fill_tube",
        json!({"points": [[-4, -2, -3, 2], [1, 0, 8, 1]], "palette": 2}),
    );
    head.ok(
        "fill_ellipsoid",
        json!({"x0": -9, "y0": -6, "z0": 7, "x1": 7, "y1": 9, "z1": 16, "palette": 3}),
    );
    head.ok("set_marker", json!({"name": "ground", "point": [0, 0, 0]}));
    head.ok("set_marker", json!({"name": "trunk", "kind": "box", "x0": -2, "y0": -1, "z0": -2, "x1": 2, "y1": 1, "z1": 8}));
    head.ok(
        "set_marker",
        json!({"name": "join", "kind": "connector", "point": [-4, 2, 10], "direction": [1, 0, 0]}),
    );
    let world = |session: &Session| -> Vec<_> {
        let mut result: Vec<_> = session
            .objects
            .iter()
            .flat_map(|object| {
                object.cells.iter().map(move |(cell, slot)| {
                    (
                        cell.0 + object.translation.0,
                        cell.1 + object.translation.1,
                        cell.2 + object.translation.2,
                        *slot,
                    )
                })
            })
            .collect();
        result.sort_unstable();
        result
    };
    let placed = |list: Value| -> Vec<_> {
        list["markers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|marker| (marker["world"].clone(), marker["gltf"].clone()))
            .collect()
    };
    let before = world(&head.session);
    let markers = placed(head.ok("list_markers", json!({})));
    let path = std::env::temp_dir().join("froxel-manual-origin-roundtrip.vox");
    for _ in 0..3 {
        head.ok("save_file", json!({"path": path.to_string_lossy()}));
        head.ok("open_file", json!({"path": path.to_string_lossy()}));
        assert_eq!(world(&head.session), before);
        assert_eq!(placed(head.ok("list_markers", json!({}))), markers);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn trim_outline_cuts_outside_a_surface_as_one_undo_step() {
    let mut head = Headless::new();
    head.ok(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 20, "y1": 10, "z1": 6, "palette": 2}),
    );
    let solid = head.session.active().cells.len();
    let outline = json!([[0, 0], [20, 0], [20, 10], [0, 10]]);
    let reply = head.ok(
        "trim_outline",
        json!({"outline": outline, "inset": [[0, 3], [3, 0]], "skin": 5}),
    );
    let removed = reply["removed"].as_u64().unwrap() as usize;
    assert!(removed > 100, "{reply}");
    assert!(reply["painted"].as_u64().unwrap() > 0, "{reply}");
    assert_eq!(reply["added"], 0, "{reply}");
    let cells = &head.session.active().cells;
    assert_eq!(cells.len(), solid - removed);
    assert!(!cells.contains_key(&(2, 5, 0)));
    assert_eq!(cells.get(&(3, 5, 0)), Some(&5));
    assert_eq!(cells.get(&(10, 5, 0)), Some(&2));
    head.ok("undo", json!({}));
    assert_eq!(head.session.active().cells.len(), solid);
    assert!(head.session.active().cells.values().all(|&slot| slot == 2));
    // One number is every height; the box limits it; a miss is explained.
    let boxed = head.ok(
        "trim_outline",
        json!({"outline": outline, "inset": 2, "x0": 0, "y0": 0, "z0": 0, "x1": 20, "y1": 10, "z1": 0}),
    );
    assert_eq!(boxed["removed"], 21 * 11 - 17 * 7, "{boxed}");
    let miss = head.ok(
        "trim_outline",
        json!({"outline": [[-5, -5], [40, -5], [40, 40], [-5, 40]]}),
    );
    assert_eq!(miss["removed"], 0, "{miss}");
    assert!(
        miss["warning"].as_str().unwrap().contains("outside"),
        "{miss}"
    );
    for (args, needle) in [
        (json!({"inset": 2}), "outline"),
        (
            json!({"outline": [[0, 0, 0], [1, 0, 0], [0, 1, 0]]}),
            "[x, y]",
        ),
        (json!({"outline": outline, "inset": "deep"}), "inset"),
        (json!({"outline": outline, "curve": "spline"}), "curve"),
        (json!({"outline": outline, "x0": 1}), "x0"),
        (json!({"outline": outline, "thickness": 2}), "skin"),
        (json!({"outline": outline, "skin": 0}), "skin"),
    ] {
        let err = error_text(&head.call("trim_outline", args));
        assert!(err.contains(needle), "{needle}: {err}");
    }
}

fn batch_of(operations: Value) -> Value {
    json!({"operations": operations})
}

#[test]
fn batch_applies_its_items_in_order_as_one_undo_step() {
    let mut head = Headless::new();
    let reply = head.call(
        "batch",
        batch_of(json!([
            {"tool": "set_palette", "arguments": {"index": 7, "r": 10, "g": 20, "b": 30}},
            {"tool": "fill_box", "arguments": {"x0": 0, "y0": 0, "z0": 0, "x1": 3, "y1": 3, "z1": 3, "palette": 7}},
            {"tool": "carve", "arguments": {"shape": "box", "x0": 0, "y0": 0, "z0": 3, "x1": 3, "y1": 3, "z1": 3}},
            {"tool": "set_voxel", "arguments": {"x": 9, "y": 9, "z": 9, "palette": 7}}
        ])),
    );
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body["items"], 4);
    assert_eq!(body["voxels"], 49);
    assert_eq!(body["changed"], 1 + 64 + 16 + 1);
    // Short form: the tool and its counts, not the running total or the call.
    assert_eq!(
        body["results"][0],
        json!({"tool": "set_palette", "changed": 1})
    );
    assert_eq!(
        body["results"][1],
        json!({"tool": "fill_box", "changed": 64})
    );
    assert_eq!(body["results"][2], json!({"tool": "carve", "removed": 16}));
    assert_eq!(
        body["results"][3],
        json!({"tool": "set_voxel", "changed": 1})
    );
    assert_eq!(head.session.palette[7][..3], [10, 20, 30]);

    // One undo takes back the whole batch, the palette too.
    let reply = head.call("undo", json!({}));
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert!(head.session.active().cells.is_empty());
    assert_ne!(head.session.palette[7][..3], [10, 20, 30]);
    assert!(!head.session.can_undo());
}

#[test]
fn a_failing_batch_item_is_named_and_leaves_the_document_as_it_was() {
    let mut head = Headless::new();
    head.call(
        "fill_box",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1, "palette": 2}),
    );
    let cells = head.session.active().cells.clone();
    let palette = head.session.palette;
    let reply = head.call(
        "batch",
        batch_of(json!([
            {"tool": "set_palette", "arguments": {"index": 7, "r": 10, "g": 20, "b": 30}},
            {"tool": "fill_box", "arguments": {"x0": 5, "y0": 5, "z0": 5, "x1": 6, "y1": 6, "z1": 6, "palette": 7}},
            {"tool": "carve", "arguments": {"shape": "star"}},
            {"tool": "set_voxel", "arguments": {"x": 9, "y": 9, "z": 9, "palette": 7}}
        ])),
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("batch item 2 (carve)"), "{text}");
    assert!(text.contains("shape must be"), "{text}");
    assert_eq!(head.session.active().cells, cells);
    assert_eq!(head.session.palette, palette);
    // The batch that failed is not an undo step: one undo clears the fill.
    head.call("undo", json!({}));
    assert!(head.session.active().cells.is_empty());
    assert!(!head.session.can_undo());
}

#[test]
fn batch_refuses_what_cannot_run_before_anything_changes() {
    let mut head = Headless::new();
    let fill = json!({"tool": "fill_box", "arguments": {"x0": 0, "y0": 0, "z0": 0, "x1": 1, "y1": 1, "z1": 1, "palette": 2}});
    let path = std::env::temp_dir().join("froxel-batch-refused.glb");
    let path = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);
    for (bad, expected) in [
        (
            json!({"tool": "export_mesh", "arguments": {"path": path}}),
            "export_mesh",
        ),
        (json!({"tool": "save_file", "arguments": {}}), "save_file"),
        (json!({"tool": "render", "arguments": {}}), "render"),
        (
            json!({"tool": "batch", "arguments": {"operations": []}}),
            "batch",
        ),
        (
            json!({"tool": "set_active_object", "arguments": {"index": 0}}),
            "set_active_object",
        ),
        (
            json!({"tool": "fill_box", "arguments": {"wrong": 1}}),
            "wrong",
        ),
        (
            json!({"tool": "fill_box", "arguments": 3}),
            "arguments must be an object",
        ),
        (json!({"name": "fill_box"}), "no field `name`"),
        (json!(5), "batch item 1"),
    ] {
        let reply = head.call("batch", batch_of(json!([fill, bad])));
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("batch item 1"), "{text}");
        assert!(text.contains(expected), "{text}");
        assert!(head.session.active().cells.is_empty(), "{text}");
    }
    assert!(!std::path::Path::new(&path).exists());
    for bad in [
        json!({}),
        json!({"operations": []}),
        json!({"operations": 3}),
    ] {
        let reply = head.call("batch", bad);
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    let many = vec![fill; remote::MAX_BATCH_ITEMS + 1];
    let reply = head.call("batch", batch_of(json!(many)));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(head.session.active().cells.is_empty());
}

#[test]
fn every_batch_tool_is_a_tool_that_writes_no_file() {
    let schema = tools();
    let listed: Vec<&str> = schema
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in remote::batch_tool_names() {
        assert!(listed.contains(&name), "{name}");
        let method = map_tool(name, &json!({})).unwrap().0;
        let args = json!({"operations": [{"tool": name}]});
        let items = remote::batch_items(&args).unwrap();
        assert_eq!(items[0].method, method, "{name}");
        // Its schema takes no path of a file to write.
        let tool = schema.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(
            tool["inputSchema"]["properties"].get("path").is_none(),
            "{name}"
        );
    }
}
