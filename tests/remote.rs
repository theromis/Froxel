use super::*;

#[test]
fn set_voxel_undoes_and_a_region_png_crops() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 1, "y": 2, "z": 3, "palette": 4}),
    )
    .unwrap();
    let got = apply(&mut session, "voxel.get", &json!({"x": 1, "y": 2, "z": 3})).unwrap();
    assert_eq!(got["palette"], 4);
    apply(&mut session, "edit.undo", &json!({})).unwrap();
    let got = apply(&mut session, "voxel.get", &json!({"x": 1, "y": 2, "z": 3})).unwrap();
    assert_eq!(got["palette"], 0);
    let err = apply(&mut session, "ui.screenshot", &json!({})).unwrap_err();
    assert!(err.contains("--control"));

    let pixels = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [1, 2, 3, 4],
    ];
    let (png, w, h) = png_region(&pixels, 2, 2, Some((1, 0, 1, 2))).unwrap();
    assert_eq!((w, h), (1, 2));
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn export_mesh_writes_a_palette_texture() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 0, "y": 0, "z": 0, "palette": 4}),
    )
    .unwrap();
    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 1, "y": 0, "z": 0, "palette": 4}),
    )
    .unwrap();
    let path = std::env::temp_dir().join("froxel-mcp-export.glb");
    let got = apply(
        &mut session,
        "mesh.export",
        &json!({"path": path.display().to_string()}),
    )
    .unwrap();
    assert_eq!(got["quads"], 10);
    assert_eq!(got["merge"], false);
    assert_eq!(got["vertexColors"], false);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"glTF");
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json = &bytes[20..20 + json_len];
    assert!(json.windows(10).any(|window| window == b"TEXCOORD_0"));
    let merged = apply(
        &mut session,
        "mesh.export",
        &json!({"path": path.display().to_string(), "merge": true}),
    )
    .unwrap();
    assert_eq!(merged["quads"], 6);
    let _ = std::fs::remove_file(path);
}
