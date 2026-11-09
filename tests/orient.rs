use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::Session;
use crate::tools::mask_from_rows;

/// A made-up creature facing −Y with +Z up and nothing symmetric to
/// confuse the turns: a long body, a raised head at −Y, a thin tail at
/// +Y, a fin on its +X side, and legs only on the −X side.
fn creature() -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    let mut fill = |a: Cell, b: Cell, slot: u8| {
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                for z in a.2..=b.2 {
                    cells.insert((x, y, z), slot);
                }
            }
        }
    };
    fill((0, 0, 2), (7, 15, 6), 1); // body
    fill((2, -5, 4), (5, -1, 9), 2); // head
    fill((3, 16, 3), (4, 23, 4), 3); // tail
    fill((8, 4, 4), (10, 9, 5), 4); // fin
    fill((0, 2, 0), (1, 3, 1), 5); // legs
    fill((0, 11, 0), (1, 12, 1), 5);
    cells
}

/// The silhouette of `cells` from `side` as rows, `px` characters per cell.
fn rows(cells: &HashMap<Cell, u8>, side: Side, px: usize) -> Vec<String> {
    let [(ra, rs), (da, ds), _] = side.axes();
    let at = |c: &Cell, axis: usize| [c.0, c.1, c.2][axis];
    let uv: Vec<(i32, i32)> = cells
        .keys()
        .map(|c| (at(c, ra) * rs, at(c, da) * ds))
        .collect();
    let u0 = uv.iter().map(|p| p.0).min().unwrap();
    let v0 = uv.iter().map(|p| p.1).min().unwrap();
    let w = (uv.iter().map(|p| p.0).max().unwrap() - u0 + 1) as usize * px;
    let h = (uv.iter().map(|p| p.1).max().unwrap() - v0 + 1) as usize * px;
    let mut grid = vec![vec!['.'; w + 4]; h + 4];
    for (u, v) in uv {
        for dy in 0..px {
            for dx in 0..px {
                grid[(v - v0) as usize * px + dy + 2][(u - u0) as usize * px + dx + 2] = '#';
            }
        }
    }
    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect()
}

fn spec_of(cells: &HashMap<Cell, u8>, sides: &[Side]) -> OrientSpec {
    OrientSpec {
        views: sides
            .iter()
            .map(|side| OrientView {
                side: *side,
                mask: mask_from_rows(&rows(cells, *side, 3), &HashMap::new()).unwrap(),
                weight: 1.0,
            })
            .collect(),
        turns: Turns::All,
        yaw_step: 0.0,
        yaw_range: 45.0,
        fine_axes: [false, false, true],
        resolution: 64,
    }
}

/// `cells` with their low corner at the origin, sorted.
fn normalised(cells: &HashMap<Cell, u8>) -> Vec<(Cell, u8)> {
    let (min, _) = bounds(cells.keys().copied()).unwrap();
    let mut out: Vec<(Cell, u8)> = cells
        .iter()
        .map(|(c, s)| ((c.0 - min.0, c.1 - min.1, c.2 - min.2), *s))
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn there_are_24_proper_turns_and_euler_angles_round_trip() {
    let turns = axis_turns(Turns::All);
    assert_eq!(turns.len(), 24);
    assert_eq!(turns[0], IDENTITY);
    for (i, a) in turns.iter().enumerate() {
        assert!((det(a) - 1.0).abs() < 1e-9);
        for b in &turns[i + 1..] {
            assert_ne!(a, b);
        }
        let back = from_euler_xyz(euler_xyz(a));
        for r in 0..3 {
            for c in 0..3 {
                assert!((back[r][c] - a[r][c]).abs() < 1e-9, "{a:?} {back:?}");
            }
        }
    }
    let odd = mul(&yaw(23.0), &turns[7]);
    let back = from_euler_xyz(euler_xyz(&odd));
    assert!((0..9).all(|i| (back[i / 3][i % 3] - odd[i / 3][i % 3]).abs() < 1e-6));
    assert_eq!(axis_turns(Turns::Upright).len(), 4);
}

#[test]
fn every_turn_of_the_creature_is_turned_back_exactly() {
    let target = creature();
    let spec = spec_of(&target, &[Side::Front, Side::Left, Side::Top]);
    let wanted = normalised(&target);
    for turn in axis_turns(Turns::All) {
        let lying = resample(&target, &turn, 1.0).unwrap();
        let found = orient(&lying, &spec).unwrap();
        assert!(found.best.score > 0.97, "{turn:?}: {}", found.best.score);
        assert!(
            found.ranked[0].score - found.ranked[1].score > 0.03,
            "{turn:?}: no margin"
        );
        let back = resample(&lying, &found.best.turn, 1.0).unwrap();
        assert_eq!(normalised(&back), wanted, "{turn:?}");
    }
}

#[test]
fn fine_yaw_finds_a_turn_between_the_axes() {
    let target = creature();
    let mut spec = spec_of(&target, &[Side::Front, Side::Left, Side::Top]);
    let skewed = resample(&target, &yaw(20.0), 1.0).unwrap();
    let coarse = orient(&skewed, &spec).unwrap();
    spec.yaw_step = 5.0;
    spec.yaw_range = 30.0;
    let fine = orient(&skewed, &spec).unwrap();
    assert_eq!(fine.best.fine, [0.0, 0.0, -20.0], "{:?}", fine.best);
    assert!(fine.best.score > coarse.best.score + 0.05);
    assert!(fine.best.score > 0.9, "{}", fine.best.score);
}

#[test]
fn fine_tilt_about_x_and_y_undoes_a_turn_off_the_up_axis() {
    // A quarter turn about X, then 20 degrees about the new up: once turned
    // back, the residue is a tilt about Y, which yaw alone cannot remove.
    let target = creature();
    let mut spec = spec_of(&target, &[Side::Front, Side::Left, Side::Top]);
    spec.yaw_step = 5.0;
    spec.yaw_range = 30.0;
    let tumbled = resample(&target, &mul(&yaw(20.0), &about(0, 90.0)), 1.0).unwrap();
    let yaw_only = orient(&tumbled, &spec).unwrap();
    spec.fine_axes = [true, true, true];
    let tilted = orient(&tumbled, &spec).unwrap();
    assert!(tilted.best.score > 0.9, "{:?}", tilted.best);
    assert!(tilted.best.score > yaw_only.best.score + 0.05);
    // What is left of the tumble is at most one 5 degree step.
    let undone = mul(&tilted.best.turn, &mul(&yaw(20.0), &about(0, 90.0)));
    let trace = undone[0][0] + undone[1][1] + undone[2][2];
    let left = ((trace - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees();
    assert!(left <= 5.5, "{left} degrees left: {:?}", tilted.best);
}

#[test]
fn orient_to_views_turns_and_scales_the_active_object() {
    let target = creature();
    let turn = axis_turns(Turns::All)[13];
    let lying = resample(&target, &turn, 1.0).unwrap();
    let mut session = Session::new();
    session.replace_empty();
    let writes: Vec<_> = lying
        .iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(*s)))
        .collect();
    session.write_cells(&writes).unwrap();
    let views: Vec<_> = [Side::Front, Side::Left, Side::Top]
        .iter()
        .map(|side| json!({"view": side.name(), "rows": rows(&target, *side, 3)}))
        .collect();
    let report = apply(
        &mut session,
        "voxel.orient_to_views",
        &json!({"views": views, "apply": false}),
    )
    .unwrap();
    assert_eq!(report["applied"], false);
    assert_eq!(session.active().cells.len(), lying.len());
    let reply = apply(
        &mut session,
        "voxel.orient_to_views",
        &json!({"views": views, "size": 58}),
    )
    .unwrap();
    assert!(reply["after"]["score"].as_f64().unwrap() > 0.9, "{reply}");
    assert!(reply["before"]["score"].as_f64().unwrap() < reply["after"]["score"].as_f64().unwrap());
    // 29 cells long (y -5..23) at twice the size.
    let long = reply["bounds"]["max"][1].as_i64().unwrap()
        - reply["bounds"]["min"][1].as_i64().unwrap()
        + 1;
    assert!((57..=58).contains(&long), "{reply}");
    let iou = reply["after"]["views"][1]["iou"].as_f64().unwrap();
    assert!(iou > 0.9, "{reply}");
    session.undo();
    assert_eq!(normalised(&session.active().cells), normalised(&lying));
}

#[test]
fn import_mesh_orient_voxelizes_the_mesh_already_turned() {
    // The creature as a mesh of cube faces, lying on its side: Y-up OBJ.
    let target = creature();
    let turn = axis_turns(Turns::All)[5];
    let lying = resample(&target, &turn, 1.0).unwrap();
    let mut obj = String::new();
    let mut count = 0;
    let faces: [[f64; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    for c in lying.keys() {
        for n in faces {
            let next = (c.0 + n[0] as i32, c.1 + n[1] as i32, c.2 + n[2] as i32);
            if lying.contains_key(&next) {
                continue;
            }
            // The four corners of the face, in cell (Z-up) space.
            let (a, b) = match n.iter().position(|v| *v != 0.0).unwrap() {
                0 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
                1 => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                _ => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            };
            let centre = [
                f64::from(c.0) + 0.5 + n[0] * 0.5,
                f64::from(c.1) + 0.5 + n[1] * 0.5,
                f64::from(c.2) + 0.5 + n[2] * 0.5,
            ];
            for (sa, sb) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                let p: Vec<f64> = (0..3).map(|i| centre[i] + a[i] * sa + b[i] * sb).collect();
                // Cell (x, y, z) is file (x, z, -y).
                obj.push_str(&format!("v {} {} {}\n", p[0], p[2], -p[1]));
            }
            obj.push_str(&format!(
                "f {} {} {} {}\n",
                count + 1,
                count + 2,
                count + 3,
                count + 4
            ));
            count += 4;
        }
    }
    let dir = std::env::temp_dir().join(format!("froxel-orient-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lying.obj");
    std::fs::write(&path, obj).unwrap();
    let views: Vec<_> = [Side::Front, Side::Left, Side::Top]
        .iter()
        .map(|side| json!({"view": side.name(), "rows": rows(&target, *side, 3)}))
        .collect();
    let mut session = Session::new();
    session.replace_empty();
    let reply = apply(
        &mut session,
        "mesh.import",
        &json!({"path": path.display().to_string(), "scale": 1, "palette": 1, "orient": {"views": views}}),
    )
    .unwrap();
    let orientation = &reply["orientation"];
    assert!(
        orientation["after"]["score"].as_f64().unwrap() > 0.9,
        "{reply}"
    );
    // The turn found undoes the one the mesh was lying in.
    let applied: Vec<f64> = orientation["rotation"]["applied"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let undone = mul(&from_euler_xyz([applied[0], applied[1], applied[2]]), &turn);
    assert!(
        (0..9).all(|i| (undone[i / 3][i % 3] - IDENTITY[i / 3][i % 3]).abs() < 1e-9),
        "{orientation}"
    );
    // And the cells are those of an import given that rotation.
    let mut plain = Session::new();
    plain.replace_empty();
    apply(
        &mut plain,
        "mesh.import",
        &json!({"path": path.display().to_string(), "scale": 1, "palette": 1, "rotation": applied}),
    )
    .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        normalised(&session.active().cells),
        normalised(&plain.active().cells)
    );
}

#[test]
fn rhai_orient_to_views_runs_the_same_code_path() {
    let target = creature();
    let turn = axis_turns(Turns::All)[9];
    let lying = resample(&target, &turn, 1.0).unwrap();
    let cells: Vec<String> = lying
        .iter()
        .map(|(c, s)| format!("[{}, {}, {}, {}]", c.0, c.1, c.2, s))
        .collect();
    let views: Vec<String> = [Side::Front, Side::Left, Side::Top]
        .iter()
        .map(|side| {
            let rows: Vec<String> = rows(&target, *side, 2)
                .into_iter()
                .map(|r| format!("\"{r}\""))
                .collect();
            format!(
                "#{{view: \"{}\", rows: [{}]}}",
                side.name(),
                rows.join(", ")
            )
        })
        .collect();
    let source = format!(
        "set_voxels([{}]);\nlet r = orient_to_views([{}], #{{turns: \"all\"}});\nif r.after.score < 0.95 {{ throw \"score \" + r.after.score; }}\n",
        cells.join(", "),
        views.join(", ")
    );
    let mut session = Session::new();
    session.replace_empty();
    session.set_script(0, &source).unwrap();
    session.regenerate_script(0).unwrap();
    assert_eq!(normalised(&session.objects[0].cells), normalised(&target));
}

#[test]
fn the_orient_panel_uses_the_carve_views() {
    let target = creature();
    let dir = std::env::temp_dir().join(format!("froxel-orient-panel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut session = Session::new();
    session.replace_empty();
    let lying = resample(&target, &axis_turns(Turns::All)[17], 1.0).unwrap();
    let writes: Vec<_> = lying
        .iter()
        .map(|(c, s)| (c.0, c.1, c.2, i32::from(*s)))
        .collect();
    session.write_cells(&writes).unwrap();
    for (row, side) in [Side::Front, Side::Left, Side::Top].iter().enumerate() {
        let lines = rows(&target, *side, 3);
        let (w, h) = (lines[0].len() as u32, lines.len() as u32);
        let mut rgba = Vec::new();
        for line in &lines {
            for c in line.chars() {
                rgba.extend(if c == '#' {
                    [40, 120, 40, 255]
                } else {
                    [255, 255, 255, 255]
                });
            }
        }
        let path = dir.join(format!("{}.png", side.name()));
        crate::trace::write_rgba_png(&path, w, h, &rgba).unwrap();
        let view = &mut session.silhouette.carve.views[row];
        view.on = true;
        view.path = path.display().to_string();
        view.side = crate::state::SIDES
            .iter()
            .position(|(_, name)| *name == side.name())
            .unwrap();
    }
    session.orient_from_panel();
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        session.status.starts_with("Oriented: IoU"),
        "{}",
        session.status
    );
    assert_eq!(
        normalised(&session.active().cells),
        normalised(&target),
        "{}",
        session.status
    );
}
