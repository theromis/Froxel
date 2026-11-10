//! Inspector sections of the organic shaping tools: Deform and Loft. Each
//! button runs the MCP tool, Deform over the selection's box or the whole
//! active object. The Sculpt tool (MCP `brush`) is in the tool strip.

use egui::{RichText, Ui};

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
                label(ui, "Op", "Bend curls the cells toward a direction; taper scales them across the axis; twist turns them about it; displace pushes the surface in and out with noise, for lumps and burls; scale enlarges a part (a head, an eye, a fin) about a point with a smooth falloff, so a small creature's features get the cells they need.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in DEFORM_OPS.into_iter().enumerate() {
                        let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                        ui.selectable_value(&mut panel.op, index, title);
                    }
                });
                ui.end_row();
                if panel.op < 3 {
                    label(ui, "Axis", "The axis the bend, taper, or twist runs along.");
                    ui.horizontal_wrapped(|ui| {
                        for (index, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                            ui.selectable_value(&mut panel.axis, index, name);
                        }
                    });
                    ui.end_row();
                    label(ui, "Origin", "Where along the axis it starts, in local cells. Off: the cells' lowest.");
                    optional(ui, &mut panel.origin, 0.0, "Give the origin");
                    ui.end_row();
                    label(ui, "Length", "How far along the axis it runs. Off: the cells' extent.");
                    optional(ui, &mut panel.length, 16.0, "Give the length");
                    ui.end_row();
                }
                match panel.op {
                    0 => {
                        label(ui, "Angle", "Degrees over the length; negative bends away from the direction.");
                        number(ui, &mut panel.angle, -180.0..=180.0, 0.5);
                        ui.end_row();
                        label(ui, "Toward", "The way the cells curl, across the axis.");
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
                        label(ui, "Start", "Scale across the axis at the origin.");
                        number(ui, &mut panel.start, 0.05..=16.0, 0.01);
                        ui.end_row();
                        label(ui, "End", "Scale at the end of the length.");
                        number(ui, &mut panel.end, 0.05..=16.0, 0.01);
                        ui.end_row();
                    }
                    2 => {
                        label(ui, "Angle", "Degrees of twist over the length.");
                        number(ui, &mut panel.angle, -2880.0..=2880.0, 1.0);
                        ui.end_row();
                    }
                    4 => {
                        label(ui, "Point", "The centre of the part to scale, in local cells. With a selection, its centre is used.");
                        ui.horizontal_wrapped(|ui| {
                            for value in &mut panel.point {
                                ui.add(egui::DragValue::new(value).speed(0.25));
                            }
                        });
                        ui.end_row();
                        label(ui, "Factor", "How much larger along x, y, and z, 0.25..4: 1.5 1.5 1.5 grows the part by half; below 1 shrinks it.");
                        ui.horizontal_wrapped(|ui| {
                            for value in &mut panel.factor {
                                number(ui, value, 0.25..=4.0, 0.01);
                            }
                        });
                        ui.end_row();
                        label(ui, "Radius", "Cells around the point scaled as one rigid piece.");
                        number(ui, &mut panel.radius, 0.5..=512.0, 0.1);
                        ui.end_row();
                        label(ui, "Falloff", "Cells past the radius over which the scale eases back to none, so the part stays attached and the rest stays put.");
                        number(ui, &mut panel.falloff, 0.5..=512.0, 0.1);
                        ui.end_row();
                    }
                    _ => {
                        label(ui, "Amplitude", "The most the surface moves, in cells.");
                        number(ui, &mut panel.amplitude, 0.5..=32.0, 0.05);
                        ui.end_row();
                        label(ui, "Scale", "Bump size in cells.");
                        number(ui, &mut panel.scale, 1.0..=512.0, 0.1);
                        ui.end_row();
                        label(ui, "Octaves", "Finer layers, each half the size and height.");
                        ui.add(egui::DragValue::new(&mut panel.octaves).range(1..=6));
                        ui.end_row();
                        label(ui, "Bias", "-1 only dents, 0 dents and bulges, 1 only bulges.");
                        number(ui, &mut panel.bias, -1.0..=1.0, 0.01);
                        ui.end_row();
                        label(ui, "Seed", "Noise seed.");
                        ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                        ui.end_row();
                    }
                }
                label(ui, "Min feature", "MCP minFeature: after the deform, report the parts thinner than this many cells (a fin, an ear, a leg too thin for the size). 0 is off.");
                ui.add(egui::DragValue::new(&mut panel.min_feature).range(0..=16));
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
                label(ui, "Points", "One per line: x y z, then optional rx ry roll flatTop noise frequency. A value left out takes the defaults below.");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.points)
                        .desired_rows(3)
                        .desired_width(150.0),
                );
                ui.end_row();
                label(ui, "Curve", "Smooth passes a curve through the points; off joins them straight.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.smooth, "Smooth");
                    ui.checkbox(&mut panel.round_caps, "Round ends");
                });
                ui.end_row();
                label(ui, "Radii", "Default half-width across the path and half-height (up for a level path).");
                ui.horizontal_wrapped(|ui| {
                    number(ui, &mut panel.rx, 0.5..=128.0, 0.05);
                    number(ui, &mut panel.ry, 0.5..=128.0, 0.05);
                });
                ui.end_row();
                label(ui, "Roll", "Default turn of the section about the path, in degrees.");
                number(ui, &mut panel.roll, -360.0..=360.0, 0.5);
                ui.end_row();
                label(ui, "Flat top", "Above 0 cuts the top flat (1 halves the section); below 0 the bottom, for a log on the ground.");
                number(ui, &mut panel.flat_top, -1.0..=1.0, 0.01);
                ui.end_row();
                label(ui, "Noise", "Knobbly outline, as a fraction of the radius.");
                number(ui, &mut panel.noise, 0.0..=0.9, 0.005);
                ui.end_row();
                label(ui, "Lumps", "About how many lumps around the outline.");
                number(ui, &mut panel.frequency, 0.0..=64.0, 0.05);
                ui.end_row();
                label(ui, "Palettes", "Slots laid out by the pattern; empty is the active slot.");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.palettes)
                        .hint_text("active slot")
                        .desired_width(120.0),
                );
                ui.end_row();
                label(ui, "Pattern", "Rings along the length, stripes around it, or random cells.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Rings", "Stripes", "Random"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.pattern, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Period", "Ring length in cells.");
                number(ui, &mut panel.period, 0.5..=512.0, 0.05);
                ui.end_row();
                label(ui, "Mode", "Replace writes every cell; Empty adds only into empty cells; Paint recolours solid cells.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Seed", "Seed of the outline noise and the random pattern.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Point at selection")
                .on_hover_text("Add the selection's centre as the next point, with the default radii.")
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
