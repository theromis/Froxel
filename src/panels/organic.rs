//! Inspector sections of the organic shaping tools: Deform, Loft, and Blend
//! shapes. Each button runs the MCP tool, Deform over the selection's box or the whole
//! active object. The Sculpt tool (MCP `brush`) is in the tool strip.

use egui::{RichText, Ui};

use super::{prop_drag, prop_label};
use super::{section, DIM};
use crate::state::{EditorMode, Session, DEFORM_OPS, DIRECTIONS};

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).small().color(DIM))
        .on_hover_text(tip);
}

fn number(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, speed: f64) {
    ui.add(
        egui::DragValue::new(value)
            .range(range)
            .speed(speed)
            .max_decimals(2),
    );
}

/// A value that is the cells' own unless Set is on.
fn optional(ui: &mut Ui, value: &mut Option<f32>, fallback: f32, tip: &str) {
    ui.horizontal_wrapped(|ui| {
        let mut set = value.is_some();
        if ui.checkbox(&mut set, "Set").on_hover_text(tip).changed() {
            *value = set.then_some(value.unwrap_or(fallback));
        }
        if let Some(v) = value.as_mut() {
            ui.add(egui::DragValue::new(v).speed(0.2).max_decimals(1));
        } else {
            ui.label(RichText::new("from the cells").small().color(DIM));
        }
    });
}

/// MCP `deform`: bend, taper, twist, or displace.
pub(super) fn deform(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Deform");
        let panel = &mut session.organic.deform;
        egui::Grid::new("deform-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "deform", "op");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in DEFORM_OPS.into_iter().enumerate() {
                        let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                        ui.selectable_value(&mut panel.op, index, title);
                    }
                });
                ui.end_row();
                if panel.op < 3 {
                    prop_label(ui, "deform", "axis");
                    ui.horizontal_wrapped(|ui| {
                        for (index, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                            ui.selectable_value(&mut panel.axis, index, name);
                        }
                    });
                    ui.end_row();
                    prop_label(ui, "deform", "origin");
                    optional(ui, &mut panel.origin, 0.0, "Give the origin");
                    ui.end_row();
                    prop_label(ui, "deform", "length");
                    optional(ui, &mut panel.length, 16.0, "Give the length");
                    ui.end_row();
                }
                match panel.op {
                    0 => {
                        prop_label(ui, "deform", "angle");
                        number(ui, &mut panel.angle, -180.0..=180.0, 0.5);
                        ui.end_row();
                        prop_label(ui, "deform", "direction");
                        egui::ComboBox::from_id_salt("deform-direction")
                            .selected_text(DIRECTIONS[panel.direction.min(5)].0)
                            .show_ui(ui, |ui| {
                                for (index, (name, _)) in DIRECTIONS.iter().enumerate() {
                                    ui.selectable_value(&mut panel.direction, index, *name);
                                }
                            });
                        ui.end_row();
                    }
                    1 => {
                        prop_label(ui, "deform", "start");
                        number(ui, &mut panel.start, 0.05..=16.0, 0.01);
                        ui.end_row();
                        prop_label(ui, "deform", "end");
                        number(ui, &mut panel.end, 0.05..=16.0, 0.01);
                        ui.end_row();
                    }
                    2 => {
                        label(ui, "Angle", "Degrees of twist over the length.");
                        number(ui, &mut panel.angle, -2880.0..=2880.0, 1.0);
                        ui.end_row();
                    }
                    4 => {
                        prop_label(ui, "deform", "point");
                        ui.horizontal_wrapped(|ui| {
                            for value in &mut panel.point {
                                ui.add(egui::DragValue::new(value).speed(0.25));
                            }
                        });
                        ui.end_row();
                        prop_label(ui, "deform", "factor");
                        ui.horizontal_wrapped(|ui| {
                            for value in &mut panel.factor {
                                number(ui, value, 0.25..=4.0, 0.01);
                            }
                        });
                        ui.end_row();
                        prop_label(ui, "deform", "radius");
                        number(ui, &mut panel.radius, 0.5..=512.0, 0.1);
                        ui.end_row();
                        prop_label(ui, "deform", "falloff");
                        number(ui, &mut panel.falloff, 0.5..=512.0, 0.1);
                        ui.end_row();
                    }
                    _ => {
                        prop_label(ui, "deform", "amplitude");
                        number(ui, &mut panel.amplitude, 0.5..=32.0, 0.05);
                        ui.end_row();
                        prop_label(ui, "deform", "scale");
                        number(ui, &mut panel.scale, 1.0..=512.0, 0.1);
                        ui.end_row();
                        prop_label(ui, "deform", "octaves");
                        prop_drag(ui, "deform", "octaves", &mut panel.octaves);
                        ui.end_row();
                        prop_label(ui, "deform", "bias");
                        number(ui, &mut panel.bias, -1.0..=1.0, 0.01);
                        ui.end_row();
                        prop_label(ui, "deform", "seed");
                        prop_drag(ui, "deform", "seed", &mut panel.seed);
                        ui.end_row();
                    }
                }
                prop_label(ui, "deform", "minFeature");
                prop_drag(ui, "deform", "minFeature", &mut panel.min_feature);
                ui.end_row();
            });
        let verb = if session.picked.is_empty() {
            "Deform object"
        } else {
            "Deform selection"
        };
        if ui
            .button(verb)
            .on_hover_text("MCP deform over the selection's box, or the whole object. Slots are kept. One undo step.")
            .clicked()
        {
            session.deform_from_panel();
        }
    });
}

/// MCP `loft`: a solid along a curve with a section per point.
pub(super) fn loft(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Loft");
        let panel = &mut session.organic.loft;
        egui::Grid::new("loft-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "loft", "points");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.points)
                        .desired_rows(3)
                        .desired_width(150.0),
                );
                ui.end_row();
                prop_label(ui, "loft", "curve");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.smooth, "Smooth");
                    ui.checkbox(&mut panel.round_caps, "Round ends");
                });
                ui.end_row();
                prop_label(ui, "loft", "rx");
                ui.horizontal_wrapped(|ui| {
                    number(ui, &mut panel.rx, 0.5..=128.0, 0.05);
                    number(ui, &mut panel.ry, 0.5..=128.0, 0.05);
                });
                ui.end_row();
                prop_label(ui, "loft", "roll");
                number(ui, &mut panel.roll, -360.0..=360.0, 0.5);
                ui.end_row();
                prop_label(ui, "loft", "flatTop");
                number(ui, &mut panel.flat_top, -1.0..=1.0, 0.01);
                ui.end_row();
                prop_label(ui, "loft", "noise");
                number(ui, &mut panel.noise, 0.0..=0.9, 0.005);
                ui.end_row();
                prop_label(ui, "loft", "frequency");
                number(ui, &mut panel.frequency, 0.0..=64.0, 0.05);
                ui.end_row();
                prop_label(ui, "loft", "palettes");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.palettes)
                        .hint_text("active slot")
                        .desired_width(120.0),
                );
                ui.end_row();
                prop_label(ui, "loft", "pattern");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Rings", "Stripes", "Random"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.pattern, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "loft", "period");
                number(ui, &mut panel.period, 0.5..=512.0, 0.05);
                ui.end_row();
                prop_label(ui, "loft", "mode");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "loft", "seed");
                prop_drag(ui, "loft", "seed", &mut panel.seed);
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Point at selection")
                .on_hover_text(
                    "Add the selection's centre as the next point, with the default radii.",
                )
                .clicked()
            {
                session.loft_point_at_selection();
            }
            if ui
                .button("Loft")
                .on_hover_text("MCP loft along the points. One undo step.")
                .clicked()
            {
                session.loft_from_panel();
            }
        });
    });
}

/// MCP `blend_shapes`: simple shapes melted together as distance fields.
pub(super) fn blend(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Blend shapes");
        let panel = &mut session.organic.blend;
        egui::Grid::new("blend-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "blend_shapes", "shapes");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.shapes)
                        .desired_rows(4)
                        .desired_width(150.0),
                );
                ui.end_row();
                prop_label(ui, "blend_shapes", "k");
                number(ui, &mut panel.k, 0.0..=64.0, 0.05);
                ui.end_row();
                prop_label(ui, "blend_shapes", "grow");
                number(ui, &mut panel.grow, -16.0..=16.0, 0.05);
                ui.end_row();
                prop_label(ui, "blend_shapes", "colors");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Nearest", "Last"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.colors, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "blend_shapes", "seam");
                number(ui, &mut panel.seam, 0.0..=32.0, 0.05);
                ui.end_row();
                prop_label(ui, "blend_shapes", "mode");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint", "Intersect"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "blend_shapes", "mirror");
                ui.checkbox(&mut panel.mirror, "Mirror x");
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Shape at selection")
                .on_hover_text("Add an ellipsoid over the selection's box, in the active slot.")
                .clicked()
            {
                session.blend_shape_at_selection();
            }
            if ui
                .button("Blend")
                .on_hover_text("MCP blend_shapes of the shapes. One undo step.")
                .clicked()
            {
                session.blend_from_panel();
            }
        });
    });
}
