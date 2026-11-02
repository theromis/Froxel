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
