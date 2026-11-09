//! Inspector sections of the silhouette tools: Carve from views (MCP
//! `carve_from_views`), Paint from view (MCP `project_paint`), Decal (MCP
//! `decal`), Fit to views
//! (MCP `fit_to_views`), and Compare silhouette (MCP `compare_silhouette`),
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
            label(ui, "Sheet", "A views sheet PNG: front, side, top, and back in two rows and two columns on a key background.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.path)
                        .hint_text("views.png")
                        .desired_width(150.0),
                );
                if ui.button("…").on_hover_text("Choose the sheet PNG.").clicked() {
                    pick = true;
                }
            });
            ui.end_row();
            label(ui, "Background", "Which pixels of the sheet are background.");
            background(ui, "measure-background", &mut panel.background);
            ui.end_row();
            label(ui, "Count parts", "Count the parts sticking out of the top view's body (flippers, legs, head, tail), to catch a top view with too many.");
            ui.checkbox(&mut panel.parts, "");
            ui.end_row();
        });
    if pick {
        session.pick_measure_sheet();
    }
    ui.horizontal(|ui| {
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
            ui.horizontal(|ui| {
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
            ui.horizontal(|ui| {
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
                label(ui, "Origin", "Cell of the box's minimum corner: local to the active object, or a world cell with World.");
                ui.horizontal(|ui| {
                    for value in &mut panel.origin {
                        ui.add(egui::DragValue::new(value).range(-1024..=1023));
                    }
                    ui.checkbox(&mut panel.world, "World")
                        .on_hover_text("MCP space world: the origin is a world cell, and the object's position is subtracted first.");
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
                label(ui, "Depth", "How the depth maps are read. Hull places each image row's nearest pixel where the silhouettes are nearest in that row; Box spreads the map over the box, for a relief. Invert: black is near. Back: mirror the surface for the far side, when the opposite view has no map. Tolerance: cells in front of the surface that stay.");
                ui.horizontal(|ui| {
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
                label(ui, "Depth parts", "Auto curve: fit the map's curve (gamma) to the silhouette contour, for a map not linear in depth such as Depth Anything's. Regions: calibrate each part of the map (one peak each, apart where the grey jumps or dips by more than Edge) on its own against the silhouettes, for a map that normalises each part like shading, as an image generator draws it.");
                ui.horizontal(|ui| {
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
                label(ui, "Depth detail", "A second depth sheet laid out as the views: its regions give the curvature inside each part, and each row's depth map gives their order and offsets. Use the image generator's sheet here and Depth Anything's as the rows' maps.");
                ui.horizontal(|ui| {
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
                label(ui, "Intersect", "MCP mode intersect: keep only the active object's cells inside the hull, as they are, and erase the rest, instead of filling the hull.");
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
                    ui.checkbox(&mut panel.world, "World")
                        .on_hover_text("MCP space world: the origin is a world cell, and the object's position is subtracted first.");
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
                super::edit::mirror_row(ui, "paint-mirror", &mut panel.mirror);
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
                label(ui, "Image", "The picture to lay on the surface: an eye, a mouth, a marking. Background pixels leave the surface alone.");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut panel.path)
                            .hint_text("eye.png")
                            .desired_width(150.0),
                    );
                    if ui.button("…").on_hover_text("Choose the PNG.").clicked() {
                        pick = true;
                    }
                    ui.checkbox(&mut panel.flip, "Flip")
                        .on_hover_text("Mirror the picture left to right.");
                });
                ui.end_row();
                label(ui, "Crop", "The image's rectangle in pixels: x y width height. Empty is the whole image.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.crop)
                        .hint_text("x y w h")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Background", "Which pixels of the image are background.");
                background(ui, "decal-background", &mut panel.background);
                ui.end_row();
                label(ui, "Point", "x y z near the surface, in the active object's local cells (or world cells with World): the decal centres on the nearest surface cell.");
                ui.horizontal(|ui| {
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
                label(ui, "Up", "x y z: the picture's up, laid onto the surface at the point. 0 0 1 keeps a face upright.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.up)
                        .hint_text("0 0 1")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Size", "Cells across along the surface; the height follows the picture.");
                ui.add(egui::DragValue::new(&mut panel.size).range(1.0..=crate::tools::MAX_DECAL_SIZE as f32).speed(0.1));
                ui.end_row();
                label(ui, "Depth", "Cells painted into the solid along the normal; 1 is the surface alone.");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_DECAL_DEPTH as i32));
                ui.end_row();
                label(ui, "Reach", "How far the point may be from the surface, in cells.");
                ui.add(egui::DragValue::new(&mut panel.reach).range(0.0..=crate::tools::MAX_DECAL_REACH as f32).speed(0.1));
                ui.end_row();
                label(ui, "Normal limit", "Degrees: the decal stops where the surface turns further than this from the point's normal, so it never wraps round a thin part.");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(1.0..=180.0)
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
                super::edit::mirror_row(ui, "decal-mirror", &mut panel.mirror);
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
                label(ui, "Align", "How each view's crop is placed over the object's projection, once, before the fit: Fit finds the scale and offset with the most overlap; Height and Width match that extent, bottoms and centres lined up.");
                egui::ComboBox::from_id_salt("fit-align")
                    .selected_text(["Fit", "Height", "Width"][panel.align.min(2)])
                    .show_ui(ui, |ui| {
                        for (index, name) in ["Fit", "Height", "Width"].iter().enumerate() {
                            ui.selectable_value(&mut panel.align, index, *name);
                        }
                    });
                ui.end_row();
                label(ui, "Moves", "Shape adds and removes cells along the view rays (excess rays cleared, missing ones filled, rough cells tidied); Colour gives every cell a view sees the slot nearest the sheet's colours.");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut panel.shape, "Shape");
                    ui.checkbox(&mut panel.recolor, "Colour");
                });
                ui.end_row();
                label(ui, "Colour from", "Which views of Carve from views (in their row order) recolour cells; a view turned off here still shapes the object. Turn the side view off when its features would land beside the front view's (a face's eyes).");
                ui.horizontal(|ui| {
                    for (index, paints) in panel.paint_views.iter_mut().enumerate() {
                        ui.checkbox(paints, format!("{}", index + 1))
                            .on_hover_text("Carve from views row");
                    }
                });
                ui.end_row();
                label(ui, "Normal limit", "Degrees: a view recolours only cells whose surface faces it within this angle; a seen cell no view faces takes the view facing it most. 90 is no limit.");
                ui.add(
                    egui::DragValue::new(&mut panel.normal_limit)
                        .range(0.0..=90.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
                label(ui, "Keep", "Slots whose cells the fit never removes or recolours, such as hand-set eyes 9 10. They still cover and hide.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.keep)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Rounding", "Weight and superellipse exponent of the rounding prior. Three axis views cannot see a round head's diagonal corners; cells outside the superellipse inscribed in each cross-section (or outside the Reference) go wherever no silhouette needs them. Weight 0 keeps the box-like hull; exponent 2 is an ellipse, larger is squarer.");
                ui.horizontal(|ui| {
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
                label(ui, "Sweeps", "Most shape sweeps; the fit stops sooner when a sweep keeps no move. 0 only recolours.");
                ui.add(egui::DragValue::new(&mut panel.iterations).range(0..=crate::tools::MAX_FIT_ITERATIONS as i32));
                ui.end_row();
                label(ui, "Seed", "Shuffles the order rays are visited in; the same seed gives the same result.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=i32::MAX));
                ui.end_row();
                label(ui, "Slots", "Palette slots the colours may take, such as 3 4 7. Empty is the slots the object already holds.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("object's slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Reference", "An object index or name, such as a mesh imported with import_mesh into a new object, whose front surface each view's rays should meet: the depth target. Empty is none.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.reference)
                        .hint_text("none")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Isolate", "Off: other visible objects cover and hide in the views but never change. On: the active object is fitted alone.");
                ui.checkbox(&mut panel.isolate, "Active object only");
                ui.end_row();
                label(ui, "Weights", "Mask: a missing or excess view pixel. Colour: the first visible cell against the sheet (up to 1 at 255 RGB apart). Depth: up to 1 at 8 cells from the reference. Smooth: each exposed cell face.");
                ui.horizontal(|ui| {
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
            label(ui, "Exclude", "Colours whose pixels are background too, such as water and splashes: #rrggbb separated by spaces, with their RGB tolerance (MCP excludeColors, excludeTolerance).");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.exclude)
                        .hint_text("#3a6ea5 #ffffff")
                        .desired_width(110.0),
                );
                ui.add(egui::DragValue::new(&mut panel.exclude_tolerance).range(0.0..=442.0));
            });
            ui.end_row();
            label(ui, "Flood", "Flood the background from the crop's border, stepping to neighbours within this RGB distance, for a photo-like or gradient background (MCP flood).");
            ui.horizontal(|ui| {
                ui.checkbox(&mut panel.flood, "");
                ui.add_enabled(
                    panel.flood,
                    egui::DragValue::new(&mut panel.flood_tolerance).range(0.0..=442.0),
                );
            });
            ui.end_row();
            label(ui, "Mask", "A mask PNG: only the concept's pixels under its ink stay (transparent or the border colour is empty, so white on black works); Invert drops them instead (MCP mask).");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut panel.mask)
                        .hint_text("mask.png")
                        .desired_width(110.0),
                );
                if ui.button("…").on_hover_text("Choose the mask PNG.").clicked() {
                    pick_mask = true;
                }
                ui.checkbox(&mut panel.invert_mask, "Invert");
            });
            ui.end_row();
            label(ui, "Erase", "Rectangles to drop, such as a rider: x y width height per line, in image pixels like Crop (MCP erase).");
            ui.add(
                egui::TextEdit::multiline(&mut panel.erase)
                    .hint_text("x y w h")
                    .desired_rows(1)
                    .desired_width(150.0),
            );
            ui.end_row();
            label(ui, "Largest only", "Keep only the largest connected region of the concept, dropping specks and a separate splash (MCP keepLargest).");
            ui.checkbox(&mut panel.keep_largest, "");
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
    if pick_mask {
        session.pick_compare_mask();
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
