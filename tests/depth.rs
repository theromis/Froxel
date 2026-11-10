//! Depth maps in `carve_from_views` and `fit_to_views`: a sphere and a dome
//! rendered to orthographic depth come back round within a cell.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::state::Cell;
use crate::tools::Side;

/// Pixels per cell of the rendered maps.
const PPC: f64 = 4.0;

/// The orthographic depth map of `inside` from `side` over a box of
/// `size` cells: per pixel, the grey `0.95 - 0.7 * t / depth` where `t` is
/// the first point inside along the ray from the camera side, so the map
/// is affine in depth, as a relative depth model's is; `null` misses.
fn render(side: Side, size: [u32; 3], inside: &dyn Fn([f64; 3]) -> bool) -> Value {
    let [(u, su), (v, sv), (d, toward)] = side.axes();
    let (w, h) = (
        (f64::from(size[u]) * PPC) as usize,
        (f64::from(size[v]) * PPC) as usize,
    );
    let depth = f64::from(size[d]);
    let mut rows = Vec::with_capacity(h);
    for py in 0..h {
        let mut row = Vec::with_capacity(w);
        for px in 0..w {
            let a = (px as f64 + 0.5) / PPC;
            let b = (py as f64 + 0.5) / PPC;
            let mut p = [0.0f64; 3];
            p[u] = if su > 0 { a } else { f64::from(size[u]) - a };
            p[v] = if sv > 0 { b } else { f64::from(size[v]) - b };
            let at = |t: f64| {
                let mut q = p;
                q[d] = if toward > 0 { depth - t } else { t };
                q
            };
            let step = 0.05;
            let mut hit = None;
            let mut t = 0.0;
            while t <= depth {
                if inside(at(t)) {
                    // Refine between the last miss and this hit.
                    let (mut lo, mut hi) = ((t - step).max(0.0), t);
                    for _ in 0..20 {
                        let mid = (lo + hi) / 2.0;
                        if inside(at(mid)) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    hit = Some(hi);
                    break;
                }
                t += step;
            }
            row.push(match hit {
                Some(t) => json!(0.95 - 0.7 * t / depth),
                None => Value::Null,
            });
        }
        rows.push(Value::Array(row));
    }
    Value::Array(rows)
}

fn carve(params: Value) -> (HashMap<Cell, u8>, Value) {
    let palette = [[128u8; 3]; 256];
    let (writes, reply) =
        crate::remote::hull_writes(&HashMap::new(), &palette, &params, false).expect("carve");
    let cells = writes
        .into_iter()
        .filter(|w| w.3 != 0)
        .map(|(x, y, z, s)| ((x, y, z), s as u8))
        .collect();
    (cells, Value::Object(reply))
}

/// The cells whose membership differs from `ideal`, and the largest
/// `|sdf|` at their centres.
fn mismatch(
    cells: &HashMap<Cell, u8>,
    size: [u32; 3],
    sdf: &dyn Fn([f64; 3]) -> f64,
) -> (usize, f64) {
    let mut wrong = 0;
    let mut worst = 0.0f64;
    for z in 0..size[2] as i32 {
        for y in 0..size[1] as i32 {
            for x in 0..size[0] as i32 {
                let c = [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5];
                let ideal = sdf(c) < 0.0;
                if ideal != cells.contains_key(&(x, y, z)) {
                    wrong += 1;
                    worst = worst.max(sdf(c).abs());
                }
            }
        }
    }
    eprintln!("mismatch: {wrong} cells, worst |sdf| {worst:.2}");
    (wrong, worst)
}

fn sphere(p: [f64; 3]) -> f64 {
    ((p[0] - 10.0).powi(2) + (p[1] - 10.0).powi(2) + (p[2] - 10.0).powi(2)).sqrt() - 9.5
}

fn dome(p: [f64; 3]) -> f64 {
    let r = ((p[0] - 10.0).powi(2) + (p[1] - 10.0).powi(2) + p[2].powi(2)).sqrt() - 9.5;
    r.max(-p[2])
}

fn depth_view(side: Side, size: [u32; 3], sdf: &dyn Fn([f64; 3]) -> f64) -> Value {
    json!({
        "view": side.name(),
        "cellsPerPixel": 1.0 / PPC,
        "depth": {"values": render(side, size, &|p| sdf(p) < 0.0)}
    })
}

#[test]
fn a_sphere_carved_from_six_depth_maps_is_round_within_a_cell() {
    let size = [20, 20, 20];
    let sides = [
        Side::Front,
        Side::Back,
        Side::Left,
        Side::Right,
        Side::Top,
        Side::Bottom,
    ];
    let views: Vec<Value> = sides
        .into_iter()
        .map(|side| depth_view(side, size, &sphere))
        .collect();
    let (cells, reply) = carve(json!({"views": views, "palette": 1}));
    let (wrong, worst) = mismatch(&cells, size, &sphere);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
    // The hull calibration finds the map's scale: grey 0.95 - 0.7 * t / 20.
    let cal = &reply["views"][0]["depth"];
    assert_eq!(cal["calibrate"], "hull");
    let far = cal["far"].as_f64().unwrap();
    assert!((far - 0.95 / 0.7 * 20.0).abs() < 2.5, "far {far}");
    // The silhouettes alone keep the diagonal corners.
    let mut hull_views = views.clone();
    for view in &mut hull_views {
        let values = view["depth"]["values"].clone();
        view["rows"] = json!(values
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row
                .as_array()
                .unwrap()
                .iter()
                .map(|v| if v.is_null() { '.' } else { '#' })
                .collect::<String>())
            .collect::<Vec<_>>());
        view.as_object_mut().unwrap().remove("depth");
    }
    let (hull, _) = carve(json!({"views": hull_views, "palette": 1}));
    let (hull_wrong, hull_worst) = mismatch(&hull, size, &sphere);
    assert!(
        hull_worst > 1.5 && hull_wrong > wrong * 3,
        "{hull_wrong} {hull_worst}"
    );
}

#[test]
fn three_depth_maps_with_back_mirrors_make_a_sphere() {
    // Front, left, and top see no part of the octant toward back, right,
    // and bottom; `back` mirrors each map about the hull's middle.
    let size = [20, 20, 20];
    let views: Vec<Value> = [Side::Front, Side::Left, Side::Top]
        .into_iter()
        .map(|side| {
            let mut view = depth_view(side, size, &sphere);
            view["depth"]["back"] = json!(true);
            view
        })
        .collect();
    let (cells, reply) = carve(json!({"views": views, "palette": 1}));
    let rim = reply["views"][0]["depth"]["rim"].as_f64().unwrap();
    assert!((rim - 10.0).abs() < 0.6, "rim {rim}");
    let (wrong, worst) = mismatch(&cells, size, &sphere);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
}

#[test]
fn one_front_depth_map_and_its_mirror_make_a_sphere() {
    let size = [20, 20, 20];
    let mut front = depth_view(Side::Front, size, &sphere);
    front["depth"]["back"] = json!(true);
    // No other view measures the depth axis: box calibration, so the map's
    // range must be given: the grey is 0.95 - 0.7 * t / 20.
    front["depth"]["near"] = json!(-0.05 / 0.7 * 20.0);
    front["depth"]["far"] = json!(0.95 / 0.7 * 20.0);
    let (cells, reply) = carve(json!({"views": [front], "palette": 1, "size": [20, 20, 20]}));
    assert_eq!(reply["views"][0]["depth"]["calibrate"], "given");
    let (wrong, worst) = mismatch(&cells, size, &sphere);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
}

#[test]
fn a_dome_carved_from_a_top_depth_map_keeps_its_flat_base() {
    let size = [20, 20, 10];
    let silhouette = |side: Side| {
        let values = render(side, size, &|p| dome(p) < 0.0);
        let rows: Vec<String> = values
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| if v.is_null() { '.' } else { '#' })
                    .collect()
            })
            .collect();
        json!({"view": side.name(), "rows": rows, "legend": {"#": 3}, "cellsPerPixel": 1.0 / PPC})
    };
    let mut top = depth_view(Side::Top, size, &dome);
    top["rows"] = silhouette(Side::Top)["rows"].clone();
    top["legend"] = json!({"#": 5});
    let views = json!([silhouette(Side::Front), silhouette(Side::Left), top]);
    let (cells, reply) = carve(json!({"views": views, "slots": [3, 5]}));
    let (wrong, worst) = mismatch(&cells, size, &dome);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
    assert_eq!(reply["views"][2]["depth"]["calibrate"], "hull");
    assert!(reply["views"][2]["depth"]["carved"].as_u64().unwrap() > 100);
    // The top of the dome takes the top view's colour.
    assert_eq!(cells.get(&(10, 10, 8)), Some(&5));
    // Without the map the silhouettes leave the dome's corners.
    let mut plain = views.clone();
    plain[2].as_object_mut().unwrap().remove("depth");
    let (hull, _) = carve(json!({"views": plain, "slots": [3, 5]}));
    assert!(mismatch(&hull, size, &dome).1 > 1.5);
}

#[test]
fn box_calibration_spans_the_box_and_invert_swaps_near_and_far() {
    // A ramp: grey 1 at the left column, 0 at the right, over 4 x 1 cells.
    let ramp = json!([[1.0, 0.6667, 0.3333, 0.0]]);
    let params = |depth: Value| {
        json!({"views": [{"view": "front", "cellsPerPixel": 1.0, "depth": depth}],
               "palette": 2, "size": [4, 4, 1]})
    };
    let (cells, reply) = carve(params(json!({"values": ramp, "calibrate": "box"})));
    assert_eq!(reply["views"][0]["depth"]["calibrate"], "box");
    // Column x keeps y >= x: 4, 3, 2, 1 cells.
    let column =
        |cells: &HashMap<Cell, u8>, x| (0..4).filter(|y| cells.contains_key(&(x, *y, 0))).count();
    assert_eq!(
        (0..4).map(|x| column(&cells, x)).collect::<Vec<_>>(),
        [4, 3, 2, 1]
    );
    let (cells, _) = carve(params(
        json!({"values": ramp, "calibrate": "box", "invert": true}),
    ));
    assert_eq!(
        (0..4).map(|x| column(&cells, x)).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    // A tolerance keeps cells in front of the surface.
    let mut tolerant = params(json!({"values": ramp, "calibrate": "box"}));
    tolerant["depthTolerance"] = json!(1.0);
    let (cells, _) = carve(tolerant);
    assert_eq!(
        (0..4).map(|x| column(&cells, x)).collect::<Vec<_>>(),
        [4, 4, 3, 2]
    );
    // Hull calibration with no view across the depth axis falls back to box.
    let (_, reply) = carve(params(json!({"values": ramp})));
    assert_eq!(reply["views"][0]["depth"]["calibrate"], "box");
}

#[test]
fn bad_depth_arguments_are_refused() {
    let palette = [[128u8; 3]; 256];
    let run = |params: Value| {
        crate::remote::hull_writes(&HashMap::new(), &palette, &params, false)
            .err()
            .unwrap_or_default()
    };
    let view = |depth: Value| json!({"views": [{"view": "front", "depth": depth}], "palette": 1, "size": [2, 2, 2]});
    assert!(run(view(json!({"values": [[0.5]], "near": 1}))).contains("near and far together"));
    assert!(run(view(json!({"values": [[1.5]]}))).contains("0..1"));
    assert!(run(view(json!({"values": [[0.5]], "calibrate": "magic"}))).contains("hull or box"));
    assert!(run(view(json!({"path": "x.png"}))).contains("scripts cannot read files"));
    assert!(run(view(json!(true))).contains("depthPath"));
    assert!(run(view(json!({"values": [[null]]}))).contains("no pixel"));
    // A depth-only view with no palette has no colour.
    let mut no_colour = view(json!({"values": [[0.5]]}));
    no_colour.as_object_mut().unwrap().remove("palette");
    assert!(run(no_colour).contains("colour image"));
}

#[test]
fn a_depth_png_crop_of_a_sheet_carves_over_mcp() {
    // A colour sheet and a depth sheet, both 2 views side by side: front
    // at x 0..80, left at x 80..160, each 80 x 80 pixels (20 cells).
    let size = [20, 20, 20];
    let mut depth_rgba = Vec::new();
    let mut colour_rgba = Vec::new();
    let maps: Vec<Value> = [Side::Front, Side::Left]
        .into_iter()
        .map(|side| render(side, size, &|p| sphere(p) < 0.0))
        .collect();
    for y in 0..80 {
        for half in &maps {
            for x in 0..80 {
                match half[y][x].as_f64() {
                    Some(n) => {
                        let g = (n * 255.0).round() as u8;
                        depth_rgba.extend([g, g, g, 255]);
                        colour_rgba.extend([40, 160, 60, 255]);
                    }
                    None => {
                        // A monocular model's background is some grey.
                        depth_rgba.extend([90, 90, 90, 255]);
                        colour_rgba.extend([255, 0, 255, 255]);
                    }
                }
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("froxel-depth-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let colour = dir.join("views.png");
    let depth = dir.join("depth.png");
    crate::trace::write_rgba_png(&colour, 160, 80, &colour_rgba).unwrap();
    crate::trace::write_rgba_png(&depth, 160, 80, &depth_rgba).unwrap();
    let mut session = crate::state::Session::new();
    session.replace_empty();
    let reply = crate::remote::apply(
        &mut session,
        "voxel.carve_from_views",
        &json!({
            "path": colour.display().to_string(),
            "depthPath": depth.display().to_string(),
            "background": "#ff00ff",
            "cellsPerPixel": 0.25,
            "views": [
                {"view": "front", "crop": [0, 0, 80, 80], "depth": {"back": true}},
                {"view": "left", "crop": [80, 0, 80, 80], "depth": {"back": true}}
            ]
        }),
    )
    .unwrap();
    assert_eq!(reply["views"][0]["depth"]["calibrate"], "hull");
    let cells: HashMap<Cell, u8> = session.active().cells.clone();
    let (wrong, worst) = mismatch(&cells, size, &sphere);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn fit_to_views_follows_a_depth_map_off_the_hull() {
    let size = [20, 20, 20];
    let views: Vec<Value> = [Side::Front, Side::Left, Side::Top]
        .into_iter()
        .map(|side| depth_view(side, size, &sphere))
        .collect();
    // The blockout: the silhouettes' hull alone.
    let mut hull_views = views.clone();
    for view in &mut hull_views {
        let values = view["depth"]["values"].clone();
        view["depth"] = json!({"values": values, "calibrate": "box", "near": -1000, "far": -999});
    }
    let (hull, _) = carve(json!({"views": hull_views, "palette": 1}));
    let (hull_wrong, _) = mismatch(&hull, size, &sphere);
    let palette = [[128u8; 3]; 256];
    let fit_views: Vec<Value> = views
        .iter()
        .map(|view| {
            let mut view = view.clone();
            view.as_object_mut().unwrap().remove("cellsPerPixel");
            view
        })
        .collect();
    let (writes, reply) = crate::remote::fit_writes(
        &hull,
        &[],
        None,
        &palette,
        &json!({"views": fit_views, "recolor": false, "margin": 2, "depthWeight": 2}),
        false,
    )
    .unwrap();
    let mut cells = hull.clone();
    for (x, y, z, slot) in writes {
        if slot == 0 {
            cells.remove(&(x, y, z));
        } else {
            cells.insert((x, y, z), slot as u8);
        }
    }
    let (wrong, _) = mismatch(&cells, size, &sphere);
    let view = &reply["views"][0];
    assert_eq!(view["depth"]["calibrate"], "hull");
    let before = view["before"]["depthError"].as_f64().unwrap();
    let after = view["after"]["depthError"].as_f64().unwrap();
    assert!(
        after < before && after < 0.5,
        "depth error {before} -> {after}"
    );
    assert!(
        wrong * 2 < hull_wrong,
        "{hull_wrong} -> {wrong} wrong cells"
    );
}

/// Rhai runs the MCP code path: a depth view as values gives the same cells.
#[test]
fn a_script_carves_the_same_depth() {
    let ramp = json!([[1.0, 0.6667, 0.3333, 0.0], [1.0, 0.6667, 0.3333, null]]);
    let params = json!({"views": [{"view": "front", "cellsPerPixel": 1, "depth": {"values": ramp, "calibrate": "box"}}],
                        "palette": 2, "size": [4, 4, 2], "depthTolerance": 0.25});
    let (mcp, _) = carve(params);
    let source = r#"
        let depth = #{values: [[1.0, 0.6667, 0.3333, 0.0], [1.0, 0.6667, 0.3333, ()]], calibrate: "box"};
        let done = carve_from_views([#{view: "front", cellsPerPixel: 1, depth: depth}],
            #{palette: 2, size: [4, 4, 2], depthTolerance: 0.25});
        if done.views[0].depth.calibrate != "box" { throw "calibration"; }
    "#;
    let mut scripted = crate::state::Session::new();
    scripted.replace_empty();
    scripted.set_script(0, source).unwrap();
    scripted.regenerate_script(0).unwrap();
    assert_eq!(scripted.active().cells, mcp);
    assert_eq!(mcp.len(), 4 + 3 + 2 + 1 + 4 + 3 + 2);
}

#[test]
fn the_depth_head_example_is_round() {
    let run = |depth: i32| {
        let mut session = crate::state::Session::new();
        session.replace_empty();
        session
            .set_script(0, include_str!("fixtures/examples/depth_head.rhai"))
            .unwrap();
        session
            .set_script_params(
                0,
                &[(
                    "depth".into(),
                    crate::script::ParamValue::Int {
                        value: depth,
                        min: 0,
                        max: 1,
                    },
                )],
            )
            .unwrap();
        session.regenerate_script(0).unwrap();
        session.active().cells.clone()
    };
    // radius 8, height 16 * 80% = 12: an ellipsoid 16 x 16 x 12.
    let size = [16, 16, 12];
    let head = |p: [f64; 3]| {
        let q = ((p[0] - 8.0) / 8.0).powi(2)
            + ((p[1] - 8.0) / 8.0).powi(2)
            + ((p[2] - 6.0) / 6.0).powi(2);
        // Roughly the distance in cells.
        (q.sqrt() - 1.0) * 6.0
    };
    let round = run(1);
    let (wrong, worst) = mismatch(&round, size, &head);
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
    let boxy = run(0);
    let (hull_wrong, hull_worst) = mismatch(&boxy, size, &head);
    assert!(
        hull_wrong * 2 > wrong * 3 && hull_worst > worst * 3.0,
        "{hull_wrong} {hull_worst} vs {wrong} {worst}"
    );
    // The eyes come from the front view.
    assert!(round.values().any(|slot| *slot == 4));
}

/// A body and a head in front of it, as a turtle seen from the views.
fn body(p: [f64; 3]) -> f64 {
    ((p[0] - 12.0).powi(2) + (p[1] - 15.0).powi(2) + (p[2] - 8.0).powi(2)).sqrt() - 7.5
}

fn head(p: [f64; 3]) -> f64 {
    ((p[0] - 12.0).powi(2) + (p[1] - 5.0).powi(2) + (p[2] - 6.0).powi(2)).sqrt() - 3.5
}

fn creature(p: [f64; 3]) -> f64 {
    body(p).min(head(p))
}

const CREATURE: [u32; 3] = [24, 24, 16];

/// The first hit per pixel of `side`, and which part it hit (0 body, 1
/// head), as [`render`] casts it.
fn hits(side: Side) -> Vec<Vec<Option<(f64, usize)>>> {
    let values = render(side, CREATURE, &|p| creature(p) < 0.0);
    let [(u, su), (v, sv), (d, toward)] = side.axes();
    let depth = f64::from(CREATURE[d]);
    values
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(py, row)| {
            row.as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(px, value)| {
                    let t = (0.95 - value.as_f64()?) / 0.7 * depth;
                    let mut p = [0.0f64; 3];
                    let (a, b) = ((px as f64 + 0.5) / PPC, (py as f64 + 0.5) / PPC);
                    p[u] = if su > 0 {
                        a
                    } else {
                        f64::from(CREATURE[u]) - a
                    };
                    p[v] = if sv > 0 {
                        b
                    } else {
                        f64::from(CREATURE[v]) - b
                    };
                    p[d] = if toward > 0 { depth - t } else { t };
                    let part = usize::from(head(p) < body(p));
                    Some((t, part))
                })
                .collect()
        })
        .collect()
}

/// A map drawn like an image generator's: each part normalised on its own,
/// 1 at its nearest point and 0.3 at its farthest, as shading is.
fn per_part(side: Side) -> Value {
    let hits = hits(side);
    let mut range = [(f64::INFINITY, f64::NEG_INFINITY); 2];
    for (t, part) in hits.iter().flatten().flatten() {
        range[*part] = (range[*part].0.min(*t), range[*part].1.max(*t));
    }
    json!(hits
        .iter()
        .map(|row| row
            .iter()
            .map(|hit| hit.map(|(t, part)| {
                let (lo, hi) = range[part];
                0.3 + 0.7 * (hi - t) / (hi - lo)
            }))
            .collect::<Vec<_>>())
        .collect::<Vec<_>>())
}

/// A map drawn like a monocular model's: right in order, but flat inside
/// each part (its mean depth) and bent by a curve.
fn flat_parts(side: Side) -> Value {
    let hits = hits(side);
    let depth = f64::from(CREATURE[side.axes()[2].0]);
    let mut sum = [(0.0f64, 0usize); 2];
    for (t, part) in hits.iter().flatten().flatten() {
        sum[*part].0 += t;
        sum[*part].1 += 1;
    }
    json!(hits
        .iter()
        .map(|row| row
            .iter()
            .map(|hit| hit.map(|(_, part)| {
                let mean = sum[part].0 / sum[part].1 as f64;
                (1.0 - mean / depth).powi(2)
            }))
            .collect::<Vec<_>>())
        .collect::<Vec<_>>())
}

fn creature_carve(depth: impl Fn(Side) -> Value) -> (usize, f64, Value) {
    let views: Vec<Value> = [Side::Front, Side::Left, Side::Top]
        .into_iter()
        .map(|side| json!({"view": side.name(), "cellsPerPixel": 1.0 / PPC, "depth": depth(side)}))
        .collect();
    let (cells, reply) = carve(json!({"views": views, "palette": 1, "size": CREATURE}));
    // The octant no view sees is left out of the count: only the cells a
    // view's ray reaches before the middle of the box along it.
    let seen = |p: [f64; 3]| p[1] < 12.0 || p[0] > 12.0 || p[2] > 8.0;
    let mut wrong = 0;
    let mut worst = 0.0f64;
    for z in 0..CREATURE[2] as i32 {
        for y in 0..CREATURE[1] as i32 {
            for x in 0..CREATURE[0] as i32 {
                let c = [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5];
                if !seen(c) {
                    continue;
                }
                if (creature(c) < 0.0) != cells.contains_key(&(x, y, z)) {
                    wrong += 1;
                    worst = worst.max(creature(c).abs());
                }
            }
        }
    }
    eprintln!("creature: {wrong} cells, worst |sdf| {worst:.2}");
    (wrong, worst, reply)
}

#[test]
fn regions_calibrate_a_map_normalised_per_part() {
    let (_, global, _) = creature_carve(|side| json!({"values": per_part(side)}));
    let (wrong, worst, reply) =
        creature_carve(|side| json!({"values": per_part(side), "regions": true}));
    assert!(worst <= 1.0, "{wrong} wrong cells, worst {worst}");
    assert!(global > 2.0, "the global fit was already right: {global}");
    // The front view's head and body are two regions.
    assert!(reply["views"][0]["depth"]["regions"].as_u64().unwrap() >= 2);
}

#[test]
fn auto_gamma_finds_a_curved_map() {
    for (power, expected) in [(2, 0.5), (4, 0.25)] {
        let curved = |side: Side| {
            let values = render(side, CREATURE, &|p| creature(p) < 0.0);
            json!(values
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().map(|g| g.powi(power)))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>())
        };
        let (_, linear, _) = creature_carve(|side| json!({"values": curved(side)}));
        let (wrong, worst, reply) =
            creature_carve(|side| json!({"values": curved(side), "gamma": "auto"}));
        // The contour picks the curve that undoes grey = linear^power, to
        // within a factor of two.
        for view in reply["views"].as_array().unwrap() {
            let gamma = view["depth"]["gamma"].as_f64().unwrap();
            assert!(
                gamma >= expected / 2.05 && gamma <= expected * 2.05,
                "power {power}: gamma {gamma}"
            );
        }
        assert!(
            worst <= 1.0 && worst <= linear,
            "{wrong} wrong, worst {worst} vs {linear}"
        );
    }
}

#[test]
fn a_detail_map_gives_the_curvature_and_the_base_the_order() {
    let (_, flat, _) = creature_carve(|side| json!({"values": flat_parts(side), "gamma": "auto"}));
    let (wrong, worst, reply) = creature_carve(
        |side| json!({"values": flat_parts(side), "gamma": "auto", "detail": {"values": per_part(side)}}),
    );
    assert!(worst <= 1.25, "{wrong} wrong cells, worst {worst}");
    assert!(flat > worst, "flat {flat} vs {worst}");
    assert!(reply["views"][0]["depth"]["regions"].as_u64().unwrap() >= 2);
}

#[test]
fn the_carve_panel_passes_the_depth_options() {
    let mut panel = crate::state::Session::new().silhouette.carve;
    panel.views[0].path = "views.png".into();
    panel.views[0].depth = "depth.png".into();
    panel.views[1].path = "views.png".into();
    panel.depth_regions = true;
    panel.depth_auto_curve = true;
    panel.depth_edge = 0.04;
    panel.depth_detail = "detail.png".into();
    panel.depth_tolerance = 0.5;
    let args = panel.args(1).unwrap();
    let depth = &args["views"][0]["depth"];
    assert_eq!(depth["path"], "depth.png");
    assert_eq!(depth["regions"], true);
    assert_eq!(depth["gamma"], "auto");
    assert_eq!(depth["detail"]["path"], "detail.png");
    assert!(args["views"][1].get("depth").is_none());
    assert_eq!(args["depthTolerance"], 0.5);
}

#[test]
fn coloured_pixels_hold_no_depth() {
    // Grey, magenta, and a pink fringe.
    let rgba = [128, 128, 128, 255, 255, 0, 255, 255, 230, 120, 230, 255];
    let map = crate::tools::depth_from_rgba(3, 1, &rgba, None, None).unwrap();
    assert!(map.values[0].is_some());
    assert_eq!(&map.values[1..], &[None, None]);
}
