use std::collections::HashMap;

use crate::state::{Axis, GeneratorKind};

use super::*;

#[test]
fn line_includes_both_ends() {
    let cells = bresenham((0, 0, 0), (3, 0, 0));
    assert_eq!(cells.first(), Some(&(0, 0, 0)));
    assert_eq!(cells.last(), Some(&(3, 0, 0)));
    assert_eq!(cells.len(), 4);
}

#[test]
fn hollow_box_skips_the_center() {
    let cells = cube((0, 0, 0), (2, 2, 2), false);
    assert!(!cells.contains(&(1, 1, 1)));
    assert!(cells.contains(&(0, 0, 0)));
    assert!(cells.contains(&(1, 1, 0)));
}

#[test]
fn ellipsoid_fills_the_center_and_not_the_corner() {
    let gen = Gen {
        kind: GeneratorKind::Ellipsoid,
        axis: Axis::Y,
        apex_at_max: true,
        palette: 4,
        torus_tube: 0.3,
        round_radius: 2.0,
        helix_turns: 2.0,
        helix_radius: 0.75,
        helix_tube: 0.16,
        helix_cw: false,
        noise_scale: 8.0,
        noise_threshold: 0.5,
        noise_octaves: 4,
        noise_seed: 1,
        perlin_lo: 1,
        perlin_hi: 8,
        tree_depth: 3,
        tree_wildness: 0.35,
        tree_trunk: 0.45,
        tree_leaf: 1,
        tree_seed: 1,
        conifer_tiers: 3,
        conifer_ragged: 0.25,
        conifer_clump: 6.0,
        conifer_trunk: 0.4,
        conifer_crown: 0.5,
        conifer_needle: 1,
        conifer_seed: 1,
    };
    let cells = generate((0, 0, 0), (2, 2, 2), &gen);
    assert!(cells.contains(&((1, 1, 1), 4)));
    assert!(!cells.iter().any(|(cell, _)| *cell == (0, 0, 0)));
}

#[test]
fn flood_stops_at_a_different_color() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    cells.insert((1, 0, 0), 1);
    cells.insert((2, 0, 0), 2);
    let face = flood_face(&cells, (0, 0, 0), 2, 1, 1, false);
    assert_eq!(face.len(), 2);
}

#[test]
fn flood_empty_stays_on_the_plane_and_skips_solid_cells() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 0), 1);
    cells.insert((1, 0, 0), 1);
    let got = flood_empty(&cells, (0, 1, 0), 2, 0, 1, 0, 1, false);
    assert!(got.contains(&(0, 1, 0)));
    assert!(got.contains(&(1, 1, 0)));
    assert!(!got.contains(&(0, 0, 0)));
    assert_eq!(got.len(), 2);
}

#[test]
fn generator_fills_past_two_hundred_thousand_cells() {
    let mut gen = Gen::default();
    gen.palette = 4;
    // 60 × 60 × 56 = 201600, above the old cap and under 2_000_000.
    let cells = generate((0, 0, 0), (59, 59, 55), &gen);
    assert!(cells
        .iter()
        .any(|(cell, slot)| *cell == (30, 30, 28) && *slot == 4));
}

#[test]
fn generator_refuses_more_than_two_million_cells() {
    let gen = Gen::default();
    // 126³ = 2_000_376.
    assert!(generate((0, 0, 0), (125, 125, 125), &gen).is_empty());
}

fn tube_point(x: f64, y: f64, z: f64, radius: f64) -> TubePoint {
    TubePoint {
        at: [x, y, z],
        radius,
    }
}

#[test]
fn thin_straight_tube_is_one_cell_row() {
    let cells = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 0.5),
            tube_point(5.0, 0.0, 0.0, 0.5),
        ],
        false,
    )
    .unwrap();
    let mut got: Vec<Cell> = cells.iter().map(|c| c.cell).collect();
    got.sort();
    assert_eq!(got, (0..=5).map(|x| (x, 0, 0)).collect::<Vec<_>>());
    let end = cells.iter().find(|c| c.cell == (5, 0, 0)).unwrap();
    assert!((end.along - 5.0).abs() < 1e-9);
}

#[test]
fn tube_tapers_and_sloped_thin_tube_has_no_gaps() {
    let cells = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 3.0),
            tube_point(20.0, 0.0, 0.0, 0.5),
        ],
        false,
    )
    .unwrap();
    let width = |x: i32| {
        cells
            .iter()
            .filter(|c| c.cell.0 == x && c.cell.2 == 0)
            .count()
    };
    assert!(
        width(0) > width(10) && width(10) > width(19),
        "{} {} {}",
        width(0),
        width(10),
        width(19)
    );
    assert_eq!(width(20), 1);
    let sloped = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 0.5),
            tube_point(12.0, 5.0, 3.0, 0.5),
        ],
        false,
    )
    .unwrap();
    for x in 0..=12 {
        assert!(sloped.iter().any(|c| c.cell.0 == x), "gap at x {x}");
    }
}

#[test]
fn smooth_tube_passes_through_its_points() {
    let points = [
        tube_point(0.0, 0.0, 0.0, 1.0),
        tube_point(8.0, 6.0, 0.0, 1.0),
        tube_point(16.0, 0.0, 4.0, 1.0),
    ];
    let cells = tube(&points, true).unwrap();
    for point in &points {
        let at = (point.at[0] as i32, point.at[1] as i32, point.at[2] as i32);
        assert!(cells.iter().any(|c| c.cell == at), "{at:?}");
    }
    // A polyline corner sits on (8, 6, 0); the curve bends past it, so the
    // two differ.
    let straight = tube(&points, false).unwrap();
    assert_ne!(cells.len(), straight.len());
}

#[test]
fn tube_refuses_bad_input_before_work() {
    let one = tube(&[tube_point(0.0, 0.0, 0.0, 1.0)], false).unwrap_err();
    assert!(one.contains("at least 2"), "{one}");
    let thin = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 0.2),
            tube_point(3.0, 0.0, 0.0, 1.0),
        ],
        false,
    )
    .unwrap_err();
    assert!(thin.contains("point 0 radius"), "{thin}");
    let huge = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 128.0),
            tube_point(400.0, 0.0, 0.0, 128.0),
        ],
        false,
    )
    .unwrap_err();
    assert!(
        huge.contains("2000000") || huge.contains("too large"),
        "{huge}"
    );
}

#[test]
fn tube_stripes_go_round_the_axis() {
    let cells = tube(
        &[
            tube_point(0.0, 0.0, 0.0, 4.0),
            tube_point(10.0, 0.0, 0.0, 4.0),
        ],
        false,
    )
    .unwrap();
    let at = |cell: Cell| cells.iter().find(|c| c.cell == cell).unwrap().around;
    let up = at((5, 0, 3));
    let down = at((5, 0, -3));
    assert!(((up - down).abs() - 0.5).abs() < 1e-6, "{up} {down}");
}

#[test]
fn smooth_fills_a_pit_and_removes_a_spike_keeping_colours() {
    let mut cells = HashMap::new();
    for x in -3..=3 {
        for y in -3..=3 {
            for z in -2..=0 {
                cells.insert((x, y, z), if x < 0 { 2 } else { 5 });
            }
        }
    }
    cells.remove(&(1, 1, 0));
    cells.insert((-2, -2, 1), 7);
    let out = smooth(&cells, (-2, -2, 0), (2, 2, 1), 1, 17, 9);
    assert_eq!(out.filled, 1);
    assert_eq!(out.removed, 1);
    assert!(out.writes.contains(&(1, 1, 0, 5)), "{:?}", out.writes);
    assert!(out.writes.contains(&(-2, -2, 1, 0)), "{:?}", out.writes);
    // Flat faces and the slab edges outside the box stay.
    assert_eq!(out.writes.len(), 2);
    assert_eq!(out.iterations, 1);
    let again = smooth(&cells, (-2, -2, 0), (2, 2, 1), 8, 17, 9);
    assert_eq!(again.iterations, 1);
    let off = smooth(&cells, (-2, -2, 0), (2, 2, 1), 1, 27, -1);
    assert!(off.writes.is_empty());
}

#[test]
fn flat_tube_lies_flat_rolls_and_tapers_to_a_blade() {
    let ends = [
        tube_point(0.0, 0.0, 0.0, 4.0),
        tube_point(12.0, 0.0, 0.0, 4.0),
    ];
    let flat = TubeShape {
        aspect: 0.25,
        roll: 0.0,
        blade: false,
    };
    let cells = tube_shaped(&ends, false, flat).unwrap();
    let span = |cells: &[TubeCell], axis: usize| {
        let at = |c: &TubeCell| [c.cell.0, c.cell.1, c.cell.2][axis];
        let mid: Vec<_> = cells.iter().filter(|c| c.cell.0 == 6).collect();
        (
            mid.iter().map(|c| at(c)).min().unwrap(),
            mid.iter().map(|c| at(c)).max().unwrap(),
        )
    };
    assert_eq!(span(&cells, 1), (-4, 4));
    assert_eq!(span(&cells, 2), (-1, 1));
    let rolled = tube_shaped(
        &ends,
        false,
        TubeShape {
            roll: 90f64.to_radians(),
            ..flat
        },
    )
    .unwrap();
    assert_eq!(span(&rolled, 1), (-1, 1));
    assert_eq!(span(&rolled, 2), (-4, 4));
    let blade = tube_shaped(
        &ends,
        false,
        TubeShape {
            aspect: 0.75,
            roll: 0.0,
            blade: true,
        },
    )
    .unwrap();
    let thick = |y: i32| {
        blade
            .iter()
            .filter(|c| c.cell.0 == 6 && c.cell.1 == y)
            .count()
    };
    // The binormal of a tube along +X is -Y, so the thick edge is at +Y.
    assert!(thick(3) > thick(-3), "{} {}", thick(3), thick(-3));
    assert_eq!(thick(-3), 1);
    assert!(tube_shaped(
        &ends,
        false,
        TubeShape {
            aspect: 0.0,
            ..flat
        }
    )
    .is_err());
}

fn log(radius: i32, length: i32, slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for x in 0..length {
        for y in -radius..=radius {
            for z in -radius..=radius {
                if y * y + z * z <= radius * radius {
                    cells.insert((x, y, z), slot);
                }
            }
        }
    }
    cells
}

fn break_spec() -> BreakSpec {
    BreakSpec {
        point: [20.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
        depth: 3.0,
        splinters: Splinters::Count(20),
        length: (4.0, 10.0),
        thickness: 1.0,
        taper: 1.0,
        rim_bias: 0.6,
        bark: Some(2),
        sapwood: vec![3, 4],
        tips: Some(5),
        seed: 7,
    }
}

#[test]
fn break_end_cuts_a_ragged_face_and_grows_splinters() {
    let cells = log(5, 30, 1);
    let spec = break_spec();
    let broken = break_end(&cells, &spec).unwrap();
    assert_eq!(broken, break_end(&cells, &spec).unwrap(), "deterministic");
    let mut after = cells.clone();
    for &(x, y, z, slot) in &broken.writes {
        if slot == 0 {
            after.remove(&(x, y, z));
        } else {
            after.insert((x, y, z), slot as u8);
        }
    }
    // Wood behind the ragged band is untouched; the stump keeps its length.
    for (cell, slot) in &cells {
        if cell.0 < 16 {
            assert_eq!(after.get(cell), Some(slot), "{cell:?}");
        }
    }
    assert!(broken.removed > 0 && broken.splinters == 20);
    // The face is uneven: its depth varies across the section.
    let face: std::collections::HashSet<i32> = (-4..=4)
        .flat_map(|y| (-4..=4).map(move |z| (y, z)))
        .map(|(y, z)| (16..40).find(|x| !after.contains_key(&(*x, y, z))).unwrap())
        .collect();
    assert!(face.len() > 1, "{face:?}");
    // Splinters reach past the plane in sapwood and tip colours.
    assert!(after.iter().any(|(c, s)| c.0 > 23 && *s == 5));
    assert!(after.values().any(|s| *s == 3 || *s == 4));
    // The rim of the face is bark.
    assert!(after
        .iter()
        .any(|(c, s)| c.0 >= 16 && *s == 2 && c.1 * c.1 + c.2 * c.2 >= 16));
}

#[test]
fn break_end_refuses_bad_input() {
    let cells = log(3, 10, 1);
    let mut spec = break_spec();
    spec.normal = [0.0; 3];
    assert!(break_end(&cells, &spec).unwrap_err().contains("normal"));
    let mut spec = break_spec();
    spec.length = (8.0, 4.0);
    assert!(break_end(&cells, &spec).unwrap_err().contains("length"));
    let mut spec = break_spec();
    spec.splinters = Splinters::Count(600);
    assert!(break_end(&cells, &spec).unwrap_err().contains("splinters"));
}

fn scatter_spec() -> ScatterSpec {
    ScatterSpec {
        min: (0, 0, 1),
        max: (9, 9, 30),
        count: 40,
        kinds: PieceKind::ALL.to_vec(),
        size: (1, 3),
        palettes: vec![4, 5],
        seed: 9,
        floor: None,
        clump: 0.5,
        exclude: Exclude::default(),
    }
}

#[test]
fn scatter_skips_pieces_that_would_rest_in_an_excluded_box() {
    let mut ground = HashMap::new();
    for x in -5..15 {
        for y in -5..15 {
            ground.insert((x, y, 0), 1u8);
        }
    }
    let free = scatter(&ground, &scatter_spec()).unwrap();
    let mut spec = scatter_spec();
    spec.exclude = Exclude(vec![Region::spanning((0, 0, 1), (4, 9, 3))]);
    let kept = scatter(&ground, &spec).unwrap();
    assert!(
        kept.skipped > free.skipped,
        "{} vs {}",
        kept.skipped,
        free.skipped
    );
    assert!(kept.placed > 0);
    for &(x, y, z, _) in &kept.writes {
        assert!(
            !spec.exclude.contains((x, y, z)),
            "a piece landed in ({x}, {y}, {z})"
        );
    }
    assert_eq!(kept, scatter(&ground, &spec).unwrap());
}

#[test]
fn scatter_settles_pieces_into_a_heap_on_the_floor() {
    let mut ground = HashMap::new();
    for x in -5..15 {
        for y in -5..15 {
            ground.insert((x, y, 0), 1u8);
        }
    }
    let spec = scatter_spec();
    let done = scatter(&ground, &spec).unwrap();
    assert_eq!(done, scatter(&ground, &spec).unwrap(), "deterministic");
    assert_eq!(done.placed + done.skipped, 40);
    assert!(done.placed > 30, "{}", done.placed);
    let added: HashMap<Cell, u8> = done
        .writes
        .iter()
        .map(|&(x, y, z, slot)| ((x, y, z), slot as u8))
        .collect();
    assert!(added.keys().all(|cell| !ground.contains_key(cell)));
    assert!(added.values().all(|slot| *slot == 4 || *slot == 5));
    // Every piece rests: some cell of the pile sits on the ground, and no
    // cell floats with nothing at all under its column.
    assert!(added.keys().any(|c| c.2 == 1));
    for cell in added.keys() {
        let below = (cell.2 - 1..cell.2).any(|z| {
            ground.contains_key(&(cell.0, cell.1, z)) || added.contains_key(&(cell.0, cell.1, z))
        });
        let any_under = (0..cell.2).any(|z| {
            ground.contains_key(&(cell.0, cell.1, z)) || added.contains_key(&(cell.0, cell.1, z))
        });
        assert!(below || any_under || cell.2 == 1, "{cell:?} floats");
    }
    // A pile: it stacks higher than one piece in the middle.
    assert!(added.keys().map(|c| c.2).max().unwrap() > 3);
}

#[test]
fn scatter_lands_on_the_floor_without_ground_and_refuses_bad_input() {
    let mut spec = scatter_spec();
    spec.floor = Some(5);
    spec.kinds = vec![PieceKind::Pebble];
    let done = scatter(&HashMap::new(), &spec).unwrap();
    assert!(done.writes.iter().all(|w| w.2 >= 5));
    assert!(done.writes.iter().any(|w| w.2 == 5));
    let mut bad = scatter_spec();
    bad.size = (0, 3);
    assert!(scatter(&HashMap::new(), &bad).unwrap_err().contains("size"));
    let mut bad = scatter_spec();
    bad.palettes = vec![];
    assert!(scatter(&HashMap::new(), &bad)
        .unwrap_err()
        .contains("palettes"));
    let mut bad = scatter_spec();
    bad.count = 5000;
    assert!(scatter(&HashMap::new(), &bad)
        .unwrap_err()
        .contains("count"));
}
