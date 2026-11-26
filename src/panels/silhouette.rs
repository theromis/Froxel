//! Inspector sections of the silhouette tools: Pixelate views (MCP
//! `pixelate_views`), Carve from views (MCP `carve_from_views`), Paint from view (MCP `project_paint`), Paint from
//! views (MCP `paint_from_views`), Decal (MCP
//! `decal`), Fit to views
//! (MCP `fit_to_views`), Optimize to views (MCP `optimize_to_views`), and Compare silhouette (MCP `compare_silhouette`),
//! whose overlay the viewport shows.

use egui::{RichText, Ui};

use super::{choice_label, prop_check, prop_drag, prop_label};
use super::{section, DIM};
use crate::state::{EditorMode, Session, CAMERAS, SIDES};

fn background(ui: &mut Ui, id: &str, field: &mut crate::state::BackgroundField) {
    ui.horizontal_wrapped(|ui| {
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

/// MCP `measure_sheet`: a views sheet's crops, cube pitch, checks, and
/// parts, read before carving it.
pub(super) fn measure(ui: &mut Ui, session: &mut Session) {
    section(ui, "Measure sheet");
    let mut pick = false;
    let panel = &mut session.silhouette.measure;
    egui::Grid::new("measure-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            prop_label(ui, "measure_sheet", "path");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.path)
                        .hint_text("views.png")
                        .desired_width(150.0),
                );
                if ui
                    .button("…")
                    .on_hover_text("Choose the sheet PNG.")
                    .clicked()
                {
                    pick = true;
                }
            });
            ui.end_row();
            prop_label(ui, "measure_sheet", "background");
            background(ui, "measure-background", &mut panel.background);
            ui.end_row();
            prop_label(ui, "measure_sheet", "parts");
            ui.checkbox(&mut panel.parts, "");
            ui.end_row();
        });
    if pick {
        session.pick_measure_sheet();
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Measure")
            .on_hover_text("MCP measure_sheet: each view's crop, its cube pitch in pixels from the seams, and whether the front, side, and top agree in size. Writes no file.")
            .clicked()
        {
            session.measure_from_panel();
        }
        if !session.silhouette.measure.views.is_empty()
            && ui
                .button("Use crops")
                .on_hover_text("Put the sheet and the measured crops into the Carve from views rows (front, left, top), which Fit to views shares.")
                .clicked()
        {
            session.use_measured_crops();
        }
    });
    for line in &session.silhouette.measure.lines {
        ui.label(RichText::new(line).small().color(DIM));
    }
}

/// MCP `pixelate_views`: a views sheet as pixel art at the cell grid.
pub(super) fn pixelate(ui: &mut Ui, session: &mut Session) {
    section(ui, "Pixelate views");
    let mut pick = false;
    let mut pick_png = false;
    let panel = &mut session.silhouette.pixelate;
    egui::Grid::new("pixelate-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            prop_label(ui, "pixelate_views", "path");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.path)
                        .hint_text("Carve's views")
                        .desired_width(150.0),
                );
                if ui
                    .button("…")
                    .on_hover_text("Choose the sheet PNG.")
                    .clicked()
                {
                    pick = true;
                }
            });
            ui.end_row();
            prop_label(ui, "pixelate_views", "background");
            background(ui, "pixelate-background", &mut panel.background);
            ui.end_row();
            prop_label(ui, "pixelate_views", "size");
            ui.add(
                egui::TextEdit::singleline(&mut panel.size)
                    .hint_text("0 20 0")
                    .desired_width(90.0),
            );
            ui.end_row();
            prop_label(ui, "pixelate_views", "colors");
            prop_drag(ui, "pixelate_views", "colors", &mut panel.colors);
            ui.end_row();
            prop_label(ui, "pixelate_views", "features");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut panel.features, "Features");
                ui.checkbox(&mut panel.thin, "Thin");
                ui.add(egui::DragValue::new(&mut panel.despeckle).range(0..=16))
                    .on_hover_text("Despeckle passes.");
            });
            ui.end_row();
            prop_label(ui, "pixelate_views", "lines");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut panel.draw_lines, "Lines");
                ui.add_enabled(
                    panel.draw_lines,
                    egui::Checkbox::new(&mut panel.outline, "Outline"),
                );
                ui.add_enabled(
                    panel.draw_lines,
                    egui::Checkbox::new(&mut panel.merge_lines, "Merge"),
                );
                ui.checkbox(&mut panel.fill_holes, "Fill holes");
            });
            ui.end_row();
            prop_label(ui, "pixelate_views", "png");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.png)
                        .hint_text("pixelated.png")
                        .desired_width(150.0),
                );
                if ui
                    .button("…")
                    .on_hover_text("Choose where to write the PNG.")
                    .clicked()
                {
                    pick_png = true;
                }
            });
            ui.end_row();
        });
    if pick {
        session.pick_pixelate_sheet();
    }
    if pick_png {
        session.pick_pixelate_png();
    }
    if ui
        .button("Pixelate")
        .on_hover_text("MCP pixelate_views: the views as pixel art at the cell grid, for checking before Carve or Paint from views with Pixel art. Changes no cells.")
        .clicked()
    {
        session.pixelate_from_panel();
    }
    for line in &session.silhouette.pixelate.lines {
        ui.label(RichText::new(line).small().color(DIM));
    }
}

/// MCP `carve_from_views`: the visual hull of a turnaround sheet's views.
pub(super) fn carve(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Carve from views");
        let mut pick = None;
        let mut pick_depth = None;
        let mut pick_detail = false;
        let panel = &mut session.silhouette.carve;
        for (row, view) in panel.views.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut view.on, "")
                    .on_hover_text("Use this view. Carve needs 2 or 3, or 1 with a depth map.");
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
            ui.horizontal_wrapped(|ui| {
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
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut view.depth)
                        .hint_text("depth.png (optional)")
                        .desired_width(150.0),
                )
                .on_hover_text("A grey depth map of the same crop, white near and black far (an image generator's depth pass, or Depth Anything V2 with grayscale output). Cells in front of its surface are carved away, so a round form comes out round. With no view image the map's silhouette is used.");
                if ui.button("…").on_hover_text("Choose the depth PNG.").clicked() {
                    pick_depth = Some(row);
                }
            });
        }
        egui::Grid::new("carve-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "carve_from_views", "cellsPerPixel");
                prop_drag(ui, "carve_from_views", "cellsPerPixel", &mut panel.cells_per_pixel);
                ui.end_row();
                prop_label(ui, "carve_from_views", "size");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.size)
                        .hint_text("x y z")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "carve_from_views", "origin");
                ui.horizontal_wrapped(|ui| {
                    for value in &mut panel.origin {
                        ui.add(egui::DragValue::new(value).range(-1024..=1023));
                    }
                    ui.checkbox(&mut panel.world, "World")
                        .on_hover_text("MCP space world: the origin is a world cell, and the object's position is subtracted first.");
                });
                ui.end_row();
                prop_label(ui, "carve_from_views", "background");
                background(ui, "carve-background", &mut panel.background);
                ui.end_row();
                choice_label(ui, "carve_from_views", "colour");
                ui.checkbox(&mut panel.colour_views, "From views");
                ui.end_row();
                prop_label(ui, "carve_from_views", "shell");
                ui.add(egui::DragValue::new(&mut panel.shell).range(0..=crate::tools::MAX_CARVE_PASSES as i32));
                ui.end_row();
                prop_label(ui, "carve_from_views", "smooth");
                ui.add(egui::DragValue::new(&mut panel.smooth).range(0..=crate::tools::MAX_CARVE_PASSES as i32));
                ui.end_row();
                choice_label(ui, "carve_from_views", "depth");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("carve-depth-calibrate")
                        .width(60.0)
                        .selected_text(["Hull", "Box"][panel.depth_calibrate.min(1)])
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut panel.depth_calibrate, 0, "Hull")
                                .on_hover_text("Calibrate each map against the silhouette hull.");
                            ui.selectable_value(&mut panel.depth_calibrate, 1, "Box")
                                .on_hover_text("The map's range spans the box along the view.");
                        });
                    ui.checkbox(&mut panel.depth_invert, "Invert")
                        .on_hover_text("Black is near.");
                    ui.checkbox(&mut panel.depth_back, "Back")
                        .on_hover_text("Mirror the near surface about the hull's middle for the far side.");
                    ui.add(
                        egui::DragValue::new(&mut panel.depth_tolerance)
                            .range(0.0..=64.0)
                            .speed(0.05),
                    )
                    .on_hover_text("Tolerance: cells in front of a depth surface that stay.");
                });
                ui.end_row();
                choice_label(ui, "carve_from_views", "depthParts");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.depth_auto_curve, "Auto curve")
                        .on_hover_text("gamma auto: the exponent whose calibration meets the silhouette contour best.");
                    ui.checkbox(&mut panel.depth_regions, "Regions")
                        .on_hover_text("Calibrate each region of the depth map on its own against the silhouettes.");
                    ui.add(
                        egui::DragValue::new(&mut panel.depth_edge)
                            .range(0.005..=1.0)
                            .speed(0.002),
                    )
                    .on_hover_text("Edge: the grey step (0..1) between two regions, a jump or a valley between two peaks.");
                });
                ui.end_row();
                choice_label(ui, "carve_from_views", "depthDetail");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.depth_detail)
                            .hint_text("detail.png (optional)")
                            .desired_width(150.0),
                    );
                    if ui.button("…").on_hover_text("Choose the detail depth PNG.").clicked() {
                        pick_detail = true;
                    }
                });
                ui.end_row();
                prop_label(ui, "carve_from_views", "downscale");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.pixel_art, "Downscale");
                    ui.add_enabled(
                        panel.pixel_art,
                        egui::DragValue::new(&mut panel.pixel_colors).range(1..=32),
                    )
                    .on_hover_text("Colours of the shared palette, plus kept accents such as eyes.");
                });
                ui.end_row();
                prop_label(ui, "carve_from_views", "mode");
                ui.checkbox(&mut panel.intersect, "Keep inside only");
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
        if let Some(row) = pick_depth {
            session.pick_carve_depth(row);
        }
        if pick_detail {
            session.pick_carve_detail();
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
                prop_label(ui, "project_paint", "view");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("paint-side")
                        .width(90.0)
                        .selected_text(SIDES[panel.side.min(SIDES.len() - 1)].0)
                        .show_ui(ui, |ui| {
                            for (index, (name, _)) in SIDES.iter().enumerate() {
                                ui.selectable_value(&mut panel.side, index, *name);
                            }
                        });
                    prop_check(ui, "project_paint", "flip", &mut panel.flip);
                });
                ui.end_row();
                prop_label(ui, "project_paint", "path");
                ui.horizontal_wrapped(|ui| {
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
                prop_label(ui, "project_paint", "crop");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.crop)
                        .hint_text("x y w h")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "project_paint", "background");
                background(ui, "paint-background", &mut panel.background);
                ui.end_row();
                choice_label(ui, "project_paint", "scale");
                ui.horizontal_wrapped(|ui| {
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
                prop_label(ui, "project_paint", "origin");
                ui.horizontal_wrapped(|ui| {
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
                    ui.checkbox(&mut panel.world, "World")
                        .on_hover_text("MCP space world: the origin is a world cell, and the object's position is subtracted first.");
                });
                ui.end_row();
                prop_label(ui, "project_paint", "depth");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_PAINT_DEPTH as i32));
                ui.end_row();
                prop_label(ui, "project_paint", "normalLimit");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(0.0..=90.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
                prop_label(ui, "project_paint", "only");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.only)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                choice_label(ui, "project_paint", "colour");
                ui.checkbox(&mut panel.colour_image, "From image");
                ui.end_row();
                super::edit::mirror_row(ui, "project_paint", "paint-mirror", &mut panel.mirror);
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

/// MCP `decal`: one picture laid onto the surface around a point.
pub(super) fn decal(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Decal");
        let (mut pick, mut point) = (false, false);
        let panel = &mut session.silhouette.decal;
        egui::Grid::new("decal-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "decal", "path");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.path)
                            .hint_text("eye.png")
                            .desired_width(150.0),
                    );
                    if ui.button("…").on_hover_text("Choose the PNG.").clicked() {
                        pick = true;
                    }
                    prop_check(ui, "decal", "flip", &mut panel.flip);
                });
                ui.end_row();
                prop_label(ui, "decal", "crop");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.crop)
                        .hint_text("x y w h")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "decal", "background");
                background(ui, "decal-background", &mut panel.background);
                ui.end_row();
                prop_label(ui, "decal", "point");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.point)
                            .hint_text("x y z")
                            .desired_width(90.0),
                    );
                    if ui.button("At selection").on_hover_text("The selection's centre.").clicked() {
                        point = true;
                    }
                    ui.checkbox(&mut panel.world, "World");
                });
                ui.end_row();
                prop_label(ui, "decal", "up");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.up)
                        .hint_text("0 0 1")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "decal", "size");
                ui.add(egui::DragValue::new(&mut panel.size).range(1.0..=crate::tools::MAX_DECAL_SIZE as f32).speed(0.1));
                ui.end_row();
                prop_label(ui, "decal", "depth");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_DECAL_DEPTH as i32));
                ui.end_row();
                prop_label(ui, "decal", "reach");
                ui.add(egui::DragValue::new(&mut panel.reach).range(0.0..=crate::tools::MAX_DECAL_REACH as f32).speed(0.1));
                ui.end_row();
                prop_label(ui, "decal", "normalLimit");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(1.0..=180.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
                prop_label(ui, "decal", "only");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.only)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                choice_label(ui, "decal", "colour");
                ui.checkbox(&mut panel.colour_image, "From image");
                ui.end_row();
                super::edit::mirror_row(ui, "decal", "decal-mirror", &mut panel.mirror);
            });
        if ui
            .button("Lay decal")
            .on_hover_text("MCP decal: lay the picture on the surface around the point, following the surface so it keeps its size and shape on a curved form. Never adds or removes cells. One undo step.")
            .clicked()
        {
            session.decal_from_panel();
        }
        if pick {
            session.pick_decal_image();
        }
        if point {
            session.decal_point_at_selection();
        }
    });
}

/// MCP `fit_to_views`: refine the active object to match the views of
/// Carve from views.
pub(super) fn fit(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Fit to views");
        ui.label(
            RichText::new("Uses the views, crops, and background of Carve from views.")
                .small()
                .color(DIM),
        );
        let panel = &mut session.silhouette.fit;
        egui::Grid::new("fit-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "fit_to_views", "align");
                egui::ComboBox::from_id_salt("fit-align")
                    .selected_text(["Fit", "Height", "Width"][panel.align.min(2)])
                    .show_ui(ui, |ui| {
                        for (index, name) in ["Fit", "Height", "Width"].iter().enumerate() {
                            ui.selectable_value(&mut panel.align, index, *name);
                        }
                    });
                ui.end_row();
                prop_label(ui, "fit_to_views", "shape");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.shape, "Shape");
                    ui.checkbox(&mut panel.recolor, "Colour");
                });
                ui.end_row();
                prop_label(ui, "fit_to_views", "recolor");
                ui.horizontal_wrapped(|ui| {
                    for (index, paints) in panel.paint_views.iter_mut().enumerate() {
                        ui.checkbox(paints, format!("{}", index + 1))
                            .on_hover_text("Carve from views row");
                    }
                });
                ui.end_row();
                prop_label(ui, "fit_to_views", "normalLimit");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(0.0..=90.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
                prop_label(ui, "fit_to_views", "keep");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.keep)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "fit_to_views", "roundWeight");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.round_weight).range(0.0..=100.0).speed(0.01))
                        .on_hover_text("Round weight");
                    ui.add(egui::DragValue::new(&mut panel.roundness).range(1.0..=16.0).speed(0.05))
                        .on_hover_text("Roundness exponent");
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.round_axes)
                            .hint_text("yz")
                            .desired_width(32.0),
                    )
                    .on_hover_text("Axes whose cross-sections are rounded: y front sections, z the plan, x side sections (through a neck).");
                });
                ui.end_row();
                prop_label(ui, "fit_to_views", "iterations");
                ui.add(egui::DragValue::new(&mut panel.iterations).range(0..=crate::tools::MAX_FIT_ITERATIONS as i32));
                ui.end_row();
                prop_label(ui, "fit_to_views", "seed");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=i32::MAX));
                ui.end_row();
                prop_label(ui, "fit_to_views", "slots");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("object's slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "fit_to_views", "reference");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.reference)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "fit_to_views", "isolate");
                ui.checkbox(&mut panel.isolate, "Active object only");
                ui.end_row();
                prop_label(ui, "fit_to_views", "maskWeight");
                ui.horizontal_wrapped(|ui| {
                    for (value, tip) in [
                        (&mut panel.mask_weight, "Mask weight"),
                        (&mut panel.color_weight, "Colour weight"),
                        (&mut panel.depth_weight, "Depth weight"),
                        (&mut panel.smooth_weight, "Smooth weight"),
                    ] {
                        ui.add(egui::DragValue::new(value).range(0.0..=100.0).speed(0.01))
                            .on_hover_text(tip);
                    }
                });
                ui.end_row();
            });
        if ui
            .button("Fit")
            .on_hover_text("MCP fit_to_views: refine the active object so its orthographic views match the sheet, colours from the palette. One undo step.")
            .clicked()
        {
            session.fit_from_panel();
        }
    });
}

/// MCP `optimize_to_views`: iterate a small creature toward the pixel-art
/// views of Carve from views.
pub(super) fn optimize(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Optimize to views");
        ui.label(
            RichText::new("For a small creature (20 to 60 cells): the views of Carve from views, pixelated at the object's box.")
                .small()
                .color(DIM),
        );
        let panel = &mut session.silhouette.optimize;
        egui::Grid::new("optimize-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "optimize_to_views", "shape");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.shape, "Shape");
                    ui.checkbox(&mut panel.recolor, "Colour");
                    ui.checkbox(&mut panel.color_carve, "Carve");
                });
                ui.end_row();
                prop_label(ui, "optimize_to_views", "recolor");
                ui.horizontal_wrapped(|ui| {
                    for (index, paints) in panel.paint_views.iter_mut().enumerate() {
                        ui.checkbox(paints, format!("{}", index + 1))
                            .on_hover_text("Carve from views row");
                    }
                });
                ui.end_row();
                choice_label(ui, "optimize_to_views", "colours");
                ui.add(egui::DragValue::new(&mut panel.colors).range(1..=32));
                ui.end_row();
                prop_label(ui, "optimize_to_views", "iterations");
                ui.add(egui::DragValue::new(&mut panel.iterations).range(0..=crate::tools::MAX_OPTIMIZE_ITERATIONS as i32));
                ui.end_row();
                prop_label(ui, "optimize_to_views", "band");
                ui.add(egui::DragValue::new(&mut panel.band).range(0..=crate::tools::MAX_OPTIMIZE_BAND as i32));
                ui.end_row();
                prop_label(ui, "optimize_to_views", "minFeature");
                ui.add(egui::DragValue::new(&mut panel.min_feature).range(1..=crate::tools::MAX_MIN_FEATURE as i32));
                ui.end_row();
                prop_label(ui, "optimize_to_views", "smooth");
                prop_drag(ui, "optimize_to_views", "smooth", &mut panel.smooth);
                ui.end_row();
                prop_label(ui, "optimize_to_views", "sharpness");
                prop_drag(ui, "optimize_to_views", "sharpness", &mut panel.sharpness);
                ui.end_row();
                prop_label(ui, "optimize_to_views", "accents");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.accents, "Guard");
                    ui.add_enabled(
                        panel.accents,
                        egui::DragValue::new(&mut panel.accent_share).range(0.0..=0.5).speed(0.001),
                    );
                });
                ui.end_row();
                prop_label(ui, "optimize_to_views", "slots");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("view colours")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "optimize_to_views", "keep");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.keep)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
                let clip = cfg!(feature = "clip");
                prop_label(ui, "optimize_to_views", "semantic");
                ui.add_enabled_ui(clip, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut panel.semantic, "CLIP").on_disabled_hover_text(
                            "This froxel is built without the clip feature (cargo build --release --features clip-metal).",
                        );
                        ui.add_enabled(panel.semantic, egui::DragValue::new(&mut panel.semantic_rounds).range(0..=8))
                            .on_hover_text("Rounds");
                        ui.add_enabled(panel.semantic, egui::DragValue::new(&mut panel.semantic_patches).range(0..=64))
                            .on_hover_text("Patches tried per round");
                    });
                });
                ui.end_row();
            });
        if ui
            .button("Optimize")
            .on_hover_text("MCP optimize_to_views: iterate the active object's shape and colours until its orthographic views match the pixel-art views; the status shows each view's agreement before and after. One undo step.")
            .clicked()
        {
            session.optimize_from_panel();
        }
    });
}

/// MCP `orient_to_views`: the active object turned to match the views of
/// Carve from views.
pub(super) fn orient(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Orient to views");
        ui.label(
            RichText::new("Uses the views, crops, and background of Carve from views.")
                .small()
                .color(DIM),
        );
        let panel = &mut session.silhouette.orient;
        egui::Grid::new("orient-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "orient_to_views", "turns");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.upright, false, "All 24");
                    ui.selectable_value(&mut panel.upright, true, "Upright");
                });
                ui.end_row();
                prop_label(ui, "orient_to_views", "yawStep");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.yaw_step).range(0.0..=90.0).speed(0.5).suffix("°"));
                    ui.add(egui::DragValue::new(&mut panel.yaw_range).range(0.0..=180.0).speed(1.0).prefix("±").suffix("°"));
                    ui.checkbox(&mut panel.tilt, "Tilt");
                });
                ui.end_row();
                prop_label(ui, "orient_to_views", "size");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Keep", "Longest", "Height"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.size_mode, index, name);
                    }
                    if panel.size_mode > 0 {
                        ui.add(egui::DragValue::new(&mut panel.size).range(1..=256));
                    }
                });
                ui.end_row();
            });
        if ui
            .button("Orient")
            .on_hover_text("MCP orient_to_views: turn (and scale) the active object to the turn whose silhouettes best match the views. One undo step.")
            .clicked()
        {
            session.orient_from_panel();
        }
    });
}

/// MCP `compare_silhouette`: the model against a concept view, shown over
/// the viewport.
pub(super) fn compare(ui: &mut Ui, session: &mut Session) {
    section(ui, "Compare silhouette");
    let mut pick = false;
    let mut pick_mask = false;
    let panel = &mut session.silhouette.compare;
    egui::Grid::new("compare-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            prop_label(ui, "compare_silhouette", "concept");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.path)
                        .hint_text("concept.png")
                        .desired_width(150.0),
                );
                if ui
                    .button("…")
                    .on_hover_text("Choose the concept PNG.")
                    .clicked()
                {
                    pick = true;
                }
            });
            ui.end_row();
            prop_label(ui, "compare_silhouette", "crop");
            ui.add(
                egui::TextEdit::singleline(&mut panel.crop)
                    .hint_text("x y w h")
                    .desired_width(90.0),
            );
            ui.end_row();
            prop_label(ui, "compare_silhouette", "background");
            background(ui, "compare-background", &mut panel.background);
            ui.end_row();
            prop_label(ui, "compare_silhouette", "flip");
            ui.checkbox(&mut panel.flip, "");
            ui.end_row();
            prop_label(ui, "compare_silhouette", "excludeColors");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.exclude)
                        .hint_text("#3a6ea5 #ffffff")
                        .desired_width(110.0),
                );
                ui.add(egui::DragValue::new(&mut panel.exclude_tolerance).range(0.0..=442.0));
            });
            ui.end_row();
            prop_label(ui, "compare_silhouette", "flood");
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut panel.flood, "");
                ui.add_enabled(
                    panel.flood,
                    egui::DragValue::new(&mut panel.flood_tolerance).range(0.0..=442.0),
                );
            });
            ui.end_row();
            prop_label(ui, "compare_silhouette", "mask");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.mask)
                        .hint_text("mask.png")
                        .desired_width(110.0),
                );
                if ui
                    .button("…")
                    .on_hover_text("Choose the mask PNG.")
                    .clicked()
                {
                    pick_mask = true;
                }
                ui.checkbox(&mut panel.invert_mask, "Invert");
            });
            ui.end_row();
            prop_label(ui, "compare_silhouette", "erase");
            ui.add(
                egui::TextEdit::multiline(&mut panel.erase)
                    .hint_text("x y w h")
                    .desired_rows(1)
                    .desired_width(150.0),
            );
            ui.end_row();
            prop_label(ui, "compare_silhouette", "keepLargest");
            ui.checkbox(&mut panel.keep_largest, "");
            ui.end_row();
            prop_label(ui, "compare_silhouette", "fillHoles");
            ui.checkbox(&mut panel.fill_holes, "");
            ui.end_row();
            prop_label(ui, "compare_silhouette", "preset");
            egui::ComboBox::from_id_salt("compare-camera")
                .selected_text(CAMERAS[panel.camera.min(CAMERAS.len() - 1)])
                .show_ui(ui, |ui| {
                    for (index, name) in CAMERAS.iter().enumerate() {
                        ui.selectable_value(&mut panel.camera, index, *name);
                    }
                });
            ui.end_row();
            prop_label(ui, "compare_silhouette", "align");
            ui.horizontal_wrapped(|ui| {
                for (index, name) in ["Fit", "Height", "Width"].into_iter().enumerate() {
                    ui.selectable_value(&mut panel.align, index, name);
                }
            });
            ui.end_row();
        });
    if pick {
        session.pick_compare_concept();
    }
    if pick_mask {
        session.pick_compare_mask();
    }
    ui.horizontal_wrapped(|ui| {
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

/// MCP `paint_from_views`: a whole views sheet recolours every surface cell.
pub(super) fn paint_views(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Paint from views");
        let mut pick = false;
        let panel = &mut session.silhouette.paint_views;
        egui::Grid::new("paint-views-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "paint_from_views", "path");
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.path)
                            .hint_text("Carve's views")
                            .desired_width(150.0),
                    );
                    if ui.button("…").on_hover_text("Choose the sheet PNG.").clicked() {
                        pick = true;
                    }
                });
                ui.end_row();
                prop_label(ui, "paint_from_views", "background");
                background(ui, "paint-views-background", &mut panel.background);
                ui.end_row();
                prop_label(ui, "paint_from_views", "blend");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.blend, 0, "Nearest");
                    ui.selectable_value(&mut panel.blend, 1, "Soft");
                    if panel.blend == 0 {
                        ui.add(egui::DragValue::new(&mut panel.seam).range(0.0..=45.0).speed(0.5).suffix("°"))
                            .on_hover_text("Seam: degrees around the boundary between two views where they blend; 0 is a hard seam.");
                    } else {
                        ui.add(egui::DragValue::new(&mut panel.sharpness).range(0.0..=64.0).speed(0.1))
                            .on_hover_text("Sharpness: the power of the facing weight; higher is closer to Nearest.");
                    }
                });
                ui.end_row();
                prop_label(ui, "paint_from_views", "mirror");
                ui.checkbox(&mut panel.mirror, "Missing side");
                ui.end_row();
                choice_label(ui, "paint_from_views", "colours");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.colours, 0, "Used");
                    ui.selectable_value(&mut panel.colours, 1, "New");
                    ui.selectable_value(&mut panel.colours, 2, "Slots");
                });
                ui.end_row();
                if panel.colours == 1 {
                    prop_label(ui, "paint_from_views", "newSlots");
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::DragValue::new(&mut panel.new_slots).range(1..=64));
                        prop_check(ui, "paint_from_views", "accents", &mut panel.accents);
                    });
                    ui.end_row();
                } else if panel.colours == 2 {
                    prop_label(ui, "paint_from_views", "slots");
                    ui.add(egui::TextEdit::singleline(&mut panel.slots).hint_text("3 4 7").desired_width(90.0));
                    ui.end_row();
                }
                prop_label(ui, "paint_from_views", "downscale");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.pixel_art, "Downscale");
                    ui.add_enabled(
                        panel.pixel_art,
                        egui::DragValue::new(&mut panel.pixel_colors).range(1..=32),
                    )
                    .on_hover_text("Colours of the shared palette, plus kept accents such as eyes.");
                });
                ui.end_row();
                prop_label(ui, "paint_from_views", "inset");
                prop_drag(ui, "paint_from_views", "inset", &mut panel.inset);
                ui.end_row();
                prop_label(ui, "paint_from_views", "reach");
                prop_drag(ui, "paint_from_views", "reach", &mut panel.reach);
                ui.end_row();
                prop_label(ui, "paint_from_views", "only");
                ui.add(egui::TextEdit::singleline(&mut panel.only).hint_text("all slots").desired_width(90.0));
                ui.end_row();
                prop_label(ui, "paint_from_views", "objects");
                ui.checkbox(&mut panel.all_objects, "All visible");
                ui.end_row();
            });
        if ui
            .button("Paint from views")
            .on_hover_text("MCP paint_from_views over the selection's box, or the whole object: every surface cell from the views that see it. Never adds or removes cells. One undo step.")
            .clicked()
        {
            session.paint_views_from_panel();
        }
        if pick {
            session.pick_paint_views_sheet();
        }
    });
}
