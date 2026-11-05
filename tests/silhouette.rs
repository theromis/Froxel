use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;
use crate::state::Cell;
use crate::tools::{mask_from_rgba, Background, Ink};

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("froxel-silhouette-{}-{name}", std::process::id()))
}

/// A PNG of `w`×`h` where `inked(x, y)` pixels are `ink` and the rest `bg`.
fn png(
    name: &str,
    w: u32,
    h: u32,
    bg: [u8; 4],
    ink: [u8; 4],
    inked: impl Fn(u32, u32) -> bool,
) -> String {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend(if inked(x, y) { ink } else { bg });
        }
    }
    let path = temp(name);
    crate::trace::write_rgba_png(&path, w, h, &rgba).unwrap();
    path.display().to_string()
}

fn empty_session() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    session
}

fn apply(session: &mut Session, method: &str, params: Value) -> Value {
    crate::remote::apply(session, method, &params).unwrap()
}

#[test]
fn a_key_colour_and_alpha_cut_the_mask() {
    let mut rgba = Vec::new();
    for y in 0..4u32 {
        for x in 0..4u32 {
            rgba.extend(if x >= 2 && y >= 1 {
                [200, 30, 30, 255]
            } else {
                [250, 250, 250, 255]
            });
        }
    }
    // Auto keys on the image border's commonest colour: white.
    let mask = mask_from_rgba(4, 4, &rgba, None, Background::default()).unwrap();
    assert_eq!(mask.count(), 6);
    assert_eq!(mask.get(3, 3), Some(Ink::Rgb([200, 30, 30])));
    assert_eq!(mask.bounds(), Some([2, 1, 3, 3]));
    let cropped = mask_from_rgba(4, 4, &rgba, Some([1, 1, 3, 3]), Background::default()).unwrap();
    assert_eq!((cropped.width, cropped.height, cropped.count()), (3, 3, 6));
    assert!(mask_from_rgba(4, 4, &rgba, Some([2, 2, 3, 3]), Background::default()).is_err());
    // Alpha: half the image transparent.
    for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
        px[3] = if i % 4 < 2 { 0 } else { 255 };
    }
    let alpha = mask_from_rgba(4, 4, &rgba, None, Background::default()).unwrap();
    assert_eq!(alpha.count(), 8);
}

/// Front: a full 4×4 square. Left: a right triangle, high at the front.
/// The hull is the triangle extruded across x.
fn wedge_views() -> Value {
    json!([
        {"view": "front", "rows": ["xxxx", "xxxx", "xxxx", "xxxx"]},
        {"view": "side", "rows": ["x...", "xx..", "xxx.", "xxxx"]}
    ])
}

#[test]
fn two_views_carve_their_hull() {
    let mut session = empty_session();
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        json!({"views": wedge_views(), "cellsPerPixel": 1, "palette": 2, "origin": [10, 0, 0]}),
    );
    assert_eq!(reply["cells"], 40);
    assert_eq!(reply["size"], json!([4, 4, 4]));
    assert_eq!(
        reply["bounds"],
        json!({"min": [10, 0, 0], "max": [13, 3, 3]})
    );
    let cells = &session.active().cells;
    assert_eq!(cells.len(), 40);
    // Seen from +X, image left is −Y, the front: the tall end of the
    // triangle is at y 0.
    assert!(cells.contains_key(&(10, 0, 3)));
    assert!(!cells.contains_key(&(10, 3, 3)));
    assert!(cells.contains_key(&(13, 3, 0)));
    assert!(cells.values().all(|slot| *slot == 2));
}

#[test]
fn a_top_view_cuts_a_cylinder_from_two_squares() {
    let disc: Vec<String> = (0..8)
        .map(|y| {
            (0..8)
                .map(|x| {
                    let (dx, dy) = (x as f32 - 3.5, y as f32 - 3.5);
                    if dx * dx + dy * dy <= 16.0 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    let disc_cells = disc
        .iter()
        .map(|row| row.chars().filter(|c| *c == '#').count())
        .sum::<usize>();
    let square: Vec<String> = vec!["########".into(); 6];
    let mut session = empty_session();
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        json!({
            "views": [
                {"view": "front", "rows": square},
                {"view": "left", "rows": square},
                {"view": "top", "rows": disc}
            ],
            "cellsPerPixel": 1,
            "palette": 1
        }),
    );
    assert_eq!(reply["size"], json!([8, 8, 6]));
    assert_eq!(reply["cells"].as_u64().unwrap() as usize, disc_cells * 6);
}

#[test]
fn size_stretches_a_view_and_shell_hollows_it() {
    let mut session = empty_session();
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        json!({
            "views": [
                {"view": "front", "rows": ["##", "##"]},
                {"view": "left", "rows": ["##", "##"]}
            ],
            "size": [6, 6, 6],
            "palette": 1,
            "shell": 1
        }),
    );
    assert_eq!(reply["cells"], 6 * 6 * 6 - 4 * 4 * 4);
    assert_eq!(reply["views"][0]["grid"], json!([6, 6]));
}

#[test]
fn each_cell_takes_the_nearest_views_colour() {
    let mut session = empty_session();
    apply(
        &mut session,
        "voxel.carve_from_views",
        json!({
            "views": [
                {"view": "front", "rows": ["aaaa", "aaaa", "aaaa", "aaaa"], "legend": {"a": 3}},
                {"view": "left", "rows": ["bbbb", "bbbb", "bbbb", "bbbb"], "legend": {"b": 5}}
            ],
            "cellsPerPixel": 1
        }),
    );
    let cells = &session.active().cells;
    // The front face (y 0) is the front view's, the +X face the side's.
    assert_eq!(cells[&(1, 0, 1)], 3);
    assert_eq!(cells[&(3, 2, 1)], 5);
}

#[test]
fn image_colours_quantise_to_the_given_slots() {
    let mut session = empty_session();
    apply(
        &mut session,
        "palette.set",
        json!({"entries": [
            {"index": 10, "r": 220, "g": 20, "b": 20},
            {"index": 11, "r": 20, "g": 20, "b": 220}
        ]}),
    );
    let red = png(
        "red.png",
        8,
        8,
        [255, 255, 255, 255],
        [230, 40, 30, 255],
        |x, _| (2..6).contains(&x),
    );
    let blue = png(
        "blue.png",
        8,
        8,
        [0, 0, 0, 0],
        [30, 30, 200, 255],
        |x, _| (2..6).contains(&x),
    );
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        json!({
            "views": [
                {"view": "front", "path": red, "crop": [2, 0, 4, 8], "cellsPerPixel": 0.5},
                {"view": "left", "path": blue, "background": "alpha", "crop": [2, 0, 4, 8], "cellsPerPixel": 0.5}
            ],
            "slots": [10, 11]
        }),
    );
    assert_eq!(reply["size"], json!([2, 2, 4]));
    assert_eq!(reply["cells"], 16);
    let cells = &session.active().cells;
    assert_eq!(cells[&(0, 0, 0)], 10);
    assert_eq!(cells[&(1, 1, 3)], 11);
}

#[test]
fn smooth_rounds_a_cube_hull() {
    let square: Vec<String> = vec!["#".repeat(10); 10];
    let mut session = empty_session();
    let reply = apply(
        &mut session,
        "voxel.carve_from_views",
        json!({
            "views": [{"view": "front", "rows": square}, {"view": "top", "rows": square}],
            "cellsPerPixel": 1,
            "palette": 1,
            "smooth": 2
        }),
    );
    let cells = reply["cells"].as_u64().unwrap();
    assert!(cells < 1000, "{cells}");
    assert!(cells > 800, "{cells}");
    assert!(!session.active().cells.contains_key(&(0, 0, 0)));
}

#[test]
fn bad_views_are_explained() {
    let err = |params: Value| {
        crate::remote::apply(&mut empty_session(), "voxel.carve_from_views", &params).unwrap_err()
    };
    let one = json!({"views": [{"view": "front", "rows": ["#"]}], "cellsPerPixel": 1});
    assert!(err(one).contains("2 or 3"));
    let uncovered = json!({"views": [
        {"view": "front", "rows": ["#"]}, {"view": "back", "rows": ["#"]}
    ], "cellsPerPixel": 1, "palette": 1});
    assert!(err(uncovered).contains("no view spans y"));
    let plain = json!({"views": [
        {"view": "front", "rows": ["x"]}, {"view": "left", "rows": ["x"]}
    ], "cellsPerPixel": 1});
    assert!(err(plain).contains("legend"));
    let unknown = json!({"views": [
        {"view": "front", "rows": ["#"]}, {"view": "left", "rows": ["#"]}
    ], "palette": 1});
    assert!(err(unknown).contains("size along"));
}

/// Rhai runs the MCP tool's code path: the same views give the same cells.
#[test]
fn a_script_carves_the_same_hull() {
    let mut session = empty_session();
    apply(
        &mut session,
        "voxel.carve_from_views",
        json!({"views": wedge_views(), "cellsPerPixel": 1, "palette": 4, "shell": 1}),
    );
    let mcp: HashMap<Cell, u8> = session.active().cells.clone();
    let source = r#"
        let views = [
            #{view: "front", rows: ["xxxx", "xxxx", "xxxx", "xxxx"]},
            #{view: "side", rows: ["x...", "xx..", "xxx.", "xxxx"]}
        ];
        let done = carve_from_views(views, #{cellsPerPixel: 1, palette: 4, shell: 1});
        if done.cells != done.changed { throw "counts"; }
        if done.bounds.max[2] != 3 { throw "bounds"; }
    "#;
    let mut scripted = empty_session();
    scripted.set_script(0, source).unwrap();
    scripted.regenerate_script(0).unwrap();
    assert_eq!(scripted.active().cells, mcp);
    let file =
        r#"carve_from_views([#{view: "front", path: "a.png"}, #{view: "left", path: "b.png"}]);"#;
    let mut failing = empty_session();
    let err = failing
        .set_script(0, file)
        .and_then(|_| failing.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("cannot read files"), "{err}");
}

/// A 6×2×3 box seen from the front, orthographic: 6 wide and 3 tall.
fn front_box() -> Session {
    let mut session = empty_session();
    apply(
        &mut session,
        "voxel.fill",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 1, "z1": 2, "palette": 1}),
    );
    session
}

#[test]
fn a_matching_concept_scores_near_one() {
    let mut session = front_box();
    // The concept: a 2:1 rectangle on white, off centre, at another scale.
    let concept = png(
        "concept.png",
        40,
        30,
        [255, 255, 255, 255],
        [90, 60, 40, 255],
        |x, y| (4..28).contains(&x) && (10..22).contains(&y),
    );
    let out = temp("overlay.png");
    let reply = apply(
        &mut session,
        "render.compare_silhouette",
        json!({
            "path": out.display().to_string(),
            "concept": concept,
            "view": "front",
            "orthographic": true,
            "height": 120
        }),
    );
    assert_eq!(reply["width"], 160);
    assert!(reply["iou"].as_f64().unwrap() > 0.95, "{reply}");
    assert!(reply["missing"].as_f64().unwrap() < 0.05, "{reply}");
    assert!(reply["excess"].as_f64().unwrap() < 0.05, "{reply}");
    assert_eq!(reply["align"]["mode"], "fit");
    let image = crate::sheet::read_png(&out).unwrap();
    assert_eq!((image.width, image.height), (160, 120));
}

#[test]
fn a_wider_concept_shows_what_is_missing() {
    let mut session = front_box();
    // Twice as wide for its height as the box: aligned on height, half of
    // it is missing and nothing is in excess.
    let concept = png(
        "wide.png",
        52,
        16,
        [0, 0, 0, 0],
        [90, 60, 40, 255],
        |x, y| (2..50).contains(&x) && (2..14).contains(&y),
    );
    let out = temp("wide-overlay.png");
    let reply = apply(
        &mut session,
        "render.compare_silhouette",
        json!({
            "path": out.display().to_string(),
            "concept": concept,
            "view": "front",
            "orthographic": true,
            "margin": 0.3,
            "align": "height",
            "width": 200,
            "height": 100
        }),
    );
    let missing = reply["missing"].as_f64().unwrap();
    assert!((missing - 0.5).abs() < 0.06, "{reply}");
    assert!(reply["excess"].as_f64().unwrap() < 0.05, "{reply}");
    assert!(
        (reply["iou"].as_f64().unwrap() - 0.5).abs() < 0.06,
        "{reply}"
    );
    assert_eq!(reply["aspect"]["concept"], 4.0);
    // The overlay shows the missing part in its colour.
    let image = crate::sheet::read_png(&out).unwrap();
    let missing_pixels = image
        .rgba
        .chunks_exact(4)
        .filter(|px| px[0] > 140 && px[1] < 80 && px[2] > 90)
        .count();
    assert!(missing_pixels > 200, "{missing_pixels}");
}

#[test]
fn game_view_and_compare_use_the_camera_section() {
    let mut session = front_box();
    session.game_view();
    assert_eq!(session.yaw.to_degrees().round(), -135.0);
    let presets = temp("project-camera.json");
    std::fs::write(&presets, r#"{"game": {"yaw": -100, "pitch": 40}}"#).unwrap();
    session.camera_presets_file = Some(presets);
    session.placement_yaw = 45.0;
    session.game_view();
    assert_eq!(
        session.yaw.to_degrees().round(),
        -145.0,
        "{}",
        session.status
    );
    assert_eq!(session.pitch.to_degrees().round(), 40.0);
    assert!(session.status.contains("-145.0"), "{}", session.status);
    let mut args = json!({"preset": "game"});
    session.add_camera_presets(&mut args);
    assert_eq!(args["placementYaw"], 45.0);
    assert!(args["presets"]
        .as_str()
        .unwrap()
        .ends_with("project-camera.json"));
}

#[test]
fn a_given_alignment_is_kept_and_presets_work() {
    let mut session = front_box();
    let concept = png(
        "given.png",
        20,
        10,
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        |x, y| x < 12 && y >= 4,
    );
    let reply = apply(
        &mut session,
        "render.compare_silhouette",
        json!({
            "path": temp("given-overlay.png").display().to_string(),
            "concept": concept,
            "preset": "game",
            "align": {"scale": 2.0, "offset": [3, 4]}
        }),
    );
    assert_eq!(
        reply["align"],
        json!({"mode": "given", "scale": 2.0, "offset": [3.0, 4.0]})
    );
    assert_eq!(reply["yaw"], -135.0);
    assert_eq!(reply["placementYaw"], 0.0);
    assert_eq!(reply["orthographic"], true);
    assert_eq!(reply["pixels"]["concept"], 12 * 6 * 4);
    let blank = png("blank.png", 4, 4, [9, 9, 9, 255], [9, 9, 9, 255], |_, _| {
        true
    });
    let err = crate::remote::apply(
        &mut session,
        "render.compare_silhouette",
        &json!({"path": temp("x.png").display().to_string(), "concept": blank}),
    )
    .unwrap_err();
    assert!(err.contains("empty"), "{err}");
}

#[test]
fn the_carve_panel_builds_mcp_arguments() {
    let mut panel = Session::new().silhouette.carve;
    assert!(panel.args(3).unwrap_err().contains("choose an image"));
    panel.views[0].path = "front.png".into();
    panel.views[0].crop = "0 0 64, 48".into();
    panel.views[1].path = "side.png".into();
    panel.views[1].flip = true;
    panel.size = "40 20 30".into();
    panel.colour_views = false;
    panel.shell = 2;
    let args = panel.args(3).unwrap();
    assert_eq!(args["views"].as_array().unwrap().len(), 2);
    assert_eq!(args["views"][0]["crop"], json!([0, 0, 64, 48]));
    assert_eq!(args["views"][1]["view"], "left");
    assert_eq!(args["views"][1]["flip"], true);
    assert_eq!(args["size"], json!([40, 20, 30]));
    assert_eq!(args["palette"], 3);
    assert_eq!(args["shell"], 2);
    panel.size = "4 4".into();
    assert!(panel.args(3).unwrap_err().contains("Size"));
}

#[test]
fn the_carved_stump_example_runs() {
    let mut session = empty_session();
    session
        .set_script(0, include_str!("../examples/carved_stump.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.active().cells;
    // Bark on the sides, rings on the cut face.
    assert!(cells.len() > 800, "{}", cells.len());
    assert!(cells.values().any(|slot| *slot == 3));
    assert!(cells.values().any(|slot| *slot == 6));
}

#[test]
fn the_panels_carve_and_compare() {
    // White paper above and beside each view, as on a sheet.
    let front = png(
        "panel-front.png",
        7,
        6,
        [255, 255, 255, 255],
        [40, 120, 40, 255],
        |x, y| (1..6).contains(&x) && (1..5).contains(&y),
    );
    let side = png(
        "panel-side.png",
        5,
        6,
        [255, 255, 255, 255],
        [40, 120, 40, 255],
        |x, y| (1..4).contains(&x) && (1..5).contains(&y),
    );
    let mut session = empty_session();
    session.active_slot = 7;
    let carve = &mut session.silhouette.carve;
    carve.views[0].path = front.clone();
    carve.views[0].crop = "1 1 5 4".into();
    carve.views[1].path = side;
    carve.views[1].crop = "1 1 3 4".into();
    carve.cells_per_pixel = 1.0;
    carve.colour_views = false;
    session.carve_from_panel();
    assert!(
        session.status.starts_with("Carved 60 cells"),
        "{}",
        session.status
    );
    assert!(session.active().cells.values().all(|slot| *slot == 7));
    let compare = &mut session.silhouette.compare;
    compare.path = front;
    compare.crop = "1 1 5 4".into();
    compare.camera = 2;
    session.compare_from_panel();
    let overlay = session.silhouette.overlay.as_ref().expect("overlay");
    assert!(overlay.iou > 0.95, "{}", session.status);
    assert_eq!(overlay.epoch, 1);
    assert!(session.status.starts_with("IoU"), "{}", session.status);
}
