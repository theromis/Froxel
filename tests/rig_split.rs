use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

fn fill(cells: &mut HashMap<Cell, u8>, lo: Cell, hi: Cell, slot: u8) {
    for z in lo.2..=hi.2 {
        for y in lo.1..=hi.1 {
            for x in lo.0..=hi.0 {
                cells.insert((x, y, z), slot);
            }
        }
    }
}

/// A U: two arms 2 cells apart joined at the bottom.
fn u_shape() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (9, 2, 2), 1); // the base, along x
    fill(&mut cells, (0, 0, 3), (2, 2, 20), 1); // the left arm
    fill(&mut cells, (7, 0, 3), (9, 2, 20), 1); // the right arm, 4 cells away
    cells
}

fn seed(name: &str, p: [f64; 3]) -> PartSeed {
    PartSeed {
        name: name.into(),
        points: vec![[p[0] + 0.5, p[1] + 0.5, p[2] + 0.5]],
        planes: Vec::new(),
    }
}

fn spec(parts: Vec<PartSeed>, metric: Metric) -> RigSplitSpec {
    RigSplitSpec {
        parts,
        metric,
        connectivity: 26,
        reach: 8.0,
    }
}

#[test]
fn geodesic_keeps_each_arm_whole_where_euclidean_crosses_the_gap() {
    let cells = u_shape();
    // Seeds at the bottom of the left arm and the top of the right arm:
    // the right arm's lower half is nearer the left seed in a straight line
    // across the gap, but far along the solid.
    let parts = vec![
        seed("left", [1.0, 1.0, 4.0]),
        seed("right", [8.0, 1.0, 20.0]),
    ];
    let geo = split_parts(&cells, &spec(parts.clone(), Metric::Geodesic)).unwrap();
    let euc = split_parts(&cells, &spec(parts, Metric::Euclidean)).unwrap();
    assert_eq!(geo.unreached, 0);
    // A cell low on the right arm: right geodesically, left in a straight line.
    assert_eq!(geo.labels[&(8, 1, 8)], 1);
    assert_eq!(euc.labels[&(8, 1, 8)], 0);
    assert_eq!(geo.counts.iter().sum::<usize>(), cells.len());
}

#[test]
fn a_plane_keeps_a_part_on_its_side() {
    let mut cells = HashMap::new();
    fill(&mut cells, (0, 0, 0), (2, 20, 2), 1); // a rod along y
    let mut head = seed("head", [1.0, 0.0, 1.0]);
    let tail = seed("tail", [1.0, 20.0, 1.0]);
    // The head keeps y < 5 only: its wave stops at the plane.
    head.planes.push(Plane {
        normal: [0.0, -1.0, 0.0],
        at: [0.0, 5.0, 0.0],
    });
    let done = split_parts(&cells, &spec(vec![head, tail], Metric::Geodesic)).unwrap();
    assert_eq!(done.labels[&(1, 4, 1)], 0);
    assert_eq!(done.labels[&(1, 5, 1)], 1);
    assert_eq!(done.counts[0], 3 * 3 * 5);
}

#[test]
fn a_seed_far_from_the_solid_is_refused() {
    let cells = u_shape();
    let err = split_parts(
        &cells,
        &spec(vec![seed("lost", [50.0, 50.0, 50.0])], Metric::Geodesic),
    )
    .unwrap_err();
    assert!(err.contains("no solid cell"), "{err}");
}

#[test]
fn mcp_writes_each_part_at_world_origin_and_binds_it() {
    let mut session = Session::new();
    session.replace_empty();
    let cells = u_shape();
    let writes: Vec<(i32, i32, i32, i32)> = cells.keys().map(|c| (c.0, c.1, c.2, 1)).collect();
    session.write_cells(&writes).unwrap();
    // Move the source so its local cells are not world cells.
    let name = session.objects[0].name.clone();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "position": [10, 0, 0]}),
    )
    .unwrap();
    apply(
        &mut session,
        "rig.add",
        &json!({"name": "right", "pivot": [18, 1, 3]}),
    )
    .unwrap();
    let reply = apply(
        &mut session,
        "object.split_parts",
        &json!({"parts": [
            {"name": "left", "point": [1, 1, 10]},
            {"name": "right", "bone": "right"},
            {"name": "base", "points": [[4, 1, 1], [5, 1, 1]], "planes": [{"normal": [0, 0, -1], "at": [0, 0, 2]}]}
        ]}),
    )
    .unwrap();
    assert_eq!(reply["sourceRemoved"], json!(true), "{reply}");
    assert!(session.objects.iter().all(|o| o.name != name));
    let get = |n: &str| session.objects.iter().find(|o| o.name == n).unwrap();
    for part in ["left", "right", "base"] {
        assert_eq!(get(part).translation, (0, 0, 0));
    }
    // World cells: the source sat at x + 10.
    assert!(get("left").cells.contains_key(&(11, 1, 10)));
    assert!(get("right").cells.contains_key(&(18, 1, 10)));
    assert!(get("base").cells.keys().all(|c| c.2 <= 2));
    assert_eq!(get("right").bone.as_deref(), Some("right"));
    let total: usize = ["left", "right", "base"]
        .iter()
        .map(|p| get(p).cells.len())
        .sum();
    assert_eq!(total, cells.len());
    // One undo step puts the source back.
    session.undo();
    assert!(session.objects.iter().any(|o| o.name == name));
    assert!(session.objects.iter().all(|o| o.name != "left"));
}

#[test]
fn rhai_records_split_parts_for_after_the_run() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "name": "rod"}),
    )
    .unwrap();
    session
        .set_script(
            0,
            r#"
fill_box(0, 0, 0, 2, 20, 2, 1);
split_parts("rod", [
    #{name: "rod", point: [1, 18, 1]},
    #{name: "front", point: [1, 1, 1]},
]);
"#,
        )
        .unwrap();
    session.regenerate_script(0).unwrap();
    let front = session.objects.iter().find(|o| o.name == "front").unwrap();
    let rod = session.objects.iter().find(|o| o.name == "rod").unwrap();
    assert_eq!(front.cells.len() + rod.cells.len(), 3 * 3 * 21);
    assert!(front.cells.keys().all(|c| c.1 <= 10));
}

#[test]
fn the_split_critter_example_makes_eight_parts_at_world_origin() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "object.set",
        &json!({"index": 0, "name": "critter"}),
    )
    .unwrap();
    session
        .set_script(0, include_str!("fixtures/examples/split_critter.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let names = [
        "critter", "head", "jaw", "tail", "leg_fl", "leg_fr", "leg_bl", "leg_br",
    ];
    let mut seen = std::collections::HashSet::new();
    for name in names {
        let part = session
            .objects
            .iter()
            .find(|o| o.name == name)
            .unwrap_or_else(|| panic!("no {name}"));
        assert!(!part.cells.is_empty(), "{name} is empty");
        assert_eq!(part.translation, (0, 0, 0));
        for cell in part.cells.keys() {
            assert!(seen.insert(*cell), "{name} shares {cell:?}");
        }
    }
    // The jaw stays below the mouth's plane; the legs reach the floor.
    let jaw = session.objects.iter().find(|o| o.name == "jaw").unwrap();
    assert!(jaw.cells.keys().all(|c| c.2 <= 7 + 5));
    let leg = session.objects.iter().find(|o| o.name == "leg_fl").unwrap();
    assert!(leg.cells.keys().any(|c| c.2 == 0));
    // A second run splits again into the same objects.
    let count = session.objects.len();
    session.regenerate_script(0).unwrap();
    assert_eq!(session.objects.len(), count);
}
