use super::*;

fn ball(radius: i32, slot: u8) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for z in -radius..=radius {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y + z * z <= radius * radius {
                    cells.insert((x, y, z), slot);
                }
            }
        }
    }
    cells
}

fn rod(x0: i32, x1: i32) -> HashMap<Cell, u8> {
    let mut cells = HashMap::new();
    for x in x0..x1 {
        for z in -4..=4 {
            for y in -4..=4 {
                if y * y + z * z <= 16 {
                    cells.insert((x, y, z), 1);
                }
            }
        }
    }
    cells
}

fn inside(b: &StrandBox, p: Vec3) -> bool {
    let local = b.matrix().inverse().transform_point3(p);
    local.abs().max_element() <= 1.0
}

#[test]
fn the_same_settings_give_the_same_bricks() {
    let cells = ball(8, 1);
    let skin = Skin::default();
    let a = skin_boxes(&cells, &skin, (0, 0, 0));
    assert!(a.len() > 20, "{}", a.len());
    assert_eq!(a, skin_boxes(&cells, &skin, (0, 0, 0)));
    let other = Skin {
        seed: 9,
        ..skin.clone()
    };
    assert_ne!(a, skin_boxes(&cells, &other, (0, 0, 0)));
}

#[test]
fn bricks_cover_the_surface() {
    // A fish-sized body: 60 long, 24 tall, 20 wide.
    let mut cells = HashMap::new();
    for x in -30i32..=30 {
        for y in -10i32..=10 {
            for z in -12i32..=12 {
                let (fx, fy, fz) = (x as f32 / 30.0, y as f32 / 10.0, z as f32 / 12.0);
                if fx * fx + fy * fy + fz * fz <= 1.0 {
                    cells.insert((x, y, z), 1u8);
                }
            }
        }
    }
    let skin = Skin {
        axis: SkinAxis::X,
        ..Skin::default()
    };
    let boxes = skin_boxes(&cells, &skin, (0, 0, 0));
    let mut faces = 0;
    let mut covered = 0;
    for c in cells.keys() {
        for n in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
            let next = (c.0 + n.x as i32, c.1 + n.y as i32, c.2 + n.z as i32);
            if cells.contains_key(&next) {
                continue;
            }
            faces += 1;
            // Seen from outside the body (along the ellipsoid's normal),
            // the face is behind a brick or inside one.
            let p = Vec3::new(c.0 as f32, c.1 as f32, c.2 as f32) + 0.5 + n * 0.49;
            let out = (p / Vec3::new(900.0, 100.0, 144.0)).normalize();
            let hidden = boxes.iter().any(|b| {
                let m = b.matrix();
                inside(b, p) || crate::tools::ray_box(&m.inverse(), &m, p, out).is_some()
            });
            if hidden {
                covered += 1;
            }
        }
    }
    eprintln!("{covered} of {faces}");
    assert!(covered * 10 >= faces * 7, "{covered} of {faces}");
}

#[test]
fn thin_parts_and_bare_slots_stay_cells() {
    let mut plate = HashMap::new();
    for x in 0..20 {
        for y in 0..20 {
            for z in 0..2 {
                plate.insert((x, y, z), 1u8);
            }
        }
    }
    assert!(skin_boxes(&plate, &Skin::default(), (0, 0, 0)).is_empty());
    let thin_ok = Skin {
        thin: 0.0,
        ..Skin::default()
    };
    assert!(!skin_boxes(&plate, &thin_ok, (0, 0, 0)).is_empty());

    let mut cells = ball(8, 1);
    for (c, slot) in cells.iter_mut() {
        if c.2 > 4 {
            *slot = 2;
        }
    }
    let skin = Skin {
        bare: vec![2],
        ..Skin::default()
    };
    let boxes = skin_boxes(&cells, &skin, (0, 0, 0));
    assert!(!boxes.is_empty());
    assert!(boxes.iter().all(|b| b.slot == 1));
}

#[test]
fn rig_parts_continue_each_other_rows() {
    let skin = Skin {
        axis: SkinAxis::X,
        ..Skin::default()
    };
    let whole = skin_boxes(&rod(0, 40), &skin, (0, 0, 0));
    // The left half as its own object, moved by its translation.
    let left: HashMap<Cell, u8> = rod(0, 20)
        .into_iter()
        .map(|(c, s)| ((c.0 - 5, c.1 - 1, c.2 + 2), s))
        .collect();
    let part = skin_boxes(&left, &skin, (5, 1, -2));
    let near = |boxes: &[StrandBox], shift: Vec3| -> Vec<[f32; 3]> {
        let mut out: Vec<[f32; 3]> = boxes
            .iter()
            .map(|b| (Vec3::from_array(b.center) + shift).to_array())
            .filter(|c| c[0] > 4.0 && c[0] < 13.0)
            .collect();
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out
    };
    let (a, b) = (
        near(&whole, Vec3::ZERO),
        near(&part, Vec3::new(5.0, 1.0, -2.0)),
    );
    assert!(!a.is_empty());
    assert_eq!(a.len(), b.len());
    for (p, q) in a.iter().zip(&b) {
        assert!(
            (Vec3::from_array(*p) - Vec3::from_array(*q)).length() < 1.0e-3,
            "{p:?} {q:?}"
        );
    }
}

#[test]
fn bad_settings_are_refused() {
    assert!(Skin::default().check().is_ok());
    for bad in [
        Skin {
            length: [3.0, 1.0],
            ..Skin::default()
        },
        Skin {
            width: 0.0,
            ..Skin::default()
        },
        Skin {
            bare: vec![0],
            ..Skin::default()
        },
        Skin {
            tilt: f32::NAN,
            ..Skin::default()
        },
    ] {
        assert!(bad.check().is_err(), "{bad:?}");
    }
}

/// A brick as a one-cube strand, for the prototype render.
fn as_strand(b: &StrandBox) -> crate::tools::Strand {
    let t = Vec3::from_array(b.axes[0]);
    let across = Vec3::from_array(b.axes[1]);
    let left = Vec3::Z.cross(t).try_normalize().unwrap_or(Vec3::Y);
    let up = t.cross(left);
    let twist = across.dot(up).atan2(across.dot(left)).to_degrees();
    let c = Vec3::from_array(b.center) - 0.5;
    let point = |p: Vec3| crate::tools::StrandPoint {
        at: p.to_array(),
        twist,
        scale: 1.0,
    };
    crate::tools::Strand {
        points: vec![point(c - t * b.half[0]), point(c + t * b.half[0])],
        smooth: false,
        width: b.half[1] * 2.0,
        thickness: b.half[2] * 2.0,
        step: b.half[0] * 2.0,
        slots: vec![b.slot],
        ..crate::tools::Strand::default()
    }
}

#[test]
#[ignore]
fn prototype_render() {
    let path = std::env::var("SKIN_VOX").unwrap();
    let out = std::env::var("SKIN_OUT").unwrap();
    let mut session = crate::state::Session::new();
    if path == "egg" {
        let mut cells = HashMap::new();
        for x in -30i32..=30 {
            for y in -10i32..=10 {
                for z in -12i32..=12 {
                    let (fx, fy, fz) = (x as f32 / 30.0, y as f32 / 10.0, z as f32 / 12.0);
                    if fx * fx + fy * fy + fz * fz <= 1.0 {
                        cells.insert((x, y, z), 1 + ((x * 7 + y * 3 + z * 5).rem_euclid(3)) as u8);
                    }
                }
            }
        }
        session.objects[0].cells = cells.into();
    } else if path == "ball" {
        session.objects[0].cells = ball(10, 1).into();
        for (c, slot) in session.objects[0].cells.iter_mut() {
            *slot = 1 + ((c.0 * 7 + c.1 * 3 + c.2 * 5).rem_euclid(3)) as u8;
        }
    } else {
        session.load_path(std::path::Path::new(&path));
    }
    let env = |k: &str, d: f32| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let d = Skin::default();
    let skin = Skin {
        length: [env("SKIN_LO", d.length[0]), env("SKIN_HI", d.length[1])],
        width: env("SKIN_W", d.width),
        height: env("SKIN_H", d.height),
        relief: env("SKIN_RELIEF", d.relief),
        tilt: env("SKIN_TILT", d.tilt),
        smooth: env("SKIN_SMOOTH", d.smooth),
        thin: env("SKIN_THIN", d.thin),
        depth: env("SKIN_DEPTH", d.depth),
        gap: env("SKIN_GAP", d.gap),
        follow: env("SKIN_FOLLOW", d.follow),
        axis: match std::env::var("SKIN_AXIS").as_deref() {
            Ok("x") => SkinAxis::X,
            Ok("y") => SkinAxis::Y,
            Ok("z") => SkinAxis::Z,
            _ => SkinAxis::Auto,
        },
        bare: std::env::var("SKIN_BARE")
            .unwrap_or_default()
            .split(',')
            .filter_map(|v| v.parse().ok())
            .collect(),
        ..Skin::default()
    };
    let only = std::env::var("SKIN_OBJ").unwrap_or_default();
    for o in &session.objects {
        eprintln!("object {} cells {}", o.name, o.cells.len());
    }
    let start = std::time::Instant::now();
    let mut total = 0;
    for object in &mut session.objects {
        if !object.name.contains(&only) {
            continue;
        }
        let boxes = skin_boxes(&object.cells, &skin, (0, 0, 0));
        total += boxes.len();
        object.strands = boxes.iter().map(as_strand).collect();
    }
    eprintln!("{total} bricks in {:?}", start.elapsed());
    for (name, yaw, pitch) in [("a", 110.0, 15.0), ("b", 90.0, 5.0), ("c", 30.0, 35.0)] {
        let preview = crate::preview::Preview {
            width: 1400,
            height: 900,
            margin: 0.02,
            object: std::env::var("SKIN_FOCUS")
                .ok()
                .and_then(|v| v.parse().ok()),
            yaw,
            pitch,
            ..crate::preview::Preview::default()
        };
        crate::preview::write(
            &session,
            &preview,
            std::path::Path::new(&format!("{out}-{name}.png")),
        )
        .unwrap();
    }
}

#[test]
fn the_session_draws_bricks_and_rebuilds_them_after_an_edit() {
    let mut session = crate::state::Session::new();
    session.objects[0].cells = ball(8, 1).into();
    assert!(session.strand_boxes(0).is_empty());
    session.objects[0].skin = Some(Skin::default());
    let first = session.skin_boxes(0);
    assert!(!first.is_empty());
    assert_eq!(session.strand_boxes(0).len(), first.len());
    // Unchanged inputs come from the cache.
    assert!(std::sync::Arc::ptr_eq(&first, &session.skin_boxes(0)));
    // A cut into the ball reshapes the skin.
    for z in 4..=8 {
        for y in -8..=8 {
            for x in -8..=8 {
                session.objects[0].cells.remove(&(x, y, z));
            }
        }
    }
    let after = session.skin_boxes(0);
    assert!(!std::sync::Arc::ptr_eq(&first, &after));
    assert_ne!(*first, *after);
    // Moving the object keeps the bricks where they were on it.
    session.objects[0].translation = (3, 0, 0);
    let moved = session.skin_boxes(0);
    assert!(!std::sync::Arc::ptr_eq(&after, &moved));
    assert_eq!(moved.len(), after.len());
}

#[test]
fn clearing_an_object_drops_its_skin_and_undo_brings_it_back() {
    let mut session = crate::state::Session::new();
    session.objects[0].cells = ball(6, 1).into();
    session.objects[0].skin = Some(Skin::default());
    session.clear_object(0).unwrap();
    assert!(session.objects[0].skin.is_none());
    session.undo();
    assert_eq!(session.objects[0].skin, Some(Skin::default()));
    assert!(!session.skin_boxes(0).is_empty());
}

#[test]
fn a_saved_skin_reopens_with_the_same_bricks() {
    use serde_json::json;
    let mut session = crate::state::Session::new();
    session.objects[0].name = "Fish".into();
    session.objects[0].cells = ball(7, 3).into();
    session.objects[0].translation = (4, -2, 9);
    let skin = Skin {
        axis: SkinAxis::Y,
        length: [1.5, 2.5],
        bare: vec![9, 12],
        seed: 77,
        ..Skin::default()
    };
    session.objects[0].skin = Some(skin.clone());
    let path = std::env::temp_dir().join("froxel-skin-round-trip.vox");
    let _ = std::fs::remove_file(&path);
    let save = json!({"path": path.to_string_lossy()});
    crate::remote::apply(&mut session, "document.save", &save).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(4).any(|w| w == b"MVSK"));
    let mut reopened = crate::state::Session::new();
    crate::remote::apply(&mut reopened, "document.open", &save).unwrap();
    let _ = std::fs::remove_file(&path);
    let fish = reopened
        .objects
        .iter()
        .position(|o| o.name == "Fish")
        .unwrap();
    assert_eq!(reopened.objects[fish].skin, Some(skin));
    let centres = |s: &crate::state::Session, i: usize| -> Vec<[i64; 3]> {
        let mut out: Vec<[i64; 3]> = s
            .strand_world_boxes(i)
            .iter()
            .map(|b| {
                let c = b.matrix.transform_point3(Vec3::ZERO);
                [c.x, c.y, c.z].map(|v| (v * 1000.0).round() as i64)
            })
            .collect();
        out.sort_unstable();
        out
    };
    assert_eq!(centres(&session, 0), centres(&reopened, fish));
}
