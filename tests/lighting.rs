use super::*;
use crate::preview::{render, Preview};
use crate::state::Session;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 2e-3
}

/// An `n`-slice LUT image, slices side by side, mapping each colour by `f`.
fn lut_image(n: u32, f: impl Fn([u8; 3]) -> [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (n * n, n);
    let mut rgba = vec![0; (w * h * 4) as usize];
    let level = |i: u32| (i * 255 / (n - 1)) as u8;
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let at = ((g * w + b * n + r) * 4) as usize;
                let out = f([level(r), level(g), level(b)]);
                rgba[at..at + 3].copy_from_slice(&out);
                rgba[at + 3] = 255;
            }
        }
    }
    (w, h, rgba)
}

#[test]
fn the_old_pond_sun_is_the_third_basis_column_in_froxel_axes() {
    // The Transform3D of Environment/SunNoShadow in old_pond_region.tscn.
    let basis = [
        -0.9191606,
        0.25342205,
        -0.30153123,
        0.0,
        0.765535,
        0.64339423,
        0.39388302,
        0.5913826,
        -0.70364964,
        0.0,
        0.0,
        0.0,
    ];
    let sun = godot_sun(&basis).unwrap();
    // Godot +Z (-0.302, 0.643, -0.704), Y up, is Froxel (x, -z, y).
    assert!(close(sun, Vec3::new(-0.3015, 0.7036, 0.6434)), "{sun}");
    // About 40 degrees above the horizon.
    assert!((sun.z.asin().to_degrees() - 40.0).abs() < 0.5);
    assert_eq!(godot_sun(&basis[..9]).unwrap(), sun);
    assert!(godot_sun(&basis[..6]).is_err());
}

#[test]
fn presets_resolve_by_name_field_by_field_and_refuse_unknown_ones() {
    let presets = Presets::load(None).unwrap();
    assert!(presets.resolve(&json!("viewport")).unwrap().is_none());
    let pond = presets.resolve(&json!("old_pond")).unwrap().unwrap();
    assert_eq!(pond.name, "old_pond");
    assert_eq!(pond.sun_color, [1.0, 0.76, 0.48]);
    assert_eq!(pond.sun_energy, 1.45);
    assert_eq!(pond.ambient_color, [0.62, 0.72, 0.9]);
    assert_eq!(pond.ambient_energy, 1.1);
    assert_eq!(pond.exposure, 1.05);
    assert_eq!(pond.background, Some([0x0a, 0x3f, 0x57, 255]));
    assert!(close(
        pond.sun_direction.unwrap(),
        Vec3::new(-0.3015, 0.7036, 0.6434)
    ));
    let err = presets.resolve(&json!("dusk")).err().unwrap();
    assert!(
        err.contains("viewport") && err.contains("old_pond"),
        "{err}"
    );

    // The call's presets override a built-in one field by field, and a new
    // name can start from another with preset.
    let call = json!({"lighting": {
        "old_pond": {"sunEnergy": 2.0},
        "dusk": {"preset": "old_pond", "sunColor": "#ff8040", "sunDirection": [1, 0, 1]}
    }});
    let presets = Presets::load(Some(&call)).unwrap();
    let pond = presets.resolve(&json!("old_pond")).unwrap().unwrap();
    assert_eq!(pond.sun_energy, 2.0);
    assert_eq!(pond.sun_color, [1.0, 0.76, 0.48]);
    let dusk = presets.resolve(&json!("dusk")).unwrap().unwrap();
    assert_eq!(dusk.sun_energy, 2.0, "dusk starts from the call's old_pond");
    assert_eq!(dusk.sun_color, [1.0, 128.0 / 255.0, 64.0 / 255.0]);
    assert!(close(
        dusk.sun_direction.unwrap(),
        Vec3::new(1.0, 0.0, 1.0).normalize()
    ));
    let names = presets.names();
    assert!(names.contains(&"dusk".to_string()) && names.ends_with(&["viewport".to_string()]));

    // An inline object over a preset, or over the defaults.
    let inline = presets
        .resolve(&json!({"preset": "old_pond", "exposure": 1.0}))
        .unwrap()
        .unwrap();
    assert_eq!((inline.exposure, inline.sun_energy), (1.0, 2.0));
    let bare = presets
        .resolve(&json!({"ambientEnergy": 0.5}))
        .unwrap()
        .unwrap();
    assert_eq!(bare.sun_direction, None, "the document's sun");
    assert_eq!(bare.ambient_energy, 0.5);
    for bad in [
        json!({"sunEnergy": -1}),
        json!({"sunColor": [1, 2]}),
        json!({"glow": 1}),
        json!({"sunDirection": [0, 0, 0]}),
        json!({"preset": "viewport", "sunEnergy": 1, "lut": 3}),
        json!(4),
    ] {
        assert!(presets.resolve(&bad).is_err(), "{bad}");
    }
    let looped = json!({"a": {"preset": "b"}, "b": {"preset": "a"}});
    let presets = Presets::load(Some(&looped)).unwrap();
    assert!(presets.resolve(&json!("a")).err().unwrap().contains("loop"));
}

#[test]
fn a_presets_file_reads_its_lut_beside_it() {
    let dir = std::env::temp_dir().join(format!("froxel-lighting-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("grades")).unwrap();
    let (w, h, rgba) = lut_image(5, |[r, g, b]| [255 - r, 255 - g, 255 - b]);
    crate::trace::write_rgba_png(&dir.join("grades/invert.png"), w, h, &rgba).unwrap();
    let file = dir.join("presets.json");
    std::fs::write(
        &file,
        r#"{"inverted": {"preset": "old_pond", "lut": "grades/invert.png"}}"#,
    )
    .unwrap();
    let presets = Presets::load(Some(&json!(file.display().to_string()))).unwrap();
    let lighting = presets.resolve(&json!("inverted")).unwrap().unwrap();
    assert_eq!(lighting.lut_path, Some(dir.join("grades/invert.png")));
    assert_eq!(lighting.lut.unwrap().apply([10, 200, 128]), [245, 55, 127]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_lut_is_trilinear_and_takes_either_slice_layout() {
    let (w, h, rgba) = lut_image(9, |rgb| rgb);
    let lut = Lut::from_rgba(w, h, &rgba).unwrap();
    for rgb in [[0, 0, 0], [255, 255, 255], [10, 77, 200], [128, 128, 128]] {
        let out = lut.apply(rgb);
        for c in 0..3 {
            assert!(
                (out[c] as i32 - rgb[c] as i32).abs() <= 1,
                "{rgb:?} {out:?}"
            );
        }
    }
    // Stacked slices: the same table transposed into one column.
    let warm = |[r, g, b]: [u8; 3]| [r.saturating_add(20), g, b / 2];
    let (w, h, side) = lut_image(5, warm);
    let mut stacked = vec![0; side.len()];
    for b in 0..5 {
        for g in 0..5 {
            for r in 0..5 {
                let from = ((g * w + b * 5 + r) * 4) as usize;
                let to = (((b * 5 + g) * 5 + r) * 4) as usize;
                stacked[to..to + 4].copy_from_slice(&side[from..from + 4]);
            }
        }
    }
    let a = Lut::from_rgba(w, h, &side).unwrap();
    let b = Lut::from_rgba(5, 25, &stacked).unwrap();
    assert_eq!(a.apply([255, 0, 128]), b.apply([255, 0, 128]));
    assert_eq!(a.apply([255, 255, 255]), [255, 255, 127]);
    assert!(Lut::from_rgba(10, 4, &vec![0; 160]).is_err());
}

/// A 4×4×4 block of one colour.
fn block() -> Session {
    let mut session = Session::new();
    session.palette[1] = [180, 140, 100, 255];
    let cells = &mut session.objects[0].cells;
    cells.clear();
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                cells.insert((x, y, z), 1);
            }
        }
    }
    session
}

fn centre(rgba: &[u8], w: u32, h: u32) -> [u8; 3] {
    let at = (((h / 2) * w + w / 2) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

#[test]
fn preview_lights_a_face_as_godot_and_grades_it() {
    let session = block();
    let lighting = Lighting {
        sun_direction: Some(Vec3::Z),
        sun_color: [1.0, 0.76, 0.48],
        sun_energy: 0.5,
        ambient_color: [0.62, 0.72, 0.9],
        ambient_energy: 0.3,
        exposure: 1.05,
        background: Some([1, 2, 3, 255]),
        ..Default::default()
    };
    let top = Preview {
        width: 32,
        height: 32,
        yaw: -90.0,
        pitch: 90.0,
        orthographic: true,
        antialias: false,
        lighting: Some(lighting.clone()),
        ..Default::default()
    };
    let frame = render(&session, &top).unwrap();
    // The top faces the sun: albedo × (sun × 0.5 + ambient × 0.3) × 1.05.
    let albedo = [180.0, 140.0, 100.0].map(|c: f32| to_linear(c / 255.0));
    let expected: Vec<u8> = (0..3)
        .map(|i| {
            let light =
                to_linear(lighting.sun_color[i]) * 0.5 + to_linear(lighting.ambient_color[i]) * 0.3;
            let c = (albedo[i] * light * 1.05).clamp(0.0, 1.0);
            let s = if c <= 0.003_130_8 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0 + 0.5) as u8
        })
        .collect();
    assert_eq!(centre(&frame.rgba, 32, 32)[..], expected[..]);
    // A side face sees only the ambient, which is blue: darker and bluer.
    let side = Preview {
        pitch: 0.0,
        ..top.clone()
    };
    let lit_side = centre(&render(&session, &side).unwrap().rgba, 32, 32);
    let blue = |rgb: &[u8]| rgb[2] as f32 / rgb[0] as f32;
    assert!(
        lit_side[0] < expected[0] && blue(&lit_side) > blue(&expected),
        "{lit_side:?}"
    );
    // The grade is applied last.
    let (w, h, rgba) = lut_image(17, |[r, g, b]| [255 - r, 255 - g, 255 - b]);
    let graded = Preview {
        lighting: Some(Lighting {
            lut: Some(Arc::new(Lut::from_rgba(w, h, &rgba).unwrap())),
            ..lighting
        }),
        ..top
    };
    let inverted = centre(&render(&session, &graded).unwrap().rgba, 32, 32);
    for c in 0..3 {
        assert!((inverted[c] as i32 - (255 - expected[c] as i32)).abs() <= 1);
    }
}
