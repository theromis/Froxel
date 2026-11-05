use serde_json::json;

use super::*;
use crate::state::Session;

fn deck(seed: u64) -> PlankSpec {
    PlankSpec {
        x: (0, 59),
        y: (0, 29),
        z: 4,
        axis: PlankAxis::X,
        palettes: vec![3, 4, 5],
        dirt: Some(6),
        dark: None,
        width: (3, 6),
        length: (8, 20),
        gap: (1, 1),
        thickness: 2,
        ragged: 0.0,
        nails: 0.0,
        knots: 0.0,
        only: Vec::new(),
        exclude: Exclude::default(),
        seed,
    }
}

fn empty() -> HashMap<Cell, u8> {
    HashMap::new()
}

fn laid(spec: &PlankSpec) -> Vec<Row> {
    layout(spec, &mut Rng::new(spec.seed ^ 0x706c_616e_6b73))
}

/// The cells after a run on `cells`.
fn after(cells: &HashMap<Cell, u8>, done: &Planked) -> HashMap<Cell, u8> {
    let mut out = cells.clone();
    for (x, y, z, slot) in &done.writes {
        if *slot == 0 {
            out.remove(&(*x, *y, *z));
        } else {
            out.insert((*x, *y, *z), *slot as u8);
        }
    }
    out
}

#[test]
fn the_same_seed_lays_the_same_boards_and_another_does_not() {
    let a = planks(&empty(), &deck(7)).unwrap();
    let b = planks(&empty(), &deck(7)).unwrap();
    let c = planks(&empty(), &deck(8)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a.writes, c.writes);
}

#[test]
fn rows_tile_the_rectangle_in_range_with_gaps_of_the_asked_width() {
    for seed in 0..40 {
        for gap in [(1, 1), (1, 3), (2, 2)] {
            let spec = PlankSpec { gap, ..deck(seed) };
            let rows = laid(&spec);
            assert_eq!(rows[0].from, 0);
            assert_eq!(rows.last().unwrap().to, 30);
            for pair in rows.windows(2) {
                let space = pair[1].from - pair[0].to;
                assert!((gap.0..=gap.1).contains(&space), "{space}");
            }
            for row in &rows[..rows.len() - 1] {
                assert!((3..=6).contains(&(row.to - row.from)));
            }
            for row in &rows {
                assert_eq!(row.boards[0].from, 0);
                assert_eq!(row.boards.last().unwrap().to, 60);
                for pair in row.boards.windows(2) {
                    let space = pair[1].from - pair[0].to;
                    assert!((gap.0..=gap.1).contains(&space), "{space}");
                }
                // Boards between the two sawn ends keep to the range, give or
                // take the few cells a stagger needs.
                for board in &row.boards[1..row.boards.len().saturating_sub(1)] {
                    let long = board.to - board.from;
                    assert!((5..=23).contains(&long), "{long}");
                }
                for board in &row.boards {
                    assert!(board.to - board.from >= END_MIN);
                }
            }
        }
    }
}

/// The gaps of a row, along the axis.
fn joints_of(row: &Row) -> Vec<(i32, i32)> {
    row.boards
        .windows(2)
        .map(|pair| (pair[0].to, pair[1].from))
        .collect()
}

#[test]
fn end_joints_never_line_up_between_neighbouring_rows() {
    for length in [(8, 20), (6, 14), (8, 8), (12, 12), (16, 16), (20, 40)] {
        for seed in 0..60 {
            let spec = PlankSpec {
                length,
                ..deck(seed)
            };
            let rows = laid(&spec);
            for pair in rows.windows(2) {
                let above = joints_of(&pair[0]);
                for (from, to) in joints_of(&pair[1]) {
                    let clear = clearance(from, to - from, &above);
                    assert!(
                        clear >= 1,
                        "{length:?} seed {seed}: a joint at {from} lines up with {above:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn no_layout_runs_as_a_uniform_stripe() {
    let shapes = [
        (60, 30, (8, 20), (3, 6)),
        // Longer boards than the deck: still a joint in every row.
        (60, 30, (100, 200), (3, 6)),
        (12, 30, (32, 64), (2, 2)),
        // Long uniform boards of one width.
        (64, 24, (16, 16), (4, 4)),
    ];
    for (long, wide, length, width) in shapes {
        for palettes in [vec![3, 4], vec![3, 3, 4], vec![3, 4, 5, 6]] {
            for seed in 0..30 {
                let spec = PlankSpec {
                    x: (0, long - 1),
                    y: (0, wide - 1),
                    length,
                    width,
                    palettes: palettes.clone(),
                    ..deck(seed)
                };
                let rows = laid(&spec);
                for row in &rows {
                    assert!(row.boards.len() >= 2, "a row without a joint: {spec:?}");
                    for pair in row.boards.windows(2) {
                        assert_ne!(
                            palettes[pair[0].tone], palettes[pair[1].tone],
                            "two boards of one tone end to end"
                        );
                    }
                }
                // Stripes across the rows: no joint column runs through three.
                for trio in rows.windows(3) {
                    let same = joints_of(&trio[0]) == joints_of(&trio[1])
                        && joints_of(&trio[1]) == joints_of(&trio[2]);
                    assert!(!same, "{spec:?}");
                }
            }
        }
    }
}

#[test]
fn three_tones_keep_neighbouring_boards_mostly_apart() {
    let rows = laid(&deck(4));
    let (mut beside, mut total) = (0, 0);
    for pair in rows.windows(2) {
        for board in &pair[1].boards {
            for other in &pair[0].boards {
                let overlap = (board.to.min(other.to) - board.from.max(other.from)).max(0);
                total += overlap;
                if board.tone == other.tone {
                    beside += overlap;
                }
            }
        }
    }
    assert!(beside * 4 < total, "{beside} of {total}");
    let used: std::collections::HashSet<usize> = rows
        .iter()
        .flat_map(|row| row.boards.iter().map(|board| board.tone))
        .collect();
    assert_eq!(used.len(), 3);
}

#[test]
fn a_gap_is_open_down_through_the_boards_to_a_dirt_cell() {
    let spec = deck(3);
    let done = planks(&empty(), &spec).unwrap();
    let cells = after(&empty(), &done);
    let rows = laid(&spec);
    let gap_y = rows[0].to;
    for x in 0..60 {
        assert!(!cells.contains_key(&(x, gap_y, 4)), "top of the gap");
        assert!(!cells.contains_key(&(x, gap_y, 3)), "through the board");
        assert_eq!(cells.get(&(x, gap_y, 2)), Some(&6), "dirt under the gap");
    }
    let board = &rows[0].boards[0];
    let joint = board.to;
    for y in rows[0].from..rows[0].to {
        assert!(!cells.contains_key(&(joint, y, 4)));
        assert_eq!(cells.get(&(joint, y, 2)), Some(&6));
    }
    // Under a board nothing is written.
    assert!(!cells.contains_key(&(0, 0, 2)));
    // A board is one tone, both layers.
    let tone = spec.palettes[board.tone];
    for x in board.from..board.to {
        for y in rows[0].from..rows[0].to {
            assert_eq!(cells.get(&(x, y, 4)), Some(&tone));
            assert_eq!(cells.get(&(x, y, 3)), Some(&tone));
        }
    }
    assert!(
        done.boards > 10 && done.joints > 5 && done.dirtied > 100,
        "{done:?}"
    );
}

#[test]
fn the_axis_turns_the_layout_and_gaps_cut_what_was_there() {
    let along_x = planks(&empty(), &deck(5)).unwrap();
    let spec = PlankSpec {
        x: (0, 29),
        y: (0, 59),
        axis: PlankAxis::Y,
        ..deck(5)
    };
    let along_y = planks(&empty(), &spec).unwrap();
    let swap = |writes: &[(i32, i32, i32, i32)]| {
        let mut out: Vec<(i32, i32, i32, i32)> =
            writes.iter().map(|w| (w.1, w.0, w.2, w.3)).collect();
        out.sort_unstable();
        out
    };
    let mut x = along_x.writes.clone();
    x.sort_unstable();
    assert_eq!(x, swap(&along_y.writes));

    // On a solid slab the gaps are cut and the boards painted.
    let mut slab = empty();
    for x in 0..60 {
        for y in 0..30 {
            for z in 0..=4 {
                slab.insert((x, y, z), 9);
            }
        }
    }
    let done = planks(&slab, &deck(5)).unwrap();
    assert!(
        done.removed > 0 && done.painted > 0 && done.added == 0,
        "{done:?}"
    );
}

#[test]
fn exclude_and_only_keep_what_they_name() {
    let mut slab = empty();
    for x in 0..60 {
        for y in 0..30 {
            if (x - 30) * (x - 30) + (y - 15) * (y - 15) < 200 {
                slab.insert((x, y, 4), 9);
                slab.insert((x, y, 3), 9);
            }
            slab.insert((x, y, 2), 8);
        }
    }
    let spec = PlankSpec {
        only: vec![9],
        exclude: Exclude(vec![Region::spanning((28, 10, 0), (34, 16, 9))]),
        ..deck(2)
    };
    let done = planks(&slab, &spec).unwrap();
    for (x, y, z, _) in &done.writes {
        assert!(!spec.exclude.contains((*x, *y, *z)));
        if *z >= 3 {
            assert_eq!(slab.get(&(*x, *y, *z)), Some(&9), "only touches the deck");
        }
    }
    // Only under the deck's own gaps does the slab below take dirt.
    for (x, y, z, slot) in &done.writes {
        if *z == 2 {
            assert_eq!(*slot, 6);
            assert!(slab.contains_key(&(*x, *y, 3)) || slab.contains_key(&(*x, *y, 4)));
        }
    }
    assert!(done.boards > 2 && done.boards < laid(&spec).iter().map(|r| r.boards.len()).sum());
    let cells = after(&slab, &done);
    assert_eq!(cells.get(&(30, 12, 4)), Some(&9), "the excluded box stays");
    assert_eq!(
        cells.get(&(1, 1, 3)),
        None,
        "outside the deck nothing is laid"
    );
}

#[test]
fn ragged_ends_nails_and_knots_stay_on_the_boards_and_are_optional() {
    let plain = planks(&empty(), &deck(1)).unwrap();
    assert_eq!((plain.nails, plain.knots), (0, 0));
    let rough = PlankSpec {
        ragged: 0.5,
        ..deck(1)
    };
    let done = planks(&empty(), &rough).unwrap();
    assert!(
        done.dirtied > plain.dirtied,
        "cut ends open down to the dirt"
    );
    // Cut back at most two cells from each end: a board keeps its middle.
    let cells = after(&empty(), &done);
    for row in laid(&rough) {
        for board in &row.boards {
            let mid = (board.from + board.to) / 2;
            for y in row.from..row.to {
                assert!(cells.contains_key(&(mid, y, 4)));
            }
        }
    }

    let marked = PlankSpec {
        dark: Some(7),
        nails: 0.5,
        knots: 0.5,
        ..deck(1)
    };
    let done = planks(&empty(), &marked).unwrap();
    assert!(done.nails > 0 && done.knots > 0, "{done:?}");
    let cells = after(&empty(), &done);
    let dark = cells.values().filter(|slot| **slot == 7).count();
    assert!(dark >= done.nails + done.knots);
    assert!(
        dark * 20 < cells.len(),
        "a few dark cells, not many: {dark}"
    );
    for (cell, slot) in &cells {
        if *slot == 7 {
            assert_eq!(cell.2, 4, "dark cells show in the top layer only");
        }
    }
}

#[test]
fn bad_arguments_are_refused() {
    let one_tone = PlankSpec {
        palettes: vec![3, 3],
        ..deck(0)
    };
    let err = planks(&empty(), &one_tone).unwrap_err();
    assert!(err.contains("two different"), "{err}");
    for bad in [
        PlankSpec {
            width: (0, 2),
            ..deck(0)
        },
        PlankSpec {
            length: (9, 8),
            ..deck(0)
        },
        PlankSpec {
            gap: (0, 1),
            ..deck(0)
        },
        PlankSpec {
            thickness: 0,
            ..deck(0)
        },
        PlankSpec {
            ragged: 1.5,
            ..deck(0)
        },
        PlankSpec {
            dirt: Some(0),
            ..deck(0)
        },
    ] {
        assert!(planks(&empty(), &bad).is_err(), "{bad:?}");
    }
    assert!(planks(
        &empty(),
        &PlankSpec {
            x: (-1024, 1023),
            y: (-1024, 1023),
            ..deck(0)
        }
    )
    .is_err());
}

#[test]
fn a_small_deck_still_lays_and_corners_may_come_in_any_order() {
    let tiny = PlankSpec {
        x: (2, 2),
        y: (0, 0),
        ..deck(0)
    };
    let done = planks(&empty(), &tiny).unwrap();
    assert_eq!(done.boards, 1);
    assert_eq!(done.writes.len(), 2);
    let flipped = PlankSpec {
        x: (59, 0),
        y: (29, 0),
        ..deck(6)
    };
    assert_eq!(
        planks(&empty(), &flipped).unwrap(),
        planks(&empty(), &deck(6)).unwrap()
    );
}

#[test]
fn mcp_rhai_and_the_panel_share_one_path_and_one_undo_step() {
    let args = json!({
        "x0": 0, "y0": 0, "x1": 39, "y1": 19, "z": 4, "axis": "x",
        "palettes": [3, 4, 5], "dirt": 6, "dark": 7, "ragged": 0.3, "seed": 9,
    });
    let mut mcp = Session::new();
    mcp.replace_empty();
    let reply = crate::remote::apply(&mut mcp, "voxel.planks", &args).unwrap();
    assert!(reply["boards"].as_i64().unwrap() > 5, "{reply}");
    assert!(reply["changed"].as_i64().unwrap() > 0);
    assert!(mcp.can_undo());
    let cells = mcp.active().cells.clone();
    mcp.undo();
    assert!(mcp.active().cells.is_empty());
    assert!(!mcp.can_undo());
    mcp.redo();
    assert_eq!(mcp.active().cells, cells);

    let mut script = Session::new();
    script.replace_empty();
    script
        .set_script(
            0,
            r#"let r = planks(0, 0, 39, 19, 4, "x", [3, 4, 5], #{dirt: 6, dark: 7, ragged: 0.3, seed: 9});
               if r.boards < 5 || r.changed == 0 { throw "no boards"; }"#,
        )
        .unwrap();
    script.regenerate_script(0).unwrap();
    assert_eq!(script.active().cells, cells);

    let mut panel = Session::new();
    panel.replace_empty();
    panel.objects[0].cells.insert((0, 0, 4), 1);
    panel.objects[0].cells.insert((39, 19, 4), 1);
    let fields = &mut panel.stone.planks;
    fields.palettes = "3 4 5".into();
    fields.dirt = 6;
    fields.dark = 7;
    fields.ragged = 0.3;
    fields.seed = 9;
    panel.planks_from_panel();
    assert_eq!(panel.active().cells, cells, "{}", panel.status);
    assert!(panel.status.starts_with("Planks:"), "{}", panel.status);

    let err = crate::remote::apply(
        &mut mcp,
        "voxel.planks",
        &json!({"x0": 0, "y0": 0, "x1": 9, "y1": 9, "z": 4, "axis": "z", "palettes": [3, 4]}),
    )
    .unwrap_err();
    assert!(err.contains("axis must be x or y"), "{err}");
    let err = crate::remote::apply(
        &mut mcp,
        "voxel.planks",
        &json!({"x0": 0, "y0": 0, "x1": 9, "y1": 9, "z": 4, "axis": "x", "palettes": [3, 4], "nails": 0.5}),
    )
    .unwrap_err();
    assert!(err.contains("needs dark"), "{err}");
}

#[test]
fn a_batch_runs_planks_too() {
    let mut session = Session::new();
    session.replace_empty();
    let reply = crate::remote::apply(
        &mut session,
        "voxel.batch",
        &json!({"operations": [
            {"tool": "fill_box", "arguments": {"x0": 0, "y0": 0, "z0": 0, "x1": 19, "y1": 9, "z1": 2, "palette": 2}},
            {"tool": "planks", "arguments": {"x0": 0, "y0": 0, "x1": 19, "y1": 9, "z": 4, "axis": "x", "palettes": [3, 4], "dirt": 6}},
        ]}),
    )
    .unwrap();
    assert_eq!(reply["results"][1]["tool"], "planks");
    assert!(
        reply["results"][1]["boards"].as_i64().unwrap() > 1,
        "{reply}"
    );
    session.undo();
    assert!(session.active().cells.is_empty());
}

#[test]
fn the_plank_dock_example_has_every_slot_and_rebuilds_the_same() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("../examples/plank_dock.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = session.active().cells.clone();
    for slot in 1..=7 {
        assert!(cells.values().any(|s| *s == slot), "slot {slot} is missing");
    }
    session.regenerate_script(0).unwrap();
    assert_eq!(session.active().cells, cells);
    if let Ok(dir) = std::env::var("FROXEL_EXAMPLE_OUT") {
        session
            .write_vox(&std::path::Path::new(&dir).join("plank_dock.vox"))
            .unwrap();
    }
}
