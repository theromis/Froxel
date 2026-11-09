use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

/// A slab 12 x 12 x 3 whose top layer is noisy orange in slots 10..=17,
/// with a 2 x 2 black eye (slot 20), a one-cell line (slot 21), and one
/// lone blue speck (slot 22).
fn slab() -> (HashMap<Cell, u8>, [[u8; 3]; 256]) {
    let mut palette = [[128u8; 3]; 256];
    for (i, slot) in (10..=17).enumerate() {
        let i = i as u8;
        palette[slot] = [220 - i * 3, 120 + i * 2, 30 + i];
    }
    palette[20] = [15, 10, 8];
    palette[21] = [240, 240, 235];
    palette[22] = [30, 60, 220];
    let mut cells = HashMap::new();
    for z in 0..3 {
        for y in 0..12 {
            for x in 0..12 {
                let slot = 10 + ((x * 7 + y * 3 + z) % 8) as u8;
                cells.insert((x, y, z), slot);
            }
        }
    }
    for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
        cells.insert((x, y, 2), 20);
    }
    for x in 4..10 {
        cells.insert((x, 8, 2), 21);
    }
    cells.insert((8, 4, 2), 22);
    (cells, palette)
}

fn shades(palette: &[[u8; 3]; 256], cells: &HashMap<Cell, u8>) -> Shades {
    let mut used = [false; 256];
    used[0] = true;
    for s in cells.values() {
        used[*s as usize] = true;
    }
    Shades {
        colors: std::array::from_fn(|i| [palette[i][0], palette[i][1], palette[i][2], 255]),
        materials: [Default::default(); 256],
        used,
        created: Vec::new(),
    }
}

fn spec(reduce: Reduce, despeckle: u32) -> ReduceSpec {
    ReduceSpec {
        reduce,
        guard: Some(super::super::colors::Guard::default()),
        despeckle,
        min_same: 2,
        protect: Vec::new(),
        region: None,
    }
}

fn apply_writes(cells: &HashMap<Cell, u8>, done: &Reduced) -> HashMap<Cell, u8> {
    let mut out = cells.clone();
    for (c, s) in &done.writes {
        out.insert(*c, *s);
    }
    out
}

#[test]
fn clusters_take_the_orange_noise_to_one_slot_and_keep_the_eye() {
    let (cells, palette) = slab();
    let mut exclusive = [false; 256];
    for s in [10, 11, 12, 13, 14, 15, 16, 17, 20, 21, 22] {
        exclusive[s] = true;
    }
    let mut sh = shades(&palette, &cells);
    let done = reduce_palette(
        &cells,
        &spec(Reduce::Clusters(1), 0),
        &palette,
        &mut sh,
        &exclusive,
    )
    .unwrap();
    assert_eq!(done.slots[0], 11);
    let after = apply_writes(&cells, &done);
    // One orange slot for every noisy cell.
    let orange: std::collections::HashSet<u8> = (0..12).map(|x| after[&(x, 11, 1)]).collect();
    assert_eq!(orange.len(), 1);
    // The eye, the line, and the speck are distinct enough to be accents.
    assert!(done.accents >= 2, "{}", done.accents);
    assert_ne!(after[&(2, 2, 2)], after[&(0, 0, 2)]);
    // Clusters took over existing slots in place, so no free slot was used.
    assert!(sh.created.is_empty());
    assert!(!done.recoloured.is_empty());
}

#[test]
fn despeckle_removes_a_lone_speck_but_keeps_a_patch_and_a_line() {
    let (mut cells, palette) = slab();
    // Make the top plain orange so only the marks differ.
    for v in cells.values_mut() {
        if (10..=17).contains(v) {
            *v = 10;
        }
    }
    let mut sh = shades(&palette, &cells);
    let done = reduce_palette(
        &cells,
        &spec(Reduce::Keep, 2),
        &palette,
        &mut sh,
        &[false; 256],
    )
    .unwrap();
    let after = apply_writes(&cells, &done);
    assert_eq!(after[&(8, 4, 2)], 10, "the speck goes");
    assert_eq!(after[&(2, 2, 2)], 20, "the eye stays");
    assert_eq!(after[&(6, 8, 2)], 21, "the line stays");
    assert!(done.despeckled >= 1);
    // protect keeps the speck too.
    let mut protected = spec(Reduce::Keep, 2);
    protected.protect = vec![22];
    let done = reduce_palette(&cells, &protected, &palette, &mut sh, &[false; 256]).unwrap();
    assert!(done.writes.iter().all(|(c, _)| *c != (8, 4, 2)));
}

#[test]
fn snap_takes_the_nearest_listed_colour() {
    let (cells, palette) = slab();
    let mut sh = shades(&palette, &cells);
    let done = reduce_palette(
        &cells,
        &spec(
            Reduce::Snap(vec![SnapTo::Rgb([230, 128, 32]), SnapTo::Slot(20)]),
            0,
        ),
        &palette,
        &mut sh,
        &[false; 256],
    )
    .unwrap();
    let after = apply_writes(&cells, &done);
    let mut slots: Vec<u8> = after.values().copied().collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots.len(), 2, "{slots:?}");
    assert_eq!(sh.created.len(), 1);
    assert_eq!(after[&(2, 2, 2)], 20);
}

#[test]
fn mcp_reduces_in_one_undo_step_and_leaves_other_objects_slots() {
    let mut session = Session::new();
    session.replace_empty();
    let (cells, palette) = slab();
    for (slot, rgb) in palette.iter().enumerate().skip(1) {
        session.palette[slot] = [rgb[0], rgb[1], rgb[2], 255];
    }
    let writes: Vec<(i32, i32, i32, i32)> = cells
        .iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(*s)))
        .collect();
    session.write_cells(&writes).unwrap();
    // Another object uses slot 10, so slot 10 may not be recoloured.
    apply(
        &mut session,
        "object.add",
        &json!({"name": "other", "position": [40, 0, 0]}),
    )
    .unwrap();
    session.write_cells(&[(0, 0, 0, 10)]).unwrap();
    session.select_object(0);
    let before10 = session.palette[10];
    let reply = apply(
        &mut session,
        "voxel.reduce_palette",
        &json!({"colors": 2, "despeckle": 1}),
    )
    .unwrap();
    assert!(reply["changed"].as_i64().unwrap() > 0, "{reply}");
    assert!(reply["slots"]["after"].as_u64().unwrap() < reply["slots"]["before"].as_u64().unwrap());
    assert_eq!(session.palette[10], before10);
    session.undo();
    assert_eq!(session.objects[0].cells, cells);
}

#[test]
fn import_mesh_clean_reduces_the_imported_colours() {
    let path = format!(
        "{}/tests/fixtures/mesh/quad.gltf",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut plain = Session::new();
    plain.replace_empty();
    let raw = apply(
        &mut plain,
        "mesh.import",
        &json!({"path": path, "size": 16}),
    )
    .unwrap();
    let mut session = Session::new();
    session.replace_empty();
    let reply = apply(
        &mut session,
        "mesh.import",
        &json!({"path": path, "size": 16, "clean": {"colors": 1, "accents": false}}),
    )
    .unwrap();
    assert!(raw["slots"].as_array().unwrap().len() > 1, "{raw}");
    assert_eq!(reply["slots"].as_array().unwrap().len(), 1, "{reply}");
    assert!(reply["clean"]["slots"]["before"].as_u64().unwrap() > 1);
}

#[test]
fn the_speckled_pebble_example_keeps_the_spot_and_drops_the_specks() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/speckled_pebble.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.objects[0].cells;
    let mut slots: Vec<u8> = cells.values().copied().collect();
    slots.sort_unstable();
    slots.dedup();
    // Three clusters and the dark spot; the blue specks are gone.
    assert!(slots.len() <= 5, "{slots:?}");
    let dark = cells
        .values()
        .filter(|s| {
            let c = session.palette[**s as usize];
            c[0] < 60 && c[1] < 60 && c[2] < 60
        })
        .count();
    assert!(dark >= 6, "{dark}");
    let blue = cells
        .values()
        .filter(|s| {
            let c = session.palette[**s as usize];
            c[2] > 150 && c[0] < 100
        })
        .count();
    assert_eq!(blue, 0);
}
