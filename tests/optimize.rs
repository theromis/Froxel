use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

const ORANGE: [u8; 3] = [210, 120, 40];
const DARK: [u8; 3] = [30, 20, 15];

/// A `w` x `h` mask from `ink(col, row)`.
fn mask(w: u32, h: u32, ink: impl Fn(u32, u32) -> Option<[u8; 3]>) -> Mask {
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            pixels.push(ink(x, y).map(Ink::Rgb));
        }
    }
    Mask {
        width: w,
        height: h,
        pixels,
    }
}

fn view(side: Side, mask: Mask) -> OptimizeView {
    OptimizeView {
        side,
        mask,
        depth: None,
        source: None,
        color: true,
        weight: 1.0,
    }
}

/// Every cell of the inclusive box, in `slot`.
fn cube(lo: [i32; 3], hi: [i32; 3], slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                cells.insert((x, y, z), slot);
            }
        }
    }
    cells
}

fn shades() -> Shades {
    Session::new().shades()
}

/// `cells` with `writes` applied.
fn applied(cells: &HashMap<Cell, u8>, writes: &[(Cell, u8)]) -> HashMap<Cell, u8> {
    let mut out = cells.clone();
    for (cell, slot) in writes {
        if *slot == 0 {
            out.remove(cell);
        } else {
            out.insert(*cell, *slot);
        }
    }
    out
}

fn full(w: u32, h: u32) -> Mask {
    mask(w, h, |_, _| Some(ORANGE))
}

#[test]
fn every_view_agrees_after_recolouring_and_the_eye_keeps_its_cell() {
    let cells = cube([0, 0, 0], [5, 5, 5], 1);
    let front = mask(6, 6, |x, y| {
        Some(if (x, y) == (2, 2) { DARK } else { ORANGE })
    });
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, front),
            view(Side::Left, full(6, 6)),
            view(Side::Top, full(6, 6)),
            view(Side::Back, full(6, 6)),
        ],
        ..OptimizeSpec::default()
    };
    let mut shades = shades();
    let done = optimize_to_views(&cells, &spec, &mut shades).unwrap();
    for v in &done.views {
        assert!(v.before.agreement() < 0.1, "{:?} {:?}", v.side, v.before);
        assert_eq!(v.after.agreement(), 1.0, "{:?} {:?}", v.side, v.after);
    }
    assert_eq!(done.added + done.removed, 0);
    let after = applied(&cells, &done.writes);
    let dark = done
        .colours
        .iter()
        .find(|c| c.rgb == Some(DARK))
        .expect("the eye colour");
    assert!(dark.accent);
    let eyes: Vec<&Cell> = after
        .iter()
        .filter(|(_, s)| **s == dark.slot)
        .map(|(c, _)| c)
        .collect();
    // Front image row 2 is z 3, column 2 is x 2; the front face is y 0.
    assert_eq!(eyes, vec![&(2, 0, 3)]);
    // The right view is the left one mirrored.
    assert!(done
        .views
        .iter()
        .any(|v| v.side == Side::Right && v.mirrored));
}

#[test]
fn excess_columns_go_within_the_band() {
    let cells = cube([0, 0, 0], [5, 5, 5], 1);
    // The front and top views leave the last column (x 5) empty.
    let notched = |_y| mask(6, 6, |x, _| (x < 5).then_some(ORANGE));
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, notched(0)),
            view(Side::Top, notched(0)),
            view(Side::Left, full(6, 6)),
        ],
        fit_ink: false,
        ..OptimizeSpec::default()
    };
    let done = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert_eq!(done.removed, 36, "{done:?}");
    for v in &done.views {
        assert_eq!(v.after.iou(), 1.0, "{:?}", v.side);
    }
    let after = applied(&cells, &done.writes);
    assert!(after.keys().all(|c| c.0 < 5));
    // With no band nothing moves.
    let still = OptimizeSpec {
        band: 0,
        ..spec.clone()
    };
    let done = optimize_to_views(&cells, &still, &mut shades()).unwrap();
    assert_eq!(done.removed + done.added, 0);
    assert!(done.blocked.band > 0);
}

#[test]
fn a_missing_ray_gains_a_cell_that_touches_the_body() {
    let mut cells = cube([0, 0, 0], [5, 5, 5], 1);
    for y in 0..6 {
        cells.remove(&(5, y, 5));
    }
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, full(6, 6)),
            view(Side::Left, full(6, 6)),
            view(Side::Top, full(6, 6)),
        ],
        fit_ink: false,
        ..OptimizeSpec::default()
    };
    let done = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert_eq!(done.added, 1, "{done:?}");
    assert_eq!(done.views[0].before.iou() < 1.0, true);
    assert_eq!(done.views[0].after.iou(), 1.0);
    let after = applied(&cells, &done.writes);
    assert_eq!(after.len(), cells.len() + 1);
}

#[test]
fn a_removal_never_splits_the_object() {
    // Two 3-cell cubes joined by one bridge cell the front and top views
    // leave out.
    let mut cells = cube([0, 0, 0], [2, 2, 2], 1);
    cells.extend(cube([4, 0, 0], [6, 2, 2], 1));
    cells.insert((3, 1, 1), 1);
    let gap = || mask(7, 3, |x, _| (x != 3).then_some(ORANGE));
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, gap()),
            view(Side::Top, gap()),
            view(Side::Left, full(3, 3)),
        ],
        fit_ink: false,
        recolor: false,
        ..OptimizeSpec::default()
    };
    let done = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert!(!done.writes.iter().any(|(c, _)| *c == (3, 1, 1)));
    assert!(done.blocked.connected > 0, "{:?}", done.blocked);
    let loose = OptimizeSpec {
        connected: false,
        ..spec
    };
    let done = optimize_to_views(&cells, &loose, &mut shades()).unwrap();
    assert!(done.writes.contains(&((3, 1, 1), 0)));
}

#[test]
fn min_feature_keeps_a_leg_two_cells_thick() {
    // A body on top (z 2..5) and a 2 x 2 x 2 leg under one corner, which
    // the front view draws one cell wide.
    let mut cells = cube([0, 0, 2], [5, 5, 5], 1);
    cells.extend(cube([0, 0, 0], [1, 1, 1], 1));
    let front = mask(6, 6, |x, y| (y < 4 || x == 0).then_some(ORANGE));
    let left = mask(6, 6, |x, y| (y < 4 || x < 2).then_some(ORANGE));
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, front),
            view(Side::Left, left),
            view(Side::Top, full(6, 6)),
        ],
        fit_ink: false,
        recolor: false,
        mirror: false,
        min_feature: 2,
        ..OptimizeSpec::default()
    };
    let done = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert_eq!(done.removed, 0, "{done:?}");
    assert!(done.blocked.thin > 0);
    let thin = OptimizeSpec {
        min_feature: 1,
        ..spec
    };
    let done = optimize_to_views(&cells, &thin, &mut shades()).unwrap();
    assert_eq!(done.removed, 4);
    assert_eq!(done.views[0].after.iou(), 1.0);
}

#[test]
fn the_same_views_give_the_same_cells() {
    let mut cells = cube([0, 0, 0], [7, 9, 5], 1);
    for (x, y, z) in [(0, 0, 5), (7, 9, 0), (3, 0, 5), (7, 4, 5)] {
        cells.remove(&(x, y, z));
    }
    let blob = |w: u32, h: u32| {
        mask(w, h, move |x, y| {
            let (fx, fy) = (
                x as f32 + 0.5 - w as f32 / 2.0,
                y as f32 + 0.5 - h as f32 / 2.0,
            );
            let r = (fx / (w as f32 / 2.0)).powi(2) + (fy / (h as f32 / 2.0)).powi(2);
            (r <= 1.0).then_some(if y < h / 3 { DARK } else { ORANGE })
        })
    };
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Front, blob(8, 6)),
            view(Side::Left, blob(10, 6)),
            view(Side::Top, blob(8, 10)),
        ],
        accents: None,
        ..OptimizeSpec::default()
    };
    let a = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    let b = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert_eq!(a.writes, b.writes);
    assert!(a.removed > 0);
    let mean = |after: bool| {
        a.views
            .iter()
            .map(|v| if after { v.after } else { v.before }.agreement())
            .sum::<f32>()
    };
    assert!(mean(true) > mean(false));
}

#[test]
fn keep_slots_are_never_touched() {
    let mut cells = cube([0, 0, 0], [5, 5, 5], 1);
    cells.insert((2, 0, 3), 9);
    let spec = OptimizeSpec {
        views: vec![view(Side::Front, full(6, 6))],
        keep: vec![9],
        ..OptimizeSpec::default()
    };
    let done = optimize_to_views(&cells, &spec, &mut shades()).unwrap();
    assert!(!done.writes.iter().any(|(c, _)| *c == (2, 0, 3)));
}

#[test]
fn the_tool_is_one_undo_step_and_rhai_takes_the_same_path() {
    let mut session = Session::new();
    apply(&mut session, "object.clear", &json!({})).unwrap();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 5, "z1": 5, "palette": 1}),
    )
    .unwrap();
    let before = session.active().cells.clone();
    let rows: Vec<String> = (0..6)
        .map(|y| {
            (0..6)
                .map(|x| if (x, y) == (2, 2) { 'k' } else { 'o' })
                .collect()
        })
        .collect();
    let legend = json!({"o": "#d27828", "k": "#1e140f"});
    let views = json!([
        {"view": "front", "rows": rows, "legend": legend},
        {"view": "left", "rows": rows, "legend": {"o": "#d27828", "k": "#d27828"}},
        {"view": "top", "rows": rows, "legend": {"o": "#d27828", "k": "#d27828"}}
    ]);
    let reply = apply(
        &mut session,
        "voxel.optimize_to_views",
        &json!({"views": views}),
    )
    .unwrap();
    assert_eq!(reply["agreement"]["after"], json!(1.0), "{reply:#}");
    assert!(reply["changed"].as_u64().unwrap() > 0);
    assert_eq!(reply["newSlots"].as_array().unwrap().len(), 2);
    let mcp = session.active().cells.clone();
    session.undo();
    assert_eq!(session.active().cells, before);
    // A script reaches the same cells.
    let script = format!(
        "fill_box(0, 0, 0, 5, 5, 5, 1);\nlet r = optimize_to_views({});\nif r.agreement.after < 1.0 {{ throw \"agreement\"; }}",
        views
            .to_string()
            .replace("{\"", "#{\"")
    );
    let mut other = Session::new();
    other.replace_empty();
    other.set_script(0, &script).unwrap();
    other.regenerate_script(0).unwrap();
    let scripted = &other.objects[0].cells;
    let colours = |session: &Session, cells: &HashMap<Cell, u8>| {
        let mut out: Vec<(Cell, [u8; 4])> = cells
            .iter()
            .map(|(c, s)| (*c, session.palette[*s as usize]))
            .collect();
        out.sort();
        out
    };
    session.redo();
    assert_eq!(session.active().cells, mcp);
    assert_eq!(colours(&other, scripted), colours(&session, &mcp));
}

#[test]
fn the_optimized_toad_example_agrees_better_and_keeps_its_pupils() {
    let script = include_str!("fixtures/examples/optimized_toad.rhai");
    let run = |optimize: i32| {
        let mut session = Session::new();
        session.replace_empty();
        session.set_script(0, script).unwrap();
        session
            .set_script_params(
                0,
                &[(
                    "optimize".into(),
                    crate::script::ParamValue::Int {
                        value: optimize,
                        min: 0,
                        max: 1,
                    },
                )],
            )
            .unwrap();
        session.regenerate_script(0).unwrap();
        session
    };
    let optimized = run(1);
    let carved = run(0);
    let dark = |s: &Session| {
        s.objects[0]
            .cells
            .values()
            .filter(|slot| {
                let c = s.palette[**slot as usize];
                u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]) < 150
            })
            .count()
    };
    assert!(dark(&optimized) >= 2, "{} pupil cells", dark(&optimized));
    assert!(carved.objects[0].cells.len() >= optimized.objects[0].cells.len());
}

/// A stand-in for CLIP: a patch's similarity is the share of its pixels
/// whose colours are equal.
struct SamePixels;

impl super::PatchScorer for SamePixels {
    fn similarity(
        &mut self,
        pairs: &[(super::ViewImage, super::ViewImage)],
    ) -> Result<Vec<Vec<f32>>, String> {
        Ok(pairs
            .iter()
            .map(|(a, b)| {
                let side = a.width.max(a.height) as f32;
                let (ox, oy) = (
                    (side - a.width as f32) / 2.0,
                    (side - a.height as f32) / 2.0,
                );
                let g = super::PATCH_GRID;
                let mut same = vec![0.0f32; g * g];
                let mut all = vec![0.0f32; g * g];
                for p in 0..a.width * a.height {
                    let x = (p % a.width) as f32 + 0.5 + ox;
                    let y = (p / a.width) as f32 + 0.5 + oy;
                    let k = ((y / side * g as f32) as usize).min(g - 1) * g
                        + ((x / side * g as f32) as usize).min(g - 1);
                    all[k] += 1.0;
                    if a.pixels[p] == b.pixels[p] {
                        same[k] += 1.0;
                    }
                }
                same.iter()
                    .zip(&all)
                    .map(|(s, n)| if *n > 0.0 { s / n } else { 1.0 })
                    .collect()
            })
            .collect())
    }

    fn name(&self) -> String {
        "same-pixels".into()
    }
}

#[test]
fn the_semantic_term_only_keeps_what_reads_more_alike() {
    // A box whose front and top views paint the shared edge differently.
    let cells = cube([0, 0, 0], [5, 5, 5], 1);
    let front = mask(6, 6, |x, y| {
        Some(if y == 0 && x > 1 && x < 4 {
            DARK
        } else {
            ORANGE
        })
    });
    let spec = OptimizeSpec {
        views: vec![
            view(Side::Top, full(6, 6)),
            view(Side::Front, front),
            view(Side::Left, full(6, 6)),
        ],
        fit_ink: false,
        accents: None,
        color_carve: false,
        shape: false,
        semantic: Some(Semantic {
            weight: 1.0,
            rounds: 2,
            patches: 8,
        }),
        ..OptimizeSpec::default()
    };
    let mut scorer = SamePixels;
    let done = super::optimize_with(&cells, &spec, &mut shades(), Some(&mut scorer)).unwrap();
    let sem = done.semantic.expect("a semantic report");
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(mean(&sem.after) >= mean(&sem.before), "{sem:?}");
    assert_eq!(sem.scorer, "same-pixels");
    // Without a scorer the spec is refused.
    assert!(optimize_to_views(&cells, &spec, &mut shades()).is_err());
    let again = super::optimize_with(&cells, &spec, &mut shades(), Some(&mut SamePixels)).unwrap();
    assert_eq!(again.writes, done.writes);
}

#[cfg(not(feature = "clip"))]
#[test]
fn semantic_over_mcp_needs_the_clip_feature() {
    let mut session = Session::new();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 3, "y1": 3, "z1": 3, "palette": 1}),
    )
    .unwrap();
    let err = apply(
        &mut session,
        "voxel.optimize_to_views",
        &json!({"views": [{"view": "front", "rows": ["oo", "oo"], "legend": {"o": "#d27828"}}],
                "semantic": true}),
    )
    .unwrap_err();
    assert!(err.contains("clip"), "{err}");
}

/// The real CLIP model, when `FROXEL_CLIP_MODEL` names its weights (the
/// test passes without them).
#[cfg(feature = "clip")]
#[test]
fn clip_scores_a_view_against_itself_as_alike() {
    let Some(path) = std::env::var_os("FROXEL_CLIP_MODEL") else {
        return;
    };
    let mut clip = crate::tools::clip::Clip::new(std::path::Path::new(&path)).unwrap();
    let image = |colour: [u8; 3]| super::ViewImage {
        width: 8,
        height: 6,
        pixels: (0..48)
            .map(|p| ((p % 8) > 1 && (p % 8) < 6).then_some(colour))
            .collect(),
    };
    let sims = super::PatchScorer::similarity(
        &mut clip,
        &[(image(ORANGE), image(ORANGE)), (image(ORANGE), image(DARK))],
    )
    .unwrap();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(mean(&sims[0]) > 0.99);
    assert!(mean(&sims[1]) < mean(&sims[0]));
}
