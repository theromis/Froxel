use std::path::PathBuf;

use serde_json::{json, Value};

use crate::state::Session;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("froxel-mask-edit-{}-{name}", std::process::id()))
}

fn write(name: &str, w: u32, h: u32, colour: impl Fn(u32, u32) -> [u8; 4]) -> String {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend(colour(x, y));
        }
    }
    let path = temp(name);
    crate::trace::write_rgba_png(&path, w, h, &rgba).unwrap();
    path.display().to_string()
}

const BODY: [u8; 4] = [120, 80, 40, 255];
const RIDER: [u8; 4] = [220, 30, 30, 255];
const SPLASH: [u8; 4] = [250, 250, 250, 255];

fn body(x: u32, y: u32) -> bool {
    (20..100).contains(&x) && (40..80).contains(&y)
}

fn rider(x: u32, y: u32) -> bool {
    (50..66).contains(&x) && (24..40).contains(&y)
}

fn splash(x: u32, y: u32) -> bool {
    let (dx, dy) = (x as i32 - 130, y as i32 - 95);
    dx * dx + dy * dy <= 64
}

/// A hero view: a 2:1 body on a blue-green gradient (a photo's water), a
/// red rider standing on it, and a white splash apart from it.
fn hero() -> String {
    write("hero.png", 160, 120, |x, y| {
        if rider(x, y) {
            RIDER
        } else if body(x, y) {
            BODY
        } else if splash(x, y) {
            SPLASH
        } else {
            [
                (30 + x / 2) as u8,
                (90 + y / 2) as u8,
                (210 - x / 3) as u8,
                255,
            ]
        }
    })
}

/// A 6 x 2 x 3 box, 2:1 from the front like the hero's body.
fn front_box() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    crate::remote::apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 1, "z1": 2, "palette": 1}),
    )
    .unwrap();
    session
}

fn compare(session: &mut Session, extra: Value) -> Result<Value, String> {
    let mut args = json!({
        "path": temp("overlay.png").display().to_string(),
        "concept": hero(),
        "view": "front",
        "orthographic": true,
        "height": 120
    });
    for (key, value) in extra.as_object().unwrap() {
        args[key] = value.clone();
    }
    crate::remote::apply(session, "render.compare_silhouette", &args)
}

fn iou(reply: &Value) -> f64 {
    reply["iou"].as_f64().unwrap()
}

#[test]
fn a_hero_view_scores_badly_until_its_background_is_masked() {
    let mut session = front_box();
    let plain = compare(&mut session, json!({})).unwrap();
    assert!(iou(&plain) < 0.8, "{plain}");
    assert_eq!(
        plain["conceptBounds"],
        json!([3, 0, 150, 94]),
        "the water is inked"
    );
    assert!(plain.get("maskEdits").is_none());
    // Flood the water from the border, drop the splash's colour, and erase
    // the rider: only the body is left.
    let cleaned = compare(
        &mut session,
        json!({
            "flood": true,
            "excludeColors": ["#fafafa"],
            "erase": [[50, 24, 16, 16]]
        }),
    )
    .unwrap();
    assert!(iou(&cleaned) > 0.95, "{cleaned}");
    let edits = &cleaned["maskEdits"];
    assert_eq!(edits["after"], 80 * 40, "{edits}");
    assert_eq!(edits["erased"], 16 * 16);
    assert!(edits["flooded"].as_u64().unwrap() > 1000, "{edits}");
    // The splash is apart: keepLargest drops it without naming its colour.
    let largest = compare(
        &mut session,
        json!({"flood": 24, "keepLargest": true, "erase": [[50, 24, 16, 16]]}),
    )
    .unwrap();
    assert!(iou(&largest) > 0.95, "{largest}");
    assert!(largest["maskEdits"]["dropped"].as_u64().unwrap() > 150);
    // The rider stays without its erase: it is joined to the body.
    let with_rider = compare(&mut session, json!({"flood": true, "keepLargest": true})).unwrap();
    assert!(iou(&with_rider) < iou(&largest), "{with_rider}");
}

#[test]
fn a_mask_image_keeps_only_its_ink() {
    let mut session = front_box();
    // White on black, as large as the concept: it shares the crop.
    let mask = write("mask.png", 160, 120, |x, y| {
        if body(x, y) {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 255]
        }
    });
    let masked = compare(&mut session, json!({"mask": mask})).unwrap();
    assert!(iou(&masked) > 0.95, "{masked}");
    // A transparent mask of the rider and splash, at half size, inverted.
    let half = write("mask-half.png", 80, 60, |x, y| {
        let (x, y) = (x * 2, y * 2);
        if rider(x, y) || splash(x, y) {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let inverted = compare(
        &mut session,
        json!({"mask": {"path": half, "invert": true}, "flood": true}),
    )
    .unwrap();
    assert!(iou(&inverted) > 0.9, "{inverted}");
    // A crop: the erase rectangles stay in image pixels.
    let cropped = compare(
        &mut session,
        json!({"crop": [10, 10, 140, 100], "flood": true, "keepLargest": true,
               "erase": [[50, 24, 16, 16]]}),
    )
    .unwrap();
    assert_eq!(cropped["maskEdits"]["erased"], 16 * 16, "{cropped}");
    assert!(iou(&cropped) > 0.95, "{cropped}");
}

#[test]
fn bad_mask_edits_are_explained() {
    let mut session = front_box();
    let err = compare(&mut session, json!({"erase": [[0, 0, 160, 120]]})).unwrap_err();
    assert!(err.contains("no inked pixel"), "{err}");
    let err = compare(&mut session, json!({"excludeColors": ["blue"]})).unwrap_err();
    assert!(err.contains("excludeColors"), "{err}");
    let err = compare(&mut session, json!({"flood": "yes"})).unwrap_err();
    assert!(err.contains("flood"), "{err}");
    // Rows have no background colours to flood.
    let err = crate::remote::apply(
        &mut session,
        "voxel.carve_from_views",
        &json!({"views": [
            {"view": "front", "rows": ["##", "##"], "flood": true},
            {"view": "left", "rows": ["##", "##"]}
        ], "palette": 1}),
    )
    .unwrap_err();
    assert!(err.contains("flood"), "{err}");
}

#[test]
fn a_carve_view_takes_the_same_edits() {
    let mut session = Session::new();
    session.replace_empty();
    let sheet = hero();
    let reply = crate::remote::apply(
        &mut session,
        "voxel.carve_from_views",
        &json!({
            "views": [
                {"view": "front", "erase": [[50, 24, 16, 16]]},
                {"view": "top", "rows": ["################", "################"], "cellsPerPixel": 1}
            ],
            "path": sheet,
            "flood": true,
            "keepLargest": true,
            "cellsPerPixel": 0.1,
            "palette": 1
        }),
    )
    .unwrap();
    // The body alone, 80 x 40 pixels at 0.1: 8 cells wide and 4 tall.
    assert_eq!(reply["views"][0]["covered"], 32, "{reply}");
    assert_eq!(
        reply["bounds"]["max"][2].as_i64().unwrap() - reply["bounds"]["min"][2].as_i64().unwrap()
            + 1,
        4,
        "{reply}"
    );
    assert_eq!(
        reply["bounds"]["max"][0].as_i64().unwrap() - reply["bounds"]["min"][0].as_i64().unwrap()
            + 1,
        8,
        "{reply}"
    );
}

#[test]
fn the_compare_panel_and_the_command_line_pass_the_edits() {
    let mut session = front_box();
    let mut panel = session.silhouette.compare.clone();
    panel.path = hero();
    panel.camera = 2;
    panel.flood = true;
    panel.exclude = "#fafafa".into();
    panel.erase = "50 24 16 16\n".into();
    let args = panel.args(&session, "out.png").unwrap();
    assert_eq!(args["flood"], 24.0);
    assert_eq!(args["excludeColors"], json!(["#fafafa"]));
    assert_eq!(args["excludeTolerance"], 40.0);
    assert_eq!(args["erase"], json!([[50, 24, 16, 16]]));
    session.silhouette.compare = panel;
    session.compare_from_panel();
    let overlay = session.silhouette.overlay.as_ref().expect("an overlay");
    assert!(overlay.iou > 0.95, "{}", session.status);
    let args: Vec<String> = [
        "--input",
        "x.vox",
        "--concept",
        "c.png",
        "--out",
        "o.png",
        "--flood",
        "30",
        "--exclude",
        "#ffffff,#000000",
        "--erase",
        "1,2,3,4",
        "--erase",
        "5,6,7,8",
        "--mask",
        "m.png",
        "--invert-mask",
        "--keep-largest",
        "--exclude-tolerance",
        "12",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (_, params, _) = crate::cli::compare_args(&args).unwrap().unwrap();
    assert_eq!(params["flood"], 30.0);
    assert_eq!(params["excludeColors"], json!(["#ffffff", "#000000"]));
    assert_eq!(params["excludeTolerance"], 12.0);
    assert_eq!(params["erase"], json!([[1, 2, 3, 4], [5, 6, 7, 8]]));
    assert_eq!(params["mask"], json!({"path": "m.png", "invert": true}));
    assert_eq!(params["keepLargest"], true);
}

#[test]
fn a_script_view_drops_a_speck_and_an_erased_corner() {
    let source = r#"
        let views = [
            #{view: "front", rows: ["xxxx..x", "xxxx...", "xxxx...", "xxxx..."], erase: [[0, 3, 1, 1]]},
            #{view: "left", rows: ["xxxx", "xxxx", "xxxx", "xxxx"]}
        ];
        let done = carve_from_views(views, #{cellsPerPixel: 1, palette: 4, keepLargest: true});
        if done.bounds.max[0] != 3 { throw `bounds ${done.bounds}`; }
    "#;
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    // 4 x 4 x 4 less the erased column at x 0, bottom row.
    assert_eq!(session.active().cells.len(), 64 - 4);
    let err = session
        .set_script(0, r#"carve_from_views([#{view: "front", rows: ["x"]}, #{view: "left", rows: ["x"]}], #{flood: true, palette: 1});"#)
        .and_then(|_| session.regenerate_script(0))
        .unwrap_err();
    assert!(err.contains("flood"), "{err}");
}

#[test]
fn fill_holes_brings_back_a_white_eye_the_background_rule_keyed_out() {
    // A brown disc on white with a near-white eye ringed in dark: the eye
    // is within the background tolerance, so it reads as a hole.
    let path = write("eye.png", 80, 80, |x, y| {
        let (dx, dy) = (x as i32 - 40, y as i32 - 40);
        let (ex, ey) = (x as i32 - 40, y as i32 - 30);
        let r = dx * dx + dy * dy;
        let e = ex * ex + ey * ey;
        if r > 36 * 36 {
            [255, 255, 255, 255]
        } else if e <= 8 * 8 {
            [248, 248, 246, 255]
        } else if e <= 11 * 11 {
            [20, 20, 20, 255]
        } else {
            BODY
        }
    });
    let mut session = Session::new();
    let args = |fill: Value| {
        json!({
            "views": [{"view": "front", "path": path}],
            "size": [16, null, null],
            "colors": 4,
            "fillHoles": fill
        })
    };
    let near_white = |reply: &Value| {
        reply["legend"]
            .as_object()
            .unwrap()
            .values()
            .any(|hex| hex.as_str().unwrap() > "#e0e0e0")
    };
    let holed =
        crate::remote::apply(&mut session, "image.pixelate_views", &args(json!(false))).unwrap();
    assert!(!near_white(&holed), "{holed}");
    let filled =
        crate::remote::apply(&mut session, "image.pixelate_views", &args(json!(true))).unwrap();
    assert!(near_white(&filled), "{filled}");
    // A share too small for the eye leaves it a hole.
    let small =
        crate::remote::apply(&mut session, "image.pixelate_views", &args(json!(0.01))).unwrap();
    assert!(!near_white(&small), "{small}");
    // Rows have no colours to fill a hole with: a sheet-wide fillHoles
    // passes them by, as flood does, and their own is refused.
    let rows = |fill: Value, own: Value| {
        crate::remote::apply(
            &mut Session::new(),
            "image.pixelate_views",
            &json!({"views": [{"view": "front", "rows": ["aa", "aa"],
                               "legend": {"a": "#808080"}, "fillHoles": own}],
                    "size": [2, null, null], "fillHoles": fill}),
        )
    };
    assert!(rows(json!(true), Value::Null).is_ok());
    let rows = rows(Value::Null, json!(true));
    assert!(rows.unwrap_err().contains("fillHoles"));
}
