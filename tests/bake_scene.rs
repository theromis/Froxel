use serde_json::json;

use super::*;
use crate::export::bake::Bake;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("froxel-bake-scene-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn cube_grid() -> Arc<Grid> {
    Arc::new(Grid::new(&[(0, 0, 0)]))
}

fn block_grid() -> Arc<Grid> {
    let mut cells = Vec::new();
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                cells.push((x, y, z));
            }
        }
    }
    Arc::new(Grid::new(&cells))
}

fn at(translation: [f64; 3], yaw: f64, scale: f64) -> Place {
    Place {
        translation,
        yaw,
        scale,
        lean: None,
        sway: 1.0,
    }
}

/// Light on one face of a placed cell, as the bake computes it.
fn light(world: &World, bake: &Bake, place: &Place, face: usize, cell: Cell) -> [f32; 3] {
    let patch = placed_patch(place, face, cell);
    scene_rays(bake).light(bake, world, &patch, LIFT * place.scale)
}

const EAST: usize = 0; // +X cell face, +X in the scene.
const WEST: usize = 1;
const SOUTH: usize = 3; // -Y cell face, +Z in the scene.
const UP: usize = 4;
const DOWN: usize = 5;

fn save_block(name: &str, cells: &[Cell], slot: u8) -> PathBuf {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    for cell in cells {
        session.objects[0].cells.insert(*cell, slot);
    }
    let path = temp(name);
    session.write_vox(&path).unwrap();
    path
}

fn block_cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                cells.push((x, y, z));
            }
        }
    }
    cells
}

fn glb_json(path: &Path) -> Value {
    crate::export::gltf_json(&std::fs::read(path).unwrap()).unwrap()
}

fn png_of(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let at = bytes
        .windows(8)
        .position(|w| w == [137, 80, 78, 71, 13, 10, 26, 10])
        .unwrap();
    bytes[at..].to_vec()
}

#[test]
fn one_placement_at_the_origin_is_the_per_asset_bake() {
    // A step, so it shades itself.
    let mut cells = block_cells();
    cells.extend([(0, 0, 4), (1, 0, 4), (0, 1, 4)]);
    let source = save_block("step.vox", &cells, 1);
    let mut session = Session::new();
    session.open_vox(&source).unwrap();
    let bake =
        json!({"sunDirection": [1, -1, 2], "sunEnergy": 1, "ambientEnergy": 0.4, "rays": 32});
    let asset = temp("asset.glb");
    let options = MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: true,
        ao_strength: 0.0,
        ao_radius: 3,
        ao_target: AoTarget::Auto,
        bake: Bake::parse(Some(&bake)).unwrap(),
    };
    export_with(&session, &asset, &options).unwrap();
    let scene_path = temp("scene.glb");
    let scene = SceneBake::parse(
        &json!({"instances": [{"source": source.to_str().unwrap()}], "bake": bake}),
        Path::new(""),
        None,
    )
    .unwrap();
    let reply = bake_scene(&scene, &scene_path).unwrap();
    assert_eq!(reply["instances"], 1);
    assert_eq!(png_of(&asset), png_of(&scene_path));
    let (a, b) = (glb_json(&asset), glb_json(&scene_path));
    assert_eq!(a["accessors"], b["accessors"]);
    assert_eq!(a["materials"], b["materials"]);
    assert_eq!(b["extensionsUsed"], json!(["KHR_materials_unlit"]));
    assert_eq!(
        b["scenes"][0]["extras"]["froxelScene"]["instances"][0]["yaw"],
        0.0
    );
}

#[test]
fn a_neighbour_darkens_the_side_that_faces_it_and_shadows_the_other() {
    let place = at([0.0; 3], 0.0, 1.0);
    let beside = at([5.0, 0.0, 0.0], 0.0, 1.0);
    let alone = World::new(vec![(block_grid(), place)], &[]).unwrap();
    let pair = World::new(vec![(block_grid(), place), (block_grid(), beside)], &[]).unwrap();
    let bake = Bake::default();
    let cell = (3, 1, 1);
    assert_eq!(light(&alone, &bake, &place, EAST, cell)[0], 1.0);
    assert!(light(&pair, &bake, &place, EAST, cell)[0] < 0.75);
    // The far side does not see it.
    assert_eq!(light(&pair, &bake, &place, WEST, (0, 1, 1))[0], 1.0);
    // A sun low in +X puts the near block in the far one's shadow.
    let sun = Bake {
        sun_direction: [1.0, 0.0, 0.0],
        sun_energy: 1.0,
        ambient_energy: 0.0,
        ..Bake::default()
    };
    assert_eq!(light(&alone, &sun, &place, EAST, cell)[0], 1.0);
    assert_eq!(light(&pair, &sun, &place, EAST, cell)[0], 0.0);
}

#[test]
fn ground_occludes_and_faces_under_it_go_dark() {
    let place = at([0.0; 3], 0.0, 1.0);
    let bake = Bake::default();
    let plain = World::new(vec![(block_grid(), place)], &[]).unwrap();
    let plane = World::new(
        vec![(block_grid(), place)],
        &[Occluder::Plane { height: 0.0 }],
    )
    .unwrap();
    assert_eq!(light(&plain, &bake, &place, EAST, (3, 1, 0))[0], 1.0);
    let low = light(&plane, &bake, &place, EAST, (3, 1, 0))[0];
    let high = light(&plane, &bake, &place, EAST, (3, 1, 3))[0];
    assert!(low < high, "{low} {high}");
    assert_eq!(light(&plane, &bake, &place, UP, (1, 1, 3))[0], 1.0);
    assert_eq!(light(&plane, &bake, &place, DOWN, (1, 1, 0))[0], 0.0);
    // A height field and a mesh with the same surface agree closely with it.
    let field = Occluder::Height {
        origin: [-40.0, -40.0],
        cell: 80.0,
        rows: vec![vec![0.0, 0.0], vec![0.0, 0.0]],
    };
    let hills = World::new(vec![(block_grid(), place)], &[field]).unwrap();
    assert_eq!(hills.triangle_count(), 2);
    assert_eq!(light(&hills, &bake, &place, EAST, (3, 1, 0))[0], low);
    let obj = temp("ground.obj");
    std::fs::write(&obj, "v -1 0 -1\nv 1 0 -1\nv 1 0 1\nv -1 0 1\nf 1 2 3 4\n").unwrap();
    let mesh = Occluder::Mesh {
        path: obj,
        place: at([0.0; 3], 0.0, 40.0),
    };
    let meshed = World::new(vec![(block_grid(), place)], &[mesh]).unwrap();
    assert_eq!(meshed.triangle_count(), 2);
    assert_eq!(light(&meshed, &bake, &place, EAST, (3, 1, 0))[0], low);
    // A bump of the height field shadows what is behind it.
    let bump = Occluder::Height {
        origin: [4.0, -4.0],
        cell: 4.0,
        rows: vec![vec![9.0, 9.0], vec![9.0, 9.0]],
    };
    let sun = Bake {
        sun_direction: [1.0, 0.0, 0.0],
        sun_energy: 1.0,
        ambient_energy: 0.0,
        ..Bake::default()
    };
    let flat = World::new(vec![(block_grid(), place)], &[]).unwrap();
    assert_eq!(light(&flat, &sun, &place, EAST, (3, 1, 1))[0], 1.0);
    let bump = World::new(vec![(block_grid(), place)], &[bump]).unwrap();
    // The bump is a flat roof at 9, above the sun's level path, so no shadow.
    assert_eq!(light(&bump, &sun, &place, EAST, (3, 1, 1))[0], 1.0);
    let wall = Occluder::Height {
        origin: [4.0, -4.0],
        cell: 4.0,
        rows: vec![vec![0.0, 9.0], vec![0.0, 9.0]],
    };
    let wall = World::new(vec![(block_grid(), place)], &[wall]).unwrap();
    assert_eq!(light(&wall, &sun, &place, EAST, (3, 1, 1))[0], 0.0);
}

#[test]
fn any_yaw_keeps_an_open_cube_open_and_a_quarter_turn_matches_a_turned_sun() {
    let bake = Bake::default();
    for yaw in [0.5, 3.0, 7.0, 12.5, 33.0, 45.0, 90.0, -17.0, 200.0] {
        let place = at([10.0, 3.0, -7.0], yaw, 0.1);
        let world = World::new(vec![(cube_grid(), place)], &[]).unwrap();
        for face in 0..6 {
            assert_eq!(
                light(&world, &bake, &place, face, (0, 0, 0))[0],
                1.0,
                "yaw {yaw} lets a cube shade itself on face {face}"
            );
        }
    }
    // A step lit from +X, against the same step turned a quarter under a sun
    // turned with it: the same light on every face.
    let mut cells = block_cells();
    cells.extend([(0, 0, 4), (1, 0, 4), (0, 1, 4)]);
    let grid = Arc::new(Grid::new(&cells));
    let lit = Bake {
        sun_direction: [1.0, 0.0, 0.5],
        sun_energy: 1.0,
        ambient_energy: 0.5,
        ..Bake::default()
    };
    // Yaw 90 turns the glb's +X to −Z, which is Froxel's +Y.
    let turned = Bake {
        sun_direction: [0.0, 1.0, 0.5],
        ..lit.clone()
    };
    let (place, quarter) = (at([0.0; 3], 0.0, 1.0), at([0.0; 3], 90.0, 1.0));
    let a = World::new(vec![(grid.clone(), place)], &[]).unwrap();
    let b = World::new(vec![(grid, quarter)], &[]).unwrap();
    for (face, cell) in [
        (EAST, (3, 1, 2)),
        (UP, (2, 2, 3)),
        (SOUTH, (2, 0, 1)),
        (UP, (0, 0, 4)),
    ] {
        let (x, y) = (
            light(&a, &lit, &place, face, cell),
            light(&b, &turned, &quarter, face, cell),
        );
        for c in 0..3 {
            assert!((x[c] - y[c]).abs() < 0.08, "{face} {cell:?}: {x:?} {y:?}");
        }
    }
}

#[test]
fn a_small_yaw_really_turns_what_shades_and_what_is_written() {
    // A small block far along the sun's path, turned about a pivot on that
    // path: a few degrees swing it out of the way.
    let mut cells = Vec::new();
    for x in 40..42 {
        for y in -1..1 {
            for z in 0..4 {
                cells.push((x, y, z));
            }
        }
    }
    let far = Arc::new(Grid::new(&cells));
    let place = at([0.0; 3], 0.0, 1.0);
    let sun = Bake {
        sun_direction: [1.0, 0.0, 0.0],
        sun_energy: 1.0,
        ambient_energy: 0.0,
        ..Bake::default()
    };
    let shaded = |yaw: f64| {
        let world = World::new(
            vec![
                (block_grid(), place),
                (far.clone(), at([0.0, 0.0, -3.5], yaw, 1.0)),
            ],
            &[],
        )
        .unwrap();
        // The face's centre is at z = -3.5.
        light(&world, &sun, &place, EAST, (3, 3, 1))[0]
    };
    assert_eq!(shaded(0.0), 0.0);
    assert_eq!(shaded(1.0), 0.0);
    assert_eq!(shaded(5.0), 1.0);
    assert_eq!(shaded(-5.0), 1.0);

    // The written mesh: a one-cell cube at yaw 30, scale 0.5, moved up.
    let source = save_block("one.vox", &[(0, 0, 0)], 1);
    let scene = SceneBake::parse(
        &json!({
            "instances": [{"source": source.to_str().unwrap(), "translation": [1, 2, 3], "yaw": 30, "scale": 0.5}]
        }),
        Path::new(""),
        None,
    )
    .unwrap();
    let path = temp("turned.glb");
    let reply = bake_scene(&scene, &path).unwrap();
    assert_eq!(reply["triangles"], 12);
    let root = glb_json(&path);
    let accessor = &root["accessors"][0];
    let (min, max) = (&accessor["min"], &accessor["max"]);
    // y runs 2..2.5; x and z are spread by the turned square.
    assert!((min[1].as_f64().unwrap() - 2.0).abs() < 1e-5);
    assert!((max[1].as_f64().unwrap() - 2.5).abs() < 1e-5);
    let cos = 30f64.to_radians().cos();
    let sin = 30f64.to_radians().sin();
    let span = max[0].as_f64().unwrap() - min[0].as_f64().unwrap();
    assert!((span - 0.5 * (cos + sin)).abs() < 1e-5, "{span}");
}

#[test]
fn scale_scales_the_reach_and_the_bake_is_deterministic() {
    let bake = Bake {
        distance: 6.0,
        ..Bake::default()
    };
    let small_bake = Bake {
        distance: 0.6,
        ..Bake::default()
    };
    let (one, two) = (at([0.0; 3], 0.0, 1.0), at([5.0, 0.0, 0.0], 0.0, 1.0));
    let (tenth, tenth_two) = (at([0.0; 3], 0.0, 0.1), at([0.5, 0.0, 0.0], 0.0, 0.1));
    let big = World::new(vec![(block_grid(), one), (block_grid(), two)], &[]).unwrap();
    let small = World::new(vec![(block_grid(), tenth), (block_grid(), tenth_two)], &[]).unwrap();
    let cell = (3, 1, 1);
    let a = light(&big, &bake, &one, EAST, cell)[0];
    let b = light(&small, &small_bake, &tenth, EAST, cell)[0];
    assert!(a < 1.0);
    assert!((a - b).abs() < 1e-4, "{a} {b}");

    // Bytes, and repeats of a source sharing one grid.
    let source = save_block("block.vox", &block_cells(), 1);
    let source = source.to_str().unwrap();
    let scene = SceneBake::parse(
        &json!({
            "instances": [
                {"source": source},
                {"source": source, "translation": [5, 0, 0], "yaw": 11},
                {"source": source, "translation": [0, 0, 6], "yaw": -3, "scale": 0.5}
            ],
            "occluders": [{"plane": 0}],
            "bake": {"preset": "old_pond", "sunSoftness": 2, "rays": 16}
        }),
        Path::new(""),
        None,
    )
    .unwrap();
    let (first, second) = (temp("same-1.glb"), temp("same-2.glb"));
    let reply = bake_scene(&scene, &first).unwrap();
    bake_scene(&scene, &second).unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    assert_eq!(reply["sources"], 1);
    assert_eq!(reply["instances"], 3);
    // Each 4-cube has one merged quad per side, and they add up.
    assert_eq!(reply["triangles"], 36);
    assert_eq!(reply["texels"], 96 * 3);
    assert_eq!(
        reply["memory"]["atlas"],
        reply["atlas"][0].as_u64().unwrap() * reply["atlas"][1].as_u64().unwrap() * 4
    );
    // The glb is what the preview draws.
    let preview = crate::preview::Preview {
        width: 64,
        height: 64,
        ..Default::default()
    };
    let frame = crate::preview::write_glb(&first, &preview, &temp("same.png")).unwrap();
    assert_eq!(frame.preview_triangles, 36);
}

#[test]
fn a_bad_scene_is_refused_with_its_reason() {
    let dir = Path::new("");
    let source = save_block("bad.vox", &[(0, 0, 0)], 1);
    let source = source.to_str().unwrap();
    for (scene, reason) in [
        (json!({}), "instances"),
        (json!({"instances": []}), "instances"),
        (json!({"instances": [{"source": "a.obj"}]}), ".vox"),
        (
            json!({"instances": [{"source": source, "scale": 0}]}),
            "scale",
        ),
        (
            json!({"instances": [{"source": source, "translation": [1]}]}),
            "translation",
        ),
        (
            json!({"instances": [{"source": source, "spin": 1}]}),
            "spin",
        ),
        (
            json!({"instances": [{"source": source}], "extra": 1}),
            "extra",
        ),
        (
            json!({"instances": [{"source": source, "lean": {"degrees": 5}}]}),
            "toward",
        ),
        (
            json!({"instances": [{"source": source, "lean": {"degrees": 95, "toward": 0}}]}),
            "-90..90",
        ),
        (
            json!({"instances": [{"source": source, "lean": {"degrees": 5, "toward": 0, "roll": 1}}]}),
            "roll",
        ),
        (
            json!({"instances": [{"source": source}], "occluders": [{}]}),
            "mesh, plane",
        ),
        (
            json!({"instances": [{"source": source}], "occluders": [{"height": {"rows": [[1]]}}]}),
            "rows",
        ),
        (
            json!({"instances": [{"source": source}], "bake": false}),
            "off",
        ),
        (
            json!({"instances": [{"source": source}], "bake": {"rays": 0}}),
            "rays",
        ),
    ] {
        let err = SceneBake::parse(&scene, dir, None).unwrap_err();
        assert!(err.contains(reason), "{scene}: {err}");
    }
    let good = SceneBake::parse(&json!({"instances": [{"source": source}]}), dir, None).unwrap();
    assert_eq!(good.bake, Bake::default());
    let err = bake_scene(&good, &temp("no.obj")).unwrap_err();
    assert!(err.contains(".glb"), "{err}");
    let missing = SceneBake::parse(&json!({"instances": [{"source": "nope.vox"}]}), dir, None);
    assert!(bake_scene(&missing.unwrap(), &temp("no.glb")).is_err());
    // A caller's bake replaces the scene's.
    let over = SceneBake::parse(
        &json!({"instances": [{"source": source}], "bake": {"rays": 4}}),
        dir,
        Some(&json!({"rays": 9})),
    )
    .unwrap();
    assert_eq!(over.bake.rays, 9);
}

#[test]
fn the_example_scene_bakes_through_the_tool_with_paths_from_its_folder() {
    let scene = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/stacked_blocks_scene.json"
    );
    let path = temp("example.glb");
    let reply = crate::remote::bake_scene(&json!({
        "scene": scene,
        "path": path.to_str().unwrap(),
        "bake": {"rays": 8, "preset": "old_pond", "distance": 2}
    }))
    .unwrap();
    assert_eq!(reply["instances"], 6);
    assert_eq!(reply["sources"], 1);
    assert_eq!(reply["bake"]["rays"], 8);
    let warnings = reply["structure"]["warnings"].as_array().unwrap();
    assert!(warnings.is_empty(), "{reply}");
}

#[test]
fn the_command_lines_take_the_bake() {
    let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let vox = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/stacked_blocks.vox");
    let scene = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/stacked_blocks_scene.json"
    );
    let file = temp("bake.json");
    std::fs::write(&file, r#"{"rays": 4, "distance": 3}"#).unwrap();
    let file = file.to_str().unwrap();
    let asset = temp("cli-asset.glb");
    let code = crate::cli::export(&args(&[
        "--input",
        vox,
        "--output",
        asset.to_str().unwrap(),
        "--color-atlas",
        "--bake",
        file,
    ]));
    assert_eq!(code, 0);
    let root = glb_json(&asset);
    assert_eq!(root["materials"][0]["extras"]["froxelBake"]["rays"], 4);
    let no_atlas = crate::cli::export(&args(&[
        "--input",
        vox,
        "--output",
        temp("cli-no.glb").to_str().unwrap(),
        "--bake",
        "{}",
    ]));
    assert_eq!(no_atlas, 1);
    let group = temp("cli-group.glb");
    let code = crate::cli::bake_scene(&args(&[
        "--scene",
        scene,
        "--output",
        group.to_str().unwrap(),
        "--bake",
        r#"{"rays": 4}"#,
    ]));
    assert_eq!(code, 0);
    let root = glb_json(&group);
    assert_eq!(root["materials"][0]["extras"]["froxelBake"]["rays"], 4);
    assert_eq!(crate::cli::bake_scene(&args(&["--scene", scene])), 2);
    assert!(crate::bridge::parse_bake(Some("{nope")).is_err());
    assert!(crate::bridge::parse_bake(Some("no-such-file.json")).is_err());
}

fn leaned(translation: [f64; 3], yaw: f64, scale: f64, degrees: f64, toward: f64) -> Place {
    Place {
        lean: Some(Lean { degrees, toward }),
        sway: 1.0,
        ..at(translation, yaw, scale)
    }
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|axis| (a[axis] - b[axis]).abs() < 1e-9)
}

#[test]
fn a_lean_tilts_the_top_toward_its_yaw_about_the_base() {
    // Toward 0 is −Z, toward 90 is −X, as yaw turns the glb's −Z.
    for (toward, way) in [
        (0.0, [0.0, 0.0, -1.0]),
        (90.0, [-1.0, 0.0, 0.0]),
        (-90.0, [1.0, 0.0, 0.0]),
        (180.0, [0.0, 0.0, 1.0]),
    ] {
        let place = leaned([1.0, 2.0, 3.0], 37.0, 0.5, 30.0, toward);
        // The base stays put, and a point straight up swings toward the way.
        assert!(close(place.point([0.0; 3]), [1.0, 2.0, 3.0]));
        let (sin, cos) = 30f64.to_radians().sin_cos();
        let top = place.point([0.0, 2.0, 0.0]);
        let want = [0, 1, 2].map(|axis| {
            [1.0, 2.0, 3.0][axis] + 1.0 * (sin * way[axis] + cos * [0.0, 1.0, 0.0][axis])
        });
        assert!(close(top, want), "toward {toward}: {top:?} {want:?}");
        // The inverse is exact, so rays enter the right cells.
        for p in [[0.3, 0.7, -0.2], [4.0, -1.0, 2.5]] {
            assert!(close(place.local_point(place.point(p)), p));
        }
    }
}

#[test]
fn a_leaned_open_cube_stays_open_and_a_lean_matches_a_tilted_sun() {
    let bake = Bake::default();
    for (degrees, toward) in [(3.0, 0.0), (15.0, 40.0), (45.0, -120.0), (90.0, 200.0)] {
        let place = leaned([10.0, 3.0, -7.0], 25.0, 0.1, degrees, toward);
        let world = World::new(vec![(cube_grid(), place)], &[]).unwrap();
        for face in 0..6 {
            assert_eq!(
                light(&world, &bake, &place, face, (0, 0, 0))[0],
                1.0,
                "lean {degrees} toward {toward} lets a cube shade itself on face {face}"
            );
        }
    }
    // A step under a sun, against the same step leaned a quarter toward +X
    // under that sun tilted with it: the same light on every face.
    let mut cells = block_cells();
    cells.extend([(0, 0, 4), (1, 0, 4), (0, 1, 4)]);
    let grid = Arc::new(Grid::new(&cells));
    let lit = Bake {
        sun_direction: [-1.0, 0.0, 0.5],
        sun_energy: 1.0,
        ambient_energy: 0.0,
        ..Bake::default()
    };
    // Leaning toward +X turns Froxel's +Z to +X and +X to −Z.
    let tilted = Bake {
        sun_direction: [0.5, 0.0, 1.0],
        ..lit.clone()
    };
    let (place, quarter) = (
        at([0.0; 3], 0.0, 1.0),
        leaned([0.0; 3], 0.0, 1.0, 90.0, -90.0),
    );
    let a = World::new(vec![(grid.clone(), place)], &[]).unwrap();
    let b = World::new(vec![(grid, quarter)], &[]).unwrap();
    let mut differ = false;
    for (face, cell) in [
        (WEST, (0, 1, 2)),
        (UP, (2, 2, 3)),
        (SOUTH, (2, 0, 1)),
        (UP, (0, 0, 4)),
        (WEST, (0, 0, 4)),
        (EAST, (1, 0, 4)),
    ] {
        let (x, y) = (
            light(&a, &lit, &place, face, cell),
            light(&b, &tilted, &quarter, face, cell),
        );
        differ |= x[0] != light(&b, &lit, &quarter, face, cell)[0];
        for c in 0..3 {
            assert!((x[c] - y[c]).abs() < 0.08, "{face} {cell:?}: {x:?} {y:?}");
        }
    }
    assert!(differ, "the lean changes what the untilted sun reaches");
}

#[test]
fn a_leaned_post_stops_shading_its_neighbour() {
    // A tall post beside a block, between it and the sun; leaned away from
    // the sun's path, it no longer shades the block's side.
    let mut cells = Vec::new();
    for z in 0..12 {
        cells.push((0, 0, z));
    }
    let post = Arc::new(Grid::new(&cells));
    let place = at([0.0; 3], 0.0, 1.0);
    let sun = Bake {
        sun_direction: [1.0, 0.0, 0.0],
        sun_energy: 1.0,
        ambient_energy: 0.0,
        ..Bake::default()
    };
    let shaded = |lean: Option<Lean>| {
        let pole = Place {
            lean,
            ..at([6.0, 0.0, 0.0], 0.0, 1.0)
        };
        let world = World::new(vec![(block_grid(), place), (post.clone(), pole)], &[]).unwrap();
        light(&world, &sun, &place, EAST, (3, 0, 2))[0]
    };
    assert_eq!(shaded(None), 0.0);
    // Toward +Z (yaw 180) tilts it off the ray at z −0.5 by y 2.5.
    let away = Lean {
        degrees: 30.0,
        toward: 180.0,
    };
    assert_eq!(shaded(Some(away)), 1.0);
    // Toward −X keeps its foot in the way.
    let along = Lean {
        degrees: 30.0,
        toward: 90.0,
    };
    assert_eq!(shaded(Some(along)), 0.0);
}

#[test]
fn a_leaned_instance_is_written_tilted_and_no_lean_is_unchanged() {
    let source = save_block("lean.vox", &[(0, 0, 0)], 1);
    let source = source.to_str().unwrap();
    let write = |instance: Value, name: &str| {
        let scene = SceneBake::parse(
            &json!({"instances": [instance], "occluders": [{"plane": 0}]}),
            Path::new(""),
            None,
        )
        .unwrap();
        let path = temp(name);
        bake_scene(&scene, &path).unwrap();
        path
    };
    // A unit cube leaned a quarter toward −X: its top lies along −X.
    let path = write(
        json!({"source": source, "translation": [1, 2, 3], "lean": {"degrees": 90, "toward": 90}}),
        "leaned.glb",
    );
    let root = glb_json(&path);
    let (min, max) = (&root["accessors"][0]["min"], &root["accessors"][0]["max"]);
    let want_min = [0.0, 2.0, 2.0];
    let want_max = [1.0, 3.0, 3.0];
    for axis in 0..3 {
        assert!(
            (min[axis].as_f64().unwrap() - want_min[axis]).abs() < 1e-5,
            "{min}"
        );
        assert!(
            (max[axis].as_f64().unwrap() - want_max[axis]).abs() < 1e-5,
            "{max}"
        );
    }
    assert_eq!(
        root["scenes"][0]["extras"]["froxelScene"]["instances"][0]["lean"],
        json!({"degrees": 90.0, "toward": 90.0})
    );
    // No lean and a null lean write the same bytes, with no lean in extras.
    let plain = json!({"source": source, "translation": [1, 2, 3], "yaw": 20});
    let mut null = plain.clone();
    null["lean"] = Value::Null;
    let (a, b) = (write(plain, "plain.glb"), write(null, "null.glb"));
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let root = glb_json(&a);
    assert!(root["scenes"][0]["extras"]["froxelScene"]["instances"][0]
        .get("lean")
        .is_none());
}

/// The (sway, bob) pairs of a baked glb, in vertex order.
fn motion_of(path: &Path) -> Vec<[f32; 2]> {
    let bytes = std::fs::read(path).unwrap();
    let root = glb_json(path);
    let index = root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"]
        .as_u64()
        .expect("TEXCOORD_1") as usize;
    let accessor = &root["accessors"][index];
    let view = accessor["bufferView"].as_u64().unwrap() as usize;
    let count = accessor["count"].as_u64().unwrap() as usize;
    let offset = root["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = 20 + json_len + 8;
    (0..count)
        .map(|i| {
            let at = bin + offset + i * 8;
            [
                f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
                f32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()),
            ]
        })
        .collect()
}

#[test]
fn an_instance_sway_scales_its_sway_and_leaves_bob_and_others() {
    // Slot 1 sways 1.5, slot 2 bobs 0.5; a cell of each.
    let mut session = Session::new();
    session.objects[0].cells.clear();
    session.objects[0].cells.insert((0, 0, 0), 1);
    session.objects[0].cells.insert((3, 0, 0), 2);
    session.materials[1].motion = crate::state::Motion::Sway;
    session.materials[1].motion_amount = 1.5;
    session.materials[2].motion = crate::state::Motion::Bob;
    session.materials[2].motion_amount = 0.5;
    let source = temp("sway.vox");
    session.write_vox(&source).unwrap();
    let source = source.to_str().unwrap();
    let write = |instances: Value, name: &str| {
        let scene = SceneBake::parse(
            &json!({"instances": instances, "occluders": [{"plane": 0}]}),
            Path::new(""),
            None,
        )
        .unwrap();
        let path = temp(name);
        bake_scene(&scene, &path).unwrap();
        path
    };
    // One placement as it is and the same one at sway 0.4: the same vertices,
    // sway scaled, bob kept.
    let plain = write(
        json!([{"source": source, "translation": [1, 0, 2]}]),
        "swayed_a.glb",
    );
    let swayed = write(
        json!([{"source": source, "translation": [1, 0, 2], "sway": 0.4}]),
        "swayed_b.glb",
    );
    let (a, b) = (motion_of(&plain), motion_of(&swayed));
    assert_eq!(a.len(), b.len());
    assert!(a.iter().any(|m| m[0] == 1.5) && a.iter().any(|m| m[1] == 0.5));
    for (a, b) in a.iter().zip(&b) {
        assert!((b[0] - a[0] * 0.4).abs() < 1e-6, "{a:?} {b:?}");
        assert_eq!(a[1], b[1], "bob stays the asset's");
    }
    let spans =
        |path: &Path| glb_json(path)["scenes"][0]["extras"]["froxelScene"]["instances"][0].clone();
    assert!(spans(&plain).get("sway").is_none());
    assert_eq!(spans(&swayed)["sway"], json!(0.4));
    // Sway 1 writes the same bytes as none.
    let plain = write(json!([{"source": source}]), "sway_none.glb");
    let one = write(json!([{"source": source, "sway": 1}]), "sway_one.glb");
    assert_eq!(std::fs::read(plain).unwrap(), std::fs::read(one).unwrap());
    // Out of range is refused.
    let bad = SceneBake::parse(
        &json!({"instances": [{"source": source, "sway": 5}]}),
        Path::new(""),
        None,
    );
    assert!(bad.is_err());
}
