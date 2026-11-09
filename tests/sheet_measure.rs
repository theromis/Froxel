use std::path::PathBuf;

use serde_json::{json, Value};

use crate::state::Session;

const TURTLE: &str =
    "/Users/roman/work/frog-world/docs/design/characters/ride-creatures/views-v06.png";

fn measure(params: Value) -> Result<Value, String> {
    let mut session = Session::new();
    crate::remote::apply(&mut session, "image.measure_sheet", &params)
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("froxel-sheet-{}-{name}", std::process::id()))
}

const PITCH: u32 = 8;
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

/// A sheet of cube views on magenta: each view is cubes of `PITCH` pixels
/// with a darker seam on each cube's last row and column.
struct Sheet {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

impl Sheet {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            rgba: MAGENTA.repeat((w * h) as usize),
        }
    }

    /// Cubes `cells(cx, cy)` of a `cw` x `ch` grid at pixel `(x, y)`.
    fn view(&mut self, x: u32, y: u32, cw: u32, ch: u32, cells: impl Fn(u32, u32) -> bool) {
        for cy in 0..ch {
            for cx in 0..cw {
                if !cells(cx, cy) {
                    continue;
                }
                let tone = 120 + ((cx * 7 + cy * 13) % 5) as u8 * 6;
                for py in 0..PITCH {
                    for px in 0..PITCH {
                        let seam = px == PITCH - 1 || py == PITCH - 1;
                        let v = if seam { tone - 45 } else { tone };
                        let i =
                            (((y + cy * PITCH + py) * self.w + x + cx * PITCH + px) * 4) as usize;
                        self.rgba[i..i + 4].copy_from_slice(&[v, v / 2 + 40, 30, 255]);
                    }
                }
            }
        }
    }

    fn write(&self, name: &str) -> String {
        let path = temp(name);
        crate::trace::write_rgba_png(&path, self.w, self.h, &self.rgba).unwrap();
        path.display().to_string()
    }
}

/// The top view of a turtle in cubes: an oval shell 20 x 30 cubes, a
/// flipper of 5 x 4 cubes at each of `rows`, a head in front, a tail behind.
fn top_cells(flipper_rows: &'static [u32]) -> impl Fn(u32, u32) -> bool {
    move |x, y| {
        let (ex, ey) = (
            (x as f32 + 0.5 - 15.0) / 10.0,
            (y as f32 + 0.5 - 21.0) / 15.0,
        );
        let shell = ex * ex + ey * ey <= 1.0;
        let flipper = flipper_rows.iter().any(|r| (*r..r + 4).contains(&y))
            && ((0..6).contains(&x) || (24..30).contains(&x));
        let head = (12..18).contains(&x) && (0..7).contains(&y);
        let tail = (14..16).contains(&x) && (35..39).contains(&y);
        shell || flipper || head || tail
    }
}

/// front 30 x 8 cubes at (40, 40), side 39 x 8 at (340, 40), top 30 x 39
/// at (40, 160), back 30 x 8 at (340, 300), and a label speck.
fn sheet(flipper_rows: &'static [u32]) -> Sheet {
    let mut sheet = Sheet::new(700, 500);
    sheet.view(40, 40, 30, 8, |_, _| true);
    sheet.view(340, 40, 39, 8, |_, _| true);
    sheet.view(40, 160, 30, 39, top_cells(flipper_rows));
    sheet.view(340, 300, 30, 8, |_, _| true);
    for y in 480..483 {
        for x in 600..603 {
            let i = ((y * 700 + x) * 4) as usize;
            sheet.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    sheet
}

#[test]
fn a_synthetic_sheet_gives_its_views_pitch_checks_and_parts() {
    let path = sheet(&[10, 28]).write("four.png");
    let reply = measure(json!({"path": path, "parts": true})).unwrap();
    assert_eq!(reply["layout"], true, "{reply}");
    let views = reply["views"].as_array().unwrap();
    let names: Vec<&str> = views.iter().map(|v| v["view"].as_str().unwrap()).collect();
    assert_eq!(names, ["front", "side", "top", "back"]);
    assert_eq!(views[0]["crop"], json!([40, 40, 240, 64]));
    assert_eq!(views[1]["crop"], json!([340, 40, 312, 64]));
    assert_eq!(views[2]["crop"], json!([40, 160, 240, 312]));
    for view in views {
        let pitch = view["pitch"].as_f64().unwrap();
        assert!((pitch - 8.0).abs() < 0.15, "{view}");
    }
    assert_eq!(views[0]["cubes"], json!([30.0, 8.0]));
    // The label speck is left out.
    assert_eq!(reply["ignored"], json!([[600, 480, 3, 3]]));
    let checks = reply["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 3);
    let by = |name: &str| checks.iter().find(|c| c["check"] == name).unwrap();
    assert_eq!(by("front height = side height")["percent"], 0.0);
    assert_eq!(by("front width = top width")["percent"], 0.0);
    // Side 39 cubes long, top 39 long.
    assert_eq!(by("side length = top length")["percentCubes"], 0.0);
    assert_eq!(reply["ok"], true);
    assert_eq!(
        reply["fitViews"][2],
        json!({"view": "top", "crop": [40, 160, 240, 312]})
    );
    // Four flippers, a head, and a tail.
    let parts = &reply["parts"];
    assert_eq!(parts["count"], 6, "{parts}");
    let dirs: Vec<&str> = parts["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["direction"].as_str().unwrap())
        .collect();
    assert!(dirs.contains(&"n") && dirs.contains(&"s"), "{dirs:?}");
    assert_eq!(dirs.iter().filter(|d| d.len() == 2).count(), 4, "{dirs:?}");
}

#[test]
fn a_top_view_with_six_flippers_counts_eight_parts_and_a_short_side_fails() {
    let mut bad = sheet(&[10, 19, 28]);
    // Draw the side view 4 cubes short over the old one.
    for y in 40..104 {
        for x in 340..652 {
            let i = ((y * 700 + x) * 4) as usize;
            bad.rgba[i..i + 4].copy_from_slice(&MAGENTA);
        }
    }
    bad.view(340, 40, 35, 8, |_, _| true);
    let path = bad.write("six.png");
    let reply = measure(json!({"path": path, "parts": {"view": "top"}})).unwrap();
    assert_eq!(reply["parts"]["count"], 8, "{}", reply["parts"]);
    let check = reply["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == "side length = top length")
        .unwrap()
        .clone();
    assert_eq!(check["ok"], false, "{check}");
    assert!(
        (check["percent"].as_f64().unwrap() - 10.3).abs() < 0.2,
        "{check}"
    );
    assert_eq!(reply["ok"], false);
}

#[test]
fn other_layouts_and_bad_arguments() {
    // Three views in a row are not a 2 x 2 sheet: no names, no checks.
    let mut row = Sheet::new(600, 120);
    for i in 0..3 {
        row.view(20 + i * 200, 20, 20, 10, |_, _| true);
    }
    let path = row.write("row.png");
    let reply = measure(json!({"path": path})).unwrap();
    assert_eq!(reply["layout"], false);
    assert_eq!(reply["views"].as_array().unwrap().len(), 3);
    assert!(reply["views"][0]["view"].is_null());
    assert_eq!(reply["checks"], json!([]));
    assert!(reply.get("fitViews").is_none());
    // Parts by index when there are no names.
    let parts = measure(json!({"path": path, "parts": {"view": 1}})).unwrap();
    assert_eq!(parts["parts"]["count"], 0, "{parts}");
    let err = measure(json!({"path": path, "parts": true})).unwrap_err();
    assert!(err.contains("no view `top`"), "{err}");
    let err = measure(json!({"path": path, "layout": ["front", "side"]})).unwrap_err();
    assert!(err.contains("layout"), "{err}");
    let err = measure(json!({"path": path, "pitchRange": [8, 10]})).unwrap_err();
    assert!(err.contains("pitchRange"), "{err}");
    // A crop reads part of the image; crops stay in image pixels.
    let cropped = measure(json!({"path": path, "crop": [200, 0, 400, 120]})).unwrap();
    assert_eq!(cropped["views"][0]["crop"], json!([220, 20, 160, 80]));
    // The custom layout names a 2 x 2 sheet.
    let path = sheet(&[10, 28]).write("names.png");
    let named = measure(json!({"path": path, "layout": ["front", "left", "top", "back"]})).unwrap();
    assert_eq!(named["views"][1]["view"], "left");
}

#[test]
fn the_turtle_sheet_measures_as_its_hand_crops() {
    if !std::path::Path::new(TURTLE).exists() {
        eprintln!("skipped: {TURTLE} is not here");
        return;
    }
    let reply = measure(json!({"path": TURTLE, "parts": true})).unwrap();
    let hand = [
        ("front", [40, 201, 547, 275]),
        ("side", [646, 199, 595, 280]),
        ("top", [40, 637, 537, 575]),
        ("back", [672, 773, 541, 276]),
    ];
    for (view, (name, crop)) in reply["views"].as_array().unwrap().iter().zip(hand) {
        assert_eq!(view["view"], name);
        for (got, want) in view["crop"].as_array().unwrap().iter().zip(crop) {
            assert!(
                (got.as_i64().unwrap() - want).abs() <= 1,
                "{view} vs {crop:?}"
            );
        }
        // The sheet's cubes are about 13 pixels wide and 18 tall.
        let px = view["pitchX"].as_f64().unwrap();
        let py = view["pitchY"].as_f64().unwrap();
        assert!((12.5..14.0).contains(&px), "{view}");
        assert!((16.5..18.5).contains(&py), "{view}");
        assert_eq!(view["square"], false, "{view}");
    }
    // Four flippers, the head, and the tail.
    assert_eq!(reply["parts"]["count"], 6, "{}", reply["parts"]);
    let checks = reply["checks"].as_array().unwrap();
    assert!(
        checks.iter().all(|c| c["percent"].as_f64().unwrap() < 3.5),
        "{reply}"
    );
}

#[test]
fn the_panel_measures_and_fills_the_carve_rows_and_the_command_line_parses() {
    let path = sheet(&[10, 28]).write("panel.png");
    let mut session = Session::new();
    session.replace_empty();
    session.silhouette.measure.path = path.clone();
    session.silhouette.measure.parts = true;
    session.measure_from_panel();
    assert!(
        session.status.contains("every check ok"),
        "{}",
        session.status
    );
    let lines = &session.silhouette.measure.lines;
    assert!(lines.iter().any(|l| l.contains("6 parts")), "{lines:?}");
    session.use_measured_crops();
    let rows = &session.silhouette.carve.views;
    let crops: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (crate::state::SIDES[r.side].1, r.crop.as_str()))
        .collect();
    assert_eq!(
        crops,
        [
            ("front", "40 40 240 64"),
            ("left", "340 40 312 64"),
            ("top", "40 160 240 312")
        ]
    );
    assert!(rows.iter().all(|r| r.on && r.path == path));
    let args: Vec<String> = [
        "--sheet",
        "s.png",
        "--parts",
        "top",
        "--pitch-range",
        "4,40",
        "--json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (params, as_json) = crate::cli::measure_args(&args).unwrap().unwrap();
    assert!(as_json);
    assert_eq!(params["parts"], json!({"view": "top"}));
    assert_eq!(params["pitchRange"], json!([4.0, 40.0]));
}
