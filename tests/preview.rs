use super::*;

/// One object: a 6×2×3 box of slot 1, with a second object of slot 2, a
/// 2×2×2 cube, 20 cells along +X.
fn scene() -> Session {
    let mut session = Session::new();
    session.palette[1] = [200, 120, 60, 255];
    session.palette[2] = [40, 160, 220, 255];
    let cells = &mut session.objects[0].cells;
    cells.clear();
    for x in 0..6 {
        for y in 0..2 {
            for z in 0..3 {
                cells.insert((x, y, z), 1);
            }
        }
    }
    let index = session
        .add_object(Some("cube"), [20, 0, 0], None, false)
        .unwrap();
    for x in 0..2 {
        for y in 0..2 {
            for z in 0..2 {
                session.objects[index].cells.insert((x, y, z), 2);
            }
        }
    }
    session
}

const BG: [u8; 4] = [1, 2, 3, 255];

/// Columns and rows of the first and last pixel that is not `BG`.
fn extent(frame: &Frame, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if frame.rgba[i..i + 4] != BG {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    (x0, x1, y0, y1)
}

fn pixel(frame: &Frame, w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    frame.rgba[i..i + 4].try_into().unwrap()
}

fn base(width: u32, height: u32) -> Preview {
    Preview {
        width,
        height,
        background: BG,
        antialias: false,
        ..Default::default()
    }
}

#[test]
fn orthographic_front_fills_the_frame_less_the_margin_and_centres() {
    let session = scene();
    // Front view of everything: 22 cells wide, 3 tall.
    let preview = Preview {
        yaw: -90.0,
        pitch: 0.0,
        orthographic: true,
        margin: 0.1,
        ..base(220, 100)
    };
    let frame = render(&session, &preview).unwrap();
    let (x0, x1, y0, y1) = extent(&frame, 220, 100);
    assert_eq!((x0, x1), (22, 197));
    // 176 pixels for 22 cells: 8 per cell, 24 rows centred in 100.
    assert_eq!(frame.pixels_per_voxel, Some(8.0));
    assert_eq!((y0, y1), (38, 61));
}

#[test]
fn perspective_fit_touches_the_margin_and_is_centred() {
    let session = scene();
    for (yaw, pitch) in [(30.0, 25.0), (-35.0, 30.0), (-90.0, 0.0), (10.0, 80.0)] {
        let preview = Preview {
            yaw,
            pitch,
            margin: 0.1,
            ..base(400, 200)
        };
        let frame = render(&session, &preview).unwrap();
        let (x0, x1, y0, y1) = extent(&frame, 400, 200);
        assert!(
            x0 >= 39 && x1 <= 360 && y0 >= 19 && y1 <= 180,
            "{yaw} {pitch}: {x0} {x1} {y0} {y1}"
        );
        let touches_x = x0 <= 41 && x1 >= 358;
        let touches_y = y0 <= 21 && y1 >= 178;
        assert!(touches_x || touches_y, "{yaw} {pitch}: {x0} {x1} {y0} {y1}");
        assert!(
            ((x0 + x1) as i32 - 399).abs() <= 3,
            "{yaw} {pitch}: {x0} {x1}"
        );
        assert!(
            ((y0 + y1) as i32 - 199).abs() <= 3,
            "{yaw} {pitch}: {y0} {y1}"
        );
        assert_eq!(frame.pixels_per_voxel, None);
    }
}

#[test]
fn faces_are_lit_like_the_viewport_shader() {
    let session = scene();
    let preview = Preview {
        yaw: -90.0,
        pitch: 90.0,
        orthographic: true,
        object: Some(0),
        isolate: true,
        ..base(64, 64)
    };
    let frame = render(&session, &preview).unwrap();
    let sun = session.sun_dir();
    let lit = shade(
        [200.0 / 255.0, 120.0 / 255.0, 60.0 / 255.0],
        Vec3::Z,
        sun,
        session.sun_intensity,
    );
    let expected = lit.map(to_srgb);
    assert_eq!(pixel(&frame, 64, 32, 32)[..3], expected);
    // A side face of the same slot is darker than the top under the default sun.
    let side = Preview {
        yaw: -90.0,
        pitch: 0.0,
        ..preview
    };
    let frame_side = render(&session, &side).unwrap();
    let top = pixel(&frame, 64, 32, 32);
    let front = pixel(&frame_side, 64, 32, 32);
    assert!(front[0] < top[0], "{front:?} {top:?}");
}

#[test]
fn object_frames_one_object_and_isolate_hides_the_rest() {
    let session = scene();
    let cube = find_object(&session, "cube").unwrap();
    assert_eq!(find_object(&session, "1").unwrap(), cube);
    assert!(find_object(&session, "7").is_err());
    assert!(find_object(&session, "nope").is_err());
    let preview = Preview {
        yaw: -90.0,
        pitch: 0.0,
        orthographic: true,
        object: Some(cube),
        ..base(80, 80)
    };
    let framed = render(&session, &preview).unwrap();
    let centre = pixel(&framed, 80, 40, 40);
    assert!(centre[2] > centre[0], "the cube is blue: {centre:?}");
    let isolated = render(
        &session,
        &Preview {
            isolate: true,
            margin: 0.3,
            ..preview.clone()
        },
    )
    .unwrap();
    assert_eq!(pixel(&isolated, 80, 2, 40), BG);
    // Without isolate the box still shows to the left of the cube: 16
    // pixels per cell puts its right edge 240 pixels left of the centre.
    let shown = render(
        &session,
        &Preview {
            margin: 0.3,
            width: 800,
            ..preview
        },
    )
    .unwrap();
    let left = pixel(&shown, 800, 100, 40);
    assert!(left != BG && left[0] > left[2], "{left:?}");
}

#[test]
fn transparent_background_and_antialiased_edges() {
    let session = scene();
    let preview = Preview {
        background: [0, 0, 0, 0],
        antialias: true,
        ..Default::default()
    };
    let frame = render(
        &session,
        &Preview {
            width: 120,
            height: 90,
            ..preview
        },
    )
    .unwrap();
    assert_eq!(pixel(&frame, 120, 0, 0), [0, 0, 0, 0]);
    let alphas: Vec<u8> = frame.rgba.chunks(4).map(|p| p[3]).collect();
    assert!(alphas.contains(&255));
    assert!(
        alphas.iter().any(|a| *a > 0 && *a < 255),
        "edges are blended"
    );
}

#[test]
fn check_refuses_bad_settings_and_an_empty_document() {
    for bad in [
        Preview {
            width: 0,
            ..Default::default()
        },
        Preview {
            height: MAX_SIZE + 1,
            ..Default::default()
        },
        Preview {
            pitch: 91.0,
            ..Default::default()
        },
        Preview {
            margin: 0.5,
            ..Default::default()
        },
        Preview {
            fov: 5.0,
            ..Default::default()
        },
        Preview {
            isolate: true,
            ..Default::default()
        },
    ] {
        assert!(check(&bad).is_err(), "{bad:?}");
    }
    let mut empty = Session::new();
    for object in &mut empty.objects {
        object.cells.clear();
    }
    assert!(render(&empty, &base(8, 8)).is_err());
}
