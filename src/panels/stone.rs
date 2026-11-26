//! Inspector sections of the stone tools: Joints, Weather, Bark, Planks, Cavity,
//! Radial relief,
//! Growth, Sweep, Trim, and Markers. Each button runs the MCP
//! tool over the selection's box, or the whole active object.

use egui::{RichText, Ui};

use super::{prop_drag, prop_label, prop_slider};
use super::{section, DIM};
use crate::state::{
    EditorMode, MarkerKind, Plant, Session, BARK_ALONG, DIRECTIONS, FACINGS, RADIAL_AXES,
    RADIAL_ENDS, RADIAL_KINDS, RADIAL_PROFILES, RELIEF_FACINGS,
};
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
    ui.horizontal_wrapped(|ui| {
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
    ui.horizontal_wrapped(|ui| {
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

/// MCP `exclude`: boxes Scatter, Relief, Joints, Weather, Bark, Planks, Cavity,
/// Radial relief, Foliage, and Growth leave alone.
pub(super) fn protect(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Protect");
        ui.label(
            RichText::new("Boxes Scatter, Relief, Joints, Weather, Bark, Planks, Cavity, Radial relief, Foliage, and Growth leave alone: ledge tops, cave floors, river beds.")
                .small()
                .color(DIM),
        );
        ui.add(
            egui::TextEdit::multiline(&mut session.stone.exclude)
                .hint_text("x0 y0 z0 x1 y1 z1, one box per line")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .on_hover_text("MCP exclude: inclusive boxes in local cells. No cell inside one is cut, chipped, pitted, recoloured, or grown on, and no debris lands in one.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Add selection")
                .on_hover_text("Add the selection's box, such as a walking surface.")
                .clicked()
            {
                session.protect_selection();
            }
            if ui
                .add_enabled(!session.stone.exclude.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                session.stone.exclude.clear();
            }
        });
    });
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
                prop_label(ui, "joints", "pattern");
                ui.horizontal_wrapped(|ui| {
                    for (pattern, name) in [
                        (JointPattern::Columns, "Columns"),
                        (JointPattern::Courses, "Courses"),
                        (JointPattern::Cracks, "Cracks"),
                    ] {
                        if ui
                            .selectable_label(panel.pattern == pattern, name)
                            .clicked()
                        {
                            panel.set_pattern(pattern);
                        }
                    }
                });
                ui.end_row();
                prop_label(ui, "joints", "spacing");
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
                    prop_label(ui, "joints", "density");
                    prop_slider(ui, "joints", "density", &mut panel.density);
                    ui.end_row();
                }
                prop_label(ui, "joints", "width");
                prop_slider(ui, "joints", "width", &mut panel.width);
                ui.end_row();
                prop_label(ui, "joints", "depth");
                prop_slider(ui, "joints", "depth", &mut panel.depth);
                ui.end_row();
                prop_label(ui, "joints", "dirt");
                prop_drag(ui, "joints", "dirt", &mut panel.dirt);
                ui.end_row();
                prop_label(ui, "joints", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                prop_label(ui, "joints", "seed");
                prop_drag(ui, "joints", "seed", &mut panel.seed);
                ui.end_row();
            });
        if ui
            .button(scope(session, "Cut joints in"))
            .on_hover_text(
                "MCP joints over the selection's box, or the whole object. One undo step.",
            )
            .clicked()
        {
            session.joints_from_panel();
        }
    });
}

/// MCP `column_relief`: a cliff face broken into columns.
pub(super) fn relief(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Relief");
        let panel = &mut session.stone.relief;
        egui::Grid::new("relief-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "column_relief", "facing");
                ui.horizontal_wrapped(|ui| {
                    for (index, (name, _)) in RELIEF_FACINGS.iter().enumerate() {
                        if ui.selectable_label(panel.facing == index, *name).clicked() {
                            panel.facing = index;
                        }
                    }
                });
                ui.end_row();
                label(ui, "Palette", "Slot of the columns, usually the stone. 0 is the active slot.");
                ui.add(egui::DragValue::new(&mut panel.palette).range(0..=255));
                ui.end_row();
                prop_label(ui, "column_relief", "width");
                range_i32(ui, &mut panel.width, 1..=64);
                ui.end_row();
                prop_label(ui, "column_relief", "out");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.out.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(0..=16));
                    }
                });
                ui.end_row();
                label(ui, "Crown", "Height the column tops scatter around. Off: the highest solid cell of the box.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.crown_on, "");
                    ui.add_enabled(
                        panel.crown_on,
                        egui::DragValue::new(&mut panel.crown).range(-1024..=1023),
                    );
                });
                ui.end_row();
                prop_label(ui, "column_relief", "top");
                ui.horizontal_wrapped(|ui| {
                    for value in panel.top.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-256..=256));
                    }
                });
                ui.end_row();
                prop_label(ui, "column_relief", "lower");
                ui.checkbox(&mut panel.lower, "");
                ui.end_row();
                prop_label(ui, "column_relief", "depth");
                prop_slider(ui, "column_relief", "depth", &mut panel.depth);
                ui.end_row();
                prop_label(ui, "column_relief", "seed");
                prop_drag(ui, "column_relief", "seed", &mut panel.seed);
                ui.end_row();
            });
        if ui
            .button(scope(session, "Raise columns on"))
            .on_hover_text("MCP column_relief over the selection's box, or the whole object, keeping the Protect boxes. One undo step.")
            .clicked()
        {
            session.relief_from_panel();
        }
    });
}

/// MCP `weather`: chips, pits, and colour noise.
pub(super) fn weather(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Weather");
        let panel = &mut session.stone.weather;
        egui::Grid::new("weather-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "weather", "chip");
                prop_slider(ui, "weather", "chip", &mut panel.chip);
                ui.end_row();
                prop_label(ui, "weather", "chipSize");
                prop_slider(ui, "weather", "chipSize", &mut panel.chip_size);
                ui.end_row();
                prop_label(ui, "weather", "pits");
                prop_slider(ui, "weather", "pits", &mut panel.pits);
                ui.end_row();
                prop_label(ui, "weather", "pitDepth");
                prop_slider(ui, "weather", "pitDepth", &mut panel.pit_depth);
                ui.end_row();
                prop_label(ui, "weather", "broken");
                prop_drag(ui, "weather", "broken", &mut panel.broken);
                ui.end_row();
                prop_label(ui, "weather", "noise");
                slots_field(ui, &mut panel.noise, "no noise");
                ui.end_row();
                prop_label(ui, "weather", "noiseDensity");
                prop_slider(ui, "weather", "noiseDensity", &mut panel.noise_density);
                ui.end_row();
                prop_label(ui, "weather", "noiseScale");
                prop_slider(ui, "weather", "noiseScale", &mut panel.noise_scale);
                ui.end_row();
                prop_label(ui, "weather", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                prop_label(ui, "weather", "seed");
                prop_drag(ui, "weather", "seed", &mut panel.seed);
                ui.end_row();
            });
        if ui
            .button(scope(session, "Weather"))
            .on_hover_text(
                "MCP weather over the selection's box, or the whole object. One undo step.",
            )
            .clicked()
        {
            session.weather_from_panel();
        }
    });
}

/// MCP `bark`: plates along the grain split by fissures.
pub(super) fn bark(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    let mut from_sweep = false;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Bark");
        let panel = &mut session.stone.bark;
        egui::Grid::new("bark-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Along", "The way the grain runs: an axis through the middle of the wood, or the points of the tube or sweep that made it, so the plates follow its bend.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in BARK_ALONG.iter().enumerate() {
                        ui.selectable_value(&mut panel.along, index, *name);
                    }
                });
                ui.end_row();
                if panel.along == BARK_ALONG.len() - 1 {
                    prop_label(ui, "bark", "points");
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.points)
                            .desired_rows(3)
                            .desired_width(150.0),
                    );
                    ui.end_row();
                    prop_label(ui, "bark", "curve");
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut panel.smooth, "Smooth");
                        from_sweep = ui
                            .small_button("From Sweep")
                            .on_hover_text("Copy the Sweep panel's points and curve.")
                            .clicked();
                    });
                    ui.end_row();
                }
                label(ui, "Palettes", "Plate slots, a few bark tones; each plate takes one, so the tone changes plate by plate, not in stripes. Empty is the active slot.");
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                prop_label(ui, "bark", "dirt");
                prop_drag(ui, "bark", "dirt", &mut panel.dirt);
                ui.end_row();
                prop_label(ui, "bark", "width");
                range_f32(ui, &mut panel.width, 1.0..=64.0);
                ui.end_row();
                prop_label(ui, "bark", "length");
                range_f32(ui, &mut panel.length, 2.0..=256.0);
                ui.end_row();
                prop_label(ui, "bark", "depth");
                range_i32(ui, &mut panel.depth, 1..=4);
                ui.end_row();
                prop_label(ui, "bark", "cracks");
                prop_slider(ui, "bark", "cracks", &mut panel.cracks);
                ui.end_row();
                prop_label(ui, "bark", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                prop_label(ui, "bark", "seed");
                prop_drag(ui, "bark", "seed", &mut panel.seed);
                ui.end_row();
            });
        if ui
            .button(scope(session, "Cut bark on"))
            .on_hover_text("MCP bark over the selection's box, or the whole object, keeping the Protect boxes. End grain is left alone. A flat plate gets boards across its surface and is never cut through; the status line says when a shape could not be cut. One undo step.")
            .clicked()
        {
            session.bark_from_panel();
        }
    });
    if from_sweep {
        session.bark_points_from_sweep();
    }
}

/// MCP `planks`: a deck of separate boards.
pub(super) fn planks(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Planks");
        let panel = &mut session.stone.planks;
        egui::Grid::new("planks-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "planks", "axis");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["X", "Y"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.axis, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Height", "The top layer of the boards. Off: the top of the selection's box, or of the object.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.height_on, "");
                    ui.add_enabled(
                        panel.height_on,
                        egui::DragValue::new(&mut panel.height).range(-1024..=1023),
                    );
                });
                ui.end_row();
                prop_label(ui, "planks", "palettes");
                slots_field(ui, &mut panel.palettes, "3 4 5");
                ui.end_row();
                prop_label(ui, "planks", "dirt");
                prop_drag(ui, "planks", "dirt", &mut panel.dirt);
                ui.end_row();
                prop_label(ui, "planks", "width");
                range_i32(ui, &mut panel.width, 1..=64);
                ui.end_row();
                prop_label(ui, "planks", "length");
                range_i32(ui, &mut panel.length, 2..=256);
                ui.end_row();
                prop_label(ui, "planks", "gap");
                range_i32(ui, &mut panel.gap, 1..=8);
                ui.end_row();
                prop_label(ui, "planks", "thickness");
                prop_slider(ui, "planks", "thickness", &mut panel.thickness);
                ui.end_row();
                prop_label(ui, "planks", "ragged");
                prop_slider(ui, "planks", "ragged", &mut panel.ragged);
                ui.end_row();
                prop_label(ui, "planks", "dark");
                prop_drag(ui, "planks", "dark", &mut panel.dark);
                ui.end_row();
                prop_label(ui, "planks", "nails");
                prop_slider(ui, "planks", "nails", &mut panel.nails);
                ui.end_row();
                prop_label(ui, "planks", "knots");
                prop_slider(ui, "planks", "knots", &mut panel.knots);
                ui.end_row();
                prop_label(ui, "planks", "only");
                slots_field(ui, &mut panel.only, "whole rectangle");
                ui.end_row();
                prop_label(ui, "planks", "seed");
                prop_drag(ui, "planks", "seed", &mut panel.seed);
                ui.end_row();
            });
        if ui
            .button(scope(session, "Lay planks on"))
            .on_hover_text("MCP planks over the selection's rectangle, or the whole object's, keeping the Protect boxes. One undo step.")
            .clicked()
        {
            session.planks_from_panel();
        }
    });
}

/// Selectable labels for one choice of a few names.
fn choice(ui: &mut Ui, index: &mut usize, names: &[&str]) {
    ui.horizontal_wrapped(|ui| {
        for (i, name) in names.iter().enumerate() {
            if ui.selectable_label(*index == i, *name).clicked() {
                *index = i;
            }
        }
    });
}

/// MCP `radial_relief`: flutes round a shaft or gores over a dome.
pub(super) fn radial(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Radial relief");
        let panel = &mut session.stone.radial;
        egui::Grid::new("radial-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "radial_relief", "kind");
                choice(ui, &mut panel.kind, &RADIAL_KINDS);
                ui.end_row();
                prop_label(ui, "radial_relief", "count");
                prop_drag(ui, "radial_relief", "count", &mut panel.count);
                ui.end_row();
                prop_label(ui, "radial_relief", "axis");
                choice(ui, &mut panel.axis, &RADIAL_AXES);
                ui.end_row();
                label(ui, "Center", "The axis across it, the two other coordinates in x, y, z order. Off: the middle of the solid.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.center_on, "");
                    for value in panel.center.iter_mut() {
                        ui.add_enabled(
                            panel.center_on,
                            egui::DragValue::new(value).speed(0.5).max_decimals(1),
                        );
                    }
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "phase");
                prop_drag(ui, "radial_relief", "phase", &mut panel.phase);
                ui.end_row();
                label(ui, "Width", "Flutes: cells across each flute. Off: 0.6 of the pitch.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.width_on, "");
                    ui.add_enabled(
                        panel.width_on,
                        egui::DragValue::new(&mut panel.width).range(0.5..=64.0).speed(0.1),
                    );
                });
                ui.end_row();
                label(ui, "Range", "First and last cell along the axis: the flutes' ends, or the dome's foot and top. Off: the solid's extent.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.range_on, "");
                    for value in panel.range.iter_mut() {
                        ui.add_enabled(
                            panel.range_on,
                            egui::DragValue::new(value).range(-1024..=1023),
                        );
                    }
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "ends");
                choice(ui, &mut panel.ends, &RADIAL_ENDS);
                ui.end_row();
                label(ui, "Fade", "Cells over which a recess fades in from its ends (flutes) or the foot (gores, which also fade toward the crown). Off: the width (at least 2) for flutes, 6 for gores.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.fade_on, "");
                    ui.add_enabled(
                        panel.fade_on,
                        egui::DragValue::new(&mut panel.fade).range(0.0..=128.0).speed(0.2),
                    );
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "profile");
                choice(ui, &mut panel.profile, &RADIAL_PROFILES);
                ui.end_row();
                prop_label(ui, "radial_relief", "depth");
                prop_slider(ui, "radial_relief", "depth", &mut panel.depth);
                ui.end_row();
                prop_label(ui, "radial_relief", "shade");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.shade_on, "");
                    ui.add_enabled(panel.shade_on, egui::DragValue::new(&mut panel.shade).range(0..=255));
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "light");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.light_on, "");
                    ui.add_enabled(panel.light_on, egui::DragValue::new(&mut panel.light).range(0..=255));
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "ribs");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.ribs).range(0..=255));
                    ui.add(egui::DragValue::new(&mut panel.rib_width).range(0.0..=16.0).speed(0.1));
                    ui.add(egui::DragValue::new(&mut panel.rib_height).range(0..=4));
                });
                ui.end_row();
                prop_label(ui, "radial_relief", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                prop_label(ui, "radial_relief", "jitter");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.jitter).range(0.0..=1.0).speed(0.01));
                    ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                });
                ui.end_row();
            });
        if ui
            .button(scope(session, "Cut recesses on"))
            .on_hover_text("MCP radial_relief over the selection's box, or the whole object, keeping the Protect boxes. New shades and the cells are one undo step.")
            .clicked()
        {
            session.radial_from_panel();
        }
    });
}

/// MCP `cavity_tint`: darker crevices and lighter ridges.
pub(super) fn cavity(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Cavity");
        let panel = &mut session.stone.cavity;
        egui::Grid::new("cavity-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "cavity_tint", "radius");
                prop_slider(ui, "cavity_tint", "radius", &mut panel.radius);
                ui.end_row();
                prop_label(ui, "cavity_tint", "dark");
                prop_slider(ui, "cavity_tint", "dark", &mut panel.dark);
                ui.end_row();
                prop_label(ui, "cavity_tint", "lighten");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.lighten, "");
                    ui.add_enabled(
                        panel.lighten,
                        egui::Slider::new(&mut panel.light, 0.0..=1.0).max_decimals(2),
                    );
                });
                ui.end_row();
                label(ui, "Pairs", "slot darker [lighter] per slot, separated by semicolons; slots without a pair stay. Empty makes shades of each slot's own colour in free palette slots.");
                slots_field(ui, &mut panel.pairs, "make shades");
                ui.end_row();
                prop_label(ui, "cavity_tint", "darken");
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.darken).range(0.05..=0.99).speed(0.01));
                    ui.add(egui::DragValue::new(&mut panel.brighten).range(1.01..=4.0).speed(0.01));
                });
                ui.end_row();
                prop_label(ui, "cavity_tint", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
            });
        if ui
            .button(scope(session, "Shade cavities on"))
            .on_hover_text("MCP cavity_tint over the selection's box, or the whole object, keeping the Protect boxes. New shades and the cells are one undo step.")
            .clicked()
        {
            session.cavity_from_panel();
        }
    });
}

/// MCP `moss`, `algae`, and `vines`: plants on exposed faces.
pub(super) fn growth(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Growth");
        let panel = &mut session.stone.growth;
        egui::Grid::new("growth-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                label(ui, "Plant", "Moss: caps on tops and curtains down faces, or separate cushions. Algae: a waterline band, dense low and thinning up. Vines: strands hanging from top edges.");
                ui.horizontal_wrapped(|ui| {
                    for (plant, name) in [
                        (Plant::Moss, "Moss"),
                        (Plant::Algae, "Algae"),
                        (Plant::Vines, "Vines"),
                    ] {
                        if ui.selectable_label(panel.plant == plant, name).clicked() {
                            panel.set_plant(plant);
                        }
                    }
                });
                ui.end_row();
                if panel.plant == Plant::Moss {
                    label(ui, "Style", "Caps: caps on tops (and undersides with Bottom) and curtains down the sides. Cushions: separate cushions where the surface faces up, or Facing, never one band, with drapes over the edges.");
                    ui.horizontal_wrapped(|ui| {
                        for (on, name) in [(false, "Caps"), (true, "Cushions")] {
                            if ui.selectable_label(panel.cushions == on, name).clicked() {
                                panel.set_cushions(on);
                            }
                        }
                    });
                    ui.end_row();
                }
                let tip = match panel.plant {
                    Plant::Moss => "Moss slots, picked in patches. Empty is the active slot.",
                    Plant::Algae => "Algae slots from the bottom of the band to its top. Empty is the active slot.",
                    Plant::Vines => "Vine slots from the anchor to the tip; give them rising sway in Material. Empty is the active slot.",
                };
                label(ui, "Palettes", tip);
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                let tip = match panel.plant {
                    Plant::Moss => "Coverage of the faces.",
                    Plant::Algae => "Coverage at the bottom of the band.",
                    Plant::Vines => "Share of top edge cells a strand starts from.",
                };
                label(ui, "Amount", tip);
                ui.add(egui::Slider::new(&mut panel.amount, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                prop_label(ui, "moss", "noise");
                prop_slider(ui, "moss", "noise", &mut panel.noise);
                ui.end_row();
                label(ui, "Faces", "Which faces it grows on. Bottom takes undersides, such as the dark underside of a tree crown.");
                ui.horizontal_wrapped(|ui| {
                    for (on, name) in panel.faces.iter_mut().zip(["Top", "Side", "Bottom"]) {
                        ui.toggle_value(on, name);
                    }
                });
                ui.end_row();
                label(ui, "Facing", "Only faces pointing within 60 degrees of this direction. +Z turns on Top and −Z turns on Bottom, the only faces that point that way.");
                ui.horizontal_wrapped(|ui| {
                    for (index, (name, dir)) in FACINGS.iter().enumerate() {
                        if ui.selectable_label(panel.facing == index, *name).clicked() {
                            panel.facing = index;
                            match dir {
                                Some([0, 0, 1]) => panel.faces[0] = true,
                                Some([0, 0, -1]) => panel.faces[2] = true,
                                _ => {}
                            }
                        }
                    }
                });
                ui.end_row();
                label(ui, "Band", "Only cells with z in this band, such as the waterline. Algae thins from its bottom to its top.");
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut panel.band_on, "");
                    ui.add_enabled_ui(panel.band_on, |ui| {
                        ui.add(egui::DragValue::new(&mut panel.band[0]).range(-1024..=1023));
                        ui.add(egui::DragValue::new(&mut panel.band[1]).range(-1024..=1023));
                    });
                });
                ui.end_row();
                prop_label(ui, "moss", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                if panel.plant != Plant::Vines {
                    prop_label(ui, "moss", "layer");
                    ui.checkbox(&mut panel.paint, "");
                    ui.end_row();
                }
                if panel.plant == Plant::Moss && panel.cushions {
                    prop_label(ui, "moss", "size");
                    range_i32(ui, &mut panel.size, 1..=16);
                    ui.end_row();
                    prop_label(ui, "moss", "gap");
                    prop_slider(ui, "moss", "gap", &mut panel.gap);
                    ui.end_row();
                    prop_label(ui, "moss", "exposure");
                    prop_slider(ui, "moss", "exposure", &mut panel.exposure);
                    ui.end_row();
                    prop_label(ui, "moss", "drapes");
                    prop_slider(ui, "moss", "drapes", &mut panel.drapes);
                    ui.end_row();
                    prop_label(ui, "moss", "thickness");
                    prop_slider(ui, "moss", "thickness", &mut panel.thickness);
                    ui.end_row();
                } else if panel.plant == Plant::Moss {
                    prop_label(ui, "moss", "curtains");
                    prop_slider(ui, "moss", "curtains", &mut panel.curtains);
                    ui.end_row();
                    label(ui, "Thickness", "Cushion thickness on tops.");
                    ui.add(egui::Slider::new(&mut panel.thickness, 1..=3));
                    ui.end_row();
                }
                if panel.plant != Plant::Algae {
                    let tip = if panel.plant == Plant::Moss && panel.cushions {
                        "Drape length range in cells."
                    } else if panel.plant == Plant::Moss {
                        "Curtain length range in cells."
                    } else {
                        "Strand length range in cells."
                    };
                    label(ui, "Length", tip);
                    range_i32(ui, &mut panel.length, 1..=128);
                    ui.end_row();
                }
                if panel.plant == Plant::Vines {
                    prop_label(ui, "vines", "blooms");
                    slots_field(ui, &mut panel.blooms, "none");
                    ui.end_row();
                    prop_label(ui, "vines", "bloomChance");
                    prop_slider(ui, "vines", "bloomChance", &mut panel.bloom_chance);
                    ui.end_row();
                }
                prop_label(ui, "moss", "seed");
                prop_drag(ui, "moss", "seed", &mut panel.seed);
                ui.end_row();
            });
        let verb = match session.stone.growth.plant {
            Plant::Moss => "Grow moss on",
            Plant::Algae => "Grow algae on",
            Plant::Vines => "Hang vines from",
        };
        if ui
            .button(scope(session, verb))
            .on_hover_text("MCP moss, algae, or vines over the selection's box, or the whole object. One undo step.")
            .clicked()
        {
            session.growth_from_panel();
        }
    });
}

/// MCP `sweep`: a profile along a path.
pub(super) fn sweep(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Sweep");
        let panel = &mut session.stone.sweep;
        egui::Grid::new("sweep-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "sweep", "points");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.points)
                        .desired_rows(3)
                        .desired_width(150.0),
                );
                ui.end_row();
                prop_label(ui, "sweep", "curve");
                ui.checkbox(&mut panel.smooth, "Smooth");
                ui.end_row();
                label(ui, "Profile", "A rounded rectangle standing on the path, or a polygon of u v points (u to the left of the path, v up). A cell is in when its centre is inside the profile at the nearest point of the path.");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut panel.polygon, false, "Rectangle");
                    ui.selectable_value(&mut panel.polygon, true, "Polygon");
                });
                ui.end_row();
                if panel.polygon {
                    prop_label(ui, "sweep", "profile");
                    slots_field(ui, &mut panel.profile, "-3 0; 3 0; 0 8");
                    ui.end_row();
                } else {
                    label(ui, "Width", "Across the path, centred on it.");
                    ui.add(egui::DragValue::new(&mut panel.width).range(0.5..=512.0).speed(0.1));
                    ui.end_row();
                    label(ui, "Height", "Up from the path.");
                    ui.add(egui::DragValue::new(&mut panel.height).range(0.5..=512.0).speed(0.1));
                    ui.end_row();
                    prop_label(ui, "sweep", "radius");
                    prop_drag(ui, "sweep", "radius", &mut panel.radius);
                    ui.end_row();
                }
                label(ui, "Erase", "Remove the solid cells the sweep covers instead of filling: a slanted cut along a closed or curved outline. Palettes, Pattern, Period, Mode, and Seed do not apply.");
                ui.checkbox(&mut panel.erase, "Erase");
                ui.end_row();
                label(ui, "Palettes", "Slots laid out by the pattern; empty is the active slot.");
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                label(ui, "Pattern", "Bands by height in the profile (strata), rings along the path, or random cells.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Bands", "Rings", "Random"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.pattern, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "sweep", "period");
                prop_drag(ui, "sweep", "period", &mut panel.period);
                ui.end_row();
                label(ui, "Mode", "Replace writes every cell; Empty adds only into empty cells; Paint recolours solid cells.");
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                prop_label(ui, "sweep", "seed");
                prop_drag(ui, "sweep", "seed", &mut panel.seed);
                ui.end_row();
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Point at selection")
                .on_hover_text("Add the selection's centre as the next point.")
                .clicked()
            {
                session.sweep_point_at_selection();
            }
            let name = if session.stone.sweep.erase { "Erase" } else { "Sweep" };
            if ui
                .button(name)
                .on_hover_text("MCP sweep along the points; Erase is its palette 0. One undo step.")
                .clicked()
            {
                session.sweep_from_panel();
            }
        });
    });
}

/// MCP `trim_outline`: cut away everything outside an outline and an inset per
/// height.
pub(super) fn trim(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Trim");
        let panel = &mut session.stone.trim;
        egui::Grid::new("trim-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                prop_label(ui, "trim_outline", "outline");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.outline)
                        .desired_rows(3)
                        .desired_width(150.0),
                );
                ui.end_row();
                prop_label(ui, "trim_outline", "curve");
                ui.checkbox(&mut panel.smooth, "Smooth");
                ui.end_row();
                label(ui, "Inset", "How far inward the surface stands at each height, one pair per line: z inset. Linear between pairs, the end value beyond them, and a negative inset is outside the outline. One number is the same at every height.");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.inset)
                        .desired_rows(2)
                        .desired_width(150.0),
                );
                ui.end_row();
                prop_label(ui, "trim_outline", "only");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                label(ui, "Skin", "Lay a new skin along the surface where the cut reached: solid cells next to it are recoloured, and a hollow the cut opened is sealed.");
                ui.checkbox(&mut panel.skin, "New skin");
                ui.end_row();
                if panel.skin {
                    label(ui, "Slot", "The skin's palette slot; 0 is the active slot.");
                    ui.add(egui::DragValue::new(&mut panel.slot).range(0..=255));
                    ui.end_row();
                    prop_label(ui, "trim_outline", "thickness");
                    prop_drag(ui, "trim_outline", "thickness", &mut panel.thickness);
                    ui.end_row();
                }
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Point at selection")
                .on_hover_text("Add the selection's centre as the next outline point.")
                .clicked()
            {
                session.trim_point_at_selection();
            }
            let name = scope(session, "Trim");
            if ui
                .button(name)
                .on_hover_text("MCP trim_outline, inside the selection's box or on the whole object. One undo step.")
                .clicked()
            {
                session.trim_from_panel();
            }
        });
    });
}

/// MCP `set_marker`, `remove_marker`, and `list_markers`: named points,
/// collision boxes, and connectors for a game.
pub(super) fn markers(ui: &mut Ui, session: &mut Session) {
    section(ui, "Markers");
    let mut remove = None;
    if session.markers.is_empty() {
        ui.label(RichText::new("No markers.").small().color(DIM));
    }
    egui::Grid::new("markers-list")
        .num_columns(3)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            for marker in &session.markers {
                let detail = match &marker.kind {
                    MarkerKind::Point { at } => {
                        format!("point {:.1} {:.1} {:.1}", at[0], at[1], at[2])
                    }
                    MarkerKind::Box { min, max } => format!(
                        "box {:.0}..{:.0} {:.0}..{:.0} {:.0}..{:.0}",
                        min[0], max[0], min[1], max[1], min[2], max[2]
                    ),
                    MarkerKind::Connector { at, direction } => format!(
                        "connector {:.1} {:.1} {:.1} → {:.1} {:.1} {:.1}",
                        at[0], at[1], at[2], direction[0], direction[1], direction[2]
                    ),
                };
                let scripted = if marker.scripted {
                    ", from the script"
                } else {
                    ""
                };
                ui.label(&marker.name).on_hover_text(format!(
                    "{detail}, on {}{scripted}. Local mesh units: cell (x, y, z) spans x..x+1.",
                    marker.object
                ));
                ui.label(RichText::new(marker.kind.name()).small().color(DIM));
                if ui
                    .small_button("✕")
                    .on_hover_text("Remove this marker (MCP remove_marker).")
                    .clicked()
                {
                    remove = Some(marker.name.clone());
                }
                ui.end_row();
            }
        });
    if let Some(name) = remove {
        session.remove_marker_from_panel(&name);
    }
    let panel = &mut session.stone.markers;
    egui::Grid::new("markers-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            label(ui, "Name", "The marker's name, such as ledge_top_4m; a marker of the same name is replaced. The glTF node takes it.");
            ui.add(egui::TextEdit::singleline(&mut panel.name).desired_width(120.0));
            ui.end_row();
            label(ui, "Kind", "Point: the top centre of the selection. Box: a collision box over the selection. Connector: a joining end at the selection's centre.");
            ui.horizontal_wrapped(|ui| {
                for (index, name) in ["Point", "Box", "Connector"].into_iter().enumerate() {
                    ui.selectable_value(&mut panel.kind, index, name);
                }
            });
            ui.end_row();
            if panel.kind == 2 {
                label(ui, "Facing", "The way the joining end faces.");
                ui.horizontal_wrapped(|ui| {
                    for (index, (name, _)) in DIRECTIONS.iter().enumerate() {
                        ui.selectable_value(&mut panel.direction, index, *name);
                    }
                });
                ui.end_row();
            }
        });
    if ui
        .button("Add at selection")
        .on_hover_text("MCP set_marker on the active object at the selection. Saved in the .vox and exported as a glTF node.")
        .clicked()
    {
        session.marker_from_panel();
    }
    collision_section(ui, session);
}

/// MCP `collision_from_voxels` in the Markers section.
fn collision_section(ui: &mut Ui, session: &mut Session) {
    ui.add_space(4.0);
    ui.label(RichText::new("Collision from voxels").small().color(DIM))
        .on_hover_text("Box markers over the active object's solid cells, for a game's colliders. Hollows stay open: a hold under a deck, a hatch, a hole in a hull.");
    let panel = &mut session.stone.markers;
    egui::Grid::new("collision-panel")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            label(ui, "Prefix", "Boxes are named prefix_1, prefix_2, and so on. Building again replaces this prefix's boxes on the object. Empty is the object's name plus _collider.");
            ui.add(egui::TextEdit::singleline(&mut panel.collision_prefix).hint_text("object_collider").desired_width(120.0));
            ui.end_row();
            prop_label(ui, "collision_from_voxels", "layer");
            prop_slider(ui, "collision_from_voxels", "layer", &mut panel.collision_layer);
            ui.end_row();
            label(ui, "Z bands", "Z planes for the horizontal cuts instead of Layer, ascending, such as 0 3 8 12. Put one on each deck and walking level.");
            ui.add(egui::TextEdit::singleline(&mut panel.collision_bands).hint_text("use Layer").desired_width(120.0));
            ui.end_row();
            prop_label(ui, "collision_from_voxels", "minFeature");
            prop_slider(ui, "collision_from_voxels", "minFeature", &mut panel.collision_min_feature);
            ui.end_row();
            prop_label(ui, "collision_from_voxels", "holeSize");
            prop_slider(ui, "collision_from_voxels", "holeSize", &mut panel.collision_hole_size);
            ui.end_row();
            label(ui, "Only", "Cover only these slots. Empty is every slot.");
            slots_field(ui, &mut panel.collision_only, "all slots");
            ui.end_row();
            label(ui, "Leave out", "Slots to ignore, such as moss, vines, and rope.");
            slots_field(ui, &mut panel.collision_exclude, "none");
            ui.end_row();
            prop_label(ui, "collision_from_voxels", "maxBoxes");
            prop_drag(ui, "collision_from_voxels", "maxBoxes", &mut panel.collision_max_boxes);
            ui.end_row();
        });
    if ui
        .button("Build collision")
        .on_hover_text("MCP collision_from_voxels on the active object. The status gives the box count, the solid covered, and the empty cells filled. Check with Markers on preview. One undo step.")
        .clicked()
    {
        session.collision_from_panel();
    }
}
