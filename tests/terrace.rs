use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;

/// A voxelized ellipsoid dome, 30 x 30 cells across and 9 tall, gentle
/// enough that its slopes come out as one-cell terraces.
fn dome() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in 0..=9 {
        for y in -15..=15 {
            for x in -15..=15 {
                let (fx, fy, fz) = (
                    (x as f64 + 0.5) / 15.5,
                    (y as f64 + 0.5) / 15.5,
                    (z as f64 + 0.5) / 9.5,
                );
                if fx * fx + fy * fy + fz * fz <= 1.0 {
                    cells.insert((x, y, z), if z > 6 { 3 } else { 2 });
                }
            }
        }
    }
    cells
}

fn spec(mode: TerraceMode) -> TerraceSpec {
    TerraceSpec {
        mode,
        size: 2,
        fill: 0.5,
        origin: (0, 0, 0),
        tolerance: 2,
        run: 2,
        jitter: 0.3,
        seed: 7,
        region: None,
    }
}

fn applied(cells: &HashMap<Cell, u8>, writes: &[(i32, i32, i32, i32)]) -> HashMap<Cell, u8> {
    let mut next = cells.clone();
    for &(x, y, z, slot) in writes {
        if slot == 0 {
            next.remove(&(x, y, z));
        } else {
            next.insert((x, y, z), slot as u8);
        }
    }
    next
}

#[test]
fn the_dome_starts_with_long_one_cell_contours() {
    let (steps, longest) = one_cell_steps(&tops(&dome()));
    assert!(steps > 100, "{steps}");
    assert!(longest >= 6, "{longest}");
}

#[test]
fn stagger_breaks_every_long_contour() {
    let cells = dome();
    let done = deterrace(&cells, &spec(TerraceMode::Stagger)).unwrap();
    assert!(done.longest[0] >= 6);
    // Teeth of 2: no straight step is longer than 3.
    assert!(done.longest[1] <= 3, "{:?}", done.longest);
    // It only moves step edges: a cell up or down, on top.
    let next = applied(&cells, &done.writes);
    let (before, after) = (tops(&cells), tops(&next));
    assert_eq!(before.len(), after.len());
    assert!(before.iter().all(|(c, h)| (after[c] - h).abs() <= 1));
    // The same seed, the same result; another seed, another.
    assert_eq!(
        deterrace(&cells, &spec(TerraceMode::Stagger)).unwrap(),
        done
    );
    let mut other = spec(TerraceMode::Stagger);
    other.seed = 8;
    assert_ne!(deterrace(&cells, &other).unwrap().writes, done.writes);
}

#[test]
fn blocks_leave_no_one_cell_steps_and_keep_the_silhouette() {
    let cells = dome();
    let done = deterrace(&cells, &spec(TerraceMode::Blocks)).unwrap();
    let next = applied(&cells, &done.writes);
    assert_eq!(done.steps[1], 0, "{done:?}");
    // Every cell sits in a whole 2 x 2 x 2 block.
    for c in next.keys() {
        let base = (
            c.0.div_euclid(2) * 2,
            c.1.div_euclid(2) * 2,
            c.2.div_euclid(2) * 2,
        );
        for d in 0..8 {
            let m = (base.0 + (d & 1), base.1 + (d >> 1 & 1), base.2 + (d >> 2));
            assert!(next.contains_key(&m), "{c:?} has no {m:?}");
        }
    }
    // The outline moves by at most the tolerance: every new cell is within
    // 2 of an old one and every old cell within 2 of a new one.
    let near = |set: &HashMap<Cell, u8>, c: &Cell| {
        (-2..=2).any(|dx| {
            (-2..=2).any(|dy| (-2..=2).any(|dz| set.contains_key(&(c.0 + dx, c.1 + dy, c.2 + dz))))
        })
    };
    assert!(next.keys().all(|c| near(&cells, c)));
    assert!(cells.keys().all(|c| near(&next, c)));
    let ratio = next.len() as f64 / cells.len() as f64;
    assert!((0.85..1.15).contains(&ratio), "{ratio}");
    // A cube takes its most common slot.
    assert!(next.values().all(|s| *s == 2 || *s == 3));
}

#[test]
fn a_tight_tolerance_keeps_thin_parts() {
    // A one-cell wire on a slab: two cubes of 2 would erase it.
    let mut cells = HashMap::new();
    for x in 0..8 {
        for y in 0..8 {
            cells.insert((x, y, 0), 1u8);
            cells.insert((x, y, 1), 1u8);
        }
    }
    for z in 2..8 {
        cells.insert((3, 3, z), 2u8);
    }
    let mut loose = spec(TerraceMode::Blocks);
    loose.tolerance = 2;
    let gone = applied(&cells, &deterrace(&cells, &loose).unwrap().writes);
    assert!(!gone.values().any(|s| *s == 2));
    let mut tight = loose.clone();
    tight.tolerance = 0;
    let done = deterrace(&cells, &tight).unwrap();
    let kept = applied(&cells, &done.writes);
    assert_eq!(kept.values().filter(|s| **s == 2).count(), 6);
    assert!(done.kept > 0);
}

#[test]
fn deterrace_runs_from_mcp_rhai_batch_and_the_panel() {
    let cells = dome();
    let writes: Vec<_> = cells
        .iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(*s)))
        .collect();
    let mut session = Session::new();
    session.replace_empty();
    session.write_cells(&writes).unwrap();
    let reply = apply(
        &mut session,
        "voxel.deterrace",
        &json!({"mode": "stagger", "seed": 7}),
    )
    .unwrap();
    assert!(
        reply["longestRun"]["before"].as_u64().unwrap() >= 6,
        "{reply}"
    );
    assert!(
        reply["longestRun"]["after"].as_u64().unwrap() <= 3,
        "{reply}"
    );
    session.undo();
    let reply = apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [{"tool": "deterrace", "arguments": {"mode": "blocks", "size": 2}}]}),
    )
    .unwrap();
    assert!(reply["changed"].as_u64().unwrap() > 0, "{reply}");
    session.undo();
    session.edit.terrace.mode = 1;
    session.deterrace_from_panel();
    assert!(
        session.status.starts_with("Staggered"),
        "{}",
        session.status
    );
    let mut script = Session::new();
    script.replace_empty();
    script
        .set_script(
            0,
            "fill_ellipsoid(-15, -15, -9, 15, 15, 9, 2);\ncarve_box(-16, -16, -10, 16, 16, -1);\nlet r = deterrace(#{mode: \"stagger\", seed: 7});\nif r.longestRun.after > 3 { throw `long run ${r.longestRun}`; }\n",
        )
        .unwrap();
    script.regenerate_script(0).unwrap();
}
