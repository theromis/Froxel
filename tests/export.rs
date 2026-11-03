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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
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
            color_atlas: false,
            ao_strength: 0.0,
            ao_radius: 0,
            ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
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

fn atlas_export() -> MeshExport {
    MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: true,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
    }
}

#[test]
fn colour_atlas_merges_across_slots_and_paints_each_cell() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 2);
    session.palette[1] = [200, 10, 20, 255];
    session.palette[2] = [30, 40, 250, 255];
    let built = build_with(&session, &atlas_export());
    // A 2x1x1 box: six quads, as one slot would give, not ten.
    assert_eq!(built.indices.len() / 6, 6);
    let (width, height, pixels) = &built.atlas;
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    // Each vertex's UV sits on a texel corner, and the texel just inside every
    // corner of the top face holds that cell's colour.
    let uvs: Vec<[f32; 2]> = built.extra.chunks(2).map(|uv| [uv[0], uv[1]]).collect();
    let texel = |u: f32, v: f32| {
        let x = (u * *width as f32).floor() as u32;
        let y = (v * *height as f32).floor() as u32;
        let at = ((y * width + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    };
    for quad in built.indices.chunks(6) {
        let corners = [quad[0], quad[1], quad[2], quad[5]];
        let center_u = corners.iter().map(|&i| uvs[i as usize][0]).sum::<f32>() / 4.0;
        let center_v = corners.iter().map(|&i| uvs[i as usize][1]).sum::<f32>() / 4.0;
        for &i in &corners {
            let uv = uvs[i as usize];
            assert_eq!((uv[0] * *width as f32).fract(), 0.0);
            assert_eq!((uv[1] * *height as f32).fract(), 0.0);
            // A step from the corner toward the quad's centre lands in the
            // texel of the cell at that corner.
            let inside = texel(
                uv[0] + (center_u - uv[0]).signum() * 0.5 / *width as f32,
                uv[1] + (center_v - uv[1]).signum() * 0.5 / *height as f32,
            );
            assert!(inside == session.palette[1] || inside == session.palette[2]);
        }
    }
    // Both colours are in the atlas, one texel per exposed cell face (10).
    let painted: Vec<[u8; 4]> = pixels.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
    assert_eq!(
        painted.iter().filter(|c| **c == session.palette[1]).count(),
        5
    );
    assert_eq!(
        painted.iter().filter(|c| **c == session.palette[2]).count(),
        5
    );
}

#[test]
fn colour_atlas_keeps_cells_of_the_top_face_in_place() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 2);
    session.palette[1] = [200, 10, 20, 255];
    session.palette[2] = [30, 40, 250, 255];
    let built = build_with(&session, &atlas_export());
    let (width, height, pixels) = &built.atlas;
    let positions: Vec<[f32; 3]> = built
        .positions
        .chunks(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let uvs: Vec<[f32; 2]> = built.extra.chunks(2).map(|uv| [uv[0], uv[1]]).collect();
    // The top face (+z in the document, +y in glTF) spans x 0..2. Interpolate
    // the UV at x 0.5 and 1.5 along its edge and read the texel there.
    let top: Vec<usize> = built
        .indices
        .chunks(6)
        .find(|quad| quad.iter().all(|&i| positions[i as usize][1] == 1.0))
        .unwrap()
        .iter()
        .map(|&i| i as usize)
        .collect();
    let corner = |x: f32, z: f32| {
        top.iter()
            .copied()
            .find(|&i| positions[i][0] == x && positions[i][2] == z)
            .unwrap()
    };
    let (a, b, c) = (corner(0.0, 0.0), corner(2.0, 0.0), corner(0.0, -1.0));
    let sample = |s: f32, t: f32| {
        let u = uvs[a][0] + (uvs[b][0] - uvs[a][0]) * s + (uvs[c][0] - uvs[a][0]) * t;
        let v = uvs[a][1] + (uvs[b][1] - uvs[a][1]) * s + (uvs[c][1] - uvs[a][1]) * t;
        let x = (u * *width as f32) as u32;
        let y = (v * *height as f32) as u32;
        let at = ((y * width + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
    };
    assert_eq!(sample(0.25, 0.5), session.palette[1]);
    assert_eq!(sample(0.75, 0.5), session.palette[2]);
}

#[test]
fn colour_atlas_keeps_different_motion_apart_and_refuses_split_meshes() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((1, 0, 0), 2);
    session.objects[0].cells.insert((2, 0, 0), 3);
    session.materials[1].motion = Motion::Sway;
    session.materials[1].motion_amount = 1.0;
    session.materials[2].motion = Motion::Sway;
    session.materials[2].motion_amount = 1.0;
    // Slots 1 and 2 sway alike and merge; still slot 3 stays apart.
    let built = build_with(&session, &atlas_export());
    assert_eq!(built.indices.len() / 6, 10);
    let bytes = super::encode(&session, &atlas_export()).unwrap();
    let json = glb_json(&bytes);
    assert_eq!(json["materials"][0]["name"], "voxel-atlas");
    assert_eq!(json["images"].as_array().unwrap().len(), 1);
    let uvs = texcoord_1(&bytes);
    assert!(uvs.iter().any(|uv| uv == &[1.0, 0.0]));
    assert!(uvs.iter().any(|uv| uv == &[0.0, 0.0]));
    let mut split = atlas_export();
    split.split_objects = true;
    assert!(super::encode(&session, &split).is_err());
}

#[test]
fn colour_atlas_leaves_no_t_junction() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    // A 3x2 floor with a one-cell step and a cell of another colour: the
    // step's faces end in the middle of the merged floor's edges.
    for x in 0..3 {
        for y in 0..2 {
            session.objects[0]
                .cells
                .insert((x, y, 0), 1 + (x == 2) as u8);
        }
    }
    session.objects[0].cells.insert((1, 0, 1), 1);
    let mut options = atlas_export();
    options.merge = true;
    let built = build_with(&session, &options);
    let positions: Vec<[f32; 3]> = built
        .positions
        .chunks(3)
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    // No vertex of the mesh lies strictly inside any triangle edge.
    for tri in built.indices.chunks(3) {
        for edge in 0..3 {
            let a = positions[tri[edge] as usize];
            let b = positions[tri[(edge + 1) % 3] as usize];
            for p in &positions {
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
                let cross = [
                    ab[1] * ap[2] - ab[2] * ap[1],
                    ab[2] * ap[0] - ab[0] * ap[2],
                    ab[0] * ap[1] - ab[1] * ap[0],
                ];
                let t = (ab[0] * ap[0] + ab[1] * ap[1] + ab[2] * ap[2])
                    / (ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2]);
                let on_line = cross.iter().all(|c| c.abs() < 1e-6);
                assert!(
                    !(on_line && t > 1e-6 && t < 1.0 - 1e-6),
                    "T-junction at {p:?} on {a:?}-{b:?}"
                );
            }
        }
    }
    // Fewer triangles than the per-slot merge with its splits.
    assert!(built.indices.len() < build(&session).indices.len());
}

fn ao_export(strength: f32, radius: u32) -> MeshExport {
    MeshExport {
        vertex_colors: false,
        merge: true,
        export_hidden: false,
        split_objects: false,
        color_atlas: false,
        ao_strength: strength,
        ao_radius: radius,
        ao_target: AoTarget::Auto,
    }
}

fn occlusion_of(session: &Session, radius: u32) -> super::occlusion::Occlusion {
    super::occlusion::Occlusion::new(session, &ao_export(1.0, radius)).unwrap()
}

#[test]
fn corner_occlusion_counts_the_sides_and_the_diagonal() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..4 {
        for y in 0..4 {
            session.objects[0].cells.insert((x, y, 0), 1);
        }
    }
    // One cell on the floor, and two more that touch only at a corner.
    session.objects[0].cells.insert((1, 1, 1), 1);
    session.objects[0].cells.insert((3, 3, 1), 1);
    session.objects[0].cells.insert((2, 2, 1), 1);
    session.objects[0].cells.insert((0, 3, 1), 1);
    session.objects[0].cells.insert((1, 3, 1), 1);
    let occlusion = occlusion_of(&session, 0);
    let up = 4;
    // Open floor keeps all its light.
    assert_eq!(occlusion.shade([0, 0, 1], up), 255);
    // A corner of the lone cell: one side, a quarter of the light gone.
    assert_eq!(occlusion.shade([1, 1, 1], up), 191);
    // Between (1,1) and (2,2): two diagonal cells count as both sides, three.
    assert_eq!(occlusion.shade([2, 2, 1], up), 64);
    // Two cells side by side in front of the corner.
    assert_eq!(occlusion.shade([1, 3, 1], up), 128);
    // The four corners of one floor face, in (u, v) order.
    assert_eq!(occlusion.corners(up, (0, 0, 0)), [255, 255, 255, 191]);
    // Strength scales the loss.
    let half = super::occlusion::Occlusion::new(&session, &ao_export(0.5, 0)).unwrap();
    assert_eq!(half.shade([1, 1, 1], up), 223);
    // The hemisphere term darkens a floor corner next to a wall more than
    // the corner term alone.
    let wide = occlusion_of(&session, 3);
    assert!(wide.shade([1, 1, 1], up) < 191);
}

#[test]
fn greedy_merge_keeps_apart_faces_whose_corner_occlusion_differs() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    // A floor strip with a ledge along one long side: every top face of the
    // strip shades the same way, darker on the ledge side, so the strip still
    // merges along x but not across.
    for x in 0..6 {
        session.objects[0].cells.insert((x, 0, 0), 1);
        session.objects[0].cells.insert((x, 1, 0), 1);
        session.objects[0].cells.insert((x, 2, 1), 1);
    }
    // One post makes the shades change along x near it.
    session.objects[0].cells.insert((5, 1, 1), 1);
    let occlusion = occlusion_of(&session, 0);
    let shade = |face: usize, cell: Cell| occlusion.corners(face, cell);
    let plain = merged_quads(&session.objects[0].cells, true);
    let shaded = merged_quads_shaded(&session.objects[0].cells, true, Some(&shade));
    let top = |quads: &[GreedyQuad]| {
        quads
            .iter()
            .filter(|quad| quad.face == 4 && quad.cell.2 == 0)
            .count()
    };
    // Without occlusion the post's cell splits the floor into two rectangles.
    assert_eq!(top(&plain), 2);
    assert!(top(&shaded) > top(&plain));
    assert!(shaded
        .iter()
        .any(|quad| quad.face == 4 && quad.cell.2 == 0 && quad.w > 1));
    // Interpolating a merged quad's corners gives each cell its own shades.
    for quad in &shaded {
        let (_, u_axis, v_axis) = face_axes(quad.face);
        let origin = split_origin(quad);
        let corner = |du: i32, dv: i32| {
            let mut point = origin;
            point[u_axis] += du;
            point[v_axis] += dv;
            occlusion.shade(point, quad.face) as f32
        };
        let (c00, c10, c01, c11) = (
            corner(0, 0),
            corner(quad.w, 0),
            corner(0, quad.h),
            corner(quad.w, quad.h),
        );
        for j in 0..=quad.h {
            for i in 0..=quad.w {
                let (s, t) = (i as f32 / quad.w as f32, j as f32 / quad.h as f32);
                let want = c00 * (1.0 - s) * (1.0 - t)
                    + c10 * s * (1.0 - t)
                    + c01 * (1.0 - s) * t
                    + c11 * s * t;
                assert!(
                    (corner(i, j) - want).abs() < 1e-3,
                    "quad {:?} face {} cell {:?} at ({i}, {j})",
                    (quad.w, quad.h),
                    quad.face,
                    quad.cell
                );
            }
        }
    }
}

#[test]
fn occlusion_adds_a_grey_color_to_the_texture_and_darkens_vertex_colors() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..3 {
        session.objects[0].cells.insert((x, 0, 0), 1);
    }
    session.objects[0].cells.insert((1, 0, 1), 1);
    let off = super::encode(&session, &ao_export(0.0, 3)).unwrap();
    let mut plain = ao_export(0.0, 3);
    plain.ao_radius = 0;
    assert_eq!(off, super::encode(&session, &plain).unwrap());
    assert!(glb_json(&off)["meshes"][0]["primitives"][0]["attributes"]
        .get("COLOR_0")
        .is_none());
    let on = super::encode(&session, &ao_export(1.0, 3)).unwrap();
    let json = glb_json(&on);
    let attributes = &json["meshes"][0]["primitives"][0]["attributes"];
    assert!(attributes.get("TEXCOORD_0").is_some());
    let color = &json["accessors"][attributes["COLOR_0"].as_u64().unwrap() as usize];
    assert_eq!(color["type"], "VEC4");
    assert_eq!(
        color["count"],
        json["accessors"][attributes["POSITION"].as_u64().unwrap() as usize]["count"]
    );
    super::gltf_structure(&on).unwrap();

    let built = build_with(&session, &ao_export(1.0, 0));
    assert!(built.shade.iter().any(|shade| *shade < 1.0));
    assert!(built.shade.iter().any(|shade| *shade == 1.0));
    let mut colors = ao_export(1.0, 0);
    colors.vertex_colors = true;
    let lit = build_with(
        &session,
        &MeshExport {
            ao_strength: 0.0,
            ..colors
        },
    );
    let dark = build_with(&session, &colors);
    assert!(dark.shade.is_empty());
    let sum = |built: &Built| built.extra.chunks(4).map(|c| c[0]).sum::<f32>();
    assert!(sum(&dark) < sum(&lit));
}

#[test]
fn colour_atlas_occlusion_darkens_texels_and_merges_as_without_it() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..4 {
        for y in 0..4 {
            session.objects[0]
                .cells
                .insert((x, y, 0), 1 + ((x + y) % 2) as u8);
        }
    }
    session.objects[0].cells.insert((1, 1, 1), 3);
    let plain = build_with(&session, &atlas_export());
    let mut options = atlas_export();
    options.ao_strength = 1.0;
    options.ao_radius = 0;
    let built = build_with(&session, &options);
    assert!(built.shade.is_empty());
    assert_eq!(built.indices, plain.indices);
    assert_eq!(built.positions, plain.positions);
    assert_eq!(built.extra, plain.extra);
    // Same layout, so the texels compare one by one: none brighter, and the
    // floor cells around the post darker.
    let (_, _, lit) = &plain.atlas;
    let (_, _, dark) = &built.atlas;
    assert_eq!(lit.len(), dark.len());
    let darker = lit
        .chunks(4)
        .zip(dark.chunks(4))
        .filter(|(a, b)| {
            assert!(b[0] <= a[0] && b[1] <= a[1] && b[2] <= a[2]);
            assert_eq!(a[3], b[3]);
            b[..3] != a[..3]
        })
        .count();
    assert!(darker >= 8);
    let bytes = super::encode(&session, &options).unwrap();
    assert!(glb_json(&bytes)["meshes"][0]["primitives"][0]["attributes"]
        .get("COLOR_0")
        .is_none());
}

#[test]
fn srgb_inverts_linear() {
    for byte in 0..=255u8 {
        assert_eq!(srgb(linear(byte)), byte);
    }
}

#[test]
fn ao_target_picks_vertex_colours_the_atlas_or_both() {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..4 {
        for y in 0..4 {
            session.objects[0]
                .cells
                .insert((x, y, 0), 1 + ((x + y) % 2) as u8);
        }
    }
    session.objects[0].cells.insert((1, 1, 1), 3);
    let with = |target: AoTarget| {
        let mut options = atlas_export();
        options.ao_strength = 1.0;
        options.ao_target = target;
        options
    };
    let plain = build_with(&session, &atlas_export());
    let auto = build_with(&session, &with(AoTarget::Auto));
    let texture = build_with(&session, &with(AoTarget::Texture));
    let vertex = build_with(&session, &with(AoTarget::Vertex));
    let both = build_with(&session, &with(AoTarget::Both));
    // Auto with the atlas is the texture.
    assert_eq!(auto.atlas, texture.atlas);
    assert_eq!(auto.indices, plain.indices);
    // Vertex keeps the atlas texels and adds a shade per vertex, at the cost
    // of the faces it no longer merges.
    assert_eq!(
        vertex.atlas.2.len(),
        vertex.atlas.0 as usize * vertex.atlas.1 as usize * 4
    );
    assert_eq!(vertex.shade.len(), vertex.positions.len() / 3);
    assert!(vertex.indices.len() > plain.indices.len());
    assert!(texture.shade.is_empty());
    // Both is the vertex geometry with darkened texels too.
    assert_eq!(both.indices, vertex.indices);
    assert_eq!(both.shade, vertex.shade);
    assert_ne!(both.atlas.2, vertex.atlas.2);
    let color = |target| {
        glb_json(&super::encode(&session, &with(target)).unwrap())["meshes"][0]["primitives"][0]
            ["attributes"]
            .get("COLOR_0")
            .is_some()
    };
    assert!(!color(AoTarget::Texture));
    assert!(color(AoTarget::Vertex));
    assert!(color(AoTarget::Both));
    // Texture needs the atlas; vertex without it is the palette export.
    let mut palette = with(AoTarget::Texture);
    palette.color_atlas = false;
    assert!(super::encode(&session, &palette).is_err());
    palette.ao_target = AoTarget::Vertex;
    assert!(super::encode(&session, &palette).is_ok());
    for name in ["auto", "vertex", "texture", "both"] {
        assert_eq!(AoTarget::parse(name).unwrap().name(), name);
    }
    assert!(AoTarget::parse("texel").is_none());
}

/// A stepped two-colour slab, a second object, and a hidden one, so merge,
/// the colour atlas, T-junctions, and hidden objects all change the count.
fn counted_scene() -> Session {
    let mut session = Session::new();
    let cells = &mut session.objects[0].cells;
    cells.clear();
    for x in 0..9 {
        for y in 0..5 {
            for z in 0..=(x / 3) {
                cells.insert((x, y, z), if (x + y) % 4 == 0 { 2 } else { 1 });
            }
        }
    }
    let other = session
        .add_object(Some("post"), [12, 0, 0], None, false)
        .unwrap();
    for z in 0..4 {
        session.objects[other].cells.insert((0, 0, z), 3);
    }
    let hidden = session
        .add_object(Some("hidden"), [0, 9, 0], None, false)
        .unwrap();
    session.objects[hidden].cells.insert((0, 0, 0), 1);
    session.objects[hidden].hidden = true;
    session
}

#[test]
fn counts_are_the_triangles_and_quads_the_glb_has() {
    let session = counted_scene();
    let options = |merge, color_atlas, split_objects, export_hidden, ao_strength: f32| MeshExport {
        vertex_colors: false,
        merge,
        export_hidden,
        split_objects,
        color_atlas,
        ao_strength,
        ao_radius: 2,
        ao_target: AoTarget::Auto,
    };
    let mut seen = Vec::new();
    for (merge, atlas, split, hidden, ao) in [
        (false, false, false, false, 0.0),
        (false, false, false, true, 0.0),
        (true, false, false, false, 0.0),
        (true, false, true, false, 0.0),
        (false, true, false, false, 0.0),
        (true, false, false, false, 0.6),
        (false, true, false, false, 0.6),
        (false, false, true, true, 0.6),
    ] {
        let options = options(merge, atlas, split, hidden, ao);
        let found = counts(&session, &options).unwrap();
        let bytes = super::encode(&session, &options).unwrap();
        assert_eq!(
            found.triangles,
            glb_triangles(&bytes).unwrap(),
            "{options:?}"
        );
        let built = super::build_with(&session, &options);
        assert_eq!(found.quads, built.indices.len() / 6, "{options:?}");
        seen.push(found.triangles);
    }
    // Unmerged is every exposed face; merging and the atlas cut it.
    assert!(seen[2] < seen[0] && seen[4] < seen[2], "{seen:?}");
    assert_eq!(seen[1], seen[0] + 12, "the hidden cube's six faces");
    // The same refusals as the export.
    let bad = MeshExport {
        ao_target: AoTarget::Texture,
        ..options(false, false, false, false, 0.5)
    };
    assert!(counts(&session, &bad).unwrap_err().contains("colorAtlas"));
}
