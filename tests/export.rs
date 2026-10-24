use super::quads::*;
use super::*;
use crate::state::{Motion, Session};

#[test]
fn translated_voxel_shifts_the_vertex_and_the_glb_starts_with_magic() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].translation = (2, 0, 0);
    let built = build(&session);
    let positions: Vec<[f32; 3]> = built
        .positions
        .chunks(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();
    assert!(positions.iter().all(|p| p[0] == 2.0 || p[0] == 3.0));
    assert!(positions.iter().any(|p| p[0] == 2.0));
    let path = std::env::temp_dir().join("froxel-export.glb");
    let quads = export_glb(&session, &path).unwrap();
    assert_eq!(quads, 6);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = &bytes[20..20 + json_len];
    assert!(json.windows(10).any(|window| window == b"TEXCOORD_0"));
    assert!(bytes
        .windows(8)
        .any(|window| window == [137, 80, 78, 71, 13, 10, 26, 10]));
    let u = (1.0_f32 + 0.5) / 256.0;
    assert!(built.extra.chunks(2).all(|uv| uv[0] == u && uv[1] == 0.5));
    let _ = std::fs::remove_file(path);
}

#[test]
fn vertex_colors_keep_a_linear_color_attribute() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    let path = std::env::temp_dir().join("froxel-export-vertex.glb");
    export_with(
        &session,
        &path,
        &MeshExport {
            vertex_colors: true,
            merge: false,
            export_hidden: false,
            split_objects: false,
        },
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = &bytes[20..20 + json_len];
    assert!(json.windows(7).any(|window| window == b"COLOR_0"));
    assert!(!json.windows(10).any(|window| window == b"TEXCOORD_0"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn unmerged_row_keeps_one_quad_per_face() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 1);
    let built = build_with(
        &session,
        &MeshExport {
            vertex_colors: false,
            merge: false,
            export_hidden: false,
            split_objects: false,
        },
    );
    assert_eq!(built.indices.len() / 6, 10);
}

#[test]
fn unit_rectangle_matches_the_face_winding() {
    for face in 0..6 {
        let corners = quad_corners(face, (3, 4, 5), 1, 1);
        for (i, corner) in corners.iter().enumerate() {
            assert_eq!(corner[0], 3 + QUADS[face][i][0] as i32);
            assert_eq!(corner[1], 4 + QUADS[face][i][1] as i32);
            assert_eq!(corner[2], 5 + QUADS[face][i][2] as i32);
        }
    }
}

#[test]
fn greedy_merges_a_row_of_one_color() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 1);
    assert_eq!(build(&session).indices.len() / 6, 6);
}

#[test]
fn different_colors_stay_split() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 2);
    assert_eq!(build(&session).indices.len() / 6, 10);
}

#[test]
fn a_different_slot_splits_the_long_edge() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 1);
    session.objects[0].cells.insert((2, 0, 0), 1);
    session.objects[0].cells.insert((2, 1, 0), 2);
    let merged = merged_quads(&session.objects[0].cells, true);
    let area = |quads: &[GreedyQuad]| {
        quads
            .iter()
            .filter(|quad| quad.face == 4 && quad.slot == 1)
            .map(|quad| quad.w * quad.h)
            .sum::<i32>()
    };
    assert!(merged
        .iter()
        .any(|quad| quad.face == 4 && quad.slot == 1 && quad.w == 3));
    assert_eq!(area(&merged), 3);
    let before = merged.len();
    let split = split_junctions(merged);
    assert!(split
        .iter()
        .filter(|quad| quad.face == 4 && quad.slot == 1)
        .all(|quad| quad.w < 3));
    assert_eq!(area(&split), 3);
    assert!(split.len() > before);
}

#[test]
fn hidden_object_is_left_out_of_the_mesh() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects.push(crate::state::Volume {
        name: "Hidden".into(),
        cells: session.objects[0].cells.clone(),
        translation: (3, 0, 0),
        box_min: (0, 0, 0),
        dims: (0, 0, 0),
        rot: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        pivot: (0, 0, 0),
        model_dims: (0, 0, 0),
        hidden: true,
        file: None,
        bone: None,
        script: Default::default(),
    });
    assert_eq!(build(&session).indices.len() / 6, 6);
}

#[test]
fn empty_scene_is_refused() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    let err = export_glb(&session, Path::new("/tmp/froxel-empty.glb")).unwrap_err();
    assert!(err.contains("Nothing"));
}

fn glb_json(bytes: &[u8]) -> serde_json::Value {
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = &bytes[20..20 + json_len];
    let end = json
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .unwrap()
        + 1;
    serde_json::from_slice(&json[..end]).unwrap()
}

fn texcoord_1(bytes: &[u8]) -> Vec<[f32; 2]> {
    let json = glb_json(bytes);
    let index = json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"]
        .as_u64()
        .expect("TEXCOORD_1") as usize;
    let accessor = &json["accessors"][index];
    let view_index = accessor["bufferView"].as_u64().unwrap() as usize;
    let count = accessor["count"].as_u64().unwrap() as usize;
    let offset = json["bufferViews"][view_index]["byteOffset"]
        .as_u64()
        .unwrap() as usize;
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = 20 + json_len + 8;
    (0..count)
        .map(|i| {
            let at = bin + offset + i * 8;
            [
                f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
                f32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()),
            ]
        })
        .collect()
}

#[test]
fn still_export_omits_texcoord_1_and_stays_stable() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    let options = MeshExport {
        vertex_colors: false,
        merge: true,
        export_hidden: false,
        split_objects: false,
    };
    let first = super::encode(&session, &options).unwrap();
    let second = super::encode(&session, &options).unwrap();
    assert_eq!(first, second);
    assert!(!first
        .windows(b"TEXCOORD_1".len())
        .any(|window| window == b"TEXCOORD_1"));
    let colored = super::encode(
        &session,
        &MeshExport {
            vertex_colors: true,
            merge: false,
            export_hidden: false,
            split_objects: false,
        },
    )
    .unwrap();
    assert!(!colored
        .windows(b"TEXCOORD_1".len())
        .any(|window| window == b"TEXCOORD_1"));
    assert!(colored.windows(7).any(|window| window == b"COLOR_0"));
}

#[test]
fn motion_writes_texcoord_1_for_each_slot() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((2, 0, 0), 2);
    session.materials[1].motion = Motion::Sway;
    session.materials[1].motion_amount = 1.5;
    session.materials[2].motion = Motion::Bob;
    session.materials[2].motion_amount = 0.5;
    let options = MeshExport {
        vertex_colors: false,
        merge: true,
        export_hidden: false,
        split_objects: false,
    };
    let bytes = super::encode(&session, &options).unwrap();
    let uvs = texcoord_1(&bytes);
    assert!(uvs.iter().any(|uv| uv == &[1.5, 0.0]));
    assert!(uvs.iter().any(|uv| uv == &[0.0, 0.5]));
    assert!(uvs.iter().all(|uv| *uv == [1.5, 0.0] || *uv == [0.0, 0.5]));

    let colored = super::encode(
        &session,
        &MeshExport {
            vertex_colors: true,
            merge: false,
            export_hidden: false,
            split_objects: false,
        },
    )
    .unwrap();
    assert!(colored.windows(7).any(|window| window == b"COLOR_0"));
    let uvs = texcoord_1(&colored);
    assert!(uvs.iter().any(|uv| uv == &[1.5, 0.0]));

    session
        .add_bone("root", None, [0.5, 0.0, 0.5], None)
        .unwrap();
    let skinned = super::encode(&session, &options).unwrap();
    assert!(skinned.windows(8).any(|window| window == b"JOINTS_0"));
    let uvs = texcoord_1(&skinned);
    assert!(uvs.iter().any(|uv| uv == &[1.5, 0.0]));
    assert!(uvs.iter().any(|uv| uv == &[0.0, 0.5]));
}

#[test]
fn different_motion_stays_on_separate_quads() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 2);
    session.materials[1].motion = Motion::Sway;
    session.materials[2].motion = Motion::Bob;
    let built = build_with(
        &session,
        &MeshExport {
            vertex_colors: false,
            merge: true,
            export_hidden: false,
            split_objects: false,
        },
    );
    assert_eq!(built.indices.len() / 6, 10);
}

#[test]
fn a_clip_is_a_gltf_animation_and_hidden_parts_can_be_their_own_nodes() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].name = "Body".into();
    session
        .add_object(Some("Face"), [4, 0, 0], None, false)
        .unwrap();
    session.objects[1].cells.insert((0, 0, 0), 2);
    session.objects[1].hidden = true;
    session
        .add_bone("root", None, [0.0, 0.0, 0.0], None)
        .unwrap();
    session.bind_object(0, Some("root")).unwrap();
    session.bind_object(1, Some("root")).unwrap();
    session.add_clip("wave").unwrap();
    session
        .set_clip_keys(
            "wave",
            "root",
            vec![crate::rig::ClipKey {
                time: 0.0,
                rotation: Some([0.0, 0.0, 0.0, 1.0]),
                translation: None,
                scale: [1.0, 1.0, 1.0],
            }],
        )
        .unwrap();
    let options = MeshExport {
        vertex_colors: true,
        merge: false,
        export_hidden: false,
        split_objects: false,
    };
    let bytes = super::encode(&session, &options).unwrap();
    let json = glb_json(&bytes);
    let text = serde_json::to_string(&json).unwrap();
    assert!(text.contains("\"animations\""));
    assert!(text.contains("\"wave\""));
    assert!(text.contains("\"node\""));
    assert!(!text.contains("Face"));
    let split = MeshExport {
        vertex_colors: true,
        merge: false,
        export_hidden: true,
        split_objects: true,
    };
    let bytes = super::encode(&session, &split).unwrap();
    let text = serde_json::to_string(&glb_json(&bytes)).unwrap();
    assert!(text.contains("Face"));
    assert!(text.contains("Body"));
    assert!(text.contains("\"skin\""));
}

#[test]
fn neighbouring_quads_share_corners_of_one_slot_and_face() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 1);
    let options = MeshExport {
        vertex_colors: true,
        merge: false,
        export_hidden: false,
        split_objects: false,
    };
    let built = build_with(&session, &options);
    // Ten faces; the four long sides each share two corners between the two
    // voxels: 40 - 4 * 2.
    assert_eq!(built.indices.len() / 6, 10);
    assert_eq!(built.positions.len() / 3, 32);
    assert!(built.indices.iter().all(|&i| (i as usize) < 32));
    session.objects[0].cells.insert((1, 0, 0), 2);
    let built = build_with(&session, &options);
    assert_eq!(built.positions.len() / 3, 40);
}

/// A two-bone rig whose objects carry the bone names, as a character usually does.
fn named_rig() -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].name = "head".into();
    session
        .add_object(Some("jaw"), [0, 0, -2], None, false)
        .unwrap();
    session.objects[1].cells.insert((0, 0, 0), 2);
    session
        .add_bone("head", None, [0.0, 0.0, 0.0], None)
        .unwrap();
    session
        .add_bone("jaw", Some("head"), [0.0, 2.0, -1.0], None)
        .unwrap();
    session.bind_object(0, Some("head")).unwrap();
    session.bind_object(1, Some("jaw")).unwrap();
    session
}

fn split_export() -> MeshExport {
    MeshExport {
        vertex_colors: true,
        merge: false,
        export_hidden: false,
        split_objects: true,
    }
}

#[test]
fn split_mesh_nodes_leave_the_joint_names_alone() {
    let session = named_rig();
    let json = glb_json(&super::encode(&session, &split_export()).unwrap());
    let names: Vec<&str> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["head_mesh", "jaw_mesh", "head", "jaw"]);
    let joints = json["skins"][0]["joints"].as_array().unwrap();
    for (joint, bone) in joints.iter().zip(["head", "jaw"]) {
        assert_eq!(
            json["nodes"][joint.as_u64().unwrap() as usize]["name"],
            bone
        );
    }
    // Meshes keep the object name; only the node gives way.
    assert_eq!(json["meshes"][1]["name"], "jaw");
}

#[test]
fn split_parts_with_the_same_name_get_distinct_nodes() {
    let mut session = named_rig();
    session.objects[1].name = "head".into();
    let json = glb_json(&super::encode(&session, &split_export()).unwrap());
    let names: Vec<&str> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["head_mesh", "head_mesh2", "head", "jaw"]);
}

#[test]
fn a_clip_without_translations_keeps_the_bind_offset() {
    let mut session = named_rig();
    let open = crate::rig::ClipKey {
        time: 0.3,
        rotation: Some([0.3, 0.0, 0.0, 0.954]),
        translation: None,
        scale: [1.0, 1.0, 1.0],
    };
    let rest = crate::rig::ClipKey {
        time: 0.0,
        rotation: Some([0.0, 0.0, 0.0, 1.0]),
        translation: None,
        scale: [1.0, 1.0, 1.0],
    };
    session.add_clip("croak").unwrap();
    session
        .set_clip_keys("croak", "jaw", vec![rest.clone(), open.clone()])
        .unwrap();
    let json = glb_json(&super::encode(&session, &split_export()).unwrap());
    let paths: Vec<&str> = json["animations"][0]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|channel| channel["target"]["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["rotation", "scale"]);

    // One key that moves the jaw: the channel appears, and the key without a translation
    // holds the bind offset rather than snapping the joint to its parent's pivot.
    let moved = crate::rig::ClipKey {
        translation: Some([0.0, 3.0, -1.0]),
        ..open
    };
    session
        .set_clip_keys("croak", "jaw", vec![rest, moved])
        .unwrap();
    let bytes = super::encode(&session, &split_export()).unwrap();
    let json = glb_json(&bytes);
    let channel = &json["animations"][0]["channels"][0];
    assert_eq!(channel["target"]["path"], "translation");
    let sampler = &json["animations"][0]["samplers"][channel["sampler"].as_u64().unwrap() as usize];
    let output = &json["accessors"][sampler["output"].as_u64().unwrap() as usize];
    let view = &json["bufferViews"][output["bufferView"].as_u64().unwrap() as usize];
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = &bytes[20 + json_len + 8..];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let values: Vec<f32> = bin[start..start + 24]
        .chunks(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let bind = crate::rig::bind_pose(&session.skeleton).unwrap();
    assert_eq!(&values[..3], &bind[1].translation[..]);
    assert_eq!(&values[3..], &crate::rig::gltf_vector([0.0, 3.0, -1.0])[..]);
}

#[test]
fn a_clip_without_rotations_keeps_the_bind_rotation() {
    let mut session = named_rig();
    let turned = [0.0, 0.0, 0.38268343, 0.9238795];
    session.skeleton.bones[1].rotation = turned;
    let key = |time: f32, translation: Option<[f32; 3]>, rotation: Option<[f32; 4]>| {
        crate::rig::ClipKey {
            time,
            rotation,
            translation,
            scale: [1.0, 1.0, 1.0],
        }
    };
    session.add_clip("slide").unwrap();
    session
        .set_clip_keys(
            "slide",
            "jaw",
            vec![key(0.0, None, None), key(0.4, Some([0.0, 3.0, -1.0]), None)],
        )
        .unwrap();
    let json = glb_json(&super::encode(&session, &split_export()).unwrap());
    let paths: Vec<&str> = json["animations"][0]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|channel| channel["target"]["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["translation", "scale"]);

    // A rotation on one key only: the other key holds the bind rotation, not identity.
    session
        .set_clip_keys(
            "slide",
            "jaw",
            vec![
                key(0.0, None, None),
                key(0.4, None, Some([0.0, 0.0, 0.0, 1.0])),
            ],
        )
        .unwrap();
    let bytes = super::encode(&session, &split_export()).unwrap();
    let json = glb_json(&bytes);
    let channel = &json["animations"][0]["channels"][0];
    assert_eq!(channel["target"]["path"], "rotation");
    let sampler = &json["animations"][0]["samplers"][channel["sampler"].as_u64().unwrap() as usize];
    let output = &json["accessors"][sampler["output"].as_u64().unwrap() as usize];
    let view = &json["bufferViews"][output["bufferView"].as_u64().unwrap() as usize];
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = &bytes[20 + json_len + 8..];
    let start = view["byteOffset"].as_u64().unwrap() as usize;
    let values: Vec<f32> = bin[start..start + 32]
        .chunks(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let bind = crate::rig::bind_pose(&session.skeleton).unwrap();
    for (got, want) in values[..4].iter().zip(bind[1].rotation) {
        assert!((got - want).abs() < 1e-6);
    }
    assert!((values[7] - 1.0).abs() < 1e-6);
}
