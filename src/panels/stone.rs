//! Inspector sections of the stone tools: Joints, Weather, Bark, Cavity,
//! Growth, Sweep, and Markers. Each button runs the MCP
//! tool over the selection's box, or the whole active object.

use egui::{RichText, Ui};

use super::{section, DIM};
use crate::state::{
    EditorMode, MarkerKind, Plant, Session, BARK_ALONG, DIRECTIONS, FACINGS, RELIEF_FACINGS,
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

/// MCP `exclude`: boxes Scatter, Relief, Joints, Weather, Bark, Cavity,
/// Foliage, and Growth leave alone.
pub(super) fn protect(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Protect");
        ui.label(
            RichText::new("Boxes Scatter, Relief, Joints, Weather, Bark, Cavity, Foliage, and Growth leave alone: ledge tops, cave floors, river beds.")
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
        ui.horizontal(|ui| {
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
                label(ui, "Facing", "The face's outward direction. The face is found in the cells, so a swept, curved wall works.");
                ui.horizontal(|ui| {
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
                label(ui, "Width", "Column width range along the face, in cells.");
                range_i32(ui, &mut panel.width, 1..=64);
                ui.end_row();
                label(ui, "Out", "How far a column stands out of the face, in cells.");
                ui.horizontal(|ui| {
                    for value in panel.out.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(0..=16));
                    }
                });
                ui.end_row();
                label(ui, "Crown", "Height the column tops scatter around. Off: the highest solid cell of the box.");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut panel.crown_on, "");
                    ui.add_enabled(
                        panel.crown_on,
                        egui::DragValue::new(&mut panel.crown).range(-1024..=1023),
                    );
                });
                ui.end_row();
                label(ui, "Top", "Column top range relative to the crown: negative ends short of it, positive rises above it.");
                ui.horizontal(|ui| {
                    for value in panel.top.iter_mut() {
                        ui.add(egui::DragValue::new(value).range(-256..=256));
                    }
                });
                ui.end_row();
                label(ui, "Lower", "A column ending below the crown cuts the wall above it, so the crown is uneven. Off: columns only add cells.");
                ui.checkbox(&mut panel.lower, "");
                ui.end_row();
                label(ui, "Depth", "How far a column reaches back into the face, and how deep the cut above a short column goes.");
                ui.add(egui::Slider::new(&mut panel.depth, 1..=16));
                ui.end_row();
                label(ui, "Seed", "Same seed, same columns.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
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
                label(ui, "Chip", "Chance that an edge cell loses a chip; corners are likelier.");
                ui.add(egui::Slider::new(&mut panel.chip, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Chip size", "Most cells one chip takes.");
                ui.add(egui::Slider::new(&mut panel.chip_size, 1..=8));
                ui.end_row();
                label(ui, "Pits", "Chance that a flat face cell is dented.");
                ui.add(egui::Slider::new(&mut panel.pits, 0.0..=1.0).max_decimals(3));
                ui.end_row();
                label(ui, "Pit depth", "Deepest pit in cells.");
                ui.add(egui::Slider::new(&mut panel.pit_depth, 1..=4));
                ui.end_row();
                label(ui, "Broken", "Palette slot for the faces a chip or pit uncovers, a slightly darker stone. 0 keeps their colour.");
                ui.add(egui::DragValue::new(&mut panel.broken).range(0..=255));
                ui.end_row();
                label(ui, "Noise", "Palette slots of the subtle colour noise; empty adds none.");
                slots_field(ui, &mut panel.noise, "no noise");
                ui.end_row();
                label(ui, "Density", "Share of the exposed cells the noise recolours.");
                ui.add(egui::Slider::new(&mut panel.noise_density, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Patch", "Noise patch size in cells.");
                ui.add(egui::Slider::new(&mut panel.noise_scale, 1.0..=32.0).max_decimals(1));
                ui.end_row();
                label(ui, "Only", "Act only on these palette slots; empty acts on every slot.");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                label(ui, "Seed", "Same seed, same weathering.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        if ui
            .button(scope(session, "Weather"))
            .on_hover_text("MCP weather over the selection's box, or the whole object. One undo step.")
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
                ui.horizontal(|ui| {
                    for (index, name) in BARK_ALONG.iter().enumerate() {
                        ui.selectable_value(&mut panel.along, index, *name);
                    }
                });
                ui.end_row();
                if panel.along == BARK_ALONG.len() - 1 {
                    label(ui, "Points", "The wood's path in local cells, one point per line: x y z (a tube's radius or a sweep's twist and scale after them are ignored).");
                    ui.add(
                        egui::TextEdit::multiline(&mut panel.points)
                            .desired_rows(3)
                            .desired_width(150.0),
                    );
                    ui.end_row();
                    label(ui, "Curve", "Smooth follows a curve through the points, as a smooth tube does.");
                    ui.horizontal(|ui| {
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
                label(ui, "Dirt", "Palette slot of the cell at the bottom of each fissure, a dark brown. 0 keeps its colour.");
                ui.add(egui::DragValue::new(&mut panel.dirt).range(0..=255));
                ui.end_row();
                label(ui, "Width", "Plate width range in cells around the wood.");
                range_f32(ui, &mut panel.width, 1.0..=64.0);
                ui.end_row();
                label(ui, "Length", "Plate length range in cells along the grain.");
                range_f32(ui, &mut panel.length, 2.0..=256.0);
                ui.end_row();
                label(ui, "Depth", "Fissure depth range in cells; plate ends and cracks take the smaller.");
                range_i32(ui, &mut panel.depth, 1..=4);
                ui.end_row();
                label(ui, "Cracks", "Share of plates with a crack part way across.");
                ui.add(egui::Slider::new(&mut panel.cracks, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Only", "Act only on these palette slots, the wood; empty acts on every slot.");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                label(ui, "Seed", "Same seed, same plates.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        if ui
            .button(scope(session, "Cut bark on"))
            .on_hover_text("MCP bark over the selection's box, or the whole object, keeping the Protect boxes. End grain is left alone. One undo step.")
            .clicked()
        {
            session.bark_from_panel();
        }
    });
    if from_sweep {
        session.bark_points_from_sweep();
    }
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
                label(ui, "Radius", "Radius of the ball of neighbours counted around each cell; larger finds broader hollows.");
                ui.add(egui::Slider::new(&mut panel.radius, 1..=4));
                ui.end_row();
                label(ui, "Dark", "How concave a cell must be to darken: 0 a flat face, 1 a closed pocket.");
                ui.add(egui::Slider::new(&mut panel.dark, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Lighten", "Also lighten exposed ridges and corners more convex than this: 0 a flat face, 1 a lone spike.");
                ui.horizontal(|ui| {
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
                label(ui, "Shades", "Without pairs: the darker and lighter shade as factors of the slot's colour.");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.darken).range(0.05..=0.99).speed(0.01));
                    ui.add(egui::DragValue::new(&mut panel.brighten).range(1.01..=4.0).speed(0.01));
                });
                ui.end_row();
                label(ui, "Only", "Act only on these palette slots; empty acts on every slot.");
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
                ui.horizontal(|ui| {
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
                    ui.horizontal(|ui| {
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
                label(ui, "Noise", "0 grows in smooth patches, 1 in ragged speckle.");
                ui.add(egui::Slider::new(&mut panel.noise, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Faces", "Which faces it grows on. Bottom takes undersides, such as the dark underside of a tree crown.");
                ui.horizontal(|ui| {
                    for (on, name) in panel.faces.iter_mut().zip(["Top", "Side", "Bottom"]) {
                        ui.toggle_value(on, name);
                    }
                });
                ui.end_row();
                label(ui, "Facing", "Only faces pointing within 60 degrees of this direction. +Z turns on Top and −Z turns on Bottom, the only faces that point that way.");
                ui.horizontal(|ui| {
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
                ui.horizontal(|ui| {
                    ui.checkbox(&mut panel.band_on, "");
                    ui.add_enabled_ui(panel.band_on, |ui| {
                        ui.add(egui::DragValue::new(&mut panel.band[0]).range(-1024..=1023));
                        ui.add(egui::DragValue::new(&mut panel.band[1]).range(-1024..=1023));
                    });
                });
                ui.end_row();
                label(ui, "Only", "Grow only on these palette slots, such as the stone; empty grows on every slot.");
                slots_field(ui, &mut panel.only, "every slot");
                ui.end_row();
                if panel.plant != Plant::Vines {
                    label(ui, "Paint", "Recolour the surface instead of adding a layer on it.");
                    ui.checkbox(&mut panel.paint, "");
                    ui.end_row();
                }
                if panel.plant == Plant::Moss && panel.cushions {
                    label(ui, "Size", "Cushion radius range in cells.");
                    range_i32(ui, &mut panel.size, 1..=16);
                    ui.end_row();
                    label(ui, "Gap", "Fewest cells between two cushions.");
                    ui.add(egui::Slider::new(&mut panel.gap, 1..=8));
                    ui.end_row();
                    label(ui, "Exposure", "How far a cell must face up (or Facing) to take moss: 0 the upper half of a log, 0.7 only its crown.");
                    ui.add(egui::Slider::new(&mut panel.exposure, -1.0..=1.0).max_decimals(2));
                    ui.end_row();
                    label(ui, "Drapes", "Share of cushion edges over a drop that a drape hangs from.");
                    ui.add(egui::Slider::new(&mut panel.drapes, 0.0..=1.0).max_decimals(2));
                    ui.end_row();
                    label(ui, "Thickness", "Thickness at a cushion's middle.");
                    ui.add(egui::Slider::new(&mut panel.thickness, 1..=4));
                    ui.end_row();
                } else if panel.plant == Plant::Moss {
                    label(ui, "Curtains", "Share of top edges a curtain runs down from.");
                    ui.add(egui::Slider::new(&mut panel.curtains, 0.0..=1.0).max_decimals(2));
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
                    label(ui, "Blooms", "Bloom slots beside the strands; empty grows none.");
                    slots_field(ui, &mut panel.blooms, "none");
                    ui.end_row();
                    label(ui, "Bloom chance", "Chance of a bloom beside each strand cell.");
                    ui.add(egui::Slider::new(&mut panel.bloom_chance, 0.0..=1.0).max_decimals(2));
                    ui.end_row();
                }
                label(ui, "Seed", "Same seed, same growth.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
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
                label(ui, "Points", "Path points in local cells, one per line: x y z, then optional twist (degrees) and scale.");
                ui.add(
                    egui::TextEdit::multiline(&mut panel.points)
                        .desired_rows(3)
                        .desired_width(150.0),
                );
                ui.end_row();
                label(ui, "Curve", "Smooth passes a curve through the points; off joins them with straight runs.");
                ui.checkbox(&mut panel.smooth, "Smooth");
                ui.end_row();
                label(ui, "Profile", "A rounded rectangle standing on the path, or a polygon of u v points (u to the left of the path, v up). A cell is in when its centre is inside the profile at the nearest point of the path.");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut panel.polygon, false, "Rectangle");
                    ui.selectable_value(&mut panel.polygon, true, "Polygon");
                });
                ui.end_row();
                if panel.polygon {
                    label(ui, "Polygon", "u v pairs separated by semicolons.");
                    slots_field(ui, &mut panel.profile, "-3 0; 3 0; 0 8");
                    ui.end_row();
                } else {
                    label(ui, "Width", "Across the path, centred on it.");
                    ui.add(egui::DragValue::new(&mut panel.width).range(0.5..=512.0).speed(0.1));
                    ui.end_row();
                    label(ui, "Height", "Up from the path.");
                    ui.add(egui::DragValue::new(&mut panel.height).range(0.5..=512.0).speed(0.1));
                    ui.end_row();
                    label(ui, "Radius", "Corner rounding.");
                    ui.add(egui::DragValue::new(&mut panel.radius).range(0.0..=256.0).speed(0.1));
                    ui.end_row();
                }
                label(ui, "Erase", "Remove the solid cells the sweep covers instead of filling: a slanted cut along a closed or curved outline. Palettes, Pattern, Period, Mode, and Seed do not apply.");
                ui.checkbox(&mut panel.erase, "Erase");
                ui.end_row();
                label(ui, "Palettes", "Slots laid out by the pattern; empty is the active slot.");
                slots_field(ui, &mut panel.palettes, "active slot");
                ui.end_row();
                label(ui, "Pattern", "Bands by height in the profile (strata), rings along the path, or random cells.");
                ui.horizontal(|ui| {
                    for (index, name) in ["Bands", "Rings", "Random"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.pattern, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Period", "Band or ring size in cells.");
                ui.add(egui::DragValue::new(&mut panel.period).range(0.5..=512.0).speed(0.1));
                ui.end_row();
                label(ui, "Mode", "Replace writes every cell; Empty adds only into empty cells; Paint recolours solid cells.");
                ui.horizontal(|ui| {
                    for (index, name) in ["Replace", "Empty", "Paint"].into_iter().enumerate() {
                        ui.selectable_value(&mut panel.mode, index, name);
                    }
                });
                ui.end_row();
                label(ui, "Seed", "Seed of the random pattern.");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        ui.horizontal(|ui| {
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
            ui.horizontal(|ui| {
                for (index, name) in ["Point", "Box", "Connector"].into_iter().enumerate() {
                    ui.selectable_value(&mut panel.kind, index, name);
                }
            });
            ui.end_row();
            if panel.kind == 2 {
                label(ui, "Facing", "The way the joining end faces.");
                ui.horizontal(|ui| {
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
}
