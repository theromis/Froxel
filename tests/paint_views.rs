use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

const RED: [u8; 3] = [220, 30, 30];
const GREEN: [u8; 3] = [30, 200, 40];
const BLUE: [u8; 3] = [30, 40, 220];

/// A solid box `n` cells a side, all slot 1.
fn cube(n: i32) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                cells.insert((x, y, z), 1);
            }
        }
    }
    cells
}

/// A mask of `w` x `h` pixels of one colour.
fn plain(w: u32, h: u32, rgb: [u8; 3]) -> Mask {
    Mask {
        width: w,
        height: h,
        pixels: vec![Some(Ink::Rgb(rgb)); (w * h) as usize],
    }
}

fn palette() -> [[u8; 3]; 256] {
    let mut p = [[128u8; 3]; 256];
    p[1] = [128, 128, 128];
    p[2] = RED;
    p[3] = GREEN;
    p[4] = BLUE;
    p
}

fn shades(p: &[[u8; 3]; 256], used: &[u8]) -> Shades {
    let mut taken = [false; 256];
    taken[0] = true;
    for s in used {
        taken[*s as usize] = true;
    }
    Shades {
        colors: std::array::from_fn(|i| [p[i][0], p[i][1], p[i][2], 255]),
        materials: [Default::default(); 256],
        used: taken,
        created: Vec::new(),
    }
}

fn spec(views: Vec<SheetImage>) -> PaintViewsSpec {
    PaintViewsSpec {
        views,
        blend: Blend::Nearest { seam: 0.0 },
        mirror: true,
        fit: Fit::Ink,
        reach: 3.0,
        inset: 0,
        region: None,
        only: None,
        quantize: Quantize::Slots(vec![1, 2, 3, 4]),
        guard: None,
    }
}

fn slot_at(done: &ViewsPainted, cell: Cell) -> Option<u8> {
    done.cells.iter().find(|(c, _)| *c == cell).map(|(_, s)| *s)
}

#[test]
fn each_face_takes_the_view_that_faces_it() {
    let cells = cube(8);
    let p = palette();
    let views = vec![
        SheetImage {
            side: Side::Front,
            mask: plain(16, 16, RED),
        },
        SheetImage {
            side: Side::Left,
            mask: plain(16, 16, GREEN),
        },
        SheetImage {
            side: Side::Top,
            mask: plain(16, 16, BLUE),
        },
    ];
    let done = paint_from_views(&cells, &spec(views), &p, &mut shades(&p, &[1])).unwrap();
    // The middle of the front face (y = 0), of the left face (+X, x = 7),
    // and of the top (z = 7).
    assert_eq!(slot_at(&done, (4, 0, 3)), Some(2));
    assert_eq!(slot_at(&done, (7, 4, 3)), Some(3));
    assert_eq!(slot_at(&done, (4, 4, 7)), Some(4));
    // mirror made a right view from the left: the -X face is green too.
    assert_eq!(slot_at(&done, (0, 4, 3)), Some(3));
    assert!(done
        .views
        .iter()
        .any(|v| v.side == Side::Right && v.mirrored));
    // No back or bottom view: those faces' middles are unseen and kept.
    assert_eq!(slot_at(&done, (4, 7, 3)), None);
    assert_eq!(slot_at(&done, (4, 4, 0)), None);
    assert!(done.unseen > 0);
    // Nothing inside is painted.
    assert_eq!(slot_at(&done, (4, 4, 4)), None);
}

#[test]
fn a_cell_hidden_behind_another_part_is_not_painted_from_that_view() {
    // A slab with a post in front of it: the slab cells behind the post
    // are not the first along y, so the front view does not paint them.
    let mut cells = HashMap::new();
    for z in 0..6 {
        for x in 0..6 {
            cells.insert((x, 3, z), 1);
        }
    }
    for y in 0..3 {
        cells.insert((2, y, 2), 1);
    }
    let p = palette();
    let views = vec![SheetImage {
        side: Side::Front,
        mask: plain(6, 6, RED),
    }];
    let done = paint_from_views(&cells, &spec(views), &p, &mut shades(&p, &[1])).unwrap();
    assert_eq!(slot_at(&done, (2, 0, 2)), Some(2));
    assert_eq!(slot_at(&done, (2, 3, 2)), None);
    assert_eq!(slot_at(&done, (3, 3, 2)), Some(2));
}

#[test]
fn image_halves_land_on_the_matching_halves() {
    // A front view red on its left half, blue on its right: image right is
    // +X, so low x is red.
    let cells = cube(8);
    let p = palette();
    let mut mask = plain(20, 10, RED);
    for y in 0..10 {
        for x in 10..20 {
            mask.pixels[(y * 20 + x) as usize] = Some(Ink::Rgb(BLUE));
        }
    }
    let views = vec![SheetImage {
        side: Side::Front,
        mask,
    }];
    let done = paint_from_views(&cells, &spec(views), &p, &mut shades(&p, &[1])).unwrap();
    assert_eq!(slot_at(&done, (1, 0, 4)), Some(2));
    assert_eq!(slot_at(&done, (6, 0, 4)), Some(4));
}

#[test]
fn new_slots_cluster_the_colours_into_free_slots() {
    let cells = cube(6);
    let p = palette();
    let views = vec![
        SheetImage {
            side: Side::Front,
            mask: plain(8, 8, [250, 140, 20]),
        },
        SheetImage {
            side: Side::Top,
            mask: plain(8, 8, [20, 60, 140]),
        },
    ];
    let mut s = spec(views);
    s.quantize = Quantize::New(2);
    let mut sh = shades(&p, &[1, 2, 3, 4]);
    let done = paint_from_views(&cells, &s, &p, &mut sh).unwrap();
    assert_eq!(done.new_slots.len(), 2);
    assert!(done.new_slots.iter().all(|s| *s > 4));
    let front = slot_at(&done, (3, 0, 3)).unwrap();
    let top = slot_at(&done, (3, 3, 5)).unwrap();
    assert_ne!(front, top);
    let rgba = sh.colors[front as usize];
    assert!((i32::from(rgba[0]) - 250).abs() <= 2 && (i32::from(rgba[2]) - 20).abs() <= 2);
}

#[test]
fn mcp_paints_a_sheet_of_rows_over_two_objects() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "palette.set",
        &json!({"index": 5, "color": "#dd2222"}),
    )
    .unwrap();
    apply(
        &mut session,
        "palette.set",
        &json!({"index": 6, "color": "#2222dd"}),
    )
    .unwrap();
    let block = |z0: i32, z1: i32| -> Vec<(i32, i32, i32, i32)> {
        let mut out = Vec::new();
        for z in z0..=z1 {
            for y in 0..6 {
                for x in 0..6 {
                    out.push((x, y, z, 1));
                }
            }
        }
        out
    };
    session.write_cells(&block(0, 5)).unwrap();
    apply(&mut session, "object.add", &json!({"name": "lid"})).unwrap();
    session.write_cells(&block(6, 7)).unwrap();
    let rows = |c: &str| vec![c.repeat(6); 8];
    let reply = apply(
        &mut session,
        "voxel.paint_from_views",
        &json!({
            "objects": [0, "lid"],
            "views": [
                {"view": "front", "rows": rows("r"), "legend": {"r": "#dd2222"}},
                {"view": "top", "rows": vec!["bbbbbb"; 6], "legend": {"b": "#2222dd"}}
            ],
            "slots": [5, 6],
            "seam": 0
        }),
    )
    .unwrap();
    assert!(reply["changed"].as_i64().unwrap() > 0, "{reply}");
    assert_eq!(reply["objects"], json!([session.objects[0].name, "lid"]));
    // The lid's top is blue, the lower box's front red, and the lower box's
    // top (under the lid) is not seen from above.
    let lid = session.objects.iter().find(|o| o.name == "lid").unwrap();
    assert_eq!(lid.cells[&(2, 2, 7)], 6);
    assert_eq!(session.objects[0].cells[&(2, 0, 2)], 5);
    assert_eq!(session.objects[0].cells[&(2, 2, 5)], 1);
    assert_eq!(session.selected, 1);
}

#[test]
fn the_painted_crate_example_paints_each_side_from_its_view() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("fixtures/examples/painted_crate.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let count = |slot: u8| {
        session.objects[0]
            .cells
            .values()
            .filter(|s| **s == slot)
            .count()
    };
    // Planks on the sides, the hatch and the brace, and the lid.
    for slot in [2, 3, 4, 5] {
        assert!(count(slot) > 0, "slot {slot}");
    }
    let cells = &session.objects[0].cells;
    // The middle of the top is lid or brace, never the side's plank colour.
    assert_ne!(cells[&(12, 12, 17)], 3);
}
