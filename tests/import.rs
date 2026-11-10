use super::*;

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mesh")
        .join(name)
}

fn options(fit: Fit, solid: bool) -> Voxelize {
    with_fill(fit, if solid { Fill::Solid } else { Fill::Shell })
}

fn with_fill(fit: Fit, fill: Fill) -> Voxelize {
    Voxelize {
        fit,
        up: Up::Y,
        rotation: [0.0; 3],
        anchor: Anchor::Min,
        position: [0.0; 3],
        fill,
        colors: Colors::Auto,
        coverage: None,
    }
}

fn color_at(voxels: &Voxels, cell: Cell) -> [u8; 3] {
    voxels
        .cells
        .iter()
        .find(|(c, _)| *c == cell)
        .map(|(_, rgb)| *rgb)
        .unwrap_or_else(|| panic!("no cell {cell:?}"))
}

#[test]
fn a_cube_fills_exactly_its_size() {
    let mesh = load(&fixture("cube.obj")).unwrap();
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.primitives, 1);
    let solid = voxelize(&mesh, &options(Fit::Cells(8.0), true)).unwrap();
    assert_eq!(solid.cells.len(), 512);
    assert_eq!((solid.min, solid.max), ((0, 0, 0), (7, 7, 7)));
    assert_eq!(solid.shell, 512 - 216);
    // Kd 0.8 0.1 0.1 is sRGB, and comes back as the same bytes.
    assert!(solid.cells.iter().all(|(_, rgb)| *rgb == [204, 26, 26]));
    let shell = voxelize(&mesh, &options(Fit::Cells(8.0), false)).unwrap();
    assert_eq!(shell.cells.len(), 512 - 216);
    // Wound inside out, it is still 8 cells a side.
    let mut flipped = mesh.clone();
    for triangle in &mut flipped.triangles {
        triangle.v.swap(1, 2);
    }
    let flipped = voxelize(&flipped, &options(Fit::Cells(8.0), true)).unwrap();
    assert_eq!((flipped.cells.len(), flipped.max), (512, (7, 7, 7)));
}

#[test]
fn placement_follows_anchor_rotation_and_scale() {
    let mesh = load(&fixture("cube.obj")).unwrap();
    let mut centred = options(Fit::Scale(4.0), true);
    centred.anchor = Anchor::Center;
    centred.position = [10.0, 0.0, 0.0];
    let placed = voxelize(&mesh, &centred).unwrap();
    assert_eq!((placed.min, placed.max), ((8, -2, -2), (11, 1, 1)));
    let mut bottom = options(Fit::Cells(6.0), true);
    bottom.anchor = Anchor::Bottom;
    bottom.rotation = [0.0, 0.0, 45.0];
    let turned = voxelize(&mesh, &bottom).unwrap();
    assert_eq!(turned.min.2, 0);
    assert_eq!(turned.max.0, -turned.min.0 - 1);
    // A 45 degree turn about z leaves the diagonal 6 cells long.
    assert_eq!(turned.max.0 - turned.min.0 + 1, 6);
    // z is 6 / sqrt(2) = 4.2 cells.
    assert_eq!(turned.max.2, 4);
}

#[test]
fn a_textured_quad_samples_each_quadrant() {
    for name in [
        "quad.gltf",
        "quad_embedded.gltf",
        "quad_jpeg.obj",
        "quad_webp.glb",
    ] {
        let mesh = load(&fixture(name)).unwrap();
        assert_eq!(mesh.textures.len(), 1, "{name}");
        let voxels = voxelize(&mesh, &options(Fit::Cells(4.0), true)).unwrap();
        assert_eq!(voxels.cells.len(), 16, "{name}");
        assert_eq!((voxels.min, voxels.max), ((0, 0, 0), (3, 3, 0)), "{name}");
        // glTF z 0 (the texture's top row) is the high y side once Z is up.
        let near = |got: [u8; 3], want: [u8; 3]| {
            (0..3).all(|c| (i32::from(got[c]) - i32::from(want[c])).abs() <= 24)
        };
        for (cell, want) in [
            ((0, 3, 0), [255, 0, 0]),
            ((3, 3, 0), [0, 255, 0]),
            ((0, 0, 0), [0, 0, 255]),
            ((3, 0, 0), [255, 255, 255]),
        ] {
            let got = color_at(&voxels, cell);
            assert!(near(got, want), "{name} {cell:?}: {got:?} is not {want:?}");
        }
    }
}

#[test]
fn every_primitive_lands_under_its_node_transforms() {
    let mesh = load(&fixture("parts.glb")).unwrap();
    assert_eq!(mesh.primitives, 2);
    assert_eq!(mesh.triangles.len(), 24);
    // 6 x 3 x 2 metres (x, glTF z, glTF y) at 0.5 m cells.
    let voxels = voxelize(&mesh, &options(Fit::Scale(2.0), true)).unwrap();
    assert_eq!((voxels.min, voxels.max), ((0, 0, 0), (11, 5, 3)));
    assert_eq!(voxels.cells.len(), 2 * 4 * 6 * 4);
    assert_eq!(color_at(&voxels, (1, 2, 1)), [255, 0, 0]);
    assert_eq!(color_at(&voxels, (10, 2, 1)), [0, 0, 255]);
    assert!(!voxels.cells.iter().any(|((x, _, _), _)| (4..8).contains(x)));
}

#[test]
fn a_webp_texture_is_read_and_ktx2_is_refused() {
    let mesh = load(&fixture("quad_webp.glb")).unwrap();
    assert!(mesh.warnings.is_empty(), "{:?}", mesh.warnings);
    let texture = &mesh.textures[0];
    assert_eq!((texture.width, texture.height), (2, 2));
    assert_eq!(texture.rgba[0], [255, 0, 0, 255]);
    assert_eq!(texture.rgba[3], [255, 255, 255, 255]);
    let ktx2 = b"\xabKTX 20\xbb\r\n\x1a\n\0\0\0\0";
    let err = image::decode(ktx2, "image 0").unwrap_err();
    assert!(err.contains("KTX2"), "{err}");
}

/// A sphere of `rings` x `segments` quads about `centre`, outward, or
/// inward when `radius` is negative; rows of the lower half are skipped
/// below `cut` (a z in file units, Z up).
fn sphere(mesh: &mut Mesh, centre: [f64; 3], radius: f64, cut: Option<f64>) {
    let (rings, segments) = (16u32, 24u32);
    let r = radius.abs();
    let mut at = Vec::new();
    for i in 0..=rings {
        let theta = std::f64::consts::PI * f64::from(i) / f64::from(rings);
        let mut row = Vec::new();
        for j in 0..segments {
            let phi = std::f64::consts::TAU * f64::from(j) / f64::from(segments);
            let p = [
                centre[0] + r * theta.sin() * phi.cos(),
                centre[1] + r * theta.sin() * phi.sin(),
                centre[2] + r * theta.cos(),
            ];
            // Each pole is one vertex, so the mesh is watertight.
            if j > 0 && (i == 0 || i == rings) {
                row.push(row[0]);
            } else {
                row.push(mesh.push_vertex(p, [0.0; 2], [1.0; 4]));
            }
        }
        at.push(row);
    }
    for i in 0..rings as usize {
        let low = mesh.positions[at[i + 1][0] as usize][2];
        if cut.is_some_and(|cut| low < cut) {
            continue;
        }
        for j in 0..segments as usize {
            let k = (j + 1) % segments as usize;
            let (a, b, c, d) = (at[i][j], at[i + 1][j], at[i + 1][k], at[i][k]);
            let quads = if radius > 0.0 {
                [[a, b, c], [a, c, d]]
            } else {
                [[a, c, b], [a, d, c]]
            };
            for v in quads {
                mesh.push_triangle(v, 0).unwrap();
            }
        }
    }
}

fn inside_count(voxels: &Voxels) -> usize {
    voxels.cells.len() - voxels.shell
}

#[test]
fn a_hollow_double_wall_comes_out_solid() {
    // An outer sphere and an inward inner one: a closed wall 1.5 cells
    // thick. Parity fills only the wall; solid fills the hollow too.
    let mut mesh = Mesh::default();
    sphere(&mut mesh, [0.0; 3], 10.0, None);
    sphere(&mut mesh, [0.0; 3], -8.5, None);
    let mut solid = with_fill(Fit::Scale(1.0), Fill::Solid);
    solid.up = Up::Z;
    let mut parity = solid.clone();
    parity.fill = Fill::Parity;
    let filled = voxelize(&mesh, &solid).unwrap();
    let voted = voxelize(&mesh, &parity).unwrap();
    assert_eq!(filled.open_edges, 0);
    // The same cells as the outer sphere alone, filled.
    let mut ball = Mesh::default();
    sphere(&mut ball, [0.0; 3], 10.0, None);
    let ball = voxelize(&ball, &solid).unwrap();
    let cells = |v: &Voxels| v.cells.iter().map(|(c, _)| *c).collect::<Vec<_>>();
    assert_eq!(cells(&filled), cells(&ball));
    assert!(inside_count(&voted) < inside_count(&filled) / 4);
}

#[test]
fn a_dome_open_underneath_is_sealed_at_its_rim() {
    // The upper half of a sphere with no floor, like a turtle shell open
    // underneath: the flood would run in from below, so the rim is sealed.
    let mut mesh = Mesh::default();
    sphere(&mut mesh, [0.0; 3], 12.0, Some(-1e-9));
    let mut solid = with_fill(Fit::Scale(1.0), Fill::Solid);
    solid.up = Up::Z;
    let voxels = voxelize(&mesh, &solid).unwrap();
    assert_eq!(voxels.open_edges, 24);
    // The upper half of the filled ball, cell for cell, to within its rim
    // ring.
    let mut ball = Mesh::default();
    sphere(&mut ball, [0.0; 3], 12.0, None);
    let ball = voxelize(&ball, &solid).unwrap();
    let upper: std::collections::HashSet<Cell> = ball
        .cells
        .iter()
        .filter(|((_, _, z), _)| *z >= 12)
        .map(|((x, y, z), _)| (*x, *y, z - 12))
        .collect();
    let dome: std::collections::HashSet<Cell> = voxels.cells.iter().map(|(c, _)| *c).collect();
    let differ = upper.symmetric_difference(&dome).count();
    assert!(differ < upper.len() / 50, "{differ} of {}", upper.len());
    // The top of the dome is 12 cells above the rim; nothing hangs below.
    assert_eq!(voxels.max.2 - voxels.min.2 + 1, 12);
    // The centre column is full from the rim to the top.
    let column = voxels
        .cells
        .iter()
        .filter(|((x, y, _), _)| (*x, *y) == (12, 12))
        .count();
    assert_eq!(column, 12);
}

#[test]
fn an_open_mesh_still_fills_by_the_vote_or_the_seal() {
    let mut mesh = load(&fixture("cube.obj")).unwrap();
    // Drop the two triangles of the x = 0 face. Parity: rays along x see
    // one crossing, but the y and z rays still agree the inside is inside,
    // and the layer where the face was fills from them too. Solid: that
    // layer is enclosed in 17 directions, so it seals the opening.
    mesh.triangles.truncate(10);
    for fill in [Fill::Solid, Fill::Parity] {
        let voxels = voxelize(&mesh, &with_fill(Fit::Cells(8.0), fill)).unwrap();
        assert_eq!(voxels.open_edges, 4, "{fill:?}");
        assert_eq!(voxels.cells.len(), 512, "{fill:?}");
        assert_eq!(voxels.cells.len() - voxels.shell, 216 + 36, "{fill:?}");
    }
}

#[test]
fn limits_are_checked_before_any_cell() {
    let mesh = load(&fixture("cube.obj")).unwrap();
    let err = voxelize(&mesh, &options(Fit::Scale(300.0), true))
        .err()
        .unwrap();
    assert!(err.contains("256 per axis"), "{err}");
    assert!(load(&fixture("make.py")).unwrap_err().contains(".obj"));
    let missing = load(&fixture("none.glb")).unwrap_err();
    assert!(missing.contains("Could not read"), "{missing}");
}

#[test]
fn quantize_reuses_close_slots_and_adds_up_to_the_limit() {
    let mut palette = [[0u8, 0, 0, 255]; 256];
    palette[1] = [200, 30, 30, 255];
    palette[2] = [10, 10, 200, 255];
    let colors = vec![
        ([204, 26, 26], 10),
        ([0, 250, 0], 5),
        ([0, 240, 10], 5),
        ([250, 250, 250], 3),
    ];
    let free: Vec<u8> = (3..=255u8).rev().collect();
    let fit = PaletteFit {
        palette: &palette,
        free: free.clone(),
        slots: None,
        max_new: 8,
        tolerance: 12.0,
    };
    let done = quantize(&colors, &fit).unwrap();
    assert_eq!(done.map[&[204, 26, 26]], 1);
    // The two greens, the white: three slots from 255 down.
    assert_eq!(done.new_slots.len(), 3, "{:?}", done.new_slots);
    assert!(done.new_slots.iter().all(|(slot, _)| *slot >= 253));
    assert!(done.map[&[0, 250, 0]] >= 253);
    // With room for one, the greens and the white share it or fall back.
    let one = quantize(&colors, &PaletteFit { max_new: 1, ..fit }).unwrap();
    assert_eq!(one.new_slots.len(), 1);
    assert_eq!(one.new_slots[0].0, 255);
    let mapped = quantize(
        &colors,
        &PaletteFit {
            palette: &palette,
            free,
            slots: Some(vec![2]),
            max_new: 8,
            tolerance: 12.0,
        },
    )
    .unwrap();
    assert!(mapped.new_slots.is_empty());
    assert!(mapped.map.values().all(|&s| s == 2));
}

#[test]
fn base64_and_uris_decode() {
    assert_eq!(image::base64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(image::base64("aGVsbG8").unwrap(), b"hello");
    assert_eq!(image::percent_decode("my%20tex.png"), "my tex.png");
    assert_eq!(image::percent_decode("100%"), "100%");
}

#[test]
fn degenerate_pole_triangles_do_not_turn_cells_black() {
    // A lat-long sphere whose pole rings are separate vertices at one point:
    // the triangles there have zero area, and their nearest point used to
    // come back as NaN, which coloured the cell black.
    let (rings, segments) = (12u32, 16u32);
    let mut mesh = Mesh::default();
    let green = [0.1, 0.6, 0.2, 1.0];
    let mut at = Vec::new();
    for i in 0..=rings {
        let theta = std::f64::consts::PI * f64::from(i) / f64::from(rings);
        let row: Vec<u32> = (0..=segments)
            .map(|j| {
                let phi = std::f64::consts::TAU * f64::from(j) / f64::from(segments);
                let p = [
                    8.0 * theta.sin() * phi.cos(),
                    8.0 * theta.sin() * phi.sin(),
                    8.0 * theta.cos(),
                ];
                mesh.push_vertex(p, [0.0; 2], green)
            })
            .collect();
        at.push(row);
    }
    let mut triangles = Vec::new();
    for i in 0..rings as usize {
        for j in 0..segments as usize {
            let (a, b, c, d) = (at[i][j], at[i + 1][j], at[i + 1][j + 1], at[i][j + 1]);
            // Every rotation of the vertex order, so the repeated pole
            // vertices come first, second, and third.
            for (n, mut v) in [[a, b, c], [a, c, d]].into_iter().enumerate() {
                v.rotate_left((i + j + n) % 3);
                triangles.push(v);
            }
        }
    }
    // The zero-area triangles first, so they reach their cells before any
    // neighbour does.
    let flat = |v: &[u32; 3]| {
        let p = v.map(|k| mesh.positions[k as usize]);
        p[0] == p[1] || p[1] == p[2] || p[0] == p[2]
    };
    triangles.sort_by_key(|v| !flat(v));
    assert!(triangles.iter().filter(|v| flat(v)).count() >= segments as usize);
    for v in triangles {
        mesh.push_triangle(v, 0).unwrap();
    }
    for fill in [Fill::Shell, Fill::Solid] {
        let mut options = with_fill(Fit::Scale(1.0), fill);
        options.up = Up::Z;
        let voxels = voxelize(&mesh, &options).unwrap();
        assert!(!voxels.cells.is_empty());
        for (cell, rgb) in &voxels.cells {
            assert!(
                rgb.iter().map(|&c| u32::from(c)).sum::<u32>() > 60,
                "{fill:?}: cell {cell:?} is {rgb:?}"
            );
        }
    }
}

/// An OBJ of axis-aligned boxes, each `[min, max]`, written to a temp file.
fn boxes_obj(name: &str, boxes: &[([f64; 3], [f64; 3])]) -> PathBuf {
    let mut text = String::new();
    let mut base = 0;
    for (lo, hi) in boxes {
        for i in 0..8 {
            let p = [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ];
            text.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
        }
        for f in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            text.push_str(&format!(
                "f {} {} {} {}\n",
                base + f[0] + 1,
                base + f[1] + 1,
                base + f[2] + 1,
                base + f[3] + 1
            ));
        }
        base += 8;
    }
    let path = std::env::temp_dir().join(format!("froxel-{}-{name}.obj", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn coverage_keeps_a_fin_thinner_than_a_cell_one_cell_thick() {
    // A body 10 units a side and a fin 0.3 units thick straddling a cell
    // boundary, at 1 cell a unit (the longest axis is 16 units).
    let path = boxes_obj(
        "fin",
        &[
            ([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
            ([10.0, 4.85, 2.0], [16.0, 5.15, 8.0]),
        ],
    );
    let mesh = load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let run = |coverage: Option<Coverage>| {
        let mut o = with_fill(Fit::Cells(16.0), Fill::Solid);
        o.up = Up::Z;
        o.coverage = coverage;
        voxelize(&mesh, &o).unwrap()
    };
    let fin = |v: &Voxels| -> Vec<Cell> {
        v.cells
            .iter()
            .map(|(c, _)| *c)
            .filter(|c| c.0 >= 10)
            .collect()
    };
    // Every touched cell: the fin is two cells thick.
    let touched = run(None);
    let ys: std::collections::BTreeSet<i32> = fin(&touched).iter().map(|c| c.1).collect();
    assert_eq!(ys.into_iter().collect::<Vec<_>>(), vec![4, 5]);
    // Coverage alone: the fin vanishes under half a cell.
    let lost = run(Some(Coverage {
        share: 0.5,
        keep_thin: false,
    }));
    assert!(fin(&lost).is_empty());
    assert_eq!(lost.cells.len(), 1000);
    // Coverage with keepThin: one cell thick, all along.
    let kept = run(Some(Coverage {
        share: 0.5,
        keep_thin: true,
    }));
    let cells = fin(&kept);
    let ys: std::collections::BTreeSet<i32> = cells.iter().map(|c| c.1).collect();
    assert_eq!(ys.len(), 1, "{cells:?}");
    let xs: std::collections::BTreeSet<i32> = cells.iter().map(|c| c.0).collect();
    assert_eq!(
        xs.into_iter().collect::<Vec<_>>(),
        (10..=15).collect::<Vec<_>>()
    );
    assert_eq!(cells.len(), 36);
    assert_eq!(kept.covered.unwrap().thin, 36);
}

#[test]
fn import_mesh_reports_coverage_and_thin_parts() {
    use serde_json::json;
    let path = boxes_obj(
        "fin-mcp",
        &[
            ([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
            ([10.0, 4.85, 2.0], [16.0, 5.15, 8.0]),
        ],
    );
    let mut session = crate::state::Session::new();
    let reply = crate::remote::apply(
        &mut session,
        "mesh.import",
        &json!({"path": path.display().to_string(), "size": 16, "upAxis": "z",
                "coverage": 0.5, "minFeature": 2, "palette": 3, "newObject": true}),
    );
    let _ = std::fs::remove_file(&path);
    let reply = reply.unwrap();
    assert_eq!(reply["coverage"]["thin"], json!(36));
    assert_eq!(reply["coverage"]["kept"], json!(1036));
    assert_eq!(reply["thin"]["count"], json!(1));
    assert_eq!(reply["thin"]["parts"][0]["thickness"], json!(1));
}

#[test]
fn the_finned_fish_example_keeps_its_fins_one_cell_thick() {
    let mut session = crate::state::Session::new();
    session.replace_empty();
    session
        .set_script(0, include_str!("fixtures/examples/finned_fish.rhai"))
        .unwrap();
    session.regenerate_script(0).unwrap();
    let cells = &session.objects[0].cells;
    let fins: Vec<Cell> = cells
        .iter()
        .filter(|(_, s)| **s == 3)
        .map(|(c, _)| *c)
        .collect();
    assert!(!fins.is_empty());
    // The dorsal fin, above the body, is one cell wide in x.
    let top = cells.keys().map(|c| c.2).max().unwrap();
    let xs: std::collections::BTreeSet<i32> = fins
        .iter()
        .filter(|c| c.2 >= top - 1)
        .map(|c| c.0)
        .collect();
    assert_eq!(xs.len(), 1, "{xs:?}");
}
