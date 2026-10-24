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
