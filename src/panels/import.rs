//! The Import mesh inspector section: MCP `import_mesh` from the editor.

use egui::{RichText, Ui};

use super::{prop_drag, prop_label};
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
                prop_label(ui, "import_mesh", "path");
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
                        prop_label(ui, "import_mesh", "voxelSize");
                        ui.add(egui::DragValue::new(&mut panel.voxel_size).range(0.001..=100.0).speed(0.001).suffix(" m"));
                    }
                    2 => {
                        prop_label(ui, "import_mesh", "scale");
                        prop_drag(ui, "import_mesh", "scale", &mut panel.scale);
                    }
                    _ => {
                        prop_label(ui, "import_mesh", "size");
                        prop_drag(ui, "import_mesh", "size", &mut panel.size);
                    }
                }
                ui.end_row();
                prop_label(ui, "import_mesh", "upAxis");
                ui.checkbox(&mut panel.z_up, "File is Z-up");
                ui.end_row();
                prop_label(ui, "import_mesh", "rotation");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.rotation.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-360.0..=360.0).speed(1.0).suffix("°"));
                    }
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "position");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.position.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-1024..=1023));
                    }
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "anchor");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Min", "Center", "Bottom"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.anchor, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "fill");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Solid", "Parity", "Shell"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.fill, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "colors");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.vertex_only, false, "Texture");
                    ui.selectable_value(&mut panel.vertex_only, true, "Vertex");
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "slots");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("quantize")
                        .desired_width(120.0),
                );
                ui.end_row();
                prop_label(ui, "import_mesh", "maxNewSlots");
                prop_drag(ui, "import_mesh", "maxNewSlots", &mut panel.max_new);
                ui.end_row();
                prop_label(ui, "import_mesh", "tolerance");
                prop_drag(ui, "import_mesh", "tolerance", &mut panel.tolerance);
                ui.end_row();
                prop_label(ui, "import_mesh", "mode");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "newObject");
                ui.checkbox(&mut panel.new_object, "New object");
                ui.end_row();
                prop_label(ui, "import_mesh", "orient");
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
                prop_label(ui, "import_mesh", "clean");
                ui.checkbox(&mut panel.clean, "Reduce palette");
                ui.end_row();
                prop_label(ui, "import_mesh", "coverage");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.coverage, "");
                    ui.add_enabled(
                        panel.coverage,
                        egui::DragValue::new(&mut panel.coverage_share).range(0.0..=1.0).speed(0.01),
                    );
                    ui.add_enabled(panel.coverage, egui::Checkbox::new(&mut panel.keep_thin, "Keep thin"));
                });
                ui.end_row();
                prop_label(ui, "import_mesh", "minFeature");
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
