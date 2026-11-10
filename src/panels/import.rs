//! The Import mesh inspector section: MCP `import_mesh` from the editor.

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{EditorMode, Session};

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

/// MCP `import_mesh`: a .glb, .gltf, or .obj voxelized with these options.
pub(super) fn import(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Import mesh");
        let panel = &mut session.import;
        let mut browse = false;
        egui::Grid::new("import-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "File", "A .glb, .gltf, or .obj (with its .mtl and textures). Every primitive of the default glTF scene is read under its node transforms.");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.path)
                            .hint_text("mesh.glb")
                            .desired_width(110.0),
                    );
                    browse = ui.button("…").on_hover_text("Choose a mesh and import it.").clicked();
                });
                ui.end_row();
                label(ui, "Fit", "Size: cells along the longest axis. Voxel: metres per cell, so the mesh keeps its real size (0.08 for 0.08 m voxels). Scale: cells per file unit.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Size", "Voxel", "Scale"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.fit, index, name);
                    }
                });
                ui.end_row();
                match panel.fit {
                    1 => {
                        label(ui, "Voxel size", "Metres per cell.");
                        ui.add(egui::DragValue::new(&mut panel.voxel_size).range(0.001..=100.0).speed(0.001).suffix(" m"));
                    }
                    2 => {
                        label(ui, "Scale", "Cells per file unit.");
                        ui.add(egui::DragValue::new(&mut panel.scale).range(0.01..=10_000.0).speed(0.1));
                    }
                    _ => {
                        label(ui, "Size", "Cells along the longest axis, at most 256.");
                        ui.add(egui::DragValue::new(&mut panel.size).range(1..=256));
                    }
                }
                ui.end_row();
                label(ui, "Z up", "Off turns the file's Y-up (glTF and most OBJ) to Z-up; on keeps its axes.");
                ui.checkbox(&mut panel.z_up, "File is Z-up");
                ui.end_row();
                label(ui, "Rotation", "Degrees about X, then Y, then Z, after the up axis.");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.rotation.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-360.0..=360.0).speed(1.0).suffix("°"));
                    }
                });
                ui.end_row();
                label(ui, "Position", "Where the anchor lands, in the active object's cells; with New object, the new object's world position.");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.position.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-1024..=1023));
                    }
                });
                ui.end_row();
                label(ui, "Anchor", "The point of the mesh's box placed at Position: the low corner, the centre, or the centre of the bottom.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Min", "Center", "Bottom"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.anchor, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Fill", "Solid fills everything a flood from the box border cannot reach, so a hollow or double-walled mesh comes out solid; an open mesh (holes, seams, an open underside) first has its openings sealed at their rims. Parity is the older vote of ray parity along X, Y, and Z. Shell writes only the cells the surface touches.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Solid", "Parity", "Shell"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.fill, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Colour", "Texture: the base-colour texture times its factor times vertex colours, at the nearest point of the surface. Vertex: vertex colours only.");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.vertex_only, false, "Texture");
                    ui.selectable_value(&mut panel.vertex_only, true, "Vertex");
                });
                ui.end_row();
                label(ui, "Slots", "Map colours only to these palette indices, leaving the palette alone. Empty quantizes into the palette.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("quantize")
                        .desired_width(120.0),
                );
                ui.end_row();
                label(ui, "New slots", "Most palette slots written with new colours, taken from 255 down among slots no cell uses. 0 maps to the nearest existing slots.");
                ui.add(egui::DragValue::new(&mut panel.max_new).range(0..=255));
                ui.end_row();
                label(ui, "Tolerance", "sRGB distance under which a colour reuses an existing slot.");
                ui.add(egui::DragValue::new(&mut panel.tolerance).range(0.0..=442.0).speed(0.5));
                ui.end_row();
                label(ui, "Mode", "Replace writes every cell; Empty adds only into empty cells; Paint recolours solid cells, to project a texture onto a form.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Target", "Import into a new object, or into the active one.");
                ui.checkbox(&mut panel.new_object, "New object");
                ui.end_row();
                label(ui, "Orient", "MCP import_mesh orient, before voxelizing. Upright: stand a tilted mesh level from its shape (thinnest principal axis up, narrowing upward, the taller end of its long axis toward −Y), tilting up at most Max tilt. Views: the axis-aligned turn and fine yaw whose silhouettes best match the views of Carve from views (with the Orient to views options). Both: upright, then the views' yaw.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Off", "Upright", "Views", "Both"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.orient, index, name);
                    }
                });
                ui.end_row();
                if panel.orient == 1 || panel.orient == 3 {
                    label(ui, "Max tilt", "Degrees Upright may move the mesh's up. A fish's thinnest axis lies flat, so a larger tilt would lay it on its side; past this the file's up stays.");
                    ui.add(egui::DragValue::new(&mut panel.max_tilt).range(0.0..=180.0).speed(1.0).suffix("°"));
                    ui.end_row();
                }
                label(ui, "Clean", "MCP import_mesh clean: reduce the imported colours and despeckle them with the Reduce palette section's options before they are written.");
                ui.checkbox(&mut panel.clean, "Reduce palette");
                ui.end_row();
                label(ui, "Coverage", "MCP coverage: keep a cell only when this share of it is solid (measured 2 to 4 times finer) instead of every cell the surface touches, so a small creature's legs and fins do not merge into lumps. Keep thin: a part thinner than a cell (a fin, an ear, a leg) keeps the one cell its midline crosses instead of vanishing.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.coverage, "");
                    ui.add_enabled(
                        panel.coverage,
                        egui::DragValue::new(&mut panel.coverage_share).range(0.0..=1.0).speed(0.01),
                    );
                    ui.add_enabled(panel.coverage, egui::Checkbox::new(&mut panel.keep_thin, "Keep thin"));
                });
                ui.end_row();
                label(ui, "Min feature", "MCP minFeature: report the imported parts thinner than this many cells. 0 is off.");
                ui.add(egui::DragValue::new(&mut panel.min_feature).range(0..=16));
                ui.end_row();
            });
        if browse {
            session.import_mesh_dialog();
        }
        if ui
            .button("Import mesh")
            .on_hover_text("MCP import_mesh with these options (File > Import Mesh… picks the file). One undo step.")
            .clicked()
        {
            session.import_from_panel();
        }
    });
}
