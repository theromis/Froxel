//! The Foliage inspector section: MCP `foliage`, a clustered tree crown.

use egui::{RichText, Ui};

use super::{prop_drag, prop_label, prop_slider};
use super::{section, DIM};
use crate::state::{EditorMode, Session, FOLIAGE_SOURCES};

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

fn slots_field(ui: &mut Ui, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(120.0),
    );
}

/// MCP `foliage`: separate leaf clumps with lit tops and dark gaps.
pub(super) fn foliage(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    let mut at_selection = false;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Foliage");
        let panel = &mut session.stone.foliage;
        egui::Grid::new("foliage-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Volume", "Ellipsoids: the crown's shape, a broad middle and a few lobes. From slots: the cells of a crown already sculpted, re-clustered; those no clump keeps are removed.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in FOLIAGE_SOURCES.iter().enumerate() {
                        ui.selectable_value(&mut panel.source, index, *name);
                    }
                });
                ui.end_row();
                if panel.source == 0 {
                    prop_label(ui, "foliage", "ellipsoids");
                    ui.vertical(|ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut panel.ellipsoids)
                                .desired_rows(3)
                                .desired_width(150.0),
                        );
                        at_selection = ui
                            .small_button("Ellipsoid at selection")
                            .on_hover_text("Add the ellipsoid that fills the selection's box.")
                            .clicked();
                    });
                } else {
                    prop_label(ui, "foliage", "from");
                    slots_field(ui, &mut panel.from, "leaf slots");
                }
                ui.end_row();
                prop_label(ui, "foliage", "palettes");
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                prop_label(ui, "foliage", "core");
                prop_drag(ui, "foliage", "core", &mut panel.core);
                ui.end_row();
                prop_label(ui, "foliage", "size");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.size.iter_mut() {
                        ui.add(
                            egui::DragValue::new(value)
                                .range(1.0..=32.0)
                                .speed(0.2)
                                .max_decimals(1),
                        );
                    }
                });
                ui.end_row();
                prop_label(ui, "foliage", "gap");
                prop_slider(ui, "foliage", "gap", &mut panel.gap);
                ui.end_row();
                prop_label(ui, "foliage", "flatten");
                prop_slider(ui, "foliage", "flatten", &mut panel.flatten);
                ui.end_row();
                prop_label(ui, "foliage", "ragged");
                prop_slider(ui, "foliage", "ragged", &mut panel.ragged);
                ui.end_row();
                prop_label(ui, "foliage", "light");
                prop_slider(ui, "foliage", "light", &mut panel.light);
                ui.end_row();
                prop_label(ui, "foliage", "dark");
                prop_slider(ui, "foliage", "dark", &mut panel.dark);
                ui.end_row();
                prop_label(ui, "foliage", "masses");
                prop_slider(ui, "foliage", "masses", &mut panel.masses);
                ui.end_row();
                prop_label(ui, "foliage", "groove");
                prop_slider(ui, "foliage", "groove", &mut panel.groove);
                ui.end_row();
                prop_label(ui, "foliage", "protect");
                slots_field(ui, &mut panel.protect, "none");
                ui.end_row();
                prop_label(ui, "foliage", "seed");
                prop_drag(ui, "foliage", "seed", &mut panel.seed);
                ui.end_row();
            });
        let verb = if session.picked.is_empty() {
            "Grow crown".to_string()
        } else {
            "Grow crown in selection".to_string()
        };
        if ui
            .button(verb)
            .on_hover_text("MCP foliage: within the selection's box, or the whole crown with none, keeping the Protect boxes and the wood. Leaf slots keep their motion. One undo step.")
            .clicked()
        {
            session.foliage_from_panel();
        }
    });
    if at_selection {
        session.foliage_ellipsoid_at_selection();
    }
}
