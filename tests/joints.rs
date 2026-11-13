use super::*;
use crate::remote::apply;
use serde_json::json;

fn fill(session: &mut Session, index: usize, min: [i32; 3], max: [i32; 3], slot: u8) {
    apply(session, "object.set_active", &json!({"index": index})).unwrap();
    apply(
        session,
        "voxel.fill",
        &json!({"x0": min[0], "y0": min[1], "z0": min[2],
                "x1": max[0], "y1": max[1], "z1": max[2], "palette": slot}),
    )
    .unwrap();
}

/// A body 20 x 30 x 10 cells (world x 0..19, y 0..29, z 10..19), four legs
/// 4 x 4 x 10 under its corners touching it, a head in front of it with a
/// two-cell gap, and a tail that overlaps its back by one row. Every part is
/// its own object bound to its own bone; the legs hang from a `hips` bone
/// with no object, whose parent is `body`.
fn creature() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "name": "body"}),
    )
    .unwrap();
    fill(&mut session, 0, [0, 0, 10], [19, 29, 19], 1);
    let parts: [(&str, [i32; 3], [i32; 3]); 6] = [
        ("leg_fl", [0, 0, 0], [3, 3, 9]),
        ("leg_fr", [16, 0, 0], [19, 3, 9]),
        ("leg_bl", [0, 26, 0], [3, 29, 9]),
        ("leg_br", [16, 26, 0], [19, 29, 9]),
        ("head", [6, -10, 12], [13, -3, 19]),
        ("tail", [8, 29, 12], [11, 36, 15]),
    ];
    // The tail sits one cell inside the body (y 29), as a copy split leaves it.
    for (i, (name, min, max)) in parts.iter().enumerate() {
        apply(&mut session, "object.add", &json!({"name": name})).unwrap();
        fill(&mut session, i + 1, *min, *max, 2);
    }
    apply(
        &mut session,
        "rig.add",
        &json!({"name": "body", "pivot": [0, 0, 0]}),
    )
    .unwrap();
    apply(
        &mut session,
        "rig.add",
        &json!({"name": "hips", "parent": "body", "pivot": [0, 0, 0]}),
    )
    .unwrap();
    apply(
        &mut session,
        "rig.bind",
        &json!({"index": 0, "bone": "body"}),
    )
    .unwrap();
    for (i, (name, _, _)) in parts.iter().enumerate() {
        let parent = if name.starts_with("leg") {
            "hips"
        } else {
            "body"
        };
        apply(
            &mut session,
            "rig.add",
            &json!({"name": name, "parent": parent, "pivot": [0, 0, 0]}),
        )
        .unwrap();
        apply(
            &mut session,
            "rig.bind",
            &json!({"index": i + 1, "bone": name}),
        )
        .unwrap();
    }
    session
}

fn pivot(session: &Session, bone: &str) -> [f32; 3] {
    let index = session.skeleton.index_of(bone).unwrap();
    session.skeleton.bones[index].pivot
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn a_body_with_four_legs_a_head_and_a_tail_gets_its_joints() {
    let mut session = creature();
    let reply = apply(&mut session, "rig.joints", &json!({})).unwrap();
    let placed = reply["placed"].as_array().unwrap();
    assert_eq!(placed.len(), 6, "{reply}");
    // The root and the empty hips are skipped with a reason.
    let skipped: Vec<&str> = reply["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(skipped, ["body", "hips"]);
    // A leg touches the body's underside: the centre of its top face.
    assert!(close(pivot(&session, "leg_fl"), [2.0, 2.0, 10.0]));
    assert!(close(pivot(&session, "leg_br"), [18.0, 28.0, 10.0]));
    let leg = placed.iter().find(|p| p["name"] == "leg_fl").unwrap();
    assert_eq!(leg["method"], "contact");
    assert_eq!(
        leg["parent"], "body",
        "the legs meet the body past the empty hips"
    );
    assert_eq!(leg["samples"], 16);
    // The head is two cells in front: the middle of the gap, on its face.
    let head = placed.iter().find(|p| p["name"] == "head").unwrap();
    assert_eq!(head["method"], "nearest");
    assert_eq!(head["gap"], 2.0);
    let [x, y, z] = pivot(&session, "head");
    assert!((x - 10.0).abs() < 0.01 && (y + 1.0).abs() < 0.01, "{x} {y}");
    assert!((15.0..=17.0).contains(&z), "{z}");
    // The tail overlaps the body's back row: the centre of that overlap.
    let tail = placed.iter().find(|p| p["name"] == "tail").unwrap();
    assert_eq!(tail["method"], "overlap");
    assert!(close(pivot(&session, "tail"), [10.0, 29.5, 14.0]));
    // One undo step puts every pivot back.
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert!(close(pivot(&session, "leg_fl"), [0.0, 0.0, 0.0]));
    assert!(close(pivot(&session, "head"), [0.0, 0.0, 0.0]));
}

#[test]
fn set_bone_joint_snaps_and_refuses_a_root() {
    let mut session = creature();
    let reply = apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "joint": true, "snap": "center"}),
    )
    .unwrap();
    assert_eq!(reply["method"], "overlap");
    assert!(close(pivot(&session, "tail"), [10.5, 29.5, 14.5]));
    apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "joint": true, "snap": "corner"}),
    )
    .unwrap();
    assert!(close(pivot(&session, "tail"), [10.0, 30.0, 14.0]));
    let err = apply(
        &mut session,
        "rig.set",
        &json!({"name": "body", "joint": true}),
    )
    .unwrap_err();
    assert!(err.contains("no parent"), "{err}");
    let err = apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "joint": true, "pivot": [1, 2, 3]}),
    )
    .unwrap_err();
    assert!(err.contains("not both"), "{err}");
    // A named bone that cannot be placed is an error, and nothing moves.
    let before = session.skeleton.clone();
    let err = apply(
        &mut session,
        "rig.joints",
        &json!({"bones": ["head", "hips"]}),
    )
    .unwrap_err();
    assert!(err.contains("hips"), "{err}");
    assert_eq!(session.skeleton, before);
}

#[test]
fn place_joints_runs_from_a_script_and_the_panel() {
    let mut session = creature();
    session.edit.joint_snap = 3;
    session.place_joints_from_panel();
    assert!(session.status.starts_with("Placed 6"), "{}", session.status);
    assert!(close(pivot(&session, "tail"), [10.0, 29.5, 14.0]));
    let mut session = creature();
    session
        .set_script(1, "fill_box(0, 0, 0, 3, 3, 9, 2);\nplace_joints(#{bones: [\"leg_fl\"]});\nset_bone(\"head\", #{joint: true, snap: \"half\"});")
        .unwrap();
    session.regenerate_script(1).unwrap();
    assert!(close(pivot(&session, "leg_fl"), [2.0, 2.0, 10.0]));
    assert!(close(
        pivot(&session, "head"),
        [10.0, -1.0, pivot(&session, "head")[2]]
    ));
}

#[test]
fn set_bone_rest_and_rest_from_pose_are_one_undo_step_each() {
    let mut session = creature();
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let reply = apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "rest": [0.0, 0.0, half, half], "rename": "fin"}),
    )
    .unwrap();
    assert_eq!(reply["name"], "fin");
    assert!((reply["rest"][2].as_f64().unwrap() - half as f64).abs() < 1e-5);
    let listed = apply(&mut session, "rig.list", &json!({})).unwrap();
    let fin = listed["bones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bone| bone["name"] == "fin")
        .unwrap()
        .clone();
    assert!((fin["rest"][3].as_f64().unwrap() - half as f64).abs() < 1e-5);
    session.undo();
    assert!(session.skeleton.index_of("tail").is_some());
    assert!(!session.skeleton.has_rest());

    apply(
        &mut session,
        "pose.set",
        &json!({"pose": {"tail": [0.0, 0.0, half, half]}}),
    )
    .unwrap();
    apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "restFromPose": true}),
    )
    .unwrap();
    assert!(session.pose.is_empty());
    assert!(session.skeleton.has_rest());
    let err = apply(
        &mut session,
        "rig.set",
        &json!({"name": "tail", "rest": [0.0, 0.0, 0.0, 1.0], "restFromPose": true}),
    )
    .unwrap_err();
    assert!(err.contains("not both"), "{err}");
}
