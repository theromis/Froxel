use serde_json::json;

use crate::bevel::{Bevel, BevelMode};
use crate::export::{AoTarget, MeshExport};
use crate::remote::apply;
use crate::state::Session;

fn options(bevel: Option<Bevel>) -> MeshExport {
    MeshExport {
        vertex_colors: false,
        merge: false,
        export_hidden: false,
        split_objects: false,
        color_atlas: false,
        ao_strength: 0.0,
        ao_radius: 3,
        ao_target: AoTarget::Auto,
        bake: None,
        game_view: None,
        bevel,
    }
}

fn scene() -> Session {
    let mut session = Session::new();
    session.objects[0].cells.clear();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 0, "x1": 5, "y1": 3, "z1": 1, "palette": 1}),
    )
    .unwrap();
    apply(
        &mut session,
        "voxel.fill",
        &json!({"x0": 0, "y0": 0, "z0": 2, "x1": 2, "y1": 3, "z1": 3, "palette": 2}),
    )
    .unwrap();
    apply(
        &mut session,
        "strand.add",
        &json!({"points": [[4, 1, 2], [5, 1, 7]], "width": 0.6, "thickness": 0.4, "palette": 3}),
    )
    .unwrap();
    session
}

fn written(session: &Session, options: &MeshExport, name: &str) -> (std::path::PathBuf, usize) {
    let path = std::env::temp_dir().join(format!("froxel-bevel-{name}-{}.glb", std::process::id()));
    crate::export::export_with(session, &path, options).unwrap();
    let triangles = crate::export::glb_triangles(&std::fs::read(&path).unwrap()).unwrap();
    (path, triangles)
}

#[test]
fn every_export_option_writes_and_counts_the_bevel() {
    let session = scene();
    for mode in [BevelMode::Cubes, BevelMode::Edges] {
        let bevel = Some(Bevel { size: 0.15, mode });
        let flat = written(&session, &options(None), "flat");
        let _ = std::fs::remove_file(&flat.0);
        let variants = [
            ("plain", options(bevel)),
            (
                "colors",
                MeshExport {
                    vertex_colors: true,
                    ..options(bevel)
                },
            ),
            (
                "ao",
                MeshExport {
                    ao_strength: 0.5,
                    ..options(bevel)
                },
            ),
            (
                "atlas",
                MeshExport {
                    color_atlas: true,
                    ..options(bevel)
                },
            ),
            (
                "atlas-ao",
                MeshExport {
                    color_atlas: true,
                    ao_strength: 0.5,
                    ..options(bevel)
                },
            ),
            (
                "split",
                MeshExport {
                    split_objects: true,
                    ..options(bevel)
                },
            ),
            (
                "bake",
                MeshExport {
                    color_atlas: true,
                    bake: Some(crate::export::bake::Bake::default()),
                    ..options(bevel)
                },
            ),
        ];
        for (name, options) in variants {
            let (path, triangles) = written(&session, &options, name);
            let _ = std::fs::remove_file(&path);
            assert!(
                triangles > flat.1,
                "{mode:?} {name}: {triangles} vs {}",
                flat.1
            );
            assert_eq!(
                crate::export::counts(&session, &options).unwrap().triangles,
                triangles,
                "{mode:?} {name}"
            );
        }
    }
}

#[test]
fn the_preview_draws_the_bevel_the_glb_holds() {
    let session = scene();
    for mode in [BevelMode::Cubes, BevelMode::Edges] {
        let export = options(Some(Bevel { size: 0.3, mode }));
        let preview = crate::preview::Preview {
            width: 200,
            height: 200,
            yaw: -40.0,
            pitch: 30.0,
            background: [0, 0, 0, 0],
            antialias: false,
            export: Some(export.clone()),
            ..crate::preview::Preview::default()
        };
        let drawn = crate::preview::render(&session, &preview).unwrap();
        let flat = crate::preview::render(
            &session,
            &crate::preview::Preview {
                export: Some(options(None)),
                ..preview.clone()
            },
        )
        .unwrap();
        let (glb, _) = written(&session, &export, "silhouette");
        let png = glb.with_extension("png");
        let read = crate::preview::write_glb(&glb, &preview, &png).unwrap();
        let _ = std::fs::remove_file(&glb);
        let _ = std::fs::remove_file(&png);
        let mask = |rgba: &[u8]| -> Vec<bool> { rgba.chunks(4).map(|px| px[3] != 0).collect() };
        let (a, b, c) = (mask(&drawn.rgba), mask(&read.rgba), mask(&flat.rgba));
        let covered = a.iter().filter(|x| **x).count();
        let differ = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert!(
            differ * 200 < covered,
            "{mode:?}: {differ} of {covered} pixels differ from the glb"
        );
        // And the bevel shows: the rounded silhouette is not the flat one.
        let changed = a.iter().zip(&c).filter(|(x, y)| x != y).count();
        assert!(changed > 20, "{mode:?}: only {changed} pixels changed");
        assert!(drawn.preview_triangles > flat.preview_triangles);
    }
}
