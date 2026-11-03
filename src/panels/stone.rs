//! Inspector sections of the stone tools: Joints. Each button runs the MCP
//! tool over the selection's box, or the whole active object.

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{EditorMode, Session};
use crate::tools::JointPattern;

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

/// The button text: over the selection, or the whole object.
fn scope(session: &Session, verb: &str) -> String {
    if session.picked.is_empty() {
        format!("{verb} object")
    } else {
        format!("{verb} selection")
    }
}

fn range_f32(ui: &mut Ui, pair: &mut [f32; 2], range: std::ops::RangeInclusive<f32>) {
    ui.horizontal(|ui| {
        for value in pair.iter_mut() {
            ui.add(
                egui::DragValue::new(value)
                    .range(range.clone())
                    .speed(0.2)
                    .max_decimals(1),
            );
        }
    });
}

fn range_i32(ui: &mut Ui, pair: &mut [i32; 2], range: std::ops::RangeInclusive<i32>) {
    ui.horizontal(|ui| {
        for value in pair.iter_mut() {
            ui.add(egui::DragValue::new(value).range(range.clone()));
        }
    });
}

fn slots_field(ui: &mut Ui, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(120.0),
    );
}

/// MCP `joints`: real gaps between stones.
pub(super) fn joints(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Joints");
        let panel = &mut session.stone.joints;
        egui::Grid::new("joints-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Pattern", "Columns: vertical columns of random widths with bed joints. Courses: running bond blocks. Cracks: an irregular crack network.");
                ui.horizontal(|ui| {
                    for (pattern, name) in [
                        (JointPattern::Columns, "Columns"),
                        (JointPattern::Courses, "Courses"),
                        (JointPattern::Cracks, "Cracks"),
                    ] {
                        if ui.selectable_label(panel.pattern == pattern, name).clicked() {
                            panel.set_pattern(pattern);
                        }
                    }
                });
                ui.end_row();
                label(ui, "Spacing", "Stone width range in cells: column width, block length, or crack piece size.");
                range_f32(ui, &mut panel.spacing, 2.0..=128.0);
                ui.end_row();
                if panel.pattern != JointPattern::Cracks {
                    let tip = if panel.pattern == JointPattern::Courses {
                        "Course height range in cells."
                    } else {
                        "Spacing of the bed joints across a column, in cells."
                    };
                    label(ui, "Course", tip);
                    range_i32(ui, &mut panel.course, 1..=128);
                    ui.end_row();
                } else {
                    label(ui, "Density", "Share of piece boundaries that crack.");
                    ui.add(egui::Slider::new(&mut panel.density, 0.0..=1.0).max_decimals(2));
                    ui.end_row();
                }
                label(ui, "Width", "Gap width in cells.");
                ui.add(egui::Slider::new(&mut panel.width, 1..=3));
                ui.end_row();
                label(ui, "Depth", "Gap depth under the surface, in cells.");
                ui.add(egui::Slider::new(&mut panel.depth, 1..=16));
                ui.end_row();
                label(ui, "Dirt", "Palette slot for the cell at the bottom of each gap. 0 keeps its colour.");
                ui.add(egui::DragValue::new(&mut panel.dirt).range(0..=255));
                ui.end_row();
                label(ui, "Only", "Cut only these palette slots, such as the stone; empty cuts every slot.");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                label(ui, "Seed", "Same seed, same joints.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        if ui
            .button(scope(session, "Cut joints in"))
            .on_hover_text("MCP joints over the selection's box, or the whole object. One undo step.")
            .clicked()
        {
            session.joints_from_panel();
        }
    });
}
