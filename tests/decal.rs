use serde_json::{json, Value};

use super::*;
use crate::remote::apply;
use crate::state::Session;

/// A session whose active object is the ellipsoid in the inclusive box
/// 0..=x1, 0..=y1, 0..=z1 of slot 2.
fn head(x1: i32, y1: i32, z1: i32) -> Session {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.ellipsoid",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": x1, "y1": y1, "z1": z1, "palette": 2}),
    )
    .unwrap();
    session
}

fn of_slot(session: &Session, slot: u8) -> Vec<Cell> {
    let mut cells: Vec<Cell> = session
        .active()
        .cells
        .iter()
        .filter(|(_, s)| **s == slot)
        .map(|(c, _)| *c)
        .collect();
    cells.sort_unstable();
    cells
}

/// The extent of `cells` across (along `right`) and up (along +Z) in the
/// plane facing `normal`: what a camera looking along -`normal` sees.
fn extent(cells: &[Cell], normal: [f64; 3]) -> (f64, f64) {
    let up = [0.0, 0.0, 1.0];
    let right = normalize(cross(up, normal)).unwrap();
    let up = cross(normal, right);
    let range = |axis: [f64; 3]| {
        let values: Vec<f64> = cells.iter().map(|c| dot(pos(*c), axis)).collect();
        let lo = values.iter().cloned().fold(f64::MAX, f64::min);
        let hi = values.iter().cloned().fold(f64::MIN, f64::max);
        hi - lo + 1.0
    };
    (range(right), range(up))
}

/// The point on an ellipsoid head of box `0..=size` in direction yaw 45
/// degrees between front (-Y) and right side (+X), at `height` above its
/// middle, and the outward direction there.
fn three_quarter(size: [i32; 3], height: f64) -> ([f64; 3], [f64; 3]) {
    let c = size.map(|n| f64::from(n) / 2.0);
    let r = size.map(|n| f64::from(n + 1) / 2.0);
    let d = normalize([0.707, -0.707, height]).unwrap();
    // Scale the direction onto the ellipsoid.
    let t = 1.0 / ((d[0] / r[0]).powi(2) + (d[1] / r[1]).powi(2) + (d[2] / r[2]).powi(2)).sqrt();
    let point = [0, 1, 2].map(|a| c[a] + d[a] * t);
    let normal = normalize([0, 1, 2].map(|a| d[a] / (r[a] * r[a]))).unwrap();
    (point, normal)
}

const SQUARE: [&str; 8] = [
    "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx", "xxxxxxxx",
];

#[test]
fn a_square_keeps_its_size_and_shape_at_three_quarters_where_a_projection_stretches() {
    for size in [[31, 31, 31], [27, 35, 23]] {
        let (point, normal) = three_quarter(size, 0.0);
        let mut session = head(size[0], size[1], size[2]);
        let reply = apply(
            &mut session,
            "voxel.decal",
            &json!({"rows": SQUARE, "legend": {"x": 5}, "point": point, "size": 8}),
        )
        .unwrap();
        assert_eq!(reply["missed"], 0, "{reply}");
        let decal = of_slot(&session, 5);
        let (across, up) = extent(&decal, normal);
        assert!((7.0..=9.5).contains(&across), "{size:?} across {across}");
        assert!((7.0..=9.5).contains(&up), "{size:?} up {up}");
        // The same square projected from the front lands stretched sideways
        // on the slope: wider across than tall, as seen face on.
        let mut projected = head(size[0], size[1], size[2]);
        let origin = [point[0] - 4.0, 0.0, point[2] - 4.0].map(|v| v.round() as i32);
        apply(
            &mut projected,
            "voxel.project_paint",
            &json!({"view": "front", "rows": SQUARE, "legend": {"x": 5}, "origin": origin, "size": [8, size[1] + 1, 8]}),
        )
        .unwrap();
        let (across_p, up_p) = extent(&of_slot(&projected, 5), normal);
        assert!(
            across_p / up_p > across / up + 0.15,
            "{size:?} projected {across_p}x{up_p}, decal {across}x{up}"
        );
    }
}

/// A sleepy eye (a lid line over a half-closed eye) and a smile.
const FACE: [&str; 10] = [
    "..llllll......",
    ".l......l.....",
    "l.eeeeee.l....",
    "..eeeeee......",
    "..............",
    "..............",
    "m...........m.",
    ".m.........m..",
    "..mm.....mm...",
    "....mmmmm.....",
];

#[test]
fn a_sleepy_eye_and_a_smile_stay_undistorted_on_a_sphere_and_an_ellipsoid() {
    for size in [[31, 31, 31], [27, 35, 23]] {
        let (point, normal) = three_quarter(size, 0.15);
        let mut session = head(size[0], size[1], size[2]);
        let reply = apply(
            &mut session,
            "voxel.decal",
            &json!({"rows": FACE, "legend": {"l": 3, "e": 4, "m": 6}, "point": point, "size": 14}),
        )
        .unwrap();
        assert_eq!(reply["grid"], json!([14, 10]));
        assert_eq!(reply["missed"], 0, "{reply}");
        // Each feature keeps its size: the eye 6 x 2 cells, the smile
        // 13 across (columns 0..12) and 4 down.
        let (eye_w, eye_h) = extent(&of_slot(&session, 4), normal);
        assert!(
            (5.0..=7.5).contains(&eye_w) && (1.5..=3.5).contains(&eye_h),
            "eye {eye_w}x{eye_h}"
        );
        let (smile_w, smile_h) = extent(&of_slot(&session, 6), normal);
        assert!(
            (11.5..=14.5).contains(&smile_w) && (3.0..=5.5).contains(&smile_h),
            "smile {smile_w}x{smile_h}"
        );
        // About one painted surface cell per inked picture cell.
        let inked = FACE
            .iter()
            .map(|row| row.chars().filter(|c| *c != '.').count())
            .sum::<usize>();
        let painted = [3, 4, 6]
            .iter()
            .map(|s| of_slot(&session, *s).len())
            .sum::<usize>();
        let ratio = painted as f64 / inked as f64;
        assert!(
            (0.8..=1.6).contains(&ratio),
            "{size:?} painted {painted} of {inked}"
        );
    }
}

#[test]
fn a_ray_up_depth_only_and_the_mirror_place_the_decal() {
    let mut session = head(31, 31, 31);
    // A ray from in front, straight at the face.
    let reply = apply(
        &mut session,
        "voxel.decal",
        &json!({"rows": ["x."], "legend": {"x": 5}, "ray": {"origin": [15.5, -20, 15.5], "direction": [0, 1, 0]}, "size": [4, 2], "depth": 2}),
    )
    .unwrap();
    assert_eq!(
        reply["normal"][1].as_f64().unwrap().round(),
        -1.0,
        "{reply}"
    );
    let marked = of_slot(&session, 5);
    // Image left is -X for a face looking along -Y with up +Z.
    assert!(marked.iter().all(|c| c.0 <= 16), "{marked:?}");
    // depth 2 reaches the layer behind the surface.
    assert!(marked.iter().any(|c| c.1 >= 1));
    // up -Z turns the picture over: the ink lands on +X.
    session.undo();
    apply(
        &mut session,
        "voxel.decal",
        &json!({"rows": ["x."], "legend": {"x": 5}, "point": [15.5, 0, 15.5], "size": [4, 2], "up": [0, 0, -1]}),
    )
    .unwrap();
    assert!(of_slot(&session, 5).iter().all(|c| c.0 >= 15));
    // The mirror paints the other cheek, and only keeps slot 5 alone.
    let (point, _) = three_quarter([31, 31, 31], 0.0);
    let reply = apply(
        &mut session,
        "voxel.decal",
        &json!({"rows": SQUARE, "legend": {"x": 7}, "point": point, "size": 4, "only": [2], "mirror": true}),
    )
    .unwrap();
    assert!(reply["mirrored"].as_u64().unwrap() > 0);
    let seven = of_slot(&session, 7);
    let plane = super::super::Mirror { axis: 0, twice: 32 };
    assert!(seven.iter().all(|c| seven.contains(&plane.cell(*c))));
    assert!(!of_slot(&session, 5).is_empty());
}

#[test]
fn bad_decals_are_explained() {
    let mut session = head(15, 15, 15);
    let base = json!({"rows": ["x"], "legend": {"x": 5}, "point": [7, 0, 7], "size": 4});
    for (key, value, text) in [
        ("size", Value::Null, "size is required"),
        ("size", json!(0.5), "width must be 1"),
        ("point", json!([100, 100, 100]), "no surface cell"),
        ("depth", json!(0), "depth must be"),
        ("normalLimit", json!(0), "normalLimit"),
        ("up", json!([0, 0, 0]), "up must not"),
    ] {
        let mut args = base.clone();
        args[key] = value;
        let err = apply(&mut session, "voxel.decal", &args).unwrap_err();
        assert!(err.contains(text), "{key}: {err}");
    }
    let mut args = base.clone();
    args["ray"] = json!({"origin": [0, 0, 0], "direction": [0, 1, 0]});
    assert!(apply(&mut session, "voxel.decal", &args)
        .unwrap_err()
        .contains("not both"));
    args.as_object_mut().unwrap().remove("point");
    args["ray"] = json!({"origin": [-5, -5, -5], "direction": [-1, 0, 0]});
    assert!(apply(&mut session, "voxel.decal", &args)
        .unwrap_err()
        .contains("meets no solid"));
}

#[test]
fn the_decal_face_example_lays_a_mirrored_pair_of_eyes_and_a_smile() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/decal_face.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let plane =
        super::super::Mirror::about_centre(0, session.active().cells.keys().copied()).unwrap();
    for slot in [3, 4, 5] {
        let cells = of_slot(&session, slot);
        let paired = cells
            .iter()
            .filter(|c| cells.contains(&plane.cell(**c)))
            .count();
        assert!(
            cells.iter().any(|c| c.0 < 13) && cells.iter().any(|c| c.0 > 14),
            "slot {slot} {cells:?}"
        );
        assert!(
            paired * 10 >= cells.len() * 8,
            "slot {slot}: {paired} of {} paired",
            cells.len()
        );
    }
    let mouth = of_slot(&session, 6);
    assert!(!mouth.is_empty() && mouth.iter().all(|c| c.1 < 8));
}

#[test]
fn the_decal_runs_from_a_script_and_the_panel() {
    let mut session = head(31, 31, 31);
    session
        .set_script(
            0,
            "fill_ellipsoid(0, 0, 0, 31, 31, 31, 2);\n\
             let r = decal([\"xx\", \"xx\"], #{legend: #{x: 5}, point: [15.5, 0, 15.5], size: 4});\n\
             if r.cells == 0 || r.missed != 0 || r.centre[1] > 1 { throw \"decal\"; }",
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    assert!(!of_slot(&session, 5).is_empty());
    let err = session.set_script(
        0,
        "decal([\"x\"], #{legend: #{x: 5}, point: [1, 1, 1], size: 2, view: \"top\"});",
    );
    let ran = err.and_then(|_| session.regenerate_script(0).map(|_| ()));
    assert!(ran.unwrap_err().contains("no option"));
    // The panel builds the MCP arguments.
    let mut panel = session.silhouette.decal.clone();
    panel.path = "eye.png".into();
    panel.point = "3 4 5".into();
    panel.size = 6.0;
    panel.only = "2".into();
    panel.colour_image = false;
    let args = panel.args(7).unwrap();
    assert_eq!(args["point"], json!([3.0, 4.0, 5.0]));
    assert_eq!(args["up"], json!([0.0, 0.0, 1.0]));
    assert_eq!(args["palette"], 7);
    assert_eq!(args["only"], json!([2]));
    assert!(args.get("mirror").is_none());
    panel.point = "3 4".into();
    assert!(panel.args(1).unwrap_err().contains("x y z"));
    // The button lays it on the active object.
    session.silhouette.decal = panel;
    session.silhouette.decal.point = "15.5 0 15.5".into();
    session.silhouette.decal.path = "/nonexistent/eye.png".into();
    session.decal_from_panel();
    assert!(session.status.contains("eye.png"), "{}", session.status);
}
