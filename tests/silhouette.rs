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

/// A 4×4×3 block of slot 1 at the origin.
fn block() -> Session {
    let mut session = empty_session();
    apply(
        &mut session,
        "voxel.fill",
        json!({"x0": 0, "y0": 0, "z0": 0, "x1": 3, "y1": 3, "z1": 2, "palette": 1}),
    );
    session
}

#[test]
fn a_top_view_paints_only_the_surface_it_sees() {
    let mut session = block();
    let before = session.active().cells.len();
    // Image down is -Y: the top row is y = 3, and the image's left is x = 0.
    let reply = apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "top", "rows": ["ab..", "....", "....", "...a"], "legend": {"a": 5, "b": 6}}),
    );
    assert_eq!(reply["grid"], json!([4, 4]));
    assert_eq!(reply["cells"], 3);
    assert_eq!(reply["changed"], 3);
    let cells = &session.active().cells;
    assert_eq!(cells.len(), before);
    assert_eq!(cells[&(0, 3, 2)], 5);
    assert_eq!(cells[&(1, 3, 2)], 6);
    assert_eq!(cells[&(3, 0, 2)], 5);
    assert_eq!(cells[&(0, 3, 1)], 1);
    assert_eq!(cells.values().filter(|slot| **slot != 1).count(), 3);
    // depth 2 reaches the layer below as well; undo is one step.
    session.undo();
    assert!(session.active().cells.values().all(|slot| *slot == 1));
    let reply = apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "top", "rows": ["xxxx", "xxxx", "xxxx", "xxxx"], "legend": {"x": 3}, "depth": 2}),
    );
    assert_eq!(reply["cells"], 32);
    let cells = &session.active().cells;
    assert!((0..4).all(|x| (0..4).all(|y| cells[&(x, y, 0)] == 1 && cells[&(x, y, 1)] == 3)));
}

#[test]
fn a_front_view_maps_image_right_to_x_and_down_to_minus_z() {
    let mut session = block();
    apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "front", "rows": ["x...", "....", "...y"], "legend": {"x": 4, "y": 5}}),
    );
    let cells = &session.active().cells;
    // Seen from -Y, the front face is y = 0.
    assert_eq!(cells[&(0, 0, 2)], 4);
    assert_eq!(cells[&(3, 0, 0)], 5);
    assert_eq!(cells[&(0, 1, 2)], 1);
    // The same direction as yaw -90, pitch 0 snaps to the front view.
    let mut angled = block();
    apply(
        &mut angled,
        "voxel.project_paint",
        json!({"yaw": -90, "pitch": 0, "rows": ["x...", "....", "...y"], "legend": {"x": 4, "y": 5}}),
    );
    assert_eq!(angled.active().cells, session.active().cells);
}

#[test]
fn only_and_the_box_limit_what_is_repainted() {
    let mut session = block();
    apply(
        &mut session,
        "voxel.fill",
        json!({"x0": 0, "y0": 0, "z0": 2, "x1": 1, "y1": 3, "z1": 2, "palette": 2}),
    );
    let reply = apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "top", "rows": ["xxxx", "xxxx", "xxxx", "xxxx"], "legend": {"x": 7}, "only": [1]}),
    );
    assert_eq!(reply["cells"], 8);
    let cells = &session.active().cells;
    // Slot 2 stays, and hides the cells below it.
    assert_eq!(cells[&(0, 0, 2)], 2);
    assert_eq!(cells[&(0, 0, 1)], 1);
    assert_eq!(cells[&(3, 3, 2)], 7);
    // A box of x 3 alone: the image stretches over it.
    let mut boxed = block();
    let reply = apply(
        &mut boxed,
        "voxel.project_paint",
        json!({"view": "top", "rows": ["x"], "legend": {"x": 7}, "origin": [3, 0, 0], "size": [1, 4, 3]}),
    );
    assert_eq!(reply["grid"], json!([1, 4]));
    assert_eq!(reply["cells"], 4);
    assert!((0..4).all(|y| boxed.active().cells[&(3, y, 2)] == 7));
}

/// A dome: the top half of an ellipsoid 21 cells across.
fn dome() -> Session {
    let mut session = empty_session();
    apply(
        &mut session,
        "voxel.ellipsoid",
        json!({"x0": 0, "y0": 0, "z0": -8, "x1": 20, "y1": 20, "z1": 8, "palette": 1}),
    );
    let below: Vec<(i32, i32, i32, i32)> = session
        .active()
        .cells
        .keys()
        .filter(|cell| cell.2 < 0)
        .map(|cell| (cell.0, cell.1, cell.2, 0))
        .collect();
    session.write_cells(&below).unwrap();
    session
}

#[test]
fn normal_limit_leaves_the_steep_sides_of_a_dome() {
    let rows: Vec<String> = (0..21).map(|_| "x".repeat(21)).collect();
    let mut all = dome();
    let open = apply(
        &mut all,
        "voxel.project_paint",
        json!({"view": "top", "rows": rows, "legend": {"x": 3}}),
    );
    assert_eq!(open["steep"], 0);
    let mut limited = dome();
    let steep = apply(
        &mut limited,
        "voxel.project_paint",
        json!({"view": "top", "rows": rows, "legend": {"x": 3}, "normalLimit": 45}),
    );
    assert!(steep["steep"].as_u64().unwrap() > 0, "{steep}");
    assert!(steep["cells"].as_u64().unwrap() < open["cells"].as_u64().unwrap());
    let cells = &limited.active().cells;
    // The crown is painted, the rim at the bottom edge is not.
    assert_eq!(cells[&(10, 10, 8)], 3);
    assert_eq!(cells[&(0, 10, 0)], 1);
    // An oblique view paints the side that faces it, and no more cells.
    let mut oblique = dome();
    let count = oblique.active().cells.len();
    let reply = apply(
        &mut oblique,
        "voxel.project_paint",
        json!({"yaw": -90, "pitch": 30, "rows": rows, "legend": {"x": 4}, "normalLimit": 80}),
    );
    assert!(reply["cells"].as_u64().unwrap() > 50, "{reply}");
    assert_eq!(oblique.active().cells.len(), count);
    let cells = &oblique.active().cells;
    assert_eq!(cells[&(10, 0, 0)], 4);
    assert_eq!(cells[&(10, 20, 0)], 1);
}

#[test]
fn image_colours_quantise_when_painting_and_bad_calls_are_explained() {
    let path = png(
        "paint-pattern.png",
        4,
        4,
        [255, 255, 255, 255],
        [200, 20, 20, 255],
        |x, _| x < 2,
    );
    let mut session = block();
    apply(
        &mut session,
        "palette.set",
        json!({"entries": [{"index": 9, "r": 210, "g": 30, "b": 30}]}),
    );
    let reply = apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "top", "path": path, "slots": [1, 9], "background": "#ffffff"}),
    );
    assert_eq!(reply["cells"], 8);
    assert_eq!(session.active().cells[&(0, 0, 2)], 9);
    assert_eq!(session.active().cells[&(3, 0, 2)], 1);
    for (params, says) in [
        (json!({"rows": ["x"]}), "needs view"),
        (json!({"view": "up", "rows": ["x"]}), "view must be"),
        (json!({"view": "top", "yaw": 0, "rows": ["x"]}), "not both"),
        (json!({"view": "top", "rows": ["x"], "depth": 0}), "depth"),
        (
            json!({"view": "top", "rows": ["x"], "normalLimit": 120}),
            "normalLimit",
        ),
        (json!({"view": "top", "rows": ["x"]}), "legend"),
    ] {
        let err = crate::remote::apply(&mut session, "voxel.project_paint", &params).unwrap_err();
        assert!(err.contains(says), "{params}: {err}");
    }
    let err = crate::remote::apply(
        &mut empty_session(),
        "voxel.project_paint",
        &json!({"view": "top", "rows": ["x"], "palette": 2}),
    )
    .unwrap_err();
    assert!(err.contains("no cells"), "{err}");
}

#[test]
fn a_script_paints_the_same_cells() {
    let mut session = block();
    apply(
        &mut session,
        "voxel.project_paint",
        json!({"view": "top", "rows": ["ab..", "....", "....", "...a"], "legend": {"a": 5, "b": 6}}),
    );
    let source = r#"
        fill_box(0, 0, 0, 3, 3, 2, 1);
        let done = project_paint("top", ["ab..", "....", "....", "...a"], #{legend: #{a: 5, b: 6}});
        if done.cells != 3 || done.changed != 3 { throw "counts"; }
        if done.bounds.max[2] != 2 { throw "bounds"; }
        let side = project_paint(["x"], #{yaw: 0, pitch: 0, palette: 2, origin: [3, 3, 0], size: [1, 1, 1]});
        if side.cells != 1 { throw "oblique"; }
    "#;
    let mut scripted = empty_session();
    scripted.set_script(0, source).unwrap();
    scripted.regenerate_script(0).unwrap();
    let mut expected = session.active().cells.clone();
    expected.insert((3, 3, 0), 2);
    assert_eq!(scripted.active().cells, expected);
    let mut failing = empty_session();
    let err = failing
        .set_script(
            0,
            r#"fill_box(0, 0, 0, 1, 1, 1, 1); project_paint("top", ["x"], #{path: "a.png"});"#,
        )
        .and_then(|_| failing.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("no option `path`"), "{err}");
}

#[test]
fn the_paint_panel_paints_from_an_image() {
    let path = png(
        "panel-paint.png",
        4,
        4,
        [255, 255, 255, 255],
        [40, 120, 40, 255],
        |x, y| x == y,
    );
    let mut session = block();
    session.active_slot = 7;
    let panel = &mut session.silhouette.paint;
    panel.path = path;
    panel.colour_image = false;
    panel.only = "1".into();
    session.paint_from_panel();
    assert!(
        session.status.starts_with("Painted 4 cells (4 changed)"),
        "{}",
        session.status
    );
    // Image row 0 is y = 3.
    assert_eq!(session.active().cells[&(0, 3, 2)], 7);
    assert_eq!(session.active().cells[&(3, 0, 2)], 7);
    let args = session.silhouette.paint.args(7).unwrap();
    assert_eq!(args["view"], "top");
    assert_eq!(args["only"], json!([1]));
    assert_eq!(args["palette"], 7);
    assert!(args.get("normalLimit").is_none());
}

#[test]
fn the_painted_shell_example_runs() {
    let mut session = empty_session();
    session
        .set_script(0, include_str!("../examples/painted_shell.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.active().cells;
    // Plates, seams, and spine on top; the steep rim keeps its colour.
    for slot in [2, 3, 4, 5] {
        assert!(cells.values().any(|s| *s == slot), "slot {slot}");
    }
    let top = cells.keys().map(|c| c.2).max().unwrap();
    assert_ne!(cells[&(11, 14, top)], 2);
}

/// A 6-cell cube of slot 1 with `extra` cells added and `missing` removed.
fn fit_cube(extra: &[Cell], missing: &[Cell]) -> Session {
    let mut session = empty_session();
    let mut writes = Vec::new();
    for z in 0..6 {
        for y in 0..6 {
            for x in 0..6 {
                if !missing.contains(&(x, y, z)) {
                    writes.push((x, y, z, 1));
                }
            }
        }
    }
    writes.extend(extra.iter().map(|c| (c.0, c.1, c.2, 1)));
    session.write_cells(&writes).unwrap();
    session
}

fn square_rows() -> Value {
    json!(["xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx"])
}

fn fit_views(offsets: [[i32; 2]; 3]) -> Value {
    json!([
        {"view": "front", "rows": square_rows(), "align": {"scale": 1, "offset": offsets[0]}},
        {"view": "left", "rows": square_rows(), "align": {"scale": 1, "offset": offsets[1]}},
        {"view": "top", "rows": square_rows(), "align": {"scale": 1, "offset": offsets[2]}}
    ])
}

#[test]
fn fitting_clears_an_excess_spike() {
    let spike = [(2, 2, 6), (2, 2, 7), (2, 2, 8)];
    let mut session = fit_cube(&spike, &[]);
    // margin 2: the box runs from -2; front and left rows count down from
    // z = 10, top rows from y = 7.
    let views = fit_views([[2, 5], [2, 5], [2, 2]]);
    let reply = apply(
        &mut session,
        "voxel.fit_to_views",
        json!({"views": views, "margin": 2}),
    );
    assert_eq!(reply["removed"], 3, "{reply}");
    assert_eq!(reply["added"], 0, "{reply}");
    for view in reply["views"].as_array().unwrap() {
        assert_eq!(view["after"]["iou"], 1.0, "{view}");
    }
    assert!(reply["views"][0]["before"]["iou"].as_f64().unwrap() < 1.0);
    assert!(spike
        .iter()
        .all(|c| !session.active().cells.contains_key(c)));
    assert_eq!(session.active().cells.len(), 216);
    // One undo step puts the spike back.
    session.undo();
    assert_eq!(session.active().cells.len(), 219);

    // A script runs the same code on rows.
    let source = r#"
        let cells = [];
        for z in 0..6 { for y in 0..6 { for x in 0..6 { cells.push([x, y, z, 1]); } } }
        cells.push([2, 2, 6, 1]); cells.push([2, 2, 7, 1]); cells.push([2, 2, 8, 1]);
        set_voxels(cells);
        let sq = ["xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx", "xxxxxx"];
        let done = fit_to_views([
            #{view: "front", rows: sq, align: #{scale: 1, offset: [2, 5]}},
            #{view: "left", rows: sq, align: #{scale: 1, offset: [2, 5]}},
            #{view: "top", rows: sq, align: #{scale: 1, offset: [2, 2]}}
        ], #{margin: 2});
        if done.removed != 3 { throw "removed " + done.removed; }
        if done.views[0].after.iou != 1.0 { throw "iou"; }
    "#;
    let mut scripted = empty_session();
    scripted.set_script(0, source).unwrap();
    scripted.regenerate_script(0).unwrap();
    assert_eq!(scripted.active().cells.len(), 216);
    let err = scripted
        .set_script(0, r#"fit_to_views([#{view: "top", path: "a.png"}]);"#)
        .and_then(|_| scripted.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("cannot read files"), "{err}");
}

#[test]
fn fitting_fills_a_missing_column_and_recolours_from_the_palette() {
    let column: Vec<Cell> = (0..6).map(|z| (5, 5, z)).collect();
    let mut session = fit_cube(&[], &column);
    let mut views = fit_views([[2, 2], [2, 2], [2, 2]]);
    views[2]["rows"] = json!(["bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb"]);
    views[2]["legend"] = json!({"b": 2});
    let reply = apply(
        &mut session,
        "voxel.fit_to_views",
        json!({"views": views, "margin": 2, "slots": [1, 2]}),
    );
    assert_eq!(reply["added"], 6, "{reply}");
    assert_eq!(reply["removed"], 0, "{reply}");
    assert_eq!(
        reply["views"][2]["before"]["missing"].as_f64().unwrap() > 0.0,
        true
    );
    assert_eq!(reply["views"][2]["after"]["iou"], 1.0);
    assert_eq!(reply["views"][2]["after"]["colorError"], 0.0);
    let cells = &session.active().cells;
    assert_eq!(cells.len(), 216);
    // The top layer takes the top view's slot; the sides have no colour,
    // and the filled column took the slot of the view that filled it.
    for y in 0..6 {
        for x in 0..6 {
            assert_eq!(cells[&(x, y, 5)], 2);
            assert_eq!(cells[&(x, y, 4)], if (x, y) == (5, 5) { 2 } else { 1 });
        }
    }
    let palette: Vec<(u64, u64)> = reply["palette"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["slot"].as_u64().unwrap(), p["cells"].as_u64().unwrap()))
        .collect();
    assert_eq!(palette, vec![(1, 175), (2, 41)]);
    // Only recolouring changes no shape; the same seed gives the same cells.
    let mut again = fit_cube(&[], &column);
    let reply = apply(
        &mut again,
        "voxel.fit_to_views",
        json!({"views": views, "margin": 2, "slots": [1, 2], "shape": false}),
    );
    assert_eq!(
        (reply["added"].as_u64(), reply["removed"].as_u64()),
        (Some(0), Some(0))
    );
    assert_eq!(reply["recolored"], 35);
}

#[test]
fn a_reference_object_sets_the_depth_and_others_stay_put() {
    let mut session = fit_cube(&[], &[]);
    apply(&mut session, "object.add", json!({"name": "ref"}));
    let mut writes = Vec::new();
    for z in 0..6 {
        for y in 2..6 {
            for x in 0..6 {
                writes.push((x, y, z, 3));
            }
        }
    }
    session.write_cells(&writes).unwrap();
    session.objects[1].translation = session.objects[0].translation;
    session.selected = 0;
    let views =
        json!([{"view": "front", "rows": square_rows(), "align": {"scale": 1, "offset": [2, 2]}}]);
    let reply = apply(
        &mut session,
        "voxel.fit_to_views",
        json!({"views": views, "margin": 2, "reference": "ref", "depthWeight": 1, "smoothWeight": 0}),
    );
    assert_eq!(reply["views"][0]["before"]["depthError"], 2.0, "{reply}");
    assert_eq!(reply["views"][0]["after"]["depthError"], 0.0, "{reply}");
    assert_eq!(reply["removed"], 72);
    assert!(session.active().cells.keys().all(|c| c.1 >= 2));
    assert_eq!(session.objects[1].cells.len(), 144);

    // Without the reference, the other object is context: it covers the
    // front view and never changes.
    let mut session = fit_cube(&[], &[]);
    apply(&mut session, "object.add", json!({"name": "wing"}));
    session.write_cells(&[(6, 0, 0, 4), (7, 0, 0, 4)]).unwrap();
    session.objects[1].translation = session.objects[0].translation;
    session.selected = 0;
    let rows = json!(["xxxxxx..", "xxxxxx..", "xxxxxx..", "xxxxxx..", "xxxxxx..", "xxxxxxxx"]);
    let views = json!([{"view": "front", "rows": rows, "align": {"scale": 1, "offset": [2, 2]}}]);
    let reply = apply(
        &mut session,
        "voxel.fit_to_views",
        json!({"views": views, "margin": 2}),
    );
    assert_eq!(reply["views"][0]["before"]["iou"], 1.0, "{reply}");
    assert_eq!(reply["changed"], 0);
    let isolated = crate::remote::apply(
        &mut session,
        "voxel.fit_to_views",
        &json!({"views": views, "margin": 2, "isolate": true, "iterations": 0}),
    )
    .unwrap();
    assert!(isolated["views"][0]["before"]["missing"].as_f64().unwrap() > 0.0);
    assert_eq!(session.objects[1].cells.len(), 2);
}

#[test]
fn fit_calls_are_checked_and_the_panel_builds_arguments() {
    let mut session = fit_cube(&[], &[]);
    let one = json!({"view": "top", "rows": square_rows()});
    for (params, want) in [
        (json!({"views": [one, one]}), "two views look from the top"),
        (json!({"views": []}), "1 to 6 views"),
        (
            json!({"views": [one], "iterations": 99}),
            "iterations must be",
        ),
        (json!({"views": [one], "slots": [0]}), "slots must be"),
        (json!({"views": [one], "reference": 0}), "another object"),
        (
            json!({"views": [{"view": "top", "rows": ["...."]}]}),
            "mask is empty",
        ),
    ] {
        let err = crate::remote::apply(&mut session, "voxel.fit_to_views", &params).unwrap_err();
        assert!(err.contains(want), "{err} / {want}");
    }
    let mut panel = crate::state::SilhouettePanels::default();
    assert!(panel.fit.args(&panel.carve).is_err());
    panel.carve.views[0].path = "sheet.png".into();
    panel.carve.views[0].crop = "1 2 3 4".into();
    panel.carve.views[1].on = false;
    panel.fit.slots = "3 4".into();
    panel.fit.reference = "blockout".into();
    let args = panel.fit.args(&panel.carve).unwrap();
    assert_eq!(args["views"][0]["crop"], json!([1, 2, 3, 4]));
    assert_eq!(args["views"].as_array().unwrap().len(), 1);
    assert_eq!(args["slots"], json!([3, 4]));
    assert_eq!(args["reference"], "blockout");
    assert_eq!(args["align"], "fit");
}

#[test]
fn the_fitted_beetle_example_runs() {
    let mut session = empty_session();
    session
        .set_script(0, include_str!("../examples/fitted_beetle.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.active().cells;
    // The box is cut down to the beetle, coloured from its four slots.
    assert!(cells.len() < 12 * 16 * 6, "{}", cells.len());
    for slot in [3, 4, 5] {
        assert!(cells.values().any(|s| *s == slot), "slot {slot}");
    }
    // The top view's corners are background: the box's corners are gone.
    assert!(!cells.contains_key(&(0, 0, 5)));
}

#[test]
fn kept_slots_and_shape_only_views_leave_colours_alone() {
    let mut session = fit_cube(&[], &[]);
    session.write_cells(&[(2, 0, 5, 9)]).unwrap();
    let mut views = fit_views([[2, 2], [2, 2], [2, 2]]);
    views[2]["rows"] = json!(["bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb", "bbbbbb"]);
    views[2]["legend"] = json!({"b": 2});
    let base = json!({"views": views, "margin": 2, "slots": [1, 2]});
    // keep: the slot-9 cell stays, the rest of the top recolours.
    let mut kept = base.clone();
    kept["keep"] = json!([9]);
    let reply = apply(&mut session, "voxel.fit_to_views", kept);
    assert_eq!(reply["recolored"], 35, "{reply}");
    assert_eq!(session.active().cells[&(2, 0, 5)], 9);
    assert_eq!(session.active().cells[&(0, 0, 5)], 2);
    // A top view with color false shapes but paints nothing.
    let mut session = fit_cube(&[], &[]);
    let mut shape_only = base.clone();
    shape_only["views"][2]["color"] = json!(false);
    let reply = apply(&mut session, "voxel.fit_to_views", shape_only);
    assert_eq!(reply["changed"], 0, "{reply}");
    // The panel passes a row's Colour box and Keep through.
    let mut panel = crate::state::SilhouettePanels::default();
    panel.carve.views[0].path = "sheet.png".into();
    panel.carve.views[1].path = "sheet.png".into();
    panel.fit.paint_views = [true, false, true];
    panel.fit.keep = "9".into();
    let args = panel.fit.args(&panel.carve).unwrap();
    assert_eq!(args["views"][0].get("color"), None);
    assert_eq!(args["views"][1]["color"], false);
    assert_eq!(args["keep"], json!([9]));
    assert_eq!(args["normalLimit"], 60.0);
}
