use serde_json::json;

use crate::remote::apply;
use crate::state::Session;
use crate::tools::{Skin, SkinAxis};

fn body(session: &mut Session) {
    apply(
        session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 30, "y1": 12, "z1": 14, "palette": 2}),
    )
    .unwrap();
}

#[test]
fn skin_sets_the_whole_skin_and_off_removes_it_each_one_undo_step() {
    let mut session = Session::new();
    body(&mut session);
    let reply = apply(&mut session, "skin.set", &json!({"axis": "x", "seed": 4})).unwrap();
    assert!(reply["bricks"].as_u64().unwrap() > 50, "{reply}");
    assert_eq!(reply["skin"]["axis"], "x");
    assert_eq!(
        session.objects[0].skin,
        Some(Skin {
            axis: SkinAxis::X,
            seed: 4,
            ..Skin::default()
        })
    );
    // Another call replaces every field: the seed goes back to 0.
    apply(&mut session, "skin.set", &json!({"width": 2.0})).unwrap();
    assert_eq!(
        session.objects[0].skin,
        Some(Skin {
            width: 2.0,
            ..Skin::default()
        })
    );
    let inspected = apply(&mut session, "document.inspect", &json!({})).unwrap();
    assert_eq!(inspected["objects"][0]["skin"]["width"], 2.0);
    assert!(inspected["objects"][0]["bricks"].as_u64().unwrap() > 0);
    let off = apply(&mut session, "skin.set", &json!({"off": true})).unwrap();
    assert!(off["skin"].is_null());
    assert_eq!(off["bricks"], 0);
    session.undo();
    assert_eq!(session.objects[0].skin.as_ref().unwrap().width, 2.0);
    session.undo();
    assert_eq!(session.objects[0].skin.as_ref().unwrap().axis, SkinAxis::X);
}

#[test]
fn bad_skin_arguments_are_refused_and_change_nothing() {
    let mut session = Session::new();
    body(&mut session);
    for bad in [
        json!({"axis": "w"}),
        json!({"length": [4, 2]}),
        json!({"width": 0}),
        json!({"bare": [0]}),
        json!({"tilt": 90}),
    ] {
        assert!(apply(&mut session, "skin.set", &bad).is_err(), "{bad}");
    }
    assert!(session.objects[0].skin.is_none());
}

#[test]
fn bake_strands_with_skin_writes_the_bricks_into_cells() {
    let mut session = Session::new();
    body(&mut session);
    let before = session.objects[0].cells.len();
    apply(&mut session, "skin.set", &json!({"height": 1.0})).unwrap();
    let baked = apply(&mut session, "strand.bake", &json!({"skin": true})).unwrap();
    assert!(baked["changed"].as_u64().unwrap() > 0, "{baked}");
    assert!(session.objects[0].cells.len() > before);
    assert!(session.objects[0].skin.is_none());
    session.undo();
    assert!(session.objects[0].skin.is_some());
    assert_eq!(session.objects[0].cells.len(), before);
    // No skin, no strands: nothing to bake.
    apply(&mut session, "skin.set", &json!({"off": true})).unwrap();
    assert!(apply(&mut session, "strand.bake", &json!({"skin": true})).is_err());
}

#[test]
fn a_script_sets_and_clears_its_object_skin() {
    let mut session = Session::new();
    let source = r#"
        fill_ellipsoid(0, 0, 0, 30, 12, 14, 2);
        skin(#{axis: "x", length: [1, 2], bare: [5]});
    "#;
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    let skin = session.objects[0].skin.clone().unwrap();
    assert_eq!(skin.axis, SkinAxis::X);
    assert_eq!(skin.length, [1.0, 2.0]);
    assert_eq!(skin.bare, vec![5]);
    assert!(!session.skin_boxes(0).is_empty());
    session
        .set_script(0, "fill_ellipsoid(0, 0, 0, 30, 12, 14, 2); clear_skin();")
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert!(session.objects[0].skin.is_none());
    // A run that never calls either leaves the skin alone.
    session.objects[0].skin = Some(Skin::default());
    session
        .set_script(0, "fill_ellipsoid(0, 0, 0, 30, 12, 14, 2);")
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert!(session.objects[0].skin.is_some());
    let err = session
        .set_script(0, "skin(#{width: -1});")
        .and_then(|_| session.regenerate_script(0).map(|_| ()));
    assert!(err.is_err());
}
