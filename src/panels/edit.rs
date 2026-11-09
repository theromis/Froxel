//! Inspector sections of the whole-object edits: Split object (MCP
//! `split_object`) and Recolor (MCP `recolor`).

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{EditorMode, Session};

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
