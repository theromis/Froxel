//! Inspector sections of the whole-object edits: Split object (MCP
//! `split_object`), Clear object (MCP `clear_object`), Recolor (MCP
//! `recolor`), Intersect (the fills' `mode: "intersect"`), Surface
//! stroke (MCP `surface_stroke`), Shell plates (MCP `shell_plates`),
//! Components (MCP `components`), De-terrace (MCP `deterrace`), Reduce
//! palette (MCP `reduce_palette`), and Split parts (MCP `split_parts`).

use egui::{RichText, Ui};

use super::{prop_check, prop_drag, prop_label};
use super::{section, DIM};
use crate::state::{EditorMode, Session, INTERSECT_SHAPES};

/// The Mirror row (MCP `mirror`): on, the axis, and the plane.
pub(super) fn mirror_row(ui: &mut Ui, id: &str, field: &mut crate::state::MirrorField) {
    label(ui, "Mirror", "Paint both sides in one step: the tool runs again on the far side of a plane across the axis, on that side's own surface, with the picture mirrored. The plane is the object's centre unless At gives its coordinate in cells (a multiple of 0.5; cell c spans c..c+1, so 16 swaps cells 15 and 16).");
    ui.horizontal_wrapped(|ui| {
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
                prop_label(ui, "split_object", "boxes");
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
                prop_label(ui, "split_object", "slots");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.slots)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "split_object", "invert");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.invert, "Invert");
                    ui.checkbox(&mut panel.copy, "Copy");
                });
                ui.end_row();
                prop_label(ui, "split_object", "into");
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
                    prop_label(ui, "split_object", "name");
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
                prop_label(ui, "recolor", "map");
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
                    prop_label(ui, "fill_rounded_box", "radius");
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
                prop_label(ui, "surface_stroke", "points");
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.points)
                            .hint_text("4 0 8\n10 6 9")
                            .desired_rows(3)
                            .desired_width(150.0),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Point at selection").on_hover_text("Add the selection's centre as the next point.").clicked() {
                            point = true;
                        }
                        ui.checkbox(&mut panel.world, "World");
                    });
                });
                ui.end_row();
                prop_label(ui, "surface_stroke", "curve");
                ui.checkbox(&mut panel.smooth, "Smooth");
                ui.end_row();
                prop_label(ui, "surface_stroke", "width");
                ui.add(egui::DragValue::new(&mut panel.width).range(0.5..=crate::tools::MAX_STROKE_WIDTH as f32).speed(0.1));
                ui.end_row();
                prop_label(ui, "surface_stroke", "depth");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_STROKE_DEPTH as i32));
                ui.end_row();
                prop_label(ui, "surface_stroke", "reach");
                ui.add(egui::DragValue::new(&mut panel.reach).range(0.0..=crate::tools::MAX_STROKE_REACH as f32).speed(0.1));
                ui.end_row();
                prop_label(ui, "surface_stroke", "only");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.only)
                        .hint_text("all slots")
                        .desired_width(90.0),
                );
                ui.end_row();
                prop_label(ui, "surface_stroke", "palette");
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
                prop_label(ui, "components", "connectivity");
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
                prop_label(ui, "components", "keep");
                prop_drag(ui, "components", "keep", &mut panel.keep);
                ui.end_row();
                prop_label(ui, "components", "minCells");
                prop_drag(ui, "components", "minCells", &mut panel.min_cells);
                ui.end_row();
                prop_label(ui, "components", "seed");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut panel.seed).hint_text("x y z").desired_width(90.0));
                    if ui.button("At selection").on_hover_text("The selection's centre.").clicked() {
                        pick = Some(false);
                    }
                });
                ui.end_row();
                prop_label(ui, "components", "anchor");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut panel.anchor).hint_text("none").desired_width(90.0));
                    if ui.button("At selection").on_hover_text("The selection's centre.").clicked() {
                        pick = Some(true);
                    }
                });
                ui.end_row();
                prop_label(ui, "components", "offset");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.offset.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-256..=256));
                    }
                });
                ui.end_row();
                prop_label(ui, "components", "name");
                ui.add(egui::TextEdit::singleline(&mut panel.name).desired_width(120.0));
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
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
                prop_label(ui, "shell_plates", "layout");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.layout, 0, "Rings");
                    ui.selectable_value(&mut panel.layout, 1, "Voronoi");
                });
                ui.end_row();
                prop_label(ui, "shell_plates", "rings");
                slots(ui, &mut panel.rings, "6");
                ui.end_row();
                prop_label(ui, "shell_plates", "central");
                prop_drag(ui, "shell_plates", "central", &mut panel.central);
                ui.end_row();
                prop_label(ui, "shell_plates", "band");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.band).range(0.0..=0.6).speed(0.005));
                    ui.add(egui::DragValue::new(&mut panel.band_count).range(0..=crate::tools::MAX_PLATES_PER_RING as i32))
                        .on_hover_text("Plates in the band.");
                });
                ui.end_row();
                prop_label(ui, "shell_plates", "rim");
                prop_drag(ui, "shell_plates", "rim", &mut panel.rim);
                ui.end_row();
                prop_label(ui, "shell_plates", "palettes");
                ui.vertical(|ui| {
                    slots(ui, &mut panel.ring_slots, "ring: active");
                    slots(ui, &mut panel.central_slots, "central");
                    slots(ui, &mut panel.band_slots, "band");
                    slots(ui, &mut panel.rim_slots, "rim");
                });
                ui.end_row();
                label(ui, "Seams", "The seam slot, or Groove to cut the seams Seam depth cells deep.");
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled(!panel.groove, egui::DragValue::new(&mut panel.seam).range(1..=255));
                    ui.checkbox(&mut panel.groove, "Groove");
                });
                ui.end_row();
                prop_label(ui, "shell_plates", "seamWidth");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.seam_width).range(0.5..=crate::tools::MAX_SEAM_WIDTH as f32).speed(0.05));
                    ui.add(egui::DragValue::new(&mut panel.seam_depth).range(1..=crate::tools::MAX_PLATE_DEPTH as i32));
                });
                ui.end_row();
                prop_label(ui, "shell_plates", "depth");
                ui.add(egui::DragValue::new(&mut panel.depth).range(1..=crate::tools::MAX_PLATE_DEPTH as i32));
                ui.end_row();
                prop_label(ui, "shell_plates", "facing");
                ui.add(egui::DragValue::new(&mut panel.facing).range(0.0..=180.0).speed(0.5).suffix("°"));
                ui.end_row();
                prop_label(ui, "shell_plates", "only");
                slots(ui, &mut panel.only, "all slots");
                ui.end_row();
                prop_label(ui, "shell_plates", "jitter");
                ui.horizontal_wrapped(|ui| {
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

/// MCP `deterrace`: terraces re-quantized into blocks, or staggered.
pub(super) fn deterrace(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "De-terrace");
        let panel = &mut session.edit.terrace;
        egui::Grid::new("terrace-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "deterrace", "mode");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.mode, 0, "Blocks");
                    ui.selectable_value(&mut panel.mode, 1, "Stagger");
                });
                ui.end_row();
                if panel.mode == 0 {
                    prop_label(ui, "deterrace", "size");
                    prop_drag(ui, "deterrace", "size", &mut panel.size);
                    ui.end_row();
                    prop_label(ui, "deterrace", "fill");
                    prop_drag(ui, "deterrace", "fill", &mut panel.fill);
                    ui.end_row();
                    prop_label(ui, "deterrace", "tolerance");
                    prop_drag(ui, "deterrace", "tolerance", &mut panel.tolerance);
                    ui.end_row();
                } else {
                    prop_label(ui, "deterrace", "run");
                    prop_drag(ui, "deterrace", "run", &mut panel.run);
                    ui.end_row();
                    prop_label(ui, "deterrace", "jitter");
                    prop_drag(ui, "deterrace", "jitter", &mut panel.jitter);
                    ui.end_row();
                    prop_label(ui, "deterrace", "seed");
                    prop_drag(ui, "deterrace", "seed", &mut panel.seed);
                    ui.end_row();
                }
            });
        if ui
            .button("De-terrace")
            .on_hover_text(
                "MCP deterrace over the selection's box, or the whole object. One undo step.",
            )
            .clicked()
        {
            session.deterrace_from_panel();
        }
    });
}

/// MCP `reduce_palette`: the object's colours clustered or snapped, and
/// its surface despeckled.
pub(super) fn reduce(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model && !session.active().cells.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Reduce palette");
        let panel = &mut session.edit.reduce;
        egui::Grid::new("reduce-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Colours", "Cluster: the object's colours become Colors clusters (k-means in Oklab), with small distinct accents such as eyes kept. Snap: every colour takes the nearest of the Palette list. Despeckle: only the majority filter.");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.mode, 0, "Cluster");
                    ui.selectable_value(&mut panel.mode, 1, "Snap");
                    ui.selectable_value(&mut panel.mode, 2, "Despeckle");
                });
                ui.end_row();
                if panel.mode == 0 {
                    prop_label(ui, "reduce_palette", "colors");
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::DragValue::new(&mut panel.colors).range(1..=64));
                        prop_check(ui, "reduce_palette", "accents", &mut panel.accents);
                    });
                    ui.end_row();
                } else if panel.mode == 1 {
                    prop_label(ui, "reduce_palette", "palette");
                    ui.add(egui::TextEdit::singleline(&mut panel.palette).hint_text("3 4 #e8822a").desired_width(120.0));
                    ui.end_row();
                }
                prop_label(ui, "reduce_palette", "despeckle");
                prop_drag(ui, "reduce_palette", "despeckle", &mut panel.despeckle);
                ui.end_row();
                prop_label(ui, "reduce_palette", "minSame");
                prop_drag(ui, "reduce_palette", "minSame", &mut panel.min_same);
                ui.end_row();
                prop_label(ui, "reduce_palette", "protect");
                ui.add(egui::TextEdit::singleline(&mut panel.protect).hint_text("none").desired_width(90.0));
                ui.end_row();
            });
        if ui
            .button("Reduce palette")
            .on_hover_text("MCP reduce_palette over the selection's box, or the whole object. One undo step.")
            .clicked()
        {
            session.reduce_from_panel();
        }
    });
}

/// MCP `split_parts`: the active object cut into one object per rig part.
pub(super) fn parts(ui: &mut Ui, session: &mut Session) {
    let enabled = !session.active().cells.is_empty();
    let mut seed = false;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Split parts");
        let panel = &mut session.edit.parts;
        egui::Grid::new("parts-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "split_parts", "parts");
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.parts)
                            .hint_text("head 12 30 14\nbody bone=body\ntail 12 2 6 | 0 -1 0 12 6 6")
                            .desired_rows(4)
                            .desired_width(170.0),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Seed at selection").on_hover_text("Add the selection's centre as a new part line, named part.").clicked() {
                            seed = true;
                        }
                        ui.checkbox(&mut panel.world, "World")
                            .on_hover_text("MCP space world: the seeds and planes are world cells.");
                    });
                });
                ui.end_row();
                prop_label(ui, "split_parts", "metric");
                ui.checkbox(&mut panel.geodesic, "Through the solid");
                ui.end_row();
                prop_label(ui, "split_parts", "removeSource");
                ui.checkbox(&mut panel.remove_source, "Remove");
                ui.end_row();
                prop_label(ui, "split_parts", "bind");
                ui.checkbox(&mut panel.bind, "To bones");
                ui.end_row();
            });
        if ui
            .button("Split parts")
            .on_hover_text("MCP split_parts: each part becomes its own object at world [0, 0, 0], with its cells in world cells, ready for bind_object. One undo step.")
            .clicked()
        {
            session.split_parts_from_panel();
        }
    });
    if seed {
        session.parts_seed_at_selection();
    }
}
