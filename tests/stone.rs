use std::collections::HashMap;

use super::*;
use crate::state::Cell;

/// A solid slab of `slot` from `min` to `max`, inclusive.
fn slab(min: Cell, max: Cell, slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                cells.insert((x, y, z), slot);
            }
        }
    }
    cells
}

fn joint_spec(pattern: JointPattern) -> JointSpec {
    let (spacing, course) = pattern.defaults();
    JointSpec {
        region: Region::new((0, 0, 0), (31, 5, 23), "joint").unwrap(),
        pattern,
        spacing,
        course,
        width: 1,
        depth: 2,
        dirt: Some(9),
        only: Vec::new(),
        density: 0.8,
        seed: 4,
    }
}

#[test]
fn joints_cut_gaps_with_dirt_at_the_bottom() {
    let cells = slab((0, 0, 0), (31, 5, 23), 1);
    for pattern in [
        JointPattern::Columns,
        JointPattern::Courses,
        JointPattern::Cracks,
    ] {
        let done = joints(&cells, &joint_spec(pattern)).unwrap();
        assert!(done.removed > 20, "{pattern:?} removed {}", done.removed);
        assert!(done.dirtied > 0, "{pattern:?} left no dirt");
        // Gaps stay in the outer two cells: the core of the slab is whole.
        for &(x, y, z, slot) in &done.writes {
            if slot == 0 {
                let inside = [x, 31 - x, y, 5 - y, z, 23 - z].into_iter().min().unwrap();
                assert!(inside < 2, "{pattern:?} cut ({x}, {y}, {z}) too deep");
            } else {
                assert_eq!(slot, 9);
            }
        }
        assert_eq!(done, joints(&cells, &joint_spec(pattern)).unwrap());
    }
}

#[test]
fn courses_run_in_horizontal_rows() {
    let cells = slab((0, 0, 0), (31, 5, 23), 1);
    let done = joints(&cells, &joint_spec(JointPattern::Courses)).unwrap();
    // A bed joint takes nearly a whole row of the front face.
    let mut rows = HashMap::new();
    for &(x, y, z, slot) in &done.writes {
        if slot == 0 && y == 0 && (1..31).contains(&x) {
            *rows.entry(z).or_insert(0) += 1;
        }
    }
    assert!(rows.values().any(|&count| count >= 28), "{rows:?}");
}

#[test]
fn joints_leave_other_slots_and_refuse_bad_values() {
    let mut cells = slab((0, 0, 0), (31, 5, 23), 1);
    for x in 0..32 {
        for z in 0..24 {
            cells.insert((x, 0, z), 5);
        }
    }
    let mut spec = joint_spec(JointPattern::Columns);
    spec.only = vec![1];
    let done = joints(&cells, &spec).unwrap();
    assert!(
        done.writes.iter().all(|w| w.1 != 0),
        "the moss face was cut"
    );
    spec.depth = 0;
    assert!(joints(&cells, &spec).unwrap_err().contains("depth"));
    spec.depth = 2;
    spec.spacing = (1.0, 4.0);
    assert!(joints(&cells, &spec).unwrap_err().contains("spacing"));
}

fn weather_spec() -> WeatherSpec {
    WeatherSpec {
        region: Region::new((0, 0, 0), (19, 5, 11), "weather").unwrap(),
        chip: 0.3,
        chip_size: 3,
        pits: 0.1,
        pit_depth: 2,
        broken: Some(3),
        noise: vec![4, 5],
        noise_density: 0.4,
        noise_scale: 3.0,
        only: Vec::new(),
        seed: 8,
    }
}

#[test]
fn weather_chips_edges_dents_faces_and_speckles() {
    let cells = slab((0, 0, 0), (19, 5, 11), 1);
    let done = weather(&cells, &weather_spec()).unwrap();
    assert!(done.chips > 0 && done.pits > 0, "{done:?}");
    assert!(done.broken > 0 && done.recoloured > 0, "{done:?}");
    assert_eq!(done, weather(&cells, &weather_spec()).unwrap());
    let removed: Vec<Cell> = done
        .writes
        .iter()
        .filter(|w| w.3 == 0)
        .map(|w| (w.0, w.1, w.2))
        .collect();
    assert_eq!(removed.len(), done.removed);
    // Nothing reaches the core: chips take exposed cells, pits go two deep.
    for &(x, y, z) in &removed {
        let inside = [x, 19 - x, y, 5 - y, z, 11 - z].into_iter().min().unwrap();
        assert!(inside < 2, "({x}, {y}, {z}) is too deep");
    }
    // Recoloured cells stay solid and take a listed slot.
    for &(x, y, z, slot) in &done.writes {
        if slot != 0 {
            assert!(cells.contains_key(&(x, y, z)));
            assert!([3, 4, 5].contains(&slot), "slot {slot}");
        }
    }
}

#[test]
fn weather_leaves_other_slots_and_refuses_bad_values() {
    let mut cells = slab((0, 0, 0), (19, 5, 11), 1);
    for x in 0..20 {
        for y in 0..6 {
            cells.insert((x, y, 11), 7);
        }
    }
    let mut spec = weather_spec();
    spec.only = vec![1];
    let done = weather(&cells, &spec).unwrap();
    assert!(
        done.writes.iter().all(|w| w.2 != 11),
        "the moss top changed"
    );
    spec.chip = 1.5;
    assert!(weather(&cells, &spec).unwrap_err().contains("chip"));
    spec.chip = 0.3;
    spec.noise = vec![0];
    assert!(weather(&cells, &spec).unwrap_err().contains("noise"));
}
