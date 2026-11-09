//! Inspector sections of the silhouette tools: Carve from views (MCP
//! `carve_from_views`), Paint from view (MCP `project_paint`), and Compare
//! silhouette (MCP `compare_silhouette`),
//! whose overlay the viewport shows.

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{EditorMode, Session, CAMERAS, SIDES};

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

fn background(ui: &mut Ui, id: &str, field: &mut crate::state::BackgroundField) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(["Auto", "Alpha", "Key"][field.kind.min(2)])
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut field.kind, 0, "Auto")
                    .on_hover_text("Transparent pixels if the crop has any, else the commonest colour of the image border.");
                ui.selectable_value(&mut field.kind, 1, "Alpha")
                    .on_hover_text("Pixels with alpha below 128 are background.");
                ui.selectable_value(&mut field.kind, 2, "Key")
                    .on_hover_text("Pixels near the key colour are background.");
            });
        if field.kind == 2 {
            ui.color_edit_button_srgb(&mut field.key);
        }
        if field.kind != 1 {
            ui.add(
                egui::DragValue::new(&mut field.tolerance)
                    .range(0.0..=442.0)
                    .speed(1.0),
            )
            .on_hover_text("Tolerance: RGB distance from the key colour that still counts as background.");
        }
    });
}

/// MCP `carve_from_views`: the visual hull of a turnaround sheet's views.
pub(super) fn carve(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Carve from views");
        let mut pick = None;
        let panel = &mut session.silhouette.carve;
        for (row, view) in panel.views.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut view.on, "")
                    .on_hover_text("Use this view. Carve needs 2 or 3.");
                egui::ComboBox::from_id_salt(("carve-side", row))
                    .width(90.0)
                    .selected_text(SIDES[view.side.min(SIDES.len() - 1)].0)
                    .show_ui(ui, |ui| {
                        for (index, (name, _)) in SIDES.iter().enumerate() {
                            ui.selectable_value(&mut view.side, index, *name);
                        }
                    })
                    .response
                    .on_hover_text("The side the view looks from, for a model facing −Y: Left (side) looks from +X with the model's front on the image's left.");
                ui.checkbox(&mut view.flip, "Flip")
                    .on_hover_text("Mirror the image left to right.");
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut view.path)
                        .hint_text("view.png")
                        .desired_width(150.0),
                )
                .on_hover_text("The PNG of this view. Several views can be crops of one sheet.");
                if ui.button("…").on_hover_text("Choose the PNG.").clicked() {
                    pick = Some(row);
                }
                ui.add(
                    egui::TextEdit::singleline(&mut view.crop)
                        .hint_text("crop x y w h")
                        .desired_width(90.0),
                )
                .on_hover_text("The view's rectangle in the image, in pixels: x y width height. Empty is the whole image.");
            });
        }
        egui::Grid::new("carve-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Cells / px", "Cells per image pixel, such as 0.25 for 4 pixels a cell. Used for every view when Size is empty.");
                ui.add(
                    egui::DragValue::new(&mut panel.cells_per_pixel)
                        .range(0.01..=8.0)
                        .speed(0.005),
                );
                ui.end_row();
                label(ui, "Size", "The box x y z in cells; each view is stretched to it. Empty takes the views at Cells / px.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.size)
                        .hint_text("x y z")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Origin", "Local cell of the box's minimum corner.");
                ui.horizontal(|ui| {
                    for value in &mut panel.origin {
                        ui.add(egui::DragValue::new(value).range(-1024..=1023));
                    }
                });
                ui.end_row();
                label(ui, "Background", "Which pixels of every view are background.");
                background(ui, "carve-background", &mut panel.background);
                ui.end_row();
                label(ui, "Colour", "From the views: each cell takes the pixel of the view whose surface is nearest, quantised to the palette. Off fills the active slot.");
                ui.checkbox(&mut panel.colour_views, "From views");
                ui.end_row();
                label(ui, "Shell", "Keep only cells within this many cells of the outside. 0 is solid.");
                ui.add(egui::DragValue::new(&mut panel.shell).range(0..=crate::tools::MAX_CARVE_PASSES as i32));
                ui.end_row();
                label(ui, "Smooth", "Smooth passes over the hull.");
                ui.add(egui::DragValue::new(&mut panel.smooth).range(0..=crate::tools::MAX_CARVE_PASSES as i32));
                ui.end_row();
            });
        if ui
            .button("Carve")
            .on_hover_text("MCP carve_from_views: fill the cells every view covers into the active object. One undo step.")
            .clicked()
        {
            session.carve_from_panel();
        }
        if let Some(row) = pick {
            session.pick_carve_view(row);
        }
    });
}

/// MCP `project_paint`: one image recolours the surface a view sees.
pub(super) fn paint(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Paint from view");
        let mut pick = false;
        let panel = &mut session.silhouette.paint;
        egui::Grid::new("paint-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "View", "The side the image looks from, for a model facing −Y: Top looks down with image down toward −Y; Left (side) looks from +X with the model's front on the image's left.");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("paint-side")
                        .width(90.0)
                        .selected_text(SIDES[panel.side.min(SIDES.len() - 1)].0)
                        .show_ui(ui, |ui| {
                            for (index, (name, _)) in SIDES.iter().enumerate() {
                                ui.selectable_value(&mut panel.side, index, *name);
                            }
                        });
                    ui.checkbox(&mut panel.flip, "Flip")
                        .on_hover_text("Mirror the image left to right.");
                });
                ui.end_row();
                label(ui, "Image", "The PNG to paint: a pattern, or a crop of the turnaround sheet. Background pixels leave the surface alone.");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.path)
                            .hint_text("pattern.png")
                            .desired_width(150.0),
                    );
                    if ui.button("…").on_hover_text("Choose the PNG.").clicked() {
                        pick = true;
                    }
                });
                ui.end_row();
                label(ui, "Crop", "The image's rectangle in pixels: x y width height. Empty is the whole image.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.crop)
                        .hint_text("x y w h")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Background", "Which pixels of the image are background. Alpha on an opaque image paints every pixel.");
                background(ui, "paint-background", &mut panel.background);
                ui.end_row();
                label(ui, "Scale", "Fit stretches the image over the box as seen from the view; off places it at Cells / px, centred and standing on the box's bottom, as Carve from views does.");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut panel.fit, "Fit box");
                    ui.add_enabled(
                        !panel.fit,
                        egui::DragValue::new(&mut panel.cells_per_pixel)
                            .range(0.01..=8.0)
                            .speed(0.005),
                    )
                    .on_hover_text("Cells per image pixel.");
                });
                ui.end_row();
                label(ui, "Box", "Origin x y z and size x y z of the cells that may be painted. Empty is the object's bounds.");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.origin)
                            .hint_text("origin")
                            .desired_width(70.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.size)
                            .hint_text("size")
                            .desired_width(70.0),
                    );
                });
                ui.end_row();
                label(ui, "Depth", "Solid cells painted below the visible surface along each ray; 1 is the surface alone.");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_PAINT_DEPTH as i32));
                ui.end_row();
                label(ui, "Normal limit", "Degrees: a surface turned further than this from the view is left alone, so steep sides get no streaks. 90 is no limit.");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(0.0..=90.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
                label(ui, "Only", "Slots that may be repainted, such as 3 4; other cells keep theirs. Empty is every slot.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.only)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Colour", "From the image: each pixel quantised to the nearest palette slot. Off paints the active slot where the image is inked.");
                ui.checkbox(&mut panel.colour_image, "From image");
                ui.end_row();
            });
        if ui
            .button("Paint")
            .on_hover_text("MCP project_paint: recolour the cells the view sees under the image. Never adds or removes cells. One undo step.")
            .clicked()
        {
            session.paint_from_panel();
        }
        if pick {
            session.pick_paint_image();
        }
    });
}

/// MCP `compare_silhouette`: the model against a concept view, shown over
/// the viewport.
pub(super) fn compare(ui: &mut Ui, session: &mut Session) {
    section(ui, "Compare silhouette");
    let mut pick = false;
    let panel = &mut session.silhouette.compare;
    egui::Grid::new("compare-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            label(ui, "Concept", "The concept PNG: a turnaround sheet or one view.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.path)
                        .hint_text("concept.png")
                        .desired_width(150.0),
                );
                if ui.button("…").on_hover_text("Choose the concept PNG.").clicked() {
                    pick = true;
                }
            });
            ui.end_row();
            label(ui, "Crop", "The view's rectangle in the concept, in pixels: x y width height. Empty is the whole image.");
            ui.add(
                egui::TextEdit::singleline(&mut panel.crop)
                    .hint_text("x y w h")
                    .desired_width(90.0),
            );
            ui.end_row();
            label(ui, "Background", "Which pixels of the concept are background.");
            background(ui, "compare-background", &mut panel.background);
            ui.end_row();
            label(ui, "Flip", "Mirror the concept left to right.");
            ui.checkbox(&mut panel.flip, "");
            ui.end_row();
            label(ui, "Camera", "Viewport is the window's camera now; Game is the gameplay preset, with the Camera section's Camera presets… and Placement turn; Front, Left, and Top are orthographic views like a turnaround sheet.");
            egui::ComboBox::from_id_salt("compare-camera")
                .selected_text(CAMERAS[panel.camera.min(CAMERAS.len() - 1)])
                .show_ui(ui, |ui| {
                    for (index, name) in CAMERAS.iter().enumerate() {
                        ui.selectable_value(&mut panel.camera, index, *name);
                    }
                });
            ui.end_row();
            label(ui, "Align", "Fit finds the one scale and offset with the most overlap; Height and Width match that extent. Proportions are kept.");
            ui.horizontal(|ui| {
                for (index, name) in ["Fit", "Height", "Width"].into_iter().enumerate() {
                    ui.selectable_value(&mut panel.align, index, name);
                }
            });
            ui.end_row();
        });
    if pick {
        session.pick_compare_concept();
    }
    ui.horizontal(|ui| {
        if ui
            .button("Compare")
            .on_hover_text("MCP compare_silhouette: IoU, missing, and excess, with an overlay over the viewport: the concept's outline in cyan, what the model misses in magenta, excess in orange.")
            .clicked()
        {
            session.compare_from_panel();
        }
        if session.silhouette.overlay.is_some()
            && ui
                .button("Hide overlay")
                .on_hover_text("Close the comparison over the viewport.")
                .clicked()
        {
            session.silhouette.overlay = None;
        }
    });
    if let Some(overlay) = &session.silhouette.overlay {
        ui.label(
            RichText::new(format!(
                "IoU {:.3}   missing {:.1}%   excess {:.1}%",
                overlay.iou,
                overlay.missing * 100.0,
                overlay.excess * 100.0
            ))
            .small(),
        );
    }
}
