use std::collections::HashMap;

use super::*;

#[test]
fn rejects_a_cycle_and_a_duplicate_name() {
    let mut skeleton = Skeleton::default();
    skeleton.add("hips", None, [0.0, 0.0, 0.0], None).unwrap();
    skeleton
        .add("leg", Some("hips"), [0.0, 1.0, 0.0], None)
        .unwrap();
    let err = skeleton
        .set("hips", Some(Some("leg".into())), None, None, None)
        .unwrap_err();
    assert!(err.contains("leg"), "{err}");
    let err = skeleton
        .add("leg", Some("hips"), [0.0; 3], None)
        .unwrap_err();
    assert!(err.contains("leg"), "{err}");
}

#[test]
fn chunk_round_trips_and_rejects_an_unknown_version() {
    let mut skeleton = Skeleton::default();
    skeleton.add("hips", None, [0.5, 1.5, 0.0], None).unwrap();
    skeleton
        .add(
            "leg",
            Some("hips"),
            [0.5, 0.0, 0.0],
            Some(IDENTITY_ROTATION),
        )
        .unwrap();
    let objects = [
        ObjectRef {
            trn: -1,
            name: "Body",
            bone: Some("hips"),
        },
        ObjectRef {
            trn: -1,
            name: "Leg",
            bone: Some("leg"),
        },
    ];
    let body = encode_chunk(&objects, &skeleton).unwrap().unwrap();
    let (loaded, binds) = decode_chunk(&body).unwrap();
    assert_eq!(loaded, skeleton);
    assert_eq!(binds[1].bone, "leg");
    let assigned = assign_bones(&[(-1, "Body"), (-1, "Leg")], &binds, &loaded).unwrap();
    assert_eq!(assigned[0].as_deref(), Some("hips"));
    assert_eq!(assigned[1].as_deref(), Some("leg"));
    let mut bad = body.clone();
    bad[0] = 99;
    let err = decode_chunk(&bad).unwrap_err();
    assert!(err.contains("version 99"), "{err}");
}

#[test]
fn identity_pose_puts_the_joint_on_the_pivot() {
    let mut skeleton = Skeleton::default();
    skeleton.add("hips", None, [1.0, 2.0, 3.0], None).unwrap();
    let pose = bind_pose(&skeleton).unwrap();
    assert!((pose[0].translation[0] - 1.0).abs() < 1e-5);
    assert!((pose[0].translation[1] - 3.0).abs() < 1e-5);
    assert!((pose[0].translation[2] - -2.0).abs() < 1e-5);
    let inverse = Mat4::from_cols_array(&pose[0].inverse_bind);
    let world = Mat4::from_translation(Vec3::new(1.0, 3.0, -2.0));
    let point = Vec3::new(4.0, 3.0, -2.0);
    let skinned = world.transform_point3(inverse.transform_point3(point));
    assert!((skinned - point).length() < 1e-4);
}

#[test]
fn a_parent_pose_orbits_the_child_and_identity_is_a_no_op() {
    let mut skeleton = Skeleton::default();
    skeleton.add("root", None, [0.0, 0.0, 0.0], None).unwrap();
    skeleton
        .add("child", Some("root"), [2.0, 0.0, 0.0], None)
        .unwrap();
    let rest = skin_deltas(&skeleton, &HashMap::new()).unwrap();
    let point = Vec3::new(3.0, 1.0, 2.0);
    for delta in &rest {
        let moved = delta.transform_point3(point);
        assert!((moved - point).length() < 1e-4, "{moved}");
    }
    let half = std::f32::consts::FRAC_PI_2 * 0.5;
    let mut pose = Pose::new();
    pose.insert(
        "root".into(),
        BonePose::rotation([0.0, 0.0, half.sin(), half.cos()]),
    );
    let joints = posed_joints(&skeleton, &pose).unwrap();
    assert!(joints[1].0.x.abs() < 1e-4, "{}", joints[1].0);
    assert!((joints[1].0.y - 2.0).abs() < 1e-4, "{}", joints[1].0);
    assert!(joints[1].0.z.abs() < 1e-4, "{}", joints[1].0);
}

#[test]
fn clips_round_trip_through_the_mvan_body() {
    let clip = Clip {
        name: "wave".into(),
        tracks: vec![BoneTrack {
            bone: "root".into(),
            keys: vec![
                ClipKey {
                    time: 0.25,
                    rotation: Some(IDENTITY_ROTATION),
                    translation: Some([1.0, 2.0, 3.0]),
                    scale: [1.0, 1.0, 2.0],
                },
                ClipKey {
                    time: 0.5,
                    rotation: None,
                    translation: None,
                    scale: [1.0, 1.0, 1.0],
                },
            ],
        }],
    };
    let body = encode_clips(&[clip.clone()]).unwrap().unwrap();
    assert_eq!(decode_clips(&body).unwrap(), vec![clip]);
    assert!(encode_clips(&[]).unwrap().is_none());
}

#[test]
fn a_version_1_mvan_body_reads_every_key_with_its_translation() {
    // Version 1 had no flag: time, rotation, translation, scale for every key.
    let mut body = Vec::new();
    for word in [1u32, 1] {
        body.extend_from_slice(&word.to_le_bytes());
    }
    for text in ["wave"] {
        body.extend_from_slice(&(text.len() as u32).to_le_bytes());
        body.extend_from_slice(text.as_bytes());
    }
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"root");
    body.extend_from_slice(&1u32.to_le_bytes());
    for value in [0.0f32, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0, 1.0, 1.0, 1.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let clips = decode_clips(&body).unwrap();
    assert_eq!(
        clips[0].tracks[0].keys[0].translation,
        Some([1.0, 2.0, 3.0])
    );
}

#[test]
fn a_version_2_mvan_body_keeps_its_rotation_and_reads_the_translation_flag() {
    // Version 2: time, rotation, a translation flag, translation, scale.
    let mut body = Vec::new();
    for word in [2u32, 1] {
        body.extend_from_slice(&word.to_le_bytes());
    }
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"wave");
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"root");
    body.extend_from_slice(&1u32.to_le_bytes());
    for value in [0.0f32, 0.0, 0.0, 0.6, 0.8] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.push(0);
    for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let key = &decode_clips(&body).unwrap()[0].tracks[0].keys[0];
    assert_eq!(key.rotation, Some([0.0, 0.0, 0.6, 0.8]));
    assert_eq!(key.translation, None);
}

#[test]
fn a_scaled_parent_carries_its_child_and_scales_the_skin() {
    let mut skeleton = Skeleton::default();
    skeleton.add("root", None, [0.0, 0.0, 0.0], None).unwrap();
    skeleton
        .add("arm", Some("root"), [2.0, 0.0, 0.0], None)
        .unwrap();
    let mut pose = Pose::new();
    pose.insert(
        "root".into(),
        BonePose {
            rotation: IDENTITY_ROTATION,
            scale: [2.0, 1.0, 1.0],
        },
    );
    let joints = posed_joints(&skeleton, &pose).unwrap();
    assert!((joints[1].0 - Vec3::new(4.0, 0.0, 0.0)).length() < 1e-5);
    let deltas = skin_deltas(&skeleton, &pose).unwrap();
    let point = deltas[0].transform_point3(Vec3::new(1.0, 1.0, 0.0));
    assert!((point - Vec3::new(2.0, 1.0, 0.0)).length() < 1e-5);
    // A zero scale flattens the bone but leaves the rotations defined.
    pose.insert(
        "arm".into(),
        BonePose {
            rotation: IDENTITY_ROTATION,
            scale: [1.0, 0.0, 1.0],
        },
    );
    let joints = posed_joints(&skeleton, &pose).unwrap();
    assert!(joints[1].1.is_finite());
    assert!(
        skin_deltas(&skeleton, &pose).unwrap()[1]
            .determinant()
            .abs()
            < 1e-6
    );
}
