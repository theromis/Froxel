use serde_json::json;

use super::*;
use crate::export::{counts, export_with, glb_triangles, AoTarget, MeshExport};
use crate::state::Session;

fn session_of(cells: &[Cell]) -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for cell in cells {
        session.objects[0].cells.insert(*cell, 1);
    }
    session
}

fn options(bake: Option<Bake>) -> MeshExport {
    MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: true,
        ao_strength: 0.0,
        ao_radius: crate::bridge::DEFAULT_AO_RADIUS,
        ao_target: AoTarget::Auto,
        bake,
    }
}

fn parsed(value: serde_json::Value) -> Bake {
    Bake::parse(Some(&value)).unwrap().unwrap()
}

/// The light on one face, per channel. Faces are +X -X +Y -Y +Z -Z.
fn light(session: &Session, bake: &Bake, face: usize, cell: Cell) -> [f32; 3] {
    session_light(session, &options(Some(bake.clone())), bake, &[(face, cell)])[0]
}

const UP: usize = 4;
const EAST: usize = 0;
const WEST: usize = 1;

/// A 9x9 floor at z = 0.
fn floor() -> Vec<Cell> {
    let mut cells = Vec::new();
    for x in -4..=4 {
        for y in -4..=4 {
            cells.push((x, y, 0));
        }
    }
    cells
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn glb(session: &Session, options: &MeshExport, name: &str) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("froxel-bake-{}-{name}.glb", std::process::id()));
    export_with(session, &path, options).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    bytes
}

/// The hashes are of files written by the binary from before baking existed
/// (`froxel export` at 367814e), so these pin the whole file, not one branch.
#[test]
fn without_a_bake_the_file_is_byte_for_byte_what_it_was() {
    let mut session = Session::new();
    let vox = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/examples/stacked_blocks.vox"
    );
    session.open_vox(std::path::Path::new(vox)).unwrap();
    let plain = MeshExport {
        color_atlas: false,
        ..options(None)
    };
    let cases = [
        ("plain", plain.clone(), 0x10a3_37ee_3790_8310u64),
        (
            "merge",
            MeshExport {
                merge: true,
                ..plain
            },
            0x433b_ac36_12b7_25d5,
        ),
        ("atlas", options(None), 0x95c4_e1f6_0270_cb99),
        (
            "atlasao",
            MeshExport {
                ao_strength: 0.5,
                ..options(None)
            },
            0x381b_022b_974a_872f,
        ),
    ];
    for (name, options, want) in cases {
        assert_eq!(fnv(&glb(&session, &options, name)), want, "{name}");
    }
    // The defaults a caller gets from the tool parser are off too.
    assert_eq!(crate::remote::mesh_options(&json!({})).unwrap().bake, None);
    let params = crate::remote::mesh_params(&options(None));
    assert!(params.get("bake").is_none());
}

#[test]
fn the_option_parses_and_round_trips() {
    assert_eq!(Bake::parse(None).unwrap(), None);
    assert_eq!(Bake::parse(Some(&json!(false))).unwrap(), None);
    assert_eq!(Bake::parse(Some(&json!(null))).unwrap(), None);
    assert_eq!(parsed(json!(true)), Bake::default());
    assert_eq!(parsed(json!({})), Bake::default());
    // Occlusion alone by default; naming a sun turns it on.
    assert_eq!(Bake::default().sun_energy, 0.0);
    assert_eq!(parsed(json!({"sunColor": "#ffffff"})).sun_energy, 1.0);
    let pond = parsed(json!({"preset": "old_pond", "sunSoftness": 2}));
    assert_eq!(pond.sun_energy, 1.45);
    assert_eq!(pond.ambient_energy, 1.1);
    assert_eq!(pond.sun_rays, 16);
    let over = parsed(json!({"preset": "old_pond", "ambientEnergy": 0.5, "sunRays": 4}));
    assert_eq!(over.ambient_energy, 0.5);
    assert_eq!(over.sun_rays, 4);
    for bad in [
        json!({"rays": 0}),
        json!({"rays": 5000}),
        json!({"occlusion": 2}),
        json!({"distance": 0}),
        json!({"exposure": 1}),
        json!({"sunSoftness": 90}),
        json!({"preset": "viewport"}),
        json!({"preset": "nope"}),
        json!({"lightingPresets": {}}),
        json!(3),
    ] {
        assert!(Bake::parse(Some(&bad)).is_err(), "{bad}");
    }
    // What a reply reports is what the option takes.
    let options = crate::remote::mesh_options(&json!({"colorAtlas": true, "bake": pond.json()}));
    let again = options.unwrap().bake.unwrap();
    assert_eq!(again.sun_color, pond.sun_color);
    assert_eq!(again.sun_rays, pond.sun_rays);
    assert_eq!(again.distance, pond.distance);
}

#[test]
fn the_grid_finds_what_a_small_step_march_finds() {
    let mut cells = Vec::new();
    let mut state = 12345u32;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        state >> 8
    };
    for _ in 0..400 {
        cells.push((
            (next() % 40) as i32 - 20,
            (next() % 40) as i32 - 20,
            (next() % 40) as i32 - 3,
        ));
    }
    let grid = Grid::new(&cells);
    let solid: std::collections::HashSet<Cell> = cells.iter().copied().collect();
    let mut hits = 0;
    for _ in 0..3000 {
        let mut coordinate = || f64::from(next() % 6000) / 100.0 - 30.0;
        let origin = [coordinate(), coordinate(), coordinate()];
        let dir = unit([coordinate(), coordinate(), coordinate()]);
        let reach = 50.0;
        let found = grid.hit(origin, dir, reach);
        let mut marched = None;
        let mut t = 0.0;
        while t < reach {
            let p = add(origin, dir, t);
            let cell = (
                p[0].floor() as i32,
                p[1].floor() as i32,
                p[2].floor() as i32,
            );
            if solid.contains(&cell) {
                marched = Some(t);
                break;
            }
            t += 0.004;
        }
        match (found, marched) {
            (Some(a), Some(b)) => {
                assert!((a - b).abs() < 0.01, "{origin:?} {dir:?}: {a} and {b}");
                hits += 1;
            }
            (None, None) => {}
            // The march can step over a corner the grid clips, or the reverse,
            // only within its step of an edge of the reach or a cell.
            (Some(a), None) => assert!(a > reach - 0.01 || grazes(&solid, origin, dir, a)),
            (None, Some(b)) => assert!(b > reach - 0.01, "{origin:?} {dir:?}: missed {b}"),
        }
    }
    assert!(hits > 100, "only {hits} hits");
    assert_eq!(Grid::new(&[]).hit([0.0; 3], [1.0, 0.0, 0.0], 9.0), None);
}

/// The ray stays in a solid cell for under the march's step.
fn grazes(
    solid: &std::collections::HashSet<Cell>,
    origin: [f64; 3],
    dir: [f64; 3],
    t: f64,
) -> bool {
    let p = add(origin, dir, t + 0.0045);
    let cell = (
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    );
    !solid.contains(&cell)
}

#[test]
fn an_open_cube_keeps_its_colours_under_a_white_ambient() {
    let mut session = session_of(&[(0, 0, 0)]);
    session.palette[1] = [180, 90, 40, 255];
    let bake = Bake::default();
    for face in 0..6 {
        assert_eq!(light(&session, &bake, face, (0, 0, 0)), [1.0; 3], "{face}");
    }
    let baked = glb(&session, &options(Some(bake)), "cube-baked");
    let plain = glb(&session, &options(None), "cube-plain");
    assert_eq!(
        glb_triangles(&baked).unwrap(),
        glb_triangles(&plain).unwrap()
    );
    let png = |bytes: &[u8]| {
        let at = bytes
            .windows(8)
            .position(|w| w == [137, 80, 78, 71, 13, 10, 26, 10])
            .unwrap();
        bytes[at..].to_vec()
    };
    assert_eq!(png(&baked), png(&plain));
}

#[test]
fn occlusion_darkens_a_pit_more_than_a_step_and_reaches_only_its_distance() {
    let mut cells = floor();
    // A wall along x = 1, three high.
    for y in -4..=4 {
        for z in 1..=3 {
            cells.push((1, y, z));
        }
    }
    let session = session_of(&cells);
    let bake = Bake::default();
    let beside = light(&session, &bake, UP, (0, 0, 0))[0];
    let farther = light(&session, &bake, UP, (-3, 0, 0))[0];
    let on_top = light(&session, &bake, UP, (1, 0, 3))[0];
    assert_eq!(on_top, 1.0);
    assert!(beside < farther, "{beside} {farther}");
    assert!(farther < 1.0);
    assert!(beside > 0.3, "half the sky is open: {beside}");
    // Past the distance, the wall is not there.
    let short = Bake {
        distance: 1.5,
        ..Bake::default()
    };
    assert_eq!(light(&session, &short, UP, (-3, 0, 0))[0], 1.0);
    // Occlusion 0 is no occlusion, and a half keeps half the loss.
    let none = Bake {
        occlusion: 0.0,
        ..Bake::default()
    };
    assert_eq!(light(&session, &none, UP, (0, 0, 0))[0], 1.0);
    let half = Bake {
        occlusion: 0.5,
        ..Bake::default()
    };
    let halved = light(&session, &half, UP, (0, 0, 0))[0];
    assert!((halved - (1.0 + beside) / 2.0).abs() < 1e-5);
}

#[test]
fn the_sun_lights_facing_faces_and_casts_a_shadow_away_from_itself() {
    let mut cells = floor();
    for z in 1..=4 {
        cells.push((0, 0, z));
    }
    let session = session_of(&cells);
    // Sun from +X, 45 degrees up, white of energy 1 and no ambient.
    let bake = parsed(json!({
        "sunDirection": [1, 0, 1],
        "sunEnergy": 1,
        "ambientEnergy": 0,
        "sunColor": "#ffffff"
    }));
    let cosine = std::f32::consts::FRAC_1_SQRT_2;
    // Open floor on the sun's side.
    assert!((light(&session, &bake, UP, (3, 0, 0))[0] - cosine).abs() < 1e-5);
    // The floor behind the pillar is in its shadow; beside the shadow it is lit.
    assert_eq!(light(&session, &bake, UP, (-2, 0, 0))[0], 0.0);
    assert!((light(&session, &bake, UP, (-2, 3, 0))[0] - cosine).abs() < 1e-5);
    // The pillar's sunny side, and its far side that faces away.
    assert!((light(&session, &bake, EAST, (0, 0, 2))[0] - cosine).abs() < 1e-5);
    assert_eq!(light(&session, &bake, WEST, (0, 0, 2))[0], 0.0);
    // Ambient and colour add in linear light.
    let tinted = parsed(json!({
        "sunDirection": [1, 0, 1],
        "sunEnergy": 2,
        "sunColor": [1, 0, 0],
        "ambientEnergy": 0.5,
        "ambientColor": [0, 0, 1],
        "occlusion": 0
    }));
    let lit = light(&session, &tinted, UP, (3, 0, 0));
    assert!((lit[0] - 2.0 * cosine).abs() < 1e-5);
    assert_eq!(lit[1], 0.0);
    assert_eq!(lit[2], 0.5);
    assert_eq!(light(&session, &tinted, UP, (-2, 0, 0)), [0.0, 0.0, 0.5]);
}

#[test]
fn a_soft_sun_leaves_a_penumbra_where_a_hard_one_has_an_edge() {
    let mut cells = floor();
    for z in 1..=4 {
        cells.push((0, 0, z));
    }
    let session = session_of(&cells);
    let hard = parsed(json!({"sunDirection": [1, 0, 1], "ambientEnergy": 0, "sunEnergy": 1}));
    let soft = Bake {
        sun_softness: 8.0,
        sun_rays: 64,
        ..hard.clone()
    };
    let shadow_row: Vec<[f32; 2]> = (-4..0)
        .flat_map(|x| (-1..=1).map(move |y| (x, y)))
        .map(|(x, y)| {
            [
                light(&session, &hard, UP, (x, y, 0))[0],
                light(&session, &soft, UP, (x, y, 0))[0],
            ]
        })
        .collect();
    let cosine = std::f32::consts::FRAC_1_SQRT_2;
    let between = |value: f32| value > 0.01 && value < cosine - 0.01;
    assert!(shadow_row.iter().all(|pair| !between(pair[0])));
    assert!(shadow_row.iter().any(|pair| between(pair[1])));
}

#[test]
fn light_passes_translucent_slots_and_other_objects_shadow_each_other() {
    let mut session = session_of(&floor());
    session.palette[2] = [200, 200, 255, 120];
    session.objects[0].cells.insert((0, 0, 1), 2);
    let bake = Bake::default();
    assert_eq!(light(&session, &bake, UP, (0, 0, 0))[0], 1.0);
    session.palette[2][3] = 255;
    assert!(light(&session, &bake, UP, (0, 0, 0))[0] < 0.2);
    // A second object, moved over the floor, blocks the sky of the first.
    let mut session = session_of(&floor());
    let mut other = session.objects[0].clone();
    other.cells.clear();
    other.cells.insert((0, 0, 0), 1);
    other.translation = (1, 0, 1);
    session.objects.push(other);
    assert!(light(&session, &bake, UP, (1, 0, 0))[0] < 0.2);
    assert_eq!(light(&session, &bake, UP, (-4, 4, 0))[0], 1.0);
    // Hidden, it does not.
    session.objects[1].hidden = true;
    assert_eq!(light(&session, &bake, UP, (1, 0, 0))[0], 1.0);
}

#[test]
fn the_export_is_deterministic_marked_unlit_and_keeps_its_triangles() {
    let mut session = Session::new();
    let vox = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/examples/stacked_blocks.vox"
    );
    session.open_vox(std::path::Path::new(vox)).unwrap();
    let bake = parsed(json!({"preset": "old_pond", "sunSoftness": 3, "rays": 16}));
    let baked = options(Some(bake));
    let first = glb(&session, &baked, "same-1");
    assert_eq!(first, glb(&session, &baked, "same-2"));
    let plain = glb(&session, &options(None), "same-3");
    assert_eq!(
        glb_triangles(&first).unwrap(),
        glb_triangles(&plain).unwrap()
    );
    assert_ne!(first, plain);
    assert_eq!(
        counts(&session, &baked).unwrap(),
        counts(&session, &options(None)).unwrap()
    );
    let root = crate::export::gltf_json(&first).unwrap();
    assert_eq!(root["extensionsUsed"], json!(["KHR_materials_unlit"]));
    let material = &root["materials"][0];
    assert_eq!(material["name"], "voxel-baked");
    assert!(material["extensions"]["KHR_materials_unlit"].is_object());
    assert_eq!(material["extras"]["froxelBake"]["sunRays"], 16);
    // The importer reads what was written.
    let path = std::env::temp_dir().join(format!("froxel-bake-{}-read.glb", std::process::id()));
    std::fs::write(&path, &first).unwrap();
    let out = path.with_extension("png");
    let preview = crate::preview::Preview {
        width: 64,
        height: 64,
        ..Default::default()
    };
    let frame = crate::preview::write_glb(&path, &preview, &out).unwrap();
    assert_eq!(frame.preview_triangles, glb_triangles(&first).unwrap());
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_bake_without_the_atlas_is_refused() {
    let session = session_of(&[(0, 0, 0)]);
    let bad = MeshExport {
        color_atlas: false,
        ..options(Some(Bake::default()))
    };
    assert!(counts(&session, &bad).unwrap_err().contains("colorAtlas"));
    let path = std::env::temp_dir().join(format!("froxel-bake-{}-no.glb", std::process::id()));
    assert!(export_with(&session, &path, &bad).is_err());
    assert!(!path.exists());
}
