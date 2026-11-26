//! Tool buttons and the generator form.

use egui::{RichText, Ui};

use super::*;

/// Edge of a strip button, and the space between buttons.
pub(super) const STRIP_ICON: f32 = 32.0;
const STRIP_GAP: f32 = 4.0;

/// The strip's tools and their names; the key is [`Tool::key`].
pub(super) const TOOLS: [(Tool, &str); 13] = [
    (Tool::Brush, "Brush"),
    (Tool::Line, "Line"),
    (Tool::Circle, "Circle"),
    (Tool::Square, "Square"),
    (Tool::Box, "Box"),
    (Tool::Face, "Face"),
    (Tool::Generator, "Generator"),
    (Tool::Tube, "Tube"),
    (Tool::Sculpt, "Sculpt"),
    (Tool::Mutator, "Mutator"),
    (Tool::Marquee, "Marquee"),
    (Tool::BoxSelect, "Box Select"),
    (Tool::FloodSelect, "Flood"),
];

/// The tool strip: one column of tool icons, then the Add, Remove, and
/// Paint modes. Their settings are in [`options`].
pub(super) fn strip(ui: &mut Ui, session: &mut Session) {
    let size = STRIP_ICON;
    ui.spacing_mut().item_spacing = egui::vec2(STRIP_GAP, STRIP_GAP);
    for (tool, label) in TOOLS {
        let tip = format!("{label} ({})", tool.key().name());
        let on = session.tool == tool;
        let resp = ui
            .add_sized(
                [size, size],
                egui::Button::new("")
                    .min_size(egui::vec2(size, size))
                    .selected(on),
            )
            .on_hover_text(tip);
        crate::icons::paint_tool(ui, &resp, tool, on);
        let rect = resp.rect;
        let clicked = resp.clicked();
        let id = format!("tool.{}", label.to_ascii_lowercase().replace(' ', "-"));
        session.note_ui(&id, label, rect, UiAction::Tool(tool));
        if clicked {
            session.tool = tool;
        }
    }

    ui.separator();
    for (name, key, mode) in [
        ("Add", "T", ToolMode::Add),
        ("Remove", "R", ToolMode::Remove),
        ("Paint", "G", ToolMode::Paint),
    ] {
        let on = session.tool_mode == mode;
        let resp = ui
            .add_sized(
                [size, size],
                egui::Button::new("")
                    .min_size(egui::vec2(size, size))
                    .selected(on),
            )
            .on_hover_text(format!("{name} ({key})"));
        crate::icons::paint(ui, &resp, crate::icons::mode(mode), on);
        let rect = resp.rect;
        let clicked = resp.clicked();
        session.note_ui(
            &format!("mode.{}", name.to_ascii_lowercase()),
            name,
            rect,
            UiAction::Mode(mode),
        );
        if clicked {
            session.tool_mode = mode;
        }
    }
}

/// Settings of the current tool, laid out in a row that wraps.
pub(super) fn options(ui: &mut Ui, session: &mut Session) {
    ui.label(RichText::new(session.tool.name()).strong());
    ui.separator();

    if !matches!(
        session.tool,
        Tool::Face | Tool::Generator | Tool::Tube | Tool::Sculpt
    ) {
        op_slider(ui, session, SliderTarget::BrushSize);
        segment(
            ui,
            &mut session.brush_shape,
            &[
                ("Square", BrushShape::Square),
                ("Circle", BrushShape::Circle),
            ],
        );
        let mut dim = session.brush_3d;
        row(ui, |ui| {
            if ui.selectable_label(!dim, "2D").clicked() {
                dim = false;
            }
            if ui.selectable_label(dim, "3D").clicked() {
                dim = true;
            }
        });
        session.brush_3d = dim;
        let n = session.brush_cell_count();
        let edge = session.brush_size;
        let summary = if session.brush_3d {
            format!("{edge} × {edge} × {edge}  ({n} cells)")
        } else {
            format!("{edge} × {edge}  ({n} cells, on face plane)")
        };
        ui.add(
            egui::Label::new(RichText::new(summary).small().monospace().color(DIM))
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    }
    if matches!(
        session.tool,
        Tool::Line | Tool::Circle | Tool::Square | Tool::Box | Tool::Mutator
    ) {
        ui.separator();
    }

    match session.tool {
        Tool::Line => {
            segment(
                ui,
                &mut session.line_mode,
                &[
                    ("Straight", LineMode::Straight),
                    ("Project", LineMode::Project),
                ],
            );
        }
        Tool::Circle => {
            row(ui, |ui| {
                ui.toggle_value(&mut session.circle_fill, "Fill");
                ui.toggle_value(&mut session.circle_even, "Even");
            });
        }
        Tool::Square => {
            row(ui, |ui| {
                ui.toggle_value(&mut session.square_fill, "Fill");
                ui.toggle_value(&mut session.square_even, "Even");
            });
        }
        Tool::Box => {
            ui.toggle_value(&mut session.box_fill, "Fill");
        }
        Tool::Face => {
            ui.label(RichText::new("Extrude").small().color(DIM));
            segment(
                ui,
                &mut session.face_extrude,
                &[
                    ("Color", FaceExtrude::Color),
                    ("Geometry", FaceExtrude::Geometry),
                ],
            );
            ui.label(RichText::new("With").small().color(DIM));
            segment(
                ui,
                &mut session.face_with,
                &[
                    ("Geometry", FaceWith::Geometry),
                    ("Palette", FaceWith::Palette),
                ],
            );
            ui.label(RichText::new("Connectivity").small().color(DIM));
            let mut eight = session.face_eight;
            row(ui, |ui| {
                if ui.selectable_label(!eight, "4-side").clicked() {
                    eight = false;
                }
                if ui.selectable_label(eight, "8-side").clicked() {
                    eight = true;
                }
            });
            session.face_eight = eight;
        }
        Tool::Generator => {
            op_combo(ui, session, "generate", "kind", |kind| {
                GeneratorKind::from_key(kind).map_or(kind, GeneratorKind::name)
            });
            generator(ui, session);
        }
        Tool::Tube => tube(ui, session),
        Tool::Sculpt => sculpt(ui, session),
        Tool::Mutator => {
            row(ui, |ui| {
                ui.add_enabled_ui(false, |ui| {
                    let mut foliage = 0;
                    egui::ComboBox::from_id_salt("mutator")
                        .selected_text("Foliage")
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut foliage, 0, "Foliage");
                        });
                });
            });
            ui.label(RichText::new("Seed color").small().color(DIM));
            op_slider(ui, session, SliderTarget::FoliageSeed);
            if ui.button("Use active palette slot").clicked() {
                session.foliage_seed = session.active_slot as i32;
            }
            op_slider(ui, session, SliderTarget::FoliageDensity);
        }
        _ => {}
    }
    ui.separator();
    viewport_mirror(ui, &mut session.edit.viewport_mirror);
}

/// Mirror of the brush and shape tools: every cell a stroke writes is
/// written again across a plane, as MCP `set_voxels` and the fills do with
/// `mirror`.
fn viewport_mirror(ui: &mut Ui, field: &mut crate::state::MirrorField) {
    row(ui, |ui| {
        ui.toggle_value(&mut field.on, "Mirror")
            .on_hover_text("Write every cell of a stroke again across a plane, so one stroke edits both sides (MCP mirror on set_voxels and the fills). The plane is the active object's centre unless At gives it.");
        ui.add_enabled_ui(field.on, |ui| {
            egui::ComboBox::from_id_salt("viewport-mirror")
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
                    .desired_width(50.0),
            )
            .on_hover_text("The plane's coordinate along the axis in local cells, a multiple of 0.5; empty is the middle of the object's bounds.");
        });
    });
}

/// The Tube tool: MCP `fill_tube` by drag. A drag runs a tube from the
/// start radius to the end radius. With Chain each drag adds a point to one
/// tube, smoothed through all of them, until Finish. Remove carves it.
fn tube(ui: &mut Ui, session: &mut Session) {
    // Labels, ranges, and tooltips are fill_tube's schema (ops::prop_ui).
    let tool = "fill_tube";
    op_slider_as(ui, session, "Start", SliderTarget::TubeStart);
    op_slider_as(ui, session, "End", SliderTarget::TubeEnd);
    op_slider(ui, session, SliderTarget::TubeAspect);
    op_slider(ui, session, SliderTarget::TubeRoll);
    row(ui, |ui| {
        op_toggle(ui, session, tool, "profile");
        op_toggle(ui, session, tool, "caps");
    });
    row(ui, |ui| {
        op_toggle(ui, session, tool, "curve");
        let chain = session.tube_chain;
        ui.toggle_value(&mut session.tube_chain, "Chain")
            .on_hover_text("Each drag adds a point to one tube");
        if chain && !session.tube_chain {
            session.finish_tube();
        }
    });
    if session.tube_chain {
        let points = session.tube_points.len();
        ui.add_enabled_ui(points > 0, |ui| {
            if ui.button(format!("Finish ({points} points)")).clicked() {
                session.finish_tube();
            }
        });
    }
    ui.separator();
    op_choice(ui, session, tool, "pattern", Some("Solid"));
    let pattern = session.op_arg(tool, "pattern");
    if !pattern.is_null() {
        slider_i32(
            ui,
            session,
            "Second slot",
            SliderTarget::TubeSecond,
            1..=255,
        );
    }
    match pattern.as_str() {
        Some("rings") => op_slider(ui, session, SliderTarget::TubePeriod),
        Some("stripes") => op_slider(ui, session, SliderTarget::TubeSectors),
        _ => {}
    }
}

/// The Sculpt tool: MCP `brush` by drag. A drag lays dabs on the surface
/// under the pointer, half a radius apart; Grab pulls the pressed point
/// along the drag in the view plane. The stroke is one undo step.
fn sculpt(ui: &mut Ui, session: &mut Session) {
    row(ui, |ui| {
        for (name, op, tip) in SCULPT_OPS {
            ui.selectable_value(&mut session.sculpt.op, op, name)
                .on_hover_text(tip);
        }
    });
    let brush = &mut session.sculpt;
    row(ui, |ui| {
        prop_label(ui, "brush", "radius");
        prop_slider(ui, "brush", "radius", &mut brush.radius);
    });
    row(ui, |ui| {
        prop_label(ui, "brush", "strength");
        prop_slider(ui, "brush", "strength", &mut brush.strength);
    });
    row(ui, |ui| {
        ui.label(RichText::new("Falloff").small().color(DIM))
            .on_hover_text("How a dab fades from its centre to its radius");
        for (index, name) in ["Smooth", "Linear", "Constant"].into_iter().enumerate() {
            ui.selectable_value(&mut brush.falloff, index, name);
        }
    });
    row(ui, |ui| {
        ui.label(RichText::new("Only").small().color(DIM))
            .on_hover_text(
                "Palette slots the brush may change, such as 3, 4; empty changes every slot",
            );
        ui.add(
            egui::TextEdit::singleline(&mut brush.only)
                .hint_text("all slots")
                .desired_width(80.0),
        );
    });
}

/// The Generator tool's options for its kind: MCP `generate` arguments in
/// `op_args`, drawn from the schema.
pub(super) fn generator(ui: &mut Ui, session: &mut Session) {
    let kind = session.op_arg("generate", "kind");
    let kind = kind
        .as_str()
        .and_then(GeneratorKind::from_key)
        .unwrap_or(GeneratorKind::Ellipsoid);
    let axis = |ui: &mut Ui, session: &mut Session| {
        prop_label(ui, "generate", "axis");
        op_choice(ui, session, "generate", "axis", None);
    };
    match kind {
        GeneratorKind::Ellipsoid => {
            ui.label(
                RichText::new("(shape derives from bounds)")
                    .small()
                    .color(DIM),
            );
        }
        GeneratorKind::RoundedBox => op_slider(ui, session, SliderTarget::RoundRadius),
        GeneratorKind::Cylinder => axis(ui, session),
        GeneratorKind::Cone | GeneratorKind::Pyramid => {
            axis(ui, session);
            prop_label(ui, "generate", "apex");
            op_choice(ui, session, "generate", "apex", None);
        }
        GeneratorKind::Torus => {
            axis(ui, session);
            op_slider(ui, session, SliderTarget::TorusTube);
        }
        GeneratorKind::Perlin => {
            for target in [
                SliderTarget::NoiseScale,
                SliderTarget::NoiseThreshold,
                SliderTarget::NoiseOctaves,
                SliderTarget::NoiseSeed,
                SliderTarget::PerlinLo,
                SliderTarget::PerlinHi,
            ] {
                op_slider(ui, session, target);
            }
        }
        GeneratorKind::Tree => {
            axis(ui, session);
            for target in [
                SliderTarget::TreeDepth,
                SliderTarget::TreeWildness,
                SliderTarget::TreeTrunk,
                SliderTarget::TreeLeaf,
                SliderTarget::TreeSeed,
            ] {
                op_slider(ui, session, target);
            }
        }
        GeneratorKind::Helix => {
            axis(ui, session);
            for target in [
                SliderTarget::HelixTurns,
                SliderTarget::HelixRadius,
                SliderTarget::HelixTube,
            ] {
                op_slider(ui, session, target);
            }
            op_toggle(ui, session, "generate", "clockwise");
        }
        GeneratorKind::Conifer => {
            axis(ui, session);
            for target in [
                SliderTarget::ConiferTiers,
                SliderTarget::ConiferRagged,
                SliderTarget::ConiferClump,
                SliderTarget::ConiferTrunk,
                SliderTarget::ConiferCrown,
                SliderTarget::ConiferNeedle,
                SliderTarget::ConiferSeed,
            ] {
                op_slider(ui, session, target);
            }
        }
    }
}
