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

fn growth_spec(kind: GrowthKind, palettes: Vec<u8>) -> GrowthSpec {
    GrowthSpec {
        kind,
        region: Region::new((-1, -1, -1), (20, 6, 16), "grow").unwrap(),
        palettes,
        amount: 0.6,
        noise: 0.5,
        faces: vec![Face::Top, Face::Side],
        facing: None,
        band: None,
        only: Vec::new(),
        layer: Layer::Grow,
        seed: 3,
    }
}

const MOSS: GrowthKind = GrowthKind::Moss {
    curtains: 0.6,
    length: (2, 6),
    thickness: 2,
};

#[test]
fn moss_caps_tops_and_hangs_curtains_on_the_sides() {
    let cells = slab((0, 0, 0), (19, 5, 15), 1);
    let done = grow(&cells, &growth_spec(MOSS, vec![4, 5])).unwrap();
    assert!(done.added > 0 && done.strands > 0, "{done:?}");
    assert_eq!(done.painted, 0);
    let mut tops = 0;
    let mut sides = 0;
    for &(x, y, z, slot) in &done.writes {
        assert!(!cells.contains_key(&(x, y, z)), "grow wrote a solid cell");
        assert!([4, 5].contains(&slot));
        if z >= 16 {
            tops += 1;
        } else {
            sides += 1;
            // A curtain hangs from the top: no moss low on the wall.
            assert!(z >= 15 - 6, "curtain cell at z {z}");
        }
    }
    assert!(tops > 0 && sides > 0, "{tops} tops, {sides} sides");
    assert_eq!(done, grow(&cells, &growth_spec(MOSS, vec![4, 5])).unwrap());
    let mut paint = growth_spec(MOSS, vec![4]);
    paint.layer = Layer::Paint;
    let painted = grow(&cells, &paint).unwrap();
    assert!(painted.added == 0 && painted.painted > 0, "{painted:?}");
    assert!(painted
        .writes
        .iter()
        .all(|w| cells.contains_key(&(w.0, w.1, w.2))));
}

#[test]
fn algae_thins_upward_inside_its_band() {
    let cells = slab((0, 0, 0), (39, 5, 15), 1);
    let mut spec = growth_spec(GrowthKind::Algae, vec![6, 7, 8]);
    spec.region = Region::new((0, 0, 0), (39, 5, 15), "grow").unwrap();
    spec.amount = 1.0;
    spec.band = Some((0, 9));
    spec.faces = vec![Face::Side];
    spec.layer = Layer::Paint;
    let done = grow(&cells, &spec).unwrap();
    let count = |z: i32| done.writes.iter().filter(|w| w.2 == z).count();
    assert!(count(0) > count(8), "{} low, {} high", count(0), count(8));
    assert!(done.writes.iter().all(|w| (0..=9).contains(&w.2)));
    // Low cells take the first slot more than the last.
    let low = done.writes.iter().filter(|w| w.2 <= 1);
    let (first, last) = low.fold((0, 0), |(f, l), w| {
        (f + usize::from(w.3 == 6), l + usize::from(w.3 == 8))
    });
    assert!(first > last, "{first} first, {last} last");
}

#[test]
fn vines_hang_from_top_edges_graded_to_the_tip() {
    let cells = slab((0, 0, 0), (19, 5, 15), 1);
    let mut spec = growth_spec(
        GrowthKind::Vines {
            length: (6, 10),
            blooms: vec![9],
            bloom_chance: 0.2,
        },
        vec![4, 5, 6],
    );
    spec.amount = 0.3;
    spec.faces = vec![Face::Side];
    spec.facing = Some([0.0, -1.0, 0.0]);
    let done = grow(&cells, &spec).unwrap();
    assert!(done.strands > 0 && done.blooms > 0, "{done:?}");
    for &(x, y, z, slot) in &done.writes {
        assert!(!cells.contains_key(&(x, y, z)));
        if slot != 9 {
            // Strands hang in front of the -Y face only.
            assert!(y < 0, "strand at y {y}");
            if z == 15 {
                assert_eq!(slot, 4, "the anchor takes the first slot");
            }
        }
    }
    assert!(done.writes.iter().any(|w| w.3 == 6 && w.2 < 12));
}

#[test]
fn growth_refuses_bad_values() {
    let cells = slab((0, 0, 0), (9, 5, 9), 1);
    let mut spec = growth_spec(MOSS, vec![]);
    assert!(grow(&cells, &spec).unwrap_err().contains("palettes"));
    spec.palettes = vec![4];
    spec.amount = 2.0;
    assert!(grow(&cells, &spec).unwrap_err().contains("amount"));
    spec.amount = 0.5;
    spec.band = Some((5, 1));
    assert!(grow(&cells, &spec).unwrap_err().contains("band"));
}

fn point(x: f64, y: f64, z: f64) -> SweepPoint {
    SweepPoint {
        at: [x, y, z],
        twist: 0.0,
        scale: 1.0,
    }
}

#[test]
fn a_straight_sweep_is_the_profile_times_the_length() {
    let rect = Profile::Rect {
        width: 4.0,
        height: 6.0,
        radius: 0.0,
    };
    let cells = sweep(&[point(0.0, 0.0, 0.0), point(10.0, 0.0, 0.0)], false, &rect).unwrap();
    // Centres within u -2..2 (4 or 5 columns) and v 0..6, x 0..10 with ends.
    let mut ys: Vec<i32> = cells.iter().map(|c| c.cell.1).collect();
    ys.sort_unstable();
    ys.dedup();
    assert!(ys.len() == 5, "{ys:?}");
    assert!(cells.iter().all(|c| (0..=6).contains(&c.cell.2)));
    assert!(cells.iter().all(|c| (0..=10).contains(&c.cell.0)));
    assert_eq!(cells.len(), 11 * 5 * 7);
}

#[test]
fn a_bent_sweep_has_no_holes_and_twist_turns_the_profile() {
    let rect = Profile::Rect {
        width: 6.0,
        height: 3.0,
        radius: 0.0,
    };
    // A right-angle bend: the outside corner is filled.
    let path = [
        point(0.0, 0.0, 0.0),
        point(20.0, 0.0, 0.0),
        point(20.0, 20.0, 0.0),
    ];
    let cells = sweep(&path, false, &rect).unwrap();
    let set: std::collections::HashSet<Cell> = cells.iter().map(|c| c.cell).collect();
    for z in 0..=3 {
        assert!(set.contains(&(22, -2, z)), "outside corner at z {z}");
        for x in 0..=20 {
            for y in -2..=2 {
                assert!(set.contains(&(x, y, z)), "hole at ({x}, {y}, {z})");
            }
        }
    }
    // Twist 90 stands the 6 x 3 profile on its side: 3 wide, 6 tall.
    let mut twisted = path[..2].to_vec();
    for p in &mut twisted {
        p.twist = 90.0;
    }
    let cells = sweep(&twisted, true, &rect).unwrap();
    let span = |axis: fn(&Cell) -> i32| {
        let values: Vec<i32> = cells.iter().map(|c| axis(&c.cell)).collect();
        values.iter().max().unwrap() - values.iter().min().unwrap()
    };
    assert!(
        span(|c| c.2) >= 5 && span(|c| c.1) <= 3,
        "turned the wrong way"
    );
}

#[test]
fn sweep_takes_a_polygon_and_refuses_bad_input() {
    let wedge = Profile::Polygon(vec![[-3.0, 0.0], [3.0, 0.0], [0.0, 6.0]]);
    let cells = sweep(&[point(0.0, 0.0, 0.0), point(8.0, 0.0, 0.0)], false, &wedge).unwrap();
    assert!(cells.iter().any(|c| c.cell.2 == 5));
    assert!(!cells.iter().any(|c| c.cell.2 == 5 && c.cell.1.abs() > 1));
    assert!(sweep(&[point(0.0, 0.0, 0.0)], false, &wedge)
        .unwrap_err()
        .contains("2 points"));
    let flat = Profile::Polygon(vec![[0.0, 0.0], [1.0, 1.0]]);
    assert!(
        sweep(&[point(0.0, 0.0, 0.0), point(4.0, 0.0, 0.0)], false, &flat)
            .unwrap_err()
            .contains("3 to 256")
    );
}
