use super::*;

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mesh")
        .join(name)
}

fn options(fit: Fit, solid: bool) -> Voxelize {
    Voxelize {
        fit,
        up: Up::Y,
        rotation: [0.0; 3],
        anchor: Anchor::Min,
        position: [0.0; 3],
        solid,
        colors: Colors::Auto,
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
    for name in ["quad.gltf", "quad_embedded.gltf", "quad_jpeg.obj"] {
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
fn an_open_mesh_still_fills_by_the_vote() {
    let mut mesh = load(&fixture("cube.obj")).unwrap();
    // Drop the two triangles of the x = 0 face: rays along x see one
    // crossing, but the y and z rays still agree the inside is inside, and
    // the layer where the face was fills from them too.
    mesh.triangles.truncate(10);
    let voxels = voxelize(&mesh, &options(Fit::Cells(8.0), true)).unwrap();
    assert_eq!(voxels.cells.len(), 512);
    assert_eq!(voxels.cells.len() - voxels.shell, 216 + 36);
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
