//! The Foliage inspector section: MCP `foliage`, a clustered tree crown.

use egui::{RichText, Ui};

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
                    label(ui, "Ellipsoids", "One per line in local cells: x y z radius, or x y z rx ry rz.");
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
                    label(ui, "From", "Slots of the existing crown, the leaves.");
                    slots_field(ui, &mut panel.from, "leaf slots");
                }
                ui.end_row();
                label(ui, "Palettes", "Leaf slots from the darkest to the lightest: shade, mid, light, sunlit tips. Each clump is shaded as one piece, top light and underside dark. Empty is the active slot.");
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                label(ui, "Core", "Slot of the dark inside seen in the gaps between clumps. 0 is the first of the palettes.");
                ui.add(egui::DragValue::new(&mut panel.core).range(0..=255));
                ui.end_row();
                label(ui, "Size", "Clump radius range in cells.");
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
                label(ui, "Gap", "Extra cells between neighbouring clumps; larger shows more dark core.");
                ui.add(egui::Slider::new(&mut panel.gap, 0.0..=8.0).max_decimals(1));
                ui.end_row();
                label(ui, "Flatten", "Clump height over width; below 1 clumps are wider than tall.");
                ui.add(egui::Slider::new(&mut panel.flatten, 0.2..=1.5).max_decimals(2));
                ui.end_row();
                label(ui, "Ragged", "Leafy outline noise of each clump; 0 is a smooth ellipsoid.");
                ui.add(egui::Slider::new(&mut panel.ragged, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Light", "How strongly sunlit tops lighten toward the last palette.");
                ui.add(egui::Slider::new(&mut panel.light, 0.0..=2.0).max_decimals(2));
                ui.end_row();
                label(ui, "Dark", "How strongly undersides and seams darken toward the first palette.");
                ui.add(egui::Slider::new(&mut panel.dark, 0.0..=2.0).max_decimals(2));
                ui.end_row();
                label(ui, "Masses", "Lobes the clumps are grouped into, split by deeper grooves; 0 is none.");
                ui.add(egui::Slider::new(&mut panel.masses, 0..=16));
                ui.end_row();
                label(ui, "Groove", "With masses: how many cells deeper the grooves between lobes go.");
                ui.add(egui::Slider::new(&mut panel.groove, 0..=8));
                ui.end_row();
                label(ui, "Keep", "Slots never written even if they are crown slots. Any slot outside Palettes, Core, and From (the wood) is kept already.");
                slots_field(ui, &mut panel.protect, "none");
                ui.end_row();
                label(ui, "Seed", "Same seed, same clumps.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
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
