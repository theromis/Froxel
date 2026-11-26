use super::super::quads::build_with;
use super::super::{AoTarget, MeshExport};
use super::View;
use crate::state::Session;
use serde_json::json;

fn options(view: Option<View>) -> MeshExport {
    MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 0,
        ao_target: AoTarget::Auto,
        bake: None,
        game_view: view,
    }
}

fn view(value: serde_json::Value) -> View {
    View::parse(Some(&value)).unwrap().unwrap()
}

fn quads(session: &Session, view: Option<View>) -> usize {
    build_with(session, &options(view)).indices.len() / 6
}

fn one_cell() -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session
}

#[test]
fn a_camera_in_front_and_above_drops_the_back_and_the_underside() {
    let session = one_cell();
    assert_eq!(quads(&session, None), 6);
    // From -Y and above, spread to each side: top, front, and both sides stay.
    assert_eq!(
        quads(&session, Some(view(json!({"yaw": -90, "pitch": 32})))),
        4
    );
    // No spread about up: the sides are edge-on and go too.
    assert_eq!(
        quads(
            &session,
            Some(view(json!({"yaw": -90, "pitch": 32, "spread": [0, 0]})))
        ),
        2
    );
}

#[test]
fn occlusion_drops_what_the_model_hides_behind_itself() {
    // A wall 5 wide and 5 high, and a tuft on its back face.
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for x in 0..5 {
        for z in 0..5 {
            session.objects[0].cells.insert((x, 0, z), 1);
        }
    }
    session.objects[0].cells.insert((2, 1, 2), 1);
    let facing = quads(
        &session,
        Some(view(json!({"yaw": -90, "pitch": 32, "occlusion": false}))),
    );
    let seen = quads(&session, Some(view(json!({"yaw": -90, "pitch": 32}))));
    // The tuft's top and two sides face the camera but the wall hides them.
    assert_eq!(facing - seen, 3);
}

#[test]
fn solid_boxes_hide_like_cells_and_are_not_written() {
    let session = one_cell();
    let next_door = view(json!({"yaw": -90, "pitch": 32, "solid": [[-3, -2, -2, -1, 2, 2]]}));
    // The face toward the neighbour goes; the others stay.
    assert_eq!(quads(&session, Some(next_door)), 3);
}

#[test]
fn view_options_are_checked() {
    assert!(View::parse(None).unwrap().is_none());
    assert!(View::parse(Some(&json!(false))).unwrap().is_none());
    assert!(View::parse(Some(&json!({"yaw": 0}))).is_err());
    assert!(View::parse(Some(&json!({"yaw": 0, "pitch": 120}))).is_err());
    assert!(View::parse(Some(&json!({"yaw": 0, "pitch": 30, "spread": 90}))).is_err());
    assert!(View::parse(Some(&json!({"yaw": 0, "pitch": 30, "camera": 1}))).is_err());
    assert!(View::parse(Some(&json!({"yaw": 0, "pitch": 30, "solid": [[1, 2, 3]]}))).is_err());
    let parsed = view(json!({"yaw": 10, "pitch": 30, "spread": 5, "solid": [[3, 0, 0, 0, 1, 1]]}));
    assert_eq!(parsed.spread, [5.0, 5.0]);
    assert_eq!(parsed.solid, vec![[0, 0, 0, 3, 1, 1]]);
    assert_eq!(View::parse(Some(&parsed.json())).unwrap().unwrap(), parsed);
}
