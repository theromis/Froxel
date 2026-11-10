use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

const ORANGE: [u8; 3] = [210, 120, 40];
const DARK: [u8; 3] = [30, 20, 15];
const CREAM: [u8; 3] = [240, 220, 170];

/// A `w` x `h` mask: `ink(x, y)` gives each pixel's colour or none.
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

fn palette() -> [[u8; 3]; 256] {
    [[128, 128, 128]; 256]
}

/// Cells of the colour nearest `rgb` in a pixelated view.
fn cells_of(p: &Pixelated, view: usize, rgb: [u8; 3]) -> usize {
    let lab = super::colors::oklab(rgb);
    let centres: Vec<_> = p
        .colours
        .iter()
        .map(|c| super::colors::oklab(c.unwrap_or([0, 0, 0])))
        .collect();
    let want = super::colors::nearest(lab, &centres);
    p.views[view]
        .cells
        .iter()
        .filter(|c| **c == Some(want))
        .count()
}

#[test]
fn an_axis_left_out_follows_the_views_proportions() {
    // A front 40 x 20 pixels and a side 80 x 20: x 10 cells gives z 5
    // from the front, then y 20 from the side.
    let (size, grids) = view_grids(
        &[Side::Front, Side::Left],
        &[[40, 20], [80, 20]],
        [Some(10), None, None],
    )
    .unwrap();
    assert_eq!(size, [10, 20, 5]);
    assert_eq!(grids, vec![[10, 5], [20, 5]]);
    assert!(view_grids(&[Side::Front], &[[4, 4]], [None, None, None]).is_err());
}

#[test]
fn a_small_eye_keeps_a_cell_that_the_vote_would_lose() {
    // An orange square 40 pixels a side at 10 cells (4 pixels a cell) with
    // a dark pupil 3 x 3 pixels straddling four cells: no cell holds a
    // majority of it.
    let view = mask(40, 40, |x, y| {
        Some(if (19..22).contains(&x) && (19..22).contains(&y) {
            DARK
        } else {
            ORANGE
        })
    });
    let views = vec![(Side::Front, view)];
    let grids = vec![[10, 10]];
    let spec = PixelateSpec {
        despeckle: 0,
        ..PixelateSpec::default()
    };
    let kept = pixelate(&views, &grids, &spec, &palette()).unwrap();
    assert_eq!(cells_of(&kept, 0, DARK), 1, "{:?}", kept.views[0].rows());
    assert_eq!(kept.views[0].features.len(), 1);
    assert_eq!(kept.views[0].features[0].added, 1);
    let lost = pixelate(
        &views,
        &grids,
        &PixelateSpec {
            features: None,
            ..spec
        },
        &palette(),
    )
    .unwrap();
    assert_eq!(cells_of(&lost, 0, DARK), 0);
}

#[test]
fn a_pupil_keeps_its_place_inside_the_eye() {
    // A cream eye 12 x 12 pixels with a dark pupil 3 x 3 at its middle, on
    // an orange head, at 4 pixels a cell: both keep cells, the pupil inside.
    let view = mask(40, 40, |x, y| {
        let eye = (14..26).contains(&x) && (14..26).contains(&y);
        let pupil = (19..22).contains(&x) && (19..22).contains(&y);
        Some(if pupil {
            DARK
        } else if eye {
            CREAM
        } else {
            ORANGE
        })
    });
    let p = pixelate(
        &[(Side::Front, view)],
        &[[10, 10]],
        &PixelateSpec::default(),
        &palette(),
    )
    .unwrap();
    assert!(cells_of(&p, 0, DARK) >= 1);
    assert!(cells_of(&p, 0, CREAM) >= 4);
    let rows = p.views[0].rows();
    let dark = p
        .colours
        .iter()
        .position(|c| *c == Some(DARK))
        .map(|i| PIXEL_CHARS.chars().nth(i).unwrap());
    if let Some(dark) = dark {
        let (row, line) = rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.contains(dark))
            .unwrap();
        let col = line.find(dark).unwrap();
        assert!((3..=6).contains(&row) && (3..=6).contains(&col), "{rows:?}");
    }
}

#[test]
fn a_leg_thinner_than_half_a_cell_survives_with_thin() {
    // A body and a vertical leg 2 pixels wide (half a cell at 4 pixels a
    // cell) on a cell boundary: each cell it crosses holds a quarter.
    let view = mask(40, 40, |x, y| {
        if y < 20 || (11..13).contains(&x) {
            Some(ORANGE)
        } else {
            None
        }
    });
    let views = vec![(Side::Front, view)];
    let spec = PixelateSpec::default();
    let with = pixelate(&views, &[[10, 10]], &spec, &palette()).unwrap();
    let leg = |p: &Pixelated| {
        let rows = p.views[0].rows();
        rows[5..]
            .iter()
            .filter(|r| r.chars().any(|c| c != '.'))
            .count()
    };
    assert_eq!(leg(&with), 5, "{:?}", with.views[0].rows());
    assert!(with.views[0].thin >= 5);
    let without = pixelate(
        &views,
        &[[10, 10]],
        &PixelateSpec { thin: None, ..spec },
        &palette(),
    )
    .unwrap();
    assert_eq!(leg(&without), 0);
}

#[test]
fn colours_are_never_mixed_and_the_palette_is_shared() {
    // Two views with stripes of two colours at a scale where each cell
    // straddles both: every cell is one of the two, never a blend.
    let stripes = mask(30, 30, |x, _| {
        Some(if (x / 5) % 2 == 0 { ORANGE } else { CREAM })
    });
    let p = pixelate(
        &[(Side::Front, stripes.clone()), (Side::Left, stripes)],
        &[[4, 4], [4, 4]],
        &PixelateSpec {
            colors: 2,
            features: None,
            ..PixelateSpec::default()
        },
        &palette(),
    )
    .unwrap();
    assert_eq!(p.colours.len(), 2);
    for c in p.colours.iter().flatten() {
        let near = |a: [u8; 3]| (0..3).all(|i| (i32::from(c[i]) - i32::from(a[i])).abs() <= 3);
        assert!(near(ORANGE) || near(CREAM), "{c:?}");
    }
}

#[test]
fn the_tool_replies_with_rows_carve_takes() {
    let mut session = Session::new();
    apply(&mut session, "object.clear", &json!({})).ok();
    let before = session.voxel_count();
    let rows: Vec<String> = (0..12)
        .map(|y| {
            (0..12)
                .map(|x| {
                    if (4..6).contains(&x) && (4..6).contains(&y) {
                        'k'
                    } else {
                        'o'
                    }
                })
                .collect()
        })
        .collect();
    let legend = json!({"o": "#d27828", "k": "#1e140f"});
    let views = json!([
        {"view": "front", "rows": rows, "legend": legend},
        {"view": "left", "rows": rows, "legend": legend},
        {"view": "top", "rows": rows, "legend": legend}
    ]);
    let reply = apply(
        &mut session,
        "image.pixelate_views",
        &json!({"views": views, "size": [6, null, null]}),
    )
    .unwrap();
    assert_eq!(reply["size"], json!([6, 6, 6]));
    let out = reply["views"].as_array().unwrap();
    assert_eq!(out[0]["rows"].as_array().unwrap().len(), 6);
    assert_eq!(session.voxel_count(), before);
    // The rows carve straight into a 6 x 6 x 6 cube with the eye kept.
    let carved = apply(
        &mut session,
        "voxel.carve_from_views",
        &json!({"views": reply["views"], "size": reply["size"]}),
    )
    .unwrap();
    assert_eq!(carved["cells"], json!(216));
    // downscale on the carve does the same in one call.
    let mut other = Session::new();
    let down = apply(
        &mut other,
        "voxel.carve_from_views",
        &json!({"views": views, "size": [6, null, null], "downscale": true}),
    )
    .unwrap();
    assert_eq!(down["cells"], json!(216));
    assert_eq!(down["downscale"]["size"], json!([6, 6, 6]));
}

#[test]
fn paint_from_views_downscales_at_the_objects_box() {
    let mut session = Session::new();
    apply(&mut session, "object.clear", &json!({})).unwrap();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 5, "z1": 5, "palette": 1}),
    )
    .unwrap();
    let rows: Vec<String> = (0..24)
        .map(|y| {
            (0..24)
                .map(|x| {
                    if (10..13).contains(&x) && (10..13).contains(&y) {
                        'k'
                    } else {
                        'o'
                    }
                })
                .collect()
        })
        .collect();
    let legend = json!({"o": "#d27828", "k": "#1e140f"});
    let reply = apply(
        &mut session,
        "voxel.paint_from_views",
        &json!({
            "views": [{"view": "front", "rows": rows, "legend": legend}],
            "newSlots": 4,
            "downscale": true
        }),
    )
    .unwrap();
    assert_eq!(reply["downscale"]["views"][0]["grid"], json!([6, 6]));
    assert_eq!(
        reply["downscale"]["views"][0]["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // The pupil, a quarter of a cell's area straddling four, lands on the
    // front face as one dark cell.
    let dark = reply["newSlots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["rgba"][0].as_u64().unwrap() < 80)
        .map(|s| s["slot"].as_u64().unwrap() as u8)
        .unwrap_or_else(|| panic!("a dark slot: {reply:#}"));
    let front = session
        .active()
        .cells
        .iter()
        .filter(|(c, s)| c.1 == 0 && **s == dark)
        .count();
    assert_eq!(front, 1);
}

#[test]
fn the_pixel_toad_example_keeps_its_pupils() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("fixtures/examples/pixel_toad.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let pupils = session.objects[0]
        .cells
        .values()
        .filter(|s| **s == 4)
        .count();
    assert!(pupils >= 2, "{pupils} pupil cells");
}
