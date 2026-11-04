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

#[test]
fn inspect_lists_only_slots_with_motion() {
    let mut session = Session::new();
    session.replace_empty();
    let got = apply(&mut session, "document.inspect", &json!({})).unwrap();
    assert_eq!(got["motion"], json!({}));

    session.materials[36].motion = crate::state::Motion::Sway;
    session.materials[36].motion_amount = 0.3;
    session.materials[10].motion_amount = 2.0;
    let got = apply(&mut session, "document.inspect", &json!({})).unwrap();
    assert_eq!(
        got["motion"],
        json!({
            "10": {"kind": "none", "amount": 2.0},
            "36": {"kind": "sway", "amount": 0.3},
        })
    );
}

#[test]
fn inspect_lists_only_used_palette_slots_unless_asked() {
    let mut session = Session::new();
    session.replace_empty();
    let got = apply(&mut session, "document.inspect", &json!({})).unwrap();
    assert_eq!(got["palette"], json!({}));

    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 0, "y": 0, "z": 0, "palette": 7}),
    )
    .unwrap();
    session.objects[0].hidden = true;
    let got = apply(&mut session, "document.inspect", &json!({})).unwrap();
    let palette = got["palette"].as_object().unwrap();
    assert_eq!(palette.keys().collect::<Vec<_>>(), ["7"]);
    let rgba = session.palette[7];
    let hex = format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        rgba[0], rgba[1], rgba[2], rgba[3]
    );
    assert_eq!(palette["7"], hex);

    let got = apply(
        &mut session,
        "document.inspect",
        &json!({"fullPalette": true}),
    )
    .unwrap();
    assert_eq!(got["palette"].as_object().unwrap().len(), 256);
    assert!(apply(&mut session, "document.inspect", &json!({"fullPalette": 1})).is_err());
}

#[test]
fn mesh_params_round_trip_through_the_export_options() {
    let options = crate::export::MeshExport {
        merge: true,
        vertex_colors: true,
        export_hidden: false,
        split_objects: true,
        color_atlas: false,
        ao_strength: 0.5,
        ao_radius: 3,
        ao_target: crate::export::AoTarget::Both,
    };
    let back = mesh_options(&mesh_params(&options)).unwrap();
    assert_eq!(mesh_params(&back), mesh_params(&options));
}

#[test]
fn file_writers_create_missing_folders() {
    let mut session = Session::new();
    session.replace_empty();
    apply(
        &mut session,
        "voxel.set",
        &json!({"x": 0, "y": 0, "z": 0, "palette": 4}),
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!("froxel-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let vox = root.join("trees/new/oak.vox");
    let saved = apply(
        &mut session,
        "document.save",
        &json!({"path": vox.display().to_string()}),
    )
    .unwrap();
    assert_eq!(saved["voxels"], 1);
    assert!(vox.is_file());
    let png = root.join("views/flat/oak.png");
    apply(
        &mut session,
        "render.flat",
        &json!({"path": png.display().to_string(), "width": 32, "height": 32}),
    )
    .unwrap();
    assert!(png.is_file());
    let glb = root.join("meshes/oak.glb");
    apply(
        &mut session,
        "mesh.export",
        &json!({"path": glb.display().to_string()}),
    )
    .unwrap();
    assert!(glb.is_file());
    let _ = std::fs::remove_dir_all(&root);
}
