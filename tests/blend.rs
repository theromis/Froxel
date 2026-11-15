use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::remote::apply;
use crate::state::{Cell, Session};

fn sphere(center: [f64; 3], radius: f64, slot: u8, k: f64) -> BlendShape {
    BlendShape {
        form: Form::Sphere { center, radius },
        rotate: [0.0; 3],
        op: BlendOp::Union,
        k,
        palette: Some(slot),
    }
}

fn spec(shapes: Vec<BlendShape>) -> BlendSpec {
    BlendSpec {
        shapes,
        grow: 0.0,
        colors: BlendColors::Nearest,
        seam: 0.0,
        seed: 0,
        bounds: None,
    }
}

fn map(blended: &Blended) -> HashMap<Cell, u8> {
    blended.cells.iter().copied().collect()
}

#[test]
fn a_hard_union_is_the_spheres_cells() {
    let done = blend_shapes(&spec(vec![
        sphere([0.0, 0.0, 0.0], 4.0, 1, 0.0),
        sphere([6.0, 0.0, 0.0], 3.0, 2, 0.0),
    ]))
    .unwrap();
    let cells = map(&done);
    for x in -6..=10 {
        for y in -6..=6 {
            for z in -6..=6 {
                let (fx, fy, fz) = (f64::from(x), f64::from(y), f64::from(z));
                let a = (fx * fx + fy * fy + fz * fz).sqrt() <= 4.0;
                let b = ((fx - 6.0).powi(2) + fy * fy + fz * fz).sqrt() <= 3.0;
                assert_eq!(cells.contains_key(&(x, y, z)), a || b, "{x} {y} {z}");
            }
        }
    }
}

#[test]
fn a_smooth_union_fills_the_neck_between_shapes() {
    let hard = blend_shapes(&spec(vec![
        sphere([0.0, 0.0, 0.0], 4.0, 1, 0.0),
        sphere([10.0, 0.0, 0.0], 4.0, 2, 0.0),
    ]))
    .unwrap();
    let soft = blend_shapes(&spec(vec![
        sphere([0.0, 0.0, 0.0], 4.0, 1, 0.0),
        sphere([10.0, 0.0, 0.0], 4.0, 2, 6.0),
    ]))
    .unwrap();
    let hard = map(&hard);
    let soft = map(&soft);
    // The two balls do not touch hard; blended a bridge joins them.
    assert!(!hard.contains_key(&(5, 0, 0)));
    assert!((0..=10).all(|x| soft.contains_key(&(x, 0, 0))));
    // Every hard cell stays: a smooth union only adds.
    assert!(hard.keys().all(|cell| soft.contains_key(cell)));
    assert!(soft.len() > hard.len());
    // The seam falls halfway, at x 5.
    assert_eq!(soft[&(4, 0, 0)], 1);
    assert_eq!(soft[&(6, 0, 0)], 2);
}

#[test]
fn subtract_carves_softly_and_paints_its_cut() {
    let mut cut = sphere([0.0, -6.0, 0.0], 3.0, 9, 2.0);
    cut.op = BlendOp::Subtract;
    let done = blend_shapes(&spec(vec![sphere([0.0, 0.0, 0.0], 6.0, 1, 0.0), cut])).unwrap();
    let cells = map(&done);
    assert!(!cells.contains_key(&(0, -5, 0)));
    assert!(!cells.contains_key(&(0, -4, 0)));
    assert!(cells.contains_key(&(0, 0, 0)));
    // The walls of the cut take its palette; the far side keeps the body's.
    assert!(cells.values().any(|slot| *slot == 9));
    assert_eq!(cells[&(0, 5, 0)], 1);
}

#[test]
fn intersect_keeps_only_the_inside() {
    let mut half = BlendShape {
        form: Form::Box {
            center: [0.0, 0.0, 5.0],
            half: [10.0, 10.0, 5.0],
            round: 0.0,
        },
        rotate: [0.0; 3],
        op: BlendOp::Intersect,
        k: 0.0,
        palette: None,
    };
    let done = blend_shapes(&spec(vec![
        sphere([0.0, 0.0, 0.0], 6.0, 1, 0.0),
        half.clone(),
    ]))
    .unwrap();
    assert!(done.cells.iter().all(|((_, _, z), _)| *z >= 0));
    assert!(done.cells.iter().any(|((_, _, z), _)| *z == 6));
    half.op = BlendOp::Subtract;
    let done = blend_shapes(&spec(vec![sphere([0.0, 0.0, 0.0], 6.0, 1, 0.0), half])).unwrap();
    assert!(done.cells.iter().all(|((_, _, z), _)| *z <= 0));
    assert!(done.cells.iter().any(|((_, _, z), _)| *z == -6));
}

#[test]
fn the_first_shape_must_add() {
    let mut cut = sphere([0.0; 3], 3.0, 1, 0.0);
    cut.op = BlendOp::Subtract;
    assert!(blend_shapes(&spec(vec![cut])).is_err());
    assert!(blend_shapes(&spec(Vec::new())).is_err());
}

#[test]
fn colors_last_keeps_an_eye_sunk_in_the_body() {
    let shapes = vec![
        sphere([0.0, 0.0, 0.0], 6.0, 1, 0.0),
        sphere([0.0, -5.0, 0.0], 1.5, 2, 0.0),
    ];
    let nearest = map(&blend_shapes(&spec(shapes.clone())).unwrap());
    let mut last = spec(shapes);
    last.colors = BlendColors::Last;
    let last = map(&blend_shapes(&last).unwrap());
    let eye = |cells: &HashMap<Cell, u8>| cells.values().filter(|s| **s == 2).count();
    assert!(eye(&last) > eye(&nearest));
    assert_eq!(last[&(0, -5, 0)], 2);
}

#[test]
fn a_seam_dithers_between_two_slots() {
    let shapes = vec![
        sphere([0.0, 0.0, 0.0], 5.0, 1, 0.0),
        sphere([8.0, 0.0, 0.0], 5.0, 2, 4.0),
    ];
    let sharp = map(&blend_shapes(&spec(shapes.clone())).unwrap());
    let mut dithered = spec(shapes);
    dithered.seam = 4.0;
    let dithered = map(&blend_shapes(&dithered).unwrap());
    assert_eq!(sharp.len(), dithered.len());
    let differ = sharp.iter().filter(|(c, s)| dithered[c] != **s).count();
    assert!(differ > 0);
    // Far from the seam nothing changes.
    assert_eq!(dithered[&(-4, 0, 0)], 1);
    assert_eq!(dithered[&(12, 0, 0)], 2);
}

#[test]
fn a_tapered_capsule_narrows_to_its_end() {
    let shape = BlendShape {
        form: Form::Capsule {
            points: vec![([0.0, 0.0, 0.0], 4.0), ([20.0, 0.0, 0.0], 1.0)],
        },
        rotate: [0.0; 3],
        op: BlendOp::Union,
        k: 0.0,
        palette: Some(3),
    };
    let cells = map(&blend_shapes(&spec(vec![shape])).unwrap());
    let width = |x: i32| (-6..=6).filter(|y| cells.contains_key(&(x, *y, 0))).count();
    assert_eq!(width(0), 9);
    assert!(width(10) < width(2));
    assert!(width(20) == 3);
    assert!(!cells.contains_key(&(22, 0, 0)));
}

#[test]
fn a_turned_box_turns_its_cells() {
    let flat = BlendShape {
        form: Form::Box {
            center: [0.0; 3],
            half: [8.0, 1.5, 1.5],
            round: 0.0,
        },
        rotate: [0.0; 3],
        op: BlendOp::Union,
        k: 0.0,
        palette: Some(1),
    };
    let mut turned = flat.clone();
    turned.rotate = [0.0, 0.0, 90.0];
    let a = map(&blend_shapes(&spec(vec![flat])).unwrap());
    let b = map(&blend_shapes(&spec(vec![turned])).unwrap());
    assert_eq!(a.len(), b.len());
    for &(x, y, z) in a.keys() {
        assert!(b.contains_key(&(-y, x, z)), "{x} {y} {z}");
    }
}

#[test]
fn a_torus_has_a_hole() {
    let ring = BlendShape {
        form: Form::Torus {
            center: [0.0; 3],
            major: 6.0,
            minor: 2.0,
        },
        rotate: [0.0; 3],
        op: BlendOp::Union,
        k: 0.0,
        palette: Some(1),
    };
    let cells = map(&blend_shapes(&spec(vec![ring])).unwrap());
    assert!(!cells.contains_key(&(0, 0, 0)));
    assert!(cells.contains_key(&(6, 0, 0)));
    assert!(cells.contains_key(&(0, -6, 2)));
    assert!(!cells.contains_key(&(6, 0, 3)));
}

#[test]
fn a_box_clips_the_evaluation() {
    let mut clipped = spec(vec![sphere([0.0; 3], 6.0, 1, 0.0)]);
    clipped.bounds = Some(((0, -10, -10), (10, 10, 10)));
    let done = blend_shapes(&clipped).unwrap();
    assert!(done.cells.iter().all(|((x, _, _), _)| *x >= 0));
    assert!(done.evaluated < 13 * 13 * 13 * 2);
}

#[test]
fn smooth_min_is_min_far_apart_and_lower_near() {
    assert_eq!(super::smooth_min(1.0, 5.0, 2.0), 1.0);
    assert!(super::smooth_min(1.0, 1.0, 2.0) < 1.0);
    assert_eq!(super::smooth_min(1.0, 1.0, 0.0), 1.0);
    assert_eq!(super::smooth_max(1.0, 5.0, 2.0), 5.0);
}

fn statue_args() -> serde_json::Value {
    json!({
        "k": 4,
        "shapes": [
            {"shape": "ellipsoid", "center": [10, 10, 8], "radii": [7, 6, 6], "palette": 1},
            {"shape": "ellipsoid", "center": [10, 6, 15], "radii": [5, 4.5, 4], "palette": 1},
            {"shape": "sphere", "center": [8, 3, 18], "radius": 1.6, "palette": 2, "k": 1},
            {"shape": "capsule", "from": [6, 7, 4], "to": [4, 4, 0], "radii": [2, 1.5], "palette": 1},
            {"shape": "box", "center": [10, 2, 14], "size": [6, 3, 1], "round": 0.5, "op": "subtract", "k": 2, "palette": 3}
        ]
    })
}

#[test]
fn mcp_blend_writes_one_undo_step() {
    let mut session = Session::new();
    session.replace_empty();
    let reply = apply(&mut session, "voxel.blend_shapes", &statue_args()).unwrap();
    assert!(reply["cells"].as_u64().unwrap() > 500);
    assert_eq!(reply["changed"], reply["cells"]);
    assert_eq!(reply["shapes"], 5);
    assert!(reply["bounds"]["min"].is_array());
    let slots: std::collections::HashSet<u8> = session.active().cells.values().copied().collect();
    assert!(slots.contains(&1) && slots.contains(&2) && slots.contains(&3));
    session.undo();
    assert!(session.active().cells.is_empty());
}

#[test]
fn mcp_blend_modes_and_mirror() {
    let mut session = Session::new();
    session.replace_empty();
    let mut args = statue_args();
    args["mirror"] = json!({"axis": "x", "at": 10.5});
    let reply = apply(&mut session, "voxel.blend_shapes", &args).unwrap();
    assert!(reply["mirrored"].as_u64().unwrap() > 0);
    // The leg came back on the far side of x 10.
    assert!(session.active().cells.contains_key(&(16, 4, 0)));
    assert!(session.active().cells.contains_key(&(4, 4, 0)));
    // Empty adds nothing over the same blend.
    args["mode"] = json!("empty");
    let reply = apply(&mut session, "voxel.blend_shapes", &args).unwrap();
    assert_eq!(reply["changed"], 0);
    // Intersect with a small ball erases the rest.
    let ball = json!({"shapes": [{"shape": "sphere", "center": [10, 10, 8], "radius": 3, "palette": 1}], "mode": "intersect"});
    apply(&mut session, "voxel.blend_shapes", &ball).unwrap();
    assert!(session.active().cells.len() < 150);
    assert!(apply(
        &mut session,
        "voxel.blend_shapes",
        &json!({"shapes": [{"shape": "sphere", "center": [0, 0, 0], "radius": 3, "palette": 1}], "mode": "intersect", "mirror": true}),
    )
    .is_err());
}

#[test]
fn mcp_blend_in_world_space_matches_local() {
    let mut local = Session::new();
    local.replace_empty();
    apply(&mut local, "voxel.blend_shapes", &statue_args()).unwrap();
    let mut world = Session::new();
    world.replace_empty();
    world.objects[0].translation = (5, -3, 2);
    let mut args = statue_args();
    for shape in args["shapes"].as_array_mut().unwrap() {
        for key in ["center", "from", "to"] {
            if let Some(p) = shape.get_mut(key) {
                let v: Vec<f64> = p
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                *p = json!([v[0] + 5.0, v[1] - 3.0, v[2] + 2.0]);
            }
        }
    }
    args["space"] = json!("world");
    let reply = apply(&mut world, "voxel.blend_shapes", &args).unwrap();
    assert_eq!(reply["space"], "world");
    assert_eq!(local.active().cells, world.active().cells);
}

#[test]
fn mcp_blend_refuses_bad_shapes() {
    let mut session = Session::new();
    session.replace_empty();
    for bad in [
        json!({"shapes": []}),
        json!({"shapes": [{"shape": "cone", "center": [0, 0, 0], "radius": 2, "palette": 1}]}),
        json!({"shapes": [{"shape": "sphere", "center": [0, 0, 0], "radius": 2}]}),
        json!({"shapes": [{"shape": "sphere", "center": [0, 0, 0], "radius": 2, "palette": 1, "colour": 2}]}),
        json!({"shapes": [{"shape": "capsule", "from": [0, 0, 0], "radius": 2, "palette": 1}]}),
        json!({"shapes": [{"shape": "sphere", "center": [0, 0, 0], "radius": 2, "palette": 1}], "k": 100}),
        json!({"shapes": [{"shape": "sphere", "center": [0, 0, 0], "radius": 2000, "palette": 1}]}),
    ] {
        assert!(
            apply(&mut session, "voxel.blend_shapes", &bad).is_err(),
            "{bad}"
        );
    }
    assert!(session.active().cells.is_empty());
}

#[test]
fn the_snowman_example_blends_every_part() {
    let mut session = Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("fixtures/examples/blob_snowman.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let count = |slot: u8| {
        session
            .active()
            .cells
            .values()
            .filter(|s| **s == slot)
            .count()
    };
    for slot in 1..=5u8 {
        assert!(count(slot) > 0, "slot {slot}");
    }
    // It stands on z 0, flat.
    let cells = &session.active().cells;
    assert!(cells.keys().all(|c| c.2 >= 0));
    assert!(cells.keys().filter(|c| c.2 == 0).count() > 20);
    // Both arms: mirrored about x 0.
    assert!(cells.iter().any(|(c, s)| *s == 4 && c.0 > 8));
    assert!(cells.iter().any(|(c, s)| *s == 4 && c.0 < -8));
}

#[test]
fn the_panel_blends_its_lines() {
    let mut session = Session::new();
    session.replace_empty();
    session.organic.blend.shapes =
        "sphere 0 0 0 4 slot=2\nsphere 7 0 0 3 k=4\nbox 0 -4 0 3 3 3 sub rot=0,0,45\ntorus 0 0 6 3 1".into();
    session.blend_from_panel();
    assert!(
        session.status.starts_with("Blended 4 shapes"),
        "{}",
        session.status
    );
    assert!(session.active().cells.values().any(|s| *s == 2));
    assert!(!session.active().cells.contains_key(&(0, -4, 0)));
    session.organic.blend.shapes = "cone 0 0 0 1".into();
    session.blend_from_panel();
    assert!(
        session.status.contains("must start with"),
        "{}",
        session.status
    );
    session.organic.blend.shapes = "sphere 0 0 0".into();
    session.blend_from_panel();
    assert!(
        session.status.contains("takes 4 to 4"),
        "{}",
        session.status
    );
}
