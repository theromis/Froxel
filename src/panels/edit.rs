//! Inspector sections of the whole-object edits: Split object (MCP
//! `split_object`), Clear object (MCP `clear_object`), Recolor (MCP
//! `recolor`), Intersect (the fills' `mode: "intersect"`), Surface
//! stroke (MCP `surface_stroke`), Shell plates (MCP `shell_plates`), and
//! Components (MCP `components`).

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{EditorMode, Session, INTERSECT_SHAPES};

/// The Mirror row (MCP `mirror`): on, the axis, and the plane.
pub(super) fn mirror_row(ui: &mut Ui, id: &str, field: &mut crate::state::MirrorField) {
    label(ui, "Mirror", "Paint both sides in one step: the tool runs again on the far side of a plane across the axis, on that side's own surface, with the picture mirrored. The plane is the object's centre unless At gives its coordinate in cells (a multiple of 0.5; cell c spans c..c+1, so 16 swaps cells 15 and 16).");
    ui.horizontal(|ui| {
        ui.checkbox(&mut field.on, "");
        ui.add_enabled_ui(field.on, |ui| {
            egui::ComboBox::from_id_salt(id)
                .width(40.0)
                .selected_text(["X", "Y", "Z"][field.axis.min(2)])
                .show_ui(ui, |ui| {
                    for (index, name) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.selectable_value(&mut field.axis, index, *name);
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut field.at)
                    .hint_text("centre")
                    .desired_width(55.0),
            )
            .on_hover_text("The plane's coordinate along the axis in cells; empty is the middle of the object's bounds.");
        });
    });
    ui.end_row();
}

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

/// MCP `split_object`: cells of the active object into another object.
pub(super) fn split(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    let names: Vec<String> = session
        .objects
        .iter()
        .map(|object| object.name.clone())
        .collect();
    let active = session.selected;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Split object");
        let panel = &mut session.edit.split;
        if panel.into.is_some_and(|target| target >= names.len() || target == active) {
            panel.into = None;
        }
        egui::Grid::new("split-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Boxes", "Inclusive boxes x0 y0 z0 x1 y1 z1, one per line, in the active object's local cells or, with World, in world cells. A cell inside any of them is taken. Empty takes by slot alone.");
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.boxes)
                            .hint_text("0 0 20 15 12 31")
                            .desired_rows(2)
                            .desired_width(150.0),
                    );
                    ui.checkbox(&mut panel.world, "World cells")
                        .on_hover_text("MCP space world: the boxes are world cells, and the object's position is subtracted first.");
                });
                ui.end_row();
                label(ui, "Slots", "Only cells of these slots, such as 3 4. Empty is every slot.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Take", "Invert takes every cell the boxes and slots do not select. Copy leaves the cells in the active object too.");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut panel.invert, "Invert");
                    ui.checkbox(&mut panel.copy, "Copy");
                });
                ui.end_row();
                label(ui, "Into", "A new object at the active object's position, or an existing one. The cells keep their world positions either way.");
                let selected = match panel.into {
                    Some(target) => names[target].clone(),
                    None => "New object".into(),
                };
                egui::ComboBox::from_id_salt("split-into")
                    .width(120.0)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut panel.into, None, "New object");
                        for (index, name) in names.iter().enumerate() {
                            if index != active {
                                ui.selectable_value(&mut panel.into, Some(index), name);
                            }
                        }
                    });
                ui.end_row();
                if panel.into.is_none() {
                    label(ui, "Name", "Name of the new object.");
                    ui.add(egui::TextEdit::singleline(&mut panel.name).desired_width(120.0));
                    ui.end_row();
                }
            });
        if ui
            .button("Split")
            .on_hover_text("MCP split_object: move (or copy) the selected cells into the other object, keeping their world positions. One undo step.")
            .clicked()
        {
            session.split_from_panel();
        }
    });
}

/// MCP `recolor`: palette slots swapped with no shape change.
pub(super) fn recolor(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Recolor");
        egui::Grid::new("recolor-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Slots", "Pairs from>to, such as 3>5 4>6: every cell of slot 3 becomes 5. 0 as the target erases. The pairs apply at once, so 3>4 4>3 swaps.");
                ui.add(
                    egui::TextEdit::singleline(&mut session.edit.recolor.map)
                        .hint_text("3>5")
                        .desired_width(120.0),
                );
                ui.end_row();
                mirror_row(ui, "recolor-mirror", &mut session.edit.recolor.mirror);
            });
        if ui
            .button("Recolor")
            .on_hover_text("MCP recolor over the selection's box, or the whole object: the slots change, the shape does not. One undo step.")
            .clicked()
        {
            session.recolor_from_panel();
        }
    });
}

/// MCP `clear_object`: the active object emptied, whatever its size.
pub(super) fn clear(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Clear object");
        if ui
            .button("Clear")
            .on_hover_text("MCP clear_object: remove every cell of the active object, with no box limit. Its name, position, bone, script, and markers stay. One undo step.")
            .clicked()
        {
            session.clear_from_panel();
        }
    });
}

/// The fills' `mode: "intersect"`: the active object cut to a shape.
pub(super) fn intersect(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Intersect");
        let panel = &mut session.edit.intersect;
        egui::Grid::new("intersect-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Shape", "The shape inscribed in the selection's box. The active object keeps its cells inside it, as they are, and loses the rest.");
                egui::ComboBox::from_id_salt("intersect-shape")
                    .width(110.0)
                    .selected_text(INTERSECT_SHAPES[panel.shape.min(INTERSECT_SHAPES.len() - 1)].0)
                    .show_ui(ui, |ui| {
                        for (index, (name, _)) in INTERSECT_SHAPES.iter().enumerate() {
                            ui.selectable_value(&mut panel.shape, index, *name);
                        }
                    });
                ui.end_row();
                if panel.shape == 2 {
                    label(ui, "Radius", "Edge bevel of the rounded box in cells.");
                    ui.add(egui::DragValue::new(&mut panel.radius).range(0.0..=64.0).speed(0.1));
                    ui.end_row();
                }
            });
        if ui
            .button("Keep inside")
            .on_hover_text("MCP fill_box, fill_ellipsoid, or fill_rounded_box with mode intersect over the selection's box: a boxy hull cut to a dome. One undo step.")
            .clicked()
        {
            session.intersect_from_panel();
        }
    });
}

/// MCP `surface_stroke`: a stroke that snaps to the surface.
pub(super) fn stroke(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Surface stroke");
        let mut point = false;
        let panel = &mut session.edit.stroke;
        egui::Grid::new("stroke-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Points", "One point x y z per line, in the active object's local cells (or world cells with World). They may float above or sink below the surface: each sample snaps to the nearest surface cell.");
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.points)
                            .hint_text("4 0 8\n10 6 9")
                            .desired_rows(3)
                            .desired_width(150.0),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Point at selection").on_hover_text("Add the selection's centre as the next point.").clicked() {
                            point = true;
                        }
                        ui.checkbox(&mut panel.world, "World");
                    });
                });
                ui.end_row();
                label(ui, "Path", "Smooth passes a curve through the points; off joins them straight.");
                ui.checkbox(&mut panel.smooth, "Smooth");
                ui.end_row();
                label(ui, "Width", "Width across the surface in cells; 1 is a one-cell line.");
                ui.add(egui::DragValue::new(&mut panel.width).range(0.5..=crate::tools::MAX_STROKE_WIDTH as f32).speed(0.1));
                ui.end_row();
                label(ui, "Depth", "Cells into the solid; 1 is the surface alone.");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_STROKE_DEPTH as i32));
                ui.end_row();
                label(ui, "Reach", "How far a point may be from the surface it snaps to, in cells.");
                ui.add(egui::DragValue::new(&mut panel.reach).range(0.0..=crate::tools::MAX_STROKE_REACH as f32).speed(0.1));
                ui.end_row();
                label(ui, "Only", "Slots that may change, such as 3 4; others keep theirs. Empty is every slot.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.only)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                label(ui, "Groove", "Erase the stroke's cells instead of painting the active slot: a seam cut Depth cells deep.");
                ui.checkbox(&mut panel.groove, "Groove");
                ui.end_row();
                mirror_row(ui, "stroke-mirror", &mut panel.mirror);
            });
        if ui
            .button("Stroke")
            .on_hover_text("MCP surface_stroke: paint the active slot (or cut a groove) along the points, on the nearest surface cells. One undo step.")
            .clicked()
        {
            session.stroke_from_panel();
        }
        if point {
            session.stroke_point_at_selection();
        }
    });
}

/// MCP `components`: connected parts of the active object.
pub(super) fn components(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Components");
        let mut run: Option<&str> = None;
        let mut pick: Option<bool> = None;
        let panel = &mut session.edit.components;
        egui::Grid::new("components-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Joined by", "Which neighbours join cells into one part: faces (6), faces and edges (18), or faces, edges, and corners (26).");
                egui::ComboBox::from_id_salt("components-connectivity")
                    .width(110.0)
                    .selected_text(match panel.connectivity {
                        6 => "Faces (6)",
                        18 => "Edges (18)",
                        _ => "Corners (26)",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut panel.connectivity, 6, "Faces (6)");
                        ui.selectable_value(&mut panel.connectivity, 18, "Edges (18)");
                        ui.selectable_value(&mut panel.connectivity, 26, "Corners (26)");
                    });
                ui.end_row();
                label(ui, "Keep", "Keep largest leaves this many of the largest parts.");
                ui.add(egui::DragValue::new(&mut panel.keep).range(1..=1024));
                ui.end_row();
                label(ui, "Min cells", "Remove small erases every part with fewer cells: floaters, foam bits.");
                ui.add(egui::DragValue::new(&mut panel.min_cells).range(1..=1_000_000));
                ui.end_row();
                label(ui, "Seed", "A cell x y z of the part Delete, Move, and Split act on, in local cells; it snaps to the nearest solid cell.");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut panel.seed).hint_text("x y z").desired_width(90.0));
                    if ui.button("At selection").on_hover_text("The selection's centre.").clicked() {
                        pick = Some(false);
                    }
                });
                ui.end_row();
                label(ui, "Anchor", "Optional: a cell of the part that stays. The seed's part is first cut from it at the thinnest neck between them, so a rider touching a shell comes off. Empty: no cut.");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut panel.anchor).hint_text("none").desired_width(90.0));
                    if ui.button("At selection").on_hover_text("The selection's centre.").clicked() {
                        pick = Some(true);
                    }
                });
                ui.end_row();
                label(ui, "Offset", "Move shifts the part by these whole cells.");
                ui.horizontal(|ui| {
                    for value in panel.offset.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-256..=256));
                    }
                });
                ui.end_row();
                label(ui, "Name", "Name of the object Split makes; empty is <object>_part.");
                ui.add(egui::TextEdit::singleline(&mut panel.name).desired_width(120.0));
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui.button("List").on_hover_text("MCP components op list: count the parts and their sizes, with no change.").clicked() {
                run = Some("list");
            }
            if ui.button("Keep largest").on_hover_text("MCP components op keep_largest: erase every part but the largest Keep. One undo step.").clicked() {
                run = Some("keep_largest");
            }
            if ui.button("Remove small").on_hover_text("MCP components op remove_small: erase the parts with fewer than Min cells. One undo step.").clicked() {
                run = Some("remove_small");
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Delete").on_hover_text("MCP components op delete: erase the seed's part (cut at the neck from the anchor, if set). One undo step.").clicked() {
                run = Some("delete");
            }
            if ui.button("Move").on_hover_text("MCP components op move: shift the seed's part by Offset. One undo step.").clicked() {
                run = Some("move");
            }
            if ui.button("Split").on_hover_text("MCP components op split: move the seed's part into a new object, keeping its world position. One undo step.").clicked() {
                run = Some("split");
            }
        });
        if let Some(anchor) = pick {
            session.components_cell_at_selection(anchor);
        }
        if let Some(op) = run {
            session.components_from_panel(op);
        }
    });
}

/// MCP `shell_plates`: a plate pattern over a dome.
pub(super) fn plates(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Shell plates");
        let panel = &mut session.edit.plates;
        let slots = |ui: &mut Ui, text: &mut String, hint: &str| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .desired_width(90.0),
            );
        };
        egui::Grid::new("plates-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Layout", "Rings cuts each ring into sectors, staggered ring to ring; Voronoi gives each cell the nearest of seeds laid out in the rings, for rounder, irregular plates.");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut panel.layout, 0, "Rings");
                    ui.selectable_value(&mut panel.layout, 1, "Voronoi");
                });
                ui.end_row();
                label(ui, "Rings", "Plates per ring from the centre out, such as 5 9.");
                slots(ui, &mut panel.rings, "6");
                ui.end_row();
                label(ui, "Central", "Share of the dome's radius the central plate spans; 0 for none.");
                ui.add(egui::DragValue::new(&mut panel.central).range(0.0..=0.9).speed(0.005));
                ui.end_row();
                label(ui, "Band", "Share of the radius the outer band spans, and its plates (0 is one unbroken band).");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.band).range(0.0..=0.6).speed(0.005));
                    ui.add(egui::DragValue::new(&mut panel.band_count).range(0..=crate::tools::MAX_PLATES_PER_RING as i32))
                        .on_hover_text("Plates in the band.");
                });
                ui.end_row();
                label(ui, "Rim", "Rim width in cells along the surface at the dome's edge; 0 for none.");
                ui.add(egui::DragValue::new(&mut panel.rim).range(0.0..=32.0).speed(0.1));
                ui.end_row();
                label(ui, "Plate slots", "Slots per kind, such as 3 4 5: each plate picks one by Seed. Empty ring slots paint the active slot; empty central and band take the ring's, an empty rim the band's.");
                ui.vertical(|ui| {
                    slots(ui, &mut panel.ring_slots, "ring: active");
                    slots(ui, &mut panel.central_slots, "central");
                    slots(ui, &mut panel.band_slots, "band");
                    slots(ui, &mut panel.rim_slots, "rim");
                });
                ui.end_row();
                label(ui, "Seams", "The seam slot, or Groove to cut the seams Seam depth cells deep.");
                ui.horizontal(|ui| {
                    ui.add_enabled(!panel.groove, egui::DragValue::new(&mut panel.seam).range(1..=255));
                    ui.checkbox(&mut panel.groove, "Groove");
                });
                ui.end_row();
                label(ui, "Seam size", "Width in cells and depth into the solid.");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.seam_width).range(0.5..=crate::tools::MAX_SEAM_WIDTH as f32).speed(0.05));
                    ui.add(egui::DragValue::new(&mut panel.seam_depth).range(1..=crate::tools::MAX_PLATE_DEPTH as i32));
                });
                ui.end_row();
                label(ui, "Depth", "Cells a plate's slot reaches in along the normal.");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_PLATE_DEPTH as i32));
                ui.end_row();
                label(ui, "Facing", "Degrees from straight up a surface may face and still take the pattern; the selection's box limits it too.");
                ui.add(egui::DragValue::new(&mut panel.facing).range(0.0..=180.0).speed(0.5).suffix("°"));
                ui.end_row();
                label(ui, "Only", "Only cells of these slots take the pattern, such as 2. Empty is every slot.");
                slots(ui, &mut panel.only, "all slots");
                ui.end_row();
                label(ui, "Variation", "Jitter moves boundaries and seeds; Symmetric mirrors them left to right; Seed changes them and the slot picks.");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.jitter).range(0.0..=1.0).speed(0.01));
                    ui.checkbox(&mut panel.symmetric, "Symmetric");
                    ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999).prefix("seed "));
                });
                ui.end_row();
            });
        if ui
            .button("Paint plates")
            .on_hover_text("MCP shell_plates over the selection's box, or the whole object: plates, seams, band, and rim that follow the surface. One undo step.")
            .clicked()
        {
            session.plates_from_panel();
        }
    });
}
