use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

/// A dome (half ellipsoid 12 x 12 x 8 over z 0..8, slot 3), a blob (a ball
/// of radius 3.5, slot 5) sitting on its top, and floaters: three single
/// cells and a 2 x 2 x 2 cube (slot 7).
fn scene() -> (HashSet<Cell>, HashSet<Cell>, HashSet<Cell>) {
    let mut dome = HashSet::new();
    for z in 0..=8 {
        for y in -12..=12 {
            for x in -12..=12 {
                let (fx, fy, fz) = (x as f64 / 12.5, y as f64 / 12.5, z as f64 / 8.5);
                if fx * fx + fy * fy + fz * fz <= 1.0 {
                    dome.insert((x, y, z));
                }
            }
        }
    }
    let mut blob = HashSet::new();
    for z in 7..=15 {
        for y in -4..=4 {
            for x in -4..=4 {
                let d = ((x * x + y * y) as f64 + (z as f64 - 11.0).powi(2)).sqrt();
                if d <= 3.5 && !dome.contains(&(x, y, z)) {
                    blob.insert((x, y, z));
                }
            }
        }
    }
    let mut floaters: HashSet<Cell> = [(20, 0, 0), (-20, 5, 3), (0, 20, 10)].into();
    for (x, y, z) in [(15, 15, 0), (16, 15, 0), (15, 16, 0), (16, 16, 0)] {
        floaters.insert((x, y, z));
        floaters.insert((x, y, z + 1));
    }
    (dome, blob, floaters)
}

fn cells_of(parts: &[(&HashSet<Cell>, u8)]) -> HashMap<Cell, u8> {
    let mut out = HashMap::new();
    for (part, slot) in parts {
        for c in *part {
            out.insert(*c, *slot);
        }
    }
    out
}

fn session_with(cells: HashMap<Cell, u8>) -> Session {
    let mut session = Session::new();
    session.replace_empty();
    let writes: Vec<_> = cells
        .iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(*s)))
        .collect();
    session.write_cells(&writes).unwrap();
    session
}

fn spec(op: ComponentOp) -> ComponentSpec {
    ComponentSpec {
        op,
        connectivity: 26,
        seed: None,
        anchor: None,
        reach: 8.0,
        radius: None,
    }
}

#[test]
fn neighbours_are_symmetric_and_counted() {
    for (k, n) in [(6, 6), (18, 18), (26, 26)] {
        let offsets = neighbours(k).unwrap();
        assert_eq!(offsets.len(), n);
        for (i, o) in offsets.iter().enumerate() {
            assert_eq!(offsets[n - 1 - i], (-o.0, -o.1, -o.2));
        }
    }
    assert!(neighbours(8).is_err());
}

#[test]
fn list_counts_parts_largest_first() {
    let (dome, blob, floaters) = scene();
    let cells = cells_of(&[(&dome, 3), (&blob, 5), (&floaters, 7)]);
    let found = components(&cells, &spec(ComponentOp::List)).unwrap();
    // Dome and blob touch; three single cells and one cube float.
    assert_eq!(found.components.len(), 5);
    assert_eq!(found.components[0].cells, dome.len() + blob.len());
    assert_eq!(found.components[1].cells, 8);
    assert!(found.writes.is_empty());
    // Corner neighbours join cells that 6-connectivity keeps apart.
    let diagonal: HashMap<Cell, u8> = [((0, 0, 0), 1), ((1, 1, 1), 1)].into();
    let mut six = spec(ComponentOp::List);
    six.connectivity = 6;
    assert_eq!(components(&diagonal, &six).unwrap().components.len(), 2);
    assert_eq!(
        components(&diagonal, &spec(ComponentOp::List))
            .unwrap()
            .components
            .len(),
        1
    );
}

#[test]
fn remove_small_and_keep_largest_clear_the_floaters() {
    let (dome, blob, floaters) = scene();
    let mut session = session_with(cells_of(&[(&dome, 3), (&blob, 5), (&floaters, 7)]));
    let reply = apply(
        &mut session,
        "voxel.components",
        &json!({"op": "remove_small", "minCells": 9}),
    )
    .unwrap();
    assert_eq!(reply["count"], 5);
    assert_eq!(reply["changed"], floaters.len());
    assert_eq!(session.active().cells.len(), dome.len() + blob.len());
    session.undo();
    assert_eq!(
        session.active().cells.len(),
        dome.len() + blob.len() + floaters.len()
    );
    let reply = apply(
        &mut session,
        "voxel.components",
        &json!({"op": "keep_largest"}),
    )
    .unwrap();
    assert_eq!(reply["changed"], floaters.len());
    let reply = apply(&mut session, "voxel.components", &json!({})).unwrap();
    assert_eq!(reply["count"], 1);
}

#[test]
fn a_rider_is_cut_off_at_its_neck_and_the_dome_comes_out_clean() {
    let (dome, blob, floaters) = scene();
    let mut session = session_with(cells_of(&[(&dome, 3), (&blob, 5), (&floaters, 7)]));
    apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [
            {"tool": "components", "arguments": {"op": "remove_small", "minCells": 9}},
            {"tool": "components", "arguments": {"op": "delete", "seed": [0, 0, 12], "anchor": [0, 0, 2]}}
        ]}),
    )
    .unwrap();
    let left: HashSet<Cell> = session.active().cells.keys().copied().collect();
    // Every dome cell stays; at most the neck's cells of the blob stay.
    assert!(dome.is_subset(&left), "the cut took dome cells");
    let extra = left.difference(&dome).count();
    let reply = apply(&mut session, "voxel.components", &json!({"op": "list"})).unwrap();
    assert_eq!(reply["count"], 1);
    assert!(extra <= 21, "{extra} blob cells stayed");
    assert!(blob.len() > 150);
}

#[test]
fn the_neck_cut_is_the_thinnest_cross_section() {
    // Two 7 x 7 x 7 cubes joined by a 2 x 2 bar four cells long: the cut
    // is the bar's cross-section, 4 cells, whatever connectivity.
    let mut cells = HashMap::new();
    for x in 0..7 {
        for y in 0..7 {
            for z in 0..7 {
                cells.insert((x, y, z), 1u8);
                cells.insert((x + 11, y, z), 2u8);
            }
        }
    }
    for x in 7..11 {
        for (y, z) in [(3, 3), (3, 4), (4, 3), (4, 4)] {
            cells.insert((x, y, z), 3u8);
        }
    }
    for connectivity in [6, 26] {
        let mut s = spec(ComponentOp::Delete);
        s.connectivity = connectivity;
        s.seed = Some([14.0, 3.0, 3.0]);
        s.anchor = Some([3.0, 3.0, 3.0]);
        let found = components(&cells, &s).unwrap();
        assert_eq!(found.cut, 4, "connectivity {connectivity}");
        // The seed's cube and the bar beyond the cut go; the anchor's cube
        // stays whole.
        assert!(found.selected.iter().all(|c| c.0 >= 7));
        assert!(found.selected.len() >= 343 + 12);
        assert!(found.selected.len() <= 343 + 16);
    }
}

#[test]
fn split_and_move_take_the_seed_part() {
    let (dome, blob, floaters) = scene();
    let mut session = session_with(cells_of(&[(&dome, 3), (&blob, 5), (&floaters, 7)]));
    let reply = apply(
        &mut session,
        "voxel.components",
        &json!({"op": "split", "seed": [15, 15, 0], "name": "cube"}),
    )
    .unwrap();
    assert_eq!(reply["selected"], 8);
    assert_eq!(reply["created"], true);
    assert_eq!(session.objects[1].name, "cube");
    assert_eq!(session.objects[1].cells.len(), 8);
    let reply = apply(
        &mut session,
        "voxel.components",
        &json!({"op": "move", "seed": [20, 0, 0], "offset": [0, 0, 5]}),
    )
    .unwrap();
    assert_eq!(reply["selected"], 1);
    assert!(session.active().cells.contains_key(&(20, 0, 5)));
    assert!(!session.active().cells.contains_key(&(20, 0, 0)));
    // A seed with no solid cell in reach is refused.
    let err = apply(
        &mut session,
        "voxel.components",
        &json!({"op": "delete", "seed": [60, 60, 60]}),
    )
    .unwrap_err();
    assert!(err.contains("no solid cell"), "{err}");
}

#[test]
fn rhai_components_runs_the_same_code_path() {
    let mut session = Session::new();
    session.replace_empty();
    let source = r#"
fill_ellipsoid(0, 0, 0, 10, 10, 10, 3);
set_voxels([[20, 0, 0, 4], [20, 20, 0, 4]]);
let listed = components("list");
let cleaned = components("remove_small", #{minCells: 2});
fill_box(12, 4, 4, 13, 5, 5, 6);
components("split", #{seed: [13, 5, 5], name: "nub", connectivity: 6});
"#;
    session.set_script(0, source).unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.objects[0].cells;
    assert!(!cells.contains_key(&(20, 0, 0)));
    assert!(!cells.contains_key(&(12, 4, 4)));
    let nub = session
        .objects
        .iter()
        .find(|o| o.name == "nub")
        .expect("split made nub");
    assert_eq!(nub.cells.len(), 8);
}

#[test]
fn the_cleaned_blockout_example_stands_the_boulder_up_and_cuts_the_snail_off() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/cleaned_blockout.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let names: Vec<&str> = session.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names[1..], ["snail"], "{names:?}");
    let boulder = &session.objects[0].cells;
    let snail = &session.objects[1].cells;
    // No foam stays, the boulder is one part, and the snail is mostly shell.
    assert!(boulder.values().all(|slot| *slot != 5));
    let parts = components(boulder, &spec(ComponentOp::List)).unwrap();
    assert_eq!(parts.components.len(), 1);
    let shell = snail.values().filter(|slot| **slot == 4).count();
    assert!(shell * 10 >= snail.len() * 9, "{shell} of {}", snail.len());
    assert!(boulder.values().filter(|slot| **slot == 4).count() <= 24);
    // orient_to_views stood the boulder up: its moss is on top.
    let mean_z = |slot: u8| {
        let zs: Vec<f64> = boulder
            .iter()
            .filter(|(_, s)| **s == slot)
            .map(|(c, _)| f64::from(c.2))
            .collect();
        zs.iter().sum::<f64>() / zs.len() as f64
    };
    assert!(mean_z(3) > mean_z(2) + 3.0);
    let first: Vec<usize> = session.objects.iter().map(|o| o.cells.len()).collect();
    session.regenerate_script(0).unwrap();
    let again: Vec<usize> = session.objects.iter().map(|o| o.cells.len()).collect();
    assert_eq!(again, first);
    if let Ok(dir) = std::env::var("FROXEL_EXAMPLE_OUT") {
        session
            .write_vox(&std::path::Path::new(&dir).join("cleaned_blockout.vox"))
            .unwrap();
    }
}

#[test]
fn the_components_panel_runs_the_tool() {
    let (dome, blob, floaters) = scene();
    let mut session = session_with(cells_of(&[(&dome, 3), (&blob, 5), (&floaters, 7)]));
    session.edit.components.min_cells = 9;
    session.components_from_panel("remove_small");
    assert_eq!(
        session.active().cells.len(),
        dome.len() + blob.len(),
        "{}",
        session.status
    );
    session.edit.components.seed = "0 0 12".into();
    session.edit.components.anchor = "0 0 2".into();
    session.edit.components.name = "rider".into();
    session.components_from_panel("split");
    assert_eq!(session.objects.len(), 2, "{}", session.status);
    assert!(session.status.contains("Neck cut"), "{}", session.status);
}
