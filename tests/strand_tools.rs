use serde_json::json;

use crate::remote::apply;
use crate::state::Session;

fn tuft(session: &mut Session) {
    apply(
        session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 3, "y1": 3, "z1": 1, "palette": 1}),
    )
    .unwrap();
    apply(
        session,
        "strand.add",
        &json!({"points": [[1, 1, 2], [2, 1, 6, 20], [5, 1, 9, 40, 0.4]], "width": 1.0, "thickness": 0.5, "palettes": [2, 3, 4]}),
    )
    .unwrap();
}

#[test]
fn add_edit_and_remove_a_strand_each_one_undo_step() {
    let mut session = Session::new();
    tuft(&mut session);
    let listed = apply(&mut session, "strand.list", &json!({})).unwrap();
    assert_eq!(listed["strands"].as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed["strands"][0]["pattern"], "gradient");
    assert!(listed["boxes"].as_u64().unwrap() >= 3);

    let moved = apply(
        &mut session,
        "strand.edit",
        &json!({"index": 0, "point": 2, "at": [6, 2, 10]}),
    )
    .unwrap();
    let tip = &moved["strand"]["points"][2];
    assert_eq!(tip[0], 6.0);
    // A position alone keeps the point's twist and scale.
    assert_eq!(tip[3], 40.0);
    assert!((tip[4].as_f64().unwrap() - 0.4).abs() < 1.0e-6);

    apply(
        &mut session,
        "strand.edit",
        &json!({"index": 0, "move": [1, 0, 0]}),
    )
    .unwrap();
    assert_eq!(session.objects[0].strands[0].points[0].at, [2.0, 1.0, 2.0]);
    apply(
        &mut session,
        "strand.edit",
        &json!({"index": 0, "insert": 1, "at": [2, 1, 4]}),
    )
    .unwrap();
    assert_eq!(session.objects[0].strands[0].points.len(), 4);

    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.objects[0].strands[0].points.len(), 3);
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.objects[0].strands[0].points[0].at, [1.0, 1.0, 2.0]);

    let removed = apply(&mut session, "strand.remove", &json!({"index": 0})).unwrap();
    assert_eq!(removed["removed"], 1);
    assert!(session.objects[0].strands.is_empty());
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    assert_eq!(session.objects[0].strands.len(), 1);
}

#[test]
fn a_bad_strand_changes_nothing() {
    let mut session = Session::new();
    tuft(&mut session);
    let before = session.objects[0].strands.clone();
    assert!(apply(
        &mut session,
        "strand.edit",
        &json!({"index": 0, "remove": 0, "point": 1})
    )
    .is_err());
    assert!(apply(
        &mut session,
        "strand.edit",
        &json!({"index": 3, "width": 2})
    )
    .is_err());
    assert!(apply(&mut session, "strand.add", &json!({"points": [[0, 0, 0]]})).is_err());
    assert!(apply(
        &mut session,
        "strand.add",
        &json!({"points": [[0, 0, 0], [1, 0, 0]], "width": 100})
    )
    .is_err());
    assert_eq!(session.objects[0].strands, before);
}

#[test]
fn strands_survive_save_and_open_where_they_were() {
    let mut session = Session::new();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "position": [10, -4, 3]}),
    )
    .unwrap();
    tuft(&mut session);
    // A strands-only object too.
    apply(
        &mut session,
        "object.add",
        &json!({"name": "Reeds", "position": [-7, 2, 0]}),
    )
    .unwrap();
    apply(
        &mut session,
        "strand.add",
        &json!({"object": "Reeds", "points": [[0, 0, 0], [0, 0, 12]], "width": 0.5, "palette": 5}),
    )
    .unwrap();
    let world = |session: &Session| -> Vec<Vec<[i64; 3]>> {
        (0..session.objects.len())
            .map(|i| {
                session
                    .strand_world_boxes(i)
                    .iter()
                    .map(|b| {
                        let c = b.matrix.transform_point3(glam::Vec3::ZERO);
                        [
                            (c.x * 1000.0).round() as i64,
                            (c.y * 1000.0).round() as i64,
                            (c.z * 1000.0).round() as i64,
                        ]
                    })
                    .collect()
            })
            .collect()
    };
    let before = world(&session);
    let path = std::env::temp_dir().join("froxel-strands-round-trip.vox");
    let _ = std::fs::remove_file(&path);
    apply(
        &mut session,
        "document.save",
        &json!({"path": path.to_string_lossy()}),
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(4).any(|w| w == b"MVST"));
    let mut reopened = Session::new();
    apply(
        &mut reopened,
        "document.open",
        &json!({"path": path.to_string_lossy()}),
    )
    .unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(reopened.objects.len(), 2);
    let reeds = reopened
        .objects
        .iter()
        .position(|o| o.name == "Reeds")
        .unwrap();
    assert_eq!(reopened.objects[reeds].strands.len(), 1);
    let mut after = world(&reopened);
    let mut expected = before;
    after.sort();
    expected.sort();
    assert_eq!(after, expected);
}

#[test]
fn bake_strands_writes_the_covered_cells() {
    let mut session = Session::new();
    apply(
        &mut session,
        "strand.add",
        &json!({"points": [[0, 0, 0], [5, 0, 0]], "palette": 7, "curve": "polyline"}),
    )
    .unwrap();
    let baked = apply(&mut session, "strand.bake", &json!({})).unwrap();
    assert_eq!(baked["cells"], 6, "{baked}");
    assert!(session.objects[0].strands.is_empty());
    assert_eq!(session.objects[0].cells.get(&(3, 0, 0)), Some(&7));
}

#[test]
fn a_script_grows_strands_and_a_rerun_replaces_them() {
    let mut session = Session::new();
    let source = r#"
let r = rng(4);
for i in 0..9 {
    let a = i * 0.7;
    let lean = r.float(3.0, 6.0);
    let tall = r.float(8.0, 14.0);
    add_strand([
        [0, 0, 0, 0, 1.0],
        [lean * 0.3 * cos(a), lean * 0.3 * sin(a), tall * 0.6, 20, 0.9],
        [lean * cos(a), lean * sin(a), tall, 40, 0.4],
    ], #{width: 1.0, thickness: 0.35, step: 1.0, palettes: [2, 3, 4]});
}
"#;
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].strands.len(), 9);
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects[0].strands.len(), 9);
}

#[test]
fn mirror_object_mirrors_strands_with_the_cells() {
    let mut session = Session::new();
    tuft(&mut session);
    apply(
        &mut session,
        "object.mirror",
        &json!({"index": 0, "axis": "x", "name": "Left"}),
    )
    .unwrap();
    let a = session.strand_world_boxes(0);
    let b = session.strand_world_boxes(1);
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        let ca = a.matrix.transform_point3(glam::Vec3::ZERO);
        let cb = b.matrix.transform_point3(glam::Vec3::ZERO);
        assert!(
            (ca.x + cb.x).abs() < 1.0e-3
                && (ca.y - cb.y).abs() < 1.0e-3
                && (ca.z - cb.z).abs() < 1.0e-3,
            "{ca} {cb}"
        );
    }
    apply(&mut session, "object.clear", &json!({"index": 1})).unwrap();
    assert!(session.objects[1].strands.is_empty());
}

fn covered(rgba: &[u8], background: [u8; 4]) -> usize {
    rgba.chunks(4).filter(|px| *px != background).count()
}

#[test]
fn preview_draws_a_strands_only_object() {
    let mut session = Session::new();
    apply(
        &mut session,
        "strand.add",
        &json!({"points": [[0, 0, 0], [0, 0, 10], [6, 0, 14]], "palette": 3}),
    )
    .unwrap();
    let preview = crate::preview::Preview {
        width: 64,
        height: 64,
        background: [0, 0, 0, 0],
        antialias: false,
        ..crate::preview::Preview::default()
    };
    let frame = crate::preview::render(&session, &preview).unwrap();
    assert!(frame.preview_triangles > 0);
    assert!(covered(&frame.rgba, [0, 0, 0, 0]) > 50);
}
