use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::state::{MarkerKind, Session};

const HULL: u8 = 1;
const DECK: u8 = 2;
const VINE: u8 = 3;

fn spec() -> CollisionSpec {
    CollisionSpec {
        only: Vec::new(),
        exclude: Vec::new(),
        layer: 1,
        bands: Vec::new(),
        min_feature: 1,
        hole_size: 2,
        max_boxes: None,
    }
}

fn fill(cells: &mut HashMap<Cell, u8>, low: Cell, high: Cell, slot: u8) {
    for z in low.2..=high.2 {
        for y in low.1..=high.1 {
            for x in low.0..=high.0 {
                cells.insert((x, y, z), slot);
            }
        }
    }
}

fn carve(cells: &mut HashMap<Cell, u8>, low: Cell, high: Cell) {
    for z in low.2..=high.2 {
        for y in low.1..=high.1 {
            for x in low.0..=high.0 {
                cells.remove(&(x, y, z));
            }
        }
    }
}

/// A box hull with a hold, a one-thick deck with a hatch, and a side hole.
fn boat() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (23, 11, 7), HULL);
    carve(&mut cells, (1, 1, 1), (22, 10, 7));
    fill(&mut cells, (1, 1, 7), (22, 10, 7), DECK);
    // The hatch through the deck.
    carve(&mut cells, (10, 4, 7), (13, 7, 7));
    // The hole through one side of the hull.
    carve(&mut cells, (4, 0, 3), (6, 0, 5));
    cells
}

fn inside(cover: &CollisionCover, cell: Cell) -> bool {
    cover.boxes.iter().any(|(low, high)| {
        low.0 <= cell.0
            && cell.0 <= high.0
            && low.1 <= cell.1
            && cell.1 <= high.1
            && low.2 <= cell.2
            && cell.2 <= high.2
    })
}

fn assert_hollows_open(cover: &CollisionCover) {
    assert!(!inside(cover, (11, 5, 7)), "the hatch closed");
    assert!(!inside(cover, (5, 0, 4)), "the hull's hole closed");
    assert!(!inside(cover, (11, 5, 3)), "the hold filled");
    assert!(!inside(cover, (3, 5, 3)), "the hold filled");
}

#[test]
fn hole_size_one_is_an_exact_cover() {
    let cells = boat();
    let cover = collision_cover(
        &cells,
        &CollisionSpec {
            hole_size: 1,
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(cover.covered, cells.len());
    assert_eq!(cover.empty_filled, 0);
    assert_eq!(cover.box_cells, cells.len());
    assert_hollows_open(&cover);
    // Far fewer boxes than cells; a boat is a few walls and a floor.
    assert!(cover.boxes.len() <= 24, "{} boxes", cover.boxes.len());
}

#[test]
fn hollows_as_wide_as_the_hole_size_stay_open() {
    let cells = boat();
    for hole_size in 2..=3 {
        let cover = collision_cover(
            &cells,
            &CollisionSpec {
                hole_size,
                ..spec()
            },
        )
        .unwrap();
        assert_eq!(cover.covered, cover.target);
        assert_hollows_open(&cover);
    }
}

#[test]
fn a_solid_block_is_one_box() {
    let mut cells = HashMap::new();
    fill(&mut cells, (-3, 2, 5), (4, 9, 12), HULL);
    let cover = collision_cover(&cells, &spec()).unwrap();
    assert_eq!(cover.boxes, vec![((-3, 2, 5), (4, 9, 12))]);
}

/// A thick wall with a dent one cell deep in its face.
fn dented_wall() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (15, 3, 15), HULL);
    carve(&mut cells, (4, 0, 4), (9, 0, 9));
    cells
}

#[test]
fn a_dent_under_the_hole_size_does_not_split_a_wall() {
    let cover = collision_cover(&dented_wall(), &spec()).unwrap();
    assert_eq!(cover.boxes, vec![((0, 0, 0), (15, 3, 15))]);
    assert_eq!(cover.empty_filled, 36);
}

#[test]
fn spare_boxes_take_the_filled_volume_away() {
    let cover = collision_cover(
        &dented_wall(),
        &CollisionSpec {
            max_boxes: Some(32),
            ..spec()
        },
    )
    .unwrap();
    assert!(cover.boxes.len() <= 32);
    assert_eq!(cover.empty_filled, 0);
    assert_eq!(cover.covered, cover.target);
}

#[test]
fn a_tight_budget_merges_and_keeps_hollows_open() {
    let cells = boat();
    let free = collision_cover(&cells, &spec()).unwrap();
    let budget = free.boxes.len() - 1;
    let cover = collision_cover(
        &cells,
        &CollisionSpec {
            max_boxes: Some(budget),
            ..spec()
        },
    )
    .unwrap();
    assert!(cover.boxes.len() <= budget);
    assert_hollows_open(&cover);
}

#[test]
fn a_budget_too_small_drops_boxes_instead_of_closing_hollows() {
    let cells = boat();
    let cover = collision_cover(
        &cells,
        &CollisionSpec {
            max_boxes: Some(2),
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(cover.boxes.len(), 2);
    assert!(cover.dropped > 0);
    assert!(cover.covered < cover.target);
    assert_hollows_open(&cover);
}

#[test]
fn a_larger_hole_size_lets_a_small_hole_close() {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (7, 7, 0), HULL);
    carve(&mut cells, (3, 3, 0), (4, 4, 0));
    let tight = collision_cover(&cells, &spec()).unwrap();
    assert!(!inside(&tight, (3, 3, 0)));
    assert!(tight.boxes.len() > 1);
    let loose = collision_cover(
        &cells,
        &CollisionSpec {
            hole_size: 3,
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(loose.boxes, vec![((0, 0, 0), (7, 7, 0))]);
    assert_eq!(loose.empty_filled, 4);
}

#[test]
fn a_gap_stays_open_from_the_hole_size_up() {
    for (gap, hole_size, open) in [(1, 2, false), (2, 2, true), (1, 1, true), (4, 2, true)] {
        let mut cells = HashMap::new();
        fill(&mut cells, (0, 0, 0), (7, 7, 2), HULL);
        fill(&mut cells, (0, 0, 3 + gap), (7, 7, 3 + gap), DECK);
        // Planes that do not meet the gap: the cut goes there anyway.
        let cover = collision_cover(
            &cells,
            &CollisionSpec {
                hole_size,
                bands: vec![0, 32],
                ..spec()
            },
        )
        .unwrap();
        assert_eq!(
            !inside(&cover, (3, 3, 3)),
            open,
            "gap {gap}, hole {hole_size}"
        );
        assert!(inside(&cover, (3, 3, 3 + gap)), "gap {gap} lost the deck");
    }
}

#[test]
fn z_cuts_lie_on_the_bands_while_those_open_the_hollows() {
    // Stairs: cuts at z 4 and 8 or at x 4 and 8 are all exact.
    let mut cells = HashMap::new();
    for (x, top) in [(0, 11), (4, 7), (8, 3)] {
        fill(&mut cells, (x, 0, 0), (x + 3, 7, top), HULL);
    }
    for (bands, layer, allowed) in [
        (vec![0, 6, 12], 1, vec![0, 6]),
        (Vec::new(), 12, vec![0, 12]),
    ] {
        let cover = collision_cover(
            &cells,
            &CollisionSpec {
                bands,
                layer,
                ..spec()
            },
        )
        .unwrap();
        for (low, _) in &cover.boxes {
            assert!(allowed.contains(&low.2), "a box starts at z {}", low.2);
        }
        assert_eq!(cover.covered, cover.target);
        assert!(!inside(&cover, (10, 3, 6)));
    }
}

#[test]
fn min_feature_drops_vines_and_keeps_a_one_thick_deck_and_wall() {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 10), (9, 9, 10), DECK);
    fill(&mut cells, (0, 0, 0), (0, 9, 9), HULL);
    fill(&mut cells, (5, 5, 2), (5, 5, 9), VINE);
    let cover = collision_cover(
        &cells,
        &CollisionSpec {
            min_feature: 2,
            ..spec()
        },
    )
    .unwrap();
    assert_eq!(cover.ignored_thin, 8);
    assert!(!inside(&cover, (5, 5, 5)));
    assert!(inside(&cover, (5, 5, 10)));
    assert!(inside(&cover, (0, 5, 5)));
}

#[test]
fn only_and_exclude_choose_the_slots() {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (3, 3, 0), HULL);
    fill(&mut cells, (0, 0, 1), (3, 3, 1), VINE);
    let left_out = CollisionSpec {
        exclude: vec![VINE],
        ..spec()
    };
    let cover = collision_cover(&cells, &left_out).unwrap();
    assert_eq!(cover.boxes, vec![((0, 0, 0), (3, 3, 0))]);
    assert_eq!(cover.ignored_slots, 16);
    let only = CollisionSpec {
        only: vec![VINE],
        ..spec()
    };
    let cover = collision_cover(&cells, &only).unwrap();
    assert_eq!(cover.boxes, vec![((0, 0, 1), (3, 3, 1))]);
    let nothing = CollisionSpec {
        only: vec![9],
        ..spec()
    };
    assert!(collision_cover(&cells, &nothing).is_err());
    assert!(collision_cover(&HashMap::new(), &spec()).is_err());
}

fn boat_session() -> Session {
    let mut session = Session::new();
    session.replace_empty();
    session.objects[0].cells = boat();
    session
}

fn run(session: &mut Session, args: serde_json::Value) -> serde_json::Value {
    crate::remote::apply(session, "marker.collision", &args).unwrap()
}

#[test]
fn the_tool_replies_with_counts_and_replaces_its_own_markers_only() {
    let mut session = boat_session();
    let object = session.active().name.clone();
    let args = json!({"object": 0, "prefix": "hull", "holeSize": 1});
    let cells = session.active().cells.clone();
    session
        .set_marker(0, "hull_keep", MarkerKind::Point { at: [0.0; 3] })
        .unwrap();
    session
        .set_marker(
            0,
            "hullish_1",
            MarkerKind::Box {
                min: [0.0; 3],
                max: [1.0; 3],
            },
        )
        .unwrap();
    let reply = run(&mut session, args.clone());
    let count = reply["boxes"].as_u64().unwrap() as usize;
    assert!(count > 1);
    assert_eq!(reply["coveredPercent"], json!(100.0));
    assert_eq!(reply["emptyFilledCells"], json!(0));
    assert_eq!(reply["solidCells"], json!(cells.len()));
    assert_eq!(reply["replaced"], json!(0));
    assert_eq!(session.markers.len(), count + 2);
    assert!(session
        .markers
        .iter()
        .any(|m| m.name == "hull_1" && m.object == object));

    // Fewer boxes on a second run leave none of the first run behind.
    let second = run(
        &mut session,
        json!({"object": 0, "prefix": "hull", "holeSize": 1, "only": [DECK]}),
    );
    assert_eq!(second["replaced"], json!(count));
    let kept = second["boxes"].as_u64().unwrap() as usize;
    assert!(kept < count);
    assert_eq!(session.markers.len(), kept + 2);
    assert!(session.markers.iter().any(|m| m.name == "hull_keep"));
    assert!(session.markers.iter().any(|m| m.name == "hullish_1"));

    session.undo();
    assert_eq!(session.markers.len(), count + 2);
}

#[test]
fn the_default_prefix_is_the_objects_and_a_taken_name_is_refused() {
    let mut session = boat_session();
    let object = session.active().name.clone();
    let reply = run(&mut session, json!({}));
    assert_eq!(reply["prefix"], json!(format!("{object}_collider")));
    assert!(session
        .markers
        .iter()
        .any(|m| m.name == format!("{object}_collider_1")));

    session
        .set_marker(0, "hull_1", MarkerKind::Point { at: [0.0; 3] })
        .unwrap();
    let before = session.markers.clone();
    let err = crate::remote::apply(&mut session, "marker.collision", &json!({"prefix": "hull"}))
        .unwrap_err();
    assert!(err.contains("hull_1"), "{err}");
    assert_eq!(session.markers, before);
}

#[test]
fn bad_arguments_are_refused() {
    let mut session = boat_session();
    for args in [
        json!({"layer": 0}),
        json!({"layer": 2, "zBands": [0, 4]}),
        json!({"zBands": [4, 0]}),
        json!({"zBands": [4]}),
        json!({"minFeature": 0}),
        json!({"holeSize": 0}),
        json!({"maxBoxes": 0}),
        json!({"only": [0]}),
        json!({"prefix": ""}),
        json!({"object": "nope"}),
    ] {
        assert!(
            crate::remote::apply(&mut session, "marker.collision", &args).is_err(),
            "{args} was accepted"
        );
    }
    assert!(session.markers.is_empty());
}

#[test]
fn the_boxes_survive_a_vox_round_trip_and_reach_the_overlay() {
    let mut session = boat_session();
    run(&mut session, json!({"prefix": "hull"}));
    let before = session.markers.clone();
    let dir = std::env::temp_dir().join(format!("froxel-collision-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("boat.vox");
    session.write_vox(&path).unwrap();
    let mut loaded = Session::new();
    loaded.open_vox(&path).unwrap();
    assert_eq!(loaded.markers.len(), before.len());
    assert_eq!(
        loaded.markers_json().as_array().unwrap()[0]["world"],
        session.markers_json().as_array().unwrap()[0]["world"]
    );
    let png = dir.join("overlay.png");
    let reply = crate::remote::apply(
        &mut session,
        "render.preview",
        &json!({"path": png, "overlay": "boxes", "width": 128, "height": 128}),
    )
    .unwrap();
    assert_eq!(
        reply["overlay"].as_array().unwrap().len(),
        before.len(),
        "{reply}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_script_builds_the_same_boxes_and_a_rerun_replaces_them() {
    let source = r#"
        fill_box(0, 0, 0, 23, 11, 7, 1);
        carve_box(1, 1, 1, 22, 10, 7);
        fill_box(1, 1, 7, 22, 10, 7, 2);
        carve_box(10, 4, 7, 13, 7, 7);
        carve_box(4, 0, 3, 6, 0, 5);
        let reply = collision_from_voxels(#{prefix: "hull", holeSize: 1});
        collision_from_voxels(#{prefix: "hull", holeSize: 1});
        if reply.emptyFilledCells != 0 { throw "filled"; }
        marker("count", reply.boxes, 0, 0);
    "#;
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    let scripted: Vec<_> = session
        .markers
        .iter()
        .filter(|m| m.name != "count")
        .cloned()
        .collect();
    assert!(scripted.iter().all(|m| m.scripted));

    let tool = {
        let mut other = Session::new();
        other.replace_empty();
        other.objects[0].cells = session.active().cells.clone();
        run(&mut other, json!({"prefix": "hull", "holeSize": 1}));
        other.markers
    };
    assert_eq!(scripted.len(), tool.len());
    for (a, b) in scripted.iter().zip(&tool) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.kind, b.kind);
    }

    session.regenerate_script(0).unwrap();
    assert_eq!(session.markers.len(), tool.len() + 1);

    let misspelt = "fill_box(0, 0, 0, 3, 3, 3, 1); collision_from_voxels(#{oops: 1});";
    assert!(session.set_script(0, misspelt).is_err());
}

#[test]
fn a_hole_taller_than_the_hole_size_is_not_sliced_into_filled_parts() {
    // A two-thick wall on a floor, with a hole of 6 by 5 through it. A cut
    // through the hole at any height leaves less than 4 tall on one side.
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (47, 19, 1), HULL);
    fill(&mut cells, (0, 18, 2), (47, 19, 15), HULL);
    carve(&mut cells, (8, 18, 7), (13, 19, 11));
    let cover = collision_cover(
        &cells,
        &CollisionSpec {
            hole_size: 4,
            ..spec()
        },
    )
    .unwrap();
    for z in 7..=11 {
        assert!(!inside(&cover, (10, 18, z)), "the hole closed at z {z}");
    }
}

#[test]
fn no_box_touches_a_hole_it_could_slice() {
    // Plates with rows of holes exactly as wide as the hole size and wider.
    for (size, hole) in [(2, 2), (3, 3), (4, 2), (5, 3)] {
        let mut cells = HashMap::new();
        fill(&mut cells, (0, 0, 0), (39, 39, 0), HULL);
        let mut holes = Vec::new();
        for i in 0..4 {
            for j in 0..4 {
                let low = (2 + i * 9, 2 + j * 9, 0);
                let high = (low.0 + size - 1, low.1 + size - 1, 0);
                carve(&mut cells, low, high);
                holes.push((low, high));
            }
        }
        let cover = collision_cover(
            &cells,
            &CollisionSpec {
                hole_size: hole,
                ..spec()
            },
        )
        .unwrap();
        for (low, high) in holes {
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    assert!(!inside(&cover, (x, y, 0)), "size {size}: cell {x} {y}");
                }
            }
        }
    }
}

#[test]
fn the_collision_hull_example_keeps_its_hollows_open() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/collision_hull.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let boxes: Vec<(Cell, Cell)> = session
        .markers
        .iter()
        .filter_map(|m| match m.kind {
            MarkerKind::Box { min, max } => Some((
                (min[0] as i32, min[1] as i32, min[2] as i32),
                (max[0] as i32 - 1, max[1] as i32 - 1, max[2] as i32 - 1),
            )),
            _ => None,
        })
        .collect();
    assert!(boxes.len() > 4 && boxes.len() < 64, "{} boxes", boxes.len());
    let cover = CollisionCover {
        boxes,
        ..CollisionCover::default()
    };
    assert!(!inside(&cover, (24, 0, 14)), "the hatch closed");
    assert!(!inside(&cover, (24, 0, 8)), "the hold filled");
    assert!(!inside(&cover, (10, 8, 9)), "the side hole closed");
    assert!(inside(&cover, (12, 4, 14)), "the deck is missing");
    assert!(inside(&cover, (7, 0, 30)), "the mast is missing");
    assert!(!inside(&cover, (14, -11, 10)), "a vine has a box");
    if let Ok(dir) = std::env::var("FROXEL_EXAMPLE_OUT") {
        session
            .write_vox(&std::path::Path::new(&dir).join("collision_hull.vox"))
            .unwrap();
    }
}
