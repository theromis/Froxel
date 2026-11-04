use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::state::{Cell, Session};

const LEAVES: [u8; 4] = [3, 4, 5, 6];
const CORE: u8 = 2;
const WOOD: u8 = 1;

fn crown_spec() -> FoliageSpec {
    FoliageSpec {
        region: Region::new((-24, -24, 0), (24, 24, 34), "foliage").unwrap(),
        shapes: vec![CrownShape::Ellipsoid {
            center: [0.0, 0.0, 16.0],
            radii: [14.0, 14.0, 9.0],
        }],
        from: Vec::new(),
        palettes: LEAVES.to_vec(),
        core: CORE,
        size: (3.0, 5.0),
        gap: 1.0,
        flatten: 0.7,
        ragged: 0.4,
        light: 1.0,
        dark: 1.0,
        sun: [-0.35, -0.45, 1.0],
        masses: 0,
        groove: 2,
        protect: Vec::new(),
        exclude: Exclude::default(),
        seed: 7,
    }
}

/// A trunk of wood up through the middle of the crown.
fn trunk() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in 0..=22 {
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            cells.insert((x, y, z), WOOD);
        }
    }
    cells
}

fn apply(cells: &mut HashMap<Cell, u8>, writes: &[(i32, i32, i32, i32)]) {
    for &(x, y, z, slot) in writes {
        if slot == 0 {
            cells.remove(&(x, y, z));
        } else {
            cells.insert((x, y, z), slot as u8);
        }
    }
}

fn tone(slot: u8) -> Option<usize> {
    LEAVES.iter().position(|s| *s == slot)
}

fn exposed(cells: &HashMap<Cell, u8>, c: Cell) -> bool {
    [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ]
    .iter()
    .any(|d| !cells.contains_key(&(c.0 + d.0, c.1 + d.1, c.2 + d.2)))
}

#[test]
fn a_crown_is_separate_clumps_lit_on_top_with_a_dark_core_in_the_gaps() {
    let mut cells = HashMap::new();
    let done = foliage(&cells, &crown_spec()).unwrap();
    assert!(done.clumps >= 20, "{} clumps", done.clumps);
    assert_eq!(done.added, done.writes.len());
    apply(&mut cells, &done.writes);
    // The middle is solid core: nothing is see-through.
    assert_eq!(cells.get(&(0, 0, 16)), Some(&CORE));
    for z in 10..=22 {
        assert!(cells.contains_key(&(0, 0, z)), "hole at z {z}");
    }
    // The gaps between clumps show the dark core.
    let gaps = cells
        .iter()
        .filter(|(c, s)| **s == CORE && exposed(&cells, **c))
        .count();
    assert!(gaps > 20, "{gaps} core cells in the gaps");
    // Clumps bulge out of the volume: the outline is not the ellipsoid.
    assert!(cells.keys().any(|c| c.2 > 25), "no clump above the volume");
    // Tops are lighter than undersides, and every tone is used.
    let mut up = Vec::new();
    let mut down = Vec::new();
    for (c, slot) in &cells {
        let Some(t) = tone(*slot) else { continue };
        if !cells.contains_key(&(c.0, c.1, c.2 + 1)) {
            up.push(t as f64);
        }
        if !cells.contains_key(&(c.0, c.1, c.2 - 1)) {
            down.push(t as f64);
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    assert!(
        mean(&up) > mean(&down) + 1.0,
        "tops {} undersides {}",
        mean(&up),
        mean(&down)
    );
    for slot in LEAVES {
        assert!(cells.values().any(|s| *s == slot), "slot {slot} unused");
    }
}

#[test]
fn the_wood_stays_whole_and_the_same_seed_builds_the_same_crown() {
    let mut cells = trunk();
    let wood = cells.clone();
    let done = foliage(&cells, &crown_spec()).unwrap();
    assert_eq!(done.writes, foliage(&cells, &crown_spec()).unwrap().writes);
    let mut other = crown_spec();
    other.seed = 8;
    assert_ne!(done.writes, foliage(&cells, &other).unwrap().writes);
    apply(&mut cells, &done.writes);
    for (c, slot) in &wood {
        assert_eq!(cells.get(c), Some(slot), "wood at {c:?} changed");
    }
    // The leaves wrap the trunk with core, not a second surface of clumps.
    assert_eq!(cells.get(&(2, 0, 16)), Some(&CORE));
}

#[test]
fn from_reclusters_an_existing_blob_and_exclude_keeps_a_space_open() {
    let blob = 9;
    let mut cells = HashMap::new();
    for z in 6..=26 {
        for y in -12..=12 {
            for x in -12..=12 {
                let d = (f64::from(x) / 12.0).powi(2)
                    + (f64::from(y) / 12.0).powi(2)
                    + (f64::from(z - 16) / 10.0).powi(2);
                if d <= 1.0 {
                    cells.insert((x, y, z), blob);
                }
            }
        }
    }
    let mut spec = crown_spec();
    spec.shapes.clear();
    spec.from = vec![blob];
    spec.masses = 3;
    let open = Region::spanning((-3, -20, 0), (3, -6, 14));
    spec.exclude = Exclude(vec![open]);
    let done = foliage(&cells, &spec).unwrap();
    assert_eq!(done.masses, 3);
    assert!(done.removed > 0 && done.recoloured > 0, "{done:?}");
    let before = cells.clone();
    apply(&mut cells, &done.writes);
    for (c, slot) in &cells {
        if open.contains(*c) {
            assert_eq!(before.get(c), Some(slot), "{c:?} written in exclude");
        } else {
            assert_ne!(*slot, blob, "{c:?} kept the old slot");
        }
    }
}

#[test]
fn protect_keeps_a_crown_slot_and_bad_values_are_refused() {
    let mut cells = HashMap::new();
    cells.insert((0, 0, 25), 5);
    let mut spec = crown_spec();
    spec.protect = vec![5];
    let done = foliage(&cells, &spec).unwrap();
    assert!(done.writes.iter().all(|w| (w.0, w.1, w.2) != (0, 0, 25)));

    let mut spec = crown_spec();
    spec.shapes.clear();
    assert!(foliage(&cells, &spec).unwrap_err().contains("volume"));
    let mut spec = crown_spec();
    spec.size = (6.0, 3.0);
    assert!(foliage(&cells, &spec).unwrap_err().contains("size"));
    let mut spec = crown_spec();
    spec.palettes.clear();
    assert!(foliage(&cells, &spec).unwrap_err().contains("palettes"));
    let mut spec = crown_spec();
    spec.sun = [0.0; 3];
    assert!(foliage(&cells, &spec).unwrap_err().contains("sun"));
}

#[test]
fn mcp_rhai_and_the_panel_build_the_same_crown() {
    let args = json!({
        "palettes": [3, 4, 5, 6], "core": 2, "ellipsoids": [[0, 0, 16, 12, 12, 8]],
        "size": [3, 5], "masses": 2, "seed": 4
    });
    let mut mcp = Session::new();
    mcp.replace_empty();
    let reply = crate::remote::apply(&mut mcp, "voxel.foliage", &args).unwrap();
    assert!(reply["clumps"].as_u64().unwrap() > 10, "{reply}");
    assert_eq!(reply["masses"], 2);
    // Without a box the crown bounds itself and reaches the volume's top.
    let top = mcp.active().cells.keys().map(|c| c.2).max().unwrap();
    assert!(top >= 24, "top {top}");

    let mut script = Session::new();
    script.replace_empty();
    script
        .set_script(
            0,
            r#"
            let r = foliage([3, 4, 5, 6], #{
                core: 2, ellipsoids: [[0, 0, 16, 12, 12, 8]], size: [3, 5], masses: 2, seed: 4
            });
            if r.clumps < 10 { throw "few clumps"; }
            "#,
        )
        .unwrap();
    script.regenerate_script(0).unwrap();
    assert_eq!(script.active().cells, mcp.active().cells);

    let mut panel = Session::new();
    panel.replace_empty();
    let fields = &mut panel.stone.foliage;
    fields.ellipsoids = "0 0 16 12 12 8".into();
    fields.palettes = "3 4 5 6".into();
    fields.core = 2;
    fields.size = [3.0, 5.0];
    fields.masses = 2;
    fields.seed = 4;
    panel.foliage_from_panel();
    assert!(panel.status.starts_with("Foliage:"), "{}", panel.status);
    assert_eq!(panel.active().cells, mcp.active().cells);
    panel.undo();
    assert!(panel.active().cells.is_empty(), "one undo step");

    // A box keeps the writes inside it.
    let mut boxed = args.clone();
    for (key, value) in ["x0", "y0", "z0", "x1", "y1", "z1"]
        .into_iter()
        .zip([-20, -20, 0, 20, 20, 20])
    {
        boxed[key] = json!(value);
    }
    let mut clipped = Session::new();
    clipped.replace_empty();
    crate::remote::apply(&mut clipped, "voxel.foliage", &boxed).unwrap();
    assert!(clipped.active().cells.keys().all(|c| c.2 <= 20));
    let err =
        crate::remote::apply(&mut clipped, "voxel.foliage", &json!({"palettes": [3]})).unwrap_err();
    assert!(err.contains("volume"), "{err}");
}

#[test]
fn the_clustered_tree_example_keeps_its_wood_and_sways_its_leaves() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/clustered_tree.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = session.active().cells.clone();
    for slot in 1..=7 {
        assert!(cells.values().any(|s| *s == slot), "slot {slot} is missing");
    }
    for slot in 3..=7 {
        assert_ne!(
            session.materials[slot].motion,
            crate::state::Motion::None,
            "slot {slot} does not sway"
        );
    }
    assert_eq!(session.materials[1].motion, crate::state::Motion::None);
    session.regenerate_script(0).unwrap();
    assert_eq!(
        session.active().cells,
        cells,
        "the same seed, the same tree"
    );
    if let Ok(dir) = std::env::var("FROXEL_EXAMPLE_OUT") {
        session
            .write_vox(&std::path::Path::new(&dir).join("clustered_tree.vox"))
            .unwrap();
    }
}
