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
    assert_eq!(body["motion"][4]["kind"], "sway");
    assert_eq!(body["motion"][4]["amount"], 1.5);
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
