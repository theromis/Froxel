//! Tool buttons and the generator form.

use egui::{RichText, Ui};

use super::*;

/// Edge of a strip button, and the space between buttons.
pub(super) const STRIP_ICON: f32 = 32.0;
const STRIP_GAP: f32 = 4.0;

/// The tool strip: one column of tool icons, then the Add, Remove, and
/// Paint modes. Their settings are in [`options`].
pub(super) fn strip(ui: &mut Ui, session: &mut Session) {
    const TOOLS: [(Tool, &str, &str); 13] = [
        (Tool::Brush, "Brush", "V"),
        (Tool::Line, "Line", "L"),
        (Tool::Circle, "Circle", ""),
        (Tool::Square, "Square", ""),
        (Tool::Box, "Box", "B"),
        (Tool::Face, "Face", "F"),
        (Tool::Generator, "Generator", ""),
        (Tool::Tube, "Tube", ""),
        (Tool::Sculpt, "Sculpt", "S"),
        (Tool::Mutator, "Mutator", "U"),
        (Tool::Marquee, "Marquee", "M"),
        (Tool::BoxSelect, "Box Select", ""),
        (Tool::FloodSelect, "Flood", ""),
    ];
    let size = STRIP_ICON;
    ui.spacing_mut().item_spacing = egui::vec2(STRIP_GAP, STRIP_GAP);
    for (tool, label, key) in TOOLS {
        let tip = if key.is_empty() {
            label.to_string()
        } else {
            format!("{label} ({key})")
        };
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
        slider_i32(ui, session, "Size", SliderTarget::BrushSize, 1..=63);
        session.brush_size = session.brush_size.clamp(1, 63);
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
            egui::ComboBox::from_id_salt("generator")
                .selected_text(session.generator.name())
                .show_ui(ui, |ui| {
                    for kind in [
                        GeneratorKind::Ellipsoid,
                        GeneratorKind::RoundedBox,
                        GeneratorKind::Cylinder,
                        GeneratorKind::Cone,
                        GeneratorKind::Pyramid,
                        GeneratorKind::Torus,
                        GeneratorKind::Perlin,
                        GeneratorKind::Tree,
                        GeneratorKind::Helix,
                        GeneratorKind::Conifer,
                    ] {
                        ui.selectable_value(&mut session.generator, kind, kind.name());
                    }
                });
            generator(ui, session);
        }
        Tool::Tube => tube(ui, session),
        Tool::Sculpt => sculpt(ui, session),
        Tool::Mutator => {
            ui.add_enabled_ui(false, |ui| {
                let mut foliage = 0;
                egui::ComboBox::from_id_salt("mutator")
                    .selected_text("Foliage")
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut foliage, 0, "Foliage");
                    });
            });
            ui.label(RichText::new("Seed color").small().color(DIM));
            slider_i32(ui, session, "Slot", SliderTarget::FoliageSeed, 1..=255);
            session.foliage_seed = session.foliage_seed.clamp(1, 255);
            if ui.button("Use active palette slot").clicked() {
                session.foliage_seed = session.active_slot as i32;
            }
            slider_f32(
                ui,
                session,
                "Density",
                SliderTarget::FoliageDensity,
                0.0..=1.0,
                2,
            );
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
    slider_f32(ui, session, "Start", SliderTarget::TubeStart, 0.5..=32.0, 1);
    slider_f32(ui, session, "End", SliderTarget::TubeEnd, 0.5..=32.0, 1);
    slider_f32(
        ui,
        session,
        "Flatten",
        SliderTarget::TubeAspect,
        0.05..=1.0,
        2,
    );
    slider_f32(
        ui,
        session,
        "Roll",
        SliderTarget::TubeRoll,
        -180.0..=180.0,
        0,
    );
    ui.toggle_value(&mut session.tube_blade, "Blade")
        .on_hover_text("A wedge: full thickness at one edge, one cell at the other");
    row(ui, |ui| {
        ui.toggle_value(&mut session.tube_smooth, "Smooth")
            .on_hover_text("A curve through the chain's points instead of straight segments");
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
    segment(
        ui,
        &mut session.tube_colors,
        &[
            ("Solid", TubeColors::Solid),
            ("Rings", TubeColors::Rings),
            ("Stripes", TubeColors::Stripes),
            ("Random", TubeColors::Random),
        ],
    );
    if session.tube_colors != TubeColors::Solid {
        slider_i32(
            ui,
            session,
            "Second slot",
            SliderTarget::TubeSecond,
            1..=255,
        );
    }
    match session.tube_colors {
        TubeColors::Rings => slider_f32(
            ui,
            session,
            "Period",
            SliderTarget::TubePeriod,
            1.0..=16.0,
            1,
        ),
        TubeColors::Stripes => {
            slider_i32(ui, session, "Sectors", SliderTarget::TubeSectors, 1..=32)
        }
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
        ui.label(RichText::new("Radius").small().color(DIM))
            .on_hover_text("Dab radius in cells (MCP brush radius)");
        ui.add(egui::Slider::new(&mut brush.radius, 0.5..=32.0).max_decimals(1));
    });
    row(ui, |ui| {
        ui.label(RichText::new("Strength").small().color(DIM)).on_hover_text(
            "0..1. Inflate and deflate push up to strength × radius / 2 cells per dab; grab moves the full drag at 1",
        );
        ui.add(egui::Slider::new(&mut brush.strength, 0.0..=1.0).max_decimals(2));
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

pub(super) fn axis_row(ui: &mut Ui, axis: &mut Axis) {
    ui.label(RichText::new("Axis").small().color(DIM));
    segment(ui, axis, &[("X", Axis::X), ("Y", Axis::Y), ("Z", Axis::Z)]);
}

pub(super) fn generator(ui: &mut Ui, session: &mut Session) {
    match session.generator {
        GeneratorKind::Ellipsoid => {
            ui.label(
                RichText::new("(shape derives from bounds)")
                    .small()
                    .color(DIM),
            );
        }
        GeneratorKind::RoundedBox => {
            slider_f32(
                ui,
                session,
                "Radius",
                SliderTarget::RoundRadius,
                0.0..=32.0,
                1,
            );
        }
        GeneratorKind::Cylinder => axis_row(ui, &mut session.gen_axis),
        GeneratorKind::Cone | GeneratorKind::Pyramid => {
            axis_row(ui, &mut session.gen_axis);
            ui.label(RichText::new("Apex at").small().color(DIM));
            let mut max = session.apex_at_max;
            row(ui, |ui| {
                if ui.selectable_label(!max, "Min").clicked() {
                    max = false;
                }
                if ui.selectable_label(max, "Max").clicked() {
                    max = true;
                }
            });
            session.apex_at_max = max;
        }
        GeneratorKind::Torus => {
            axis_row(ui, &mut session.gen_axis);
            slider_f32(ui, session, "Tube", SliderTarget::TorusTube, 0.05..=1.0, 2);
        }
        GeneratorKind::Perlin => {
            slider_f32(
                ui,
                session,
                "Scale",
                SliderTarget::NoiseScale,
                1.0..=64.0,
                1,
            );
            slider_f32(
                ui,
                session,
                "Threshold",
                SliderTarget::NoiseThreshold,
                0.0..=1.0,
                2,
            );
            slider_i32(ui, session, "Octaves", SliderTarget::NoiseOctaves, 1..=6);
            slider_i32(ui, session, "Seed", SliderTarget::NoiseSeed, 0..=9999);
            slider_i32(ui, session, "Palette lo", SliderTarget::PerlinLo, 1..=255);
            slider_i32(ui, session, "Palette hi", SliderTarget::PerlinHi, 1..=255);
        }
        GeneratorKind::Tree => {
            axis_row(ui, &mut session.gen_axis);
            slider_i32(ui, session, "Depth", SliderTarget::TreeDepth, 1..=5);
            slider_f32(
                ui,
                session,
                "Wildness",
                SliderTarget::TreeWildness,
                0.0..=1.0,
                2,
            );
            slider_f32(
                ui,
                session,
                "Trunk",
                SliderTarget::TreeTrunk,
                0.20..=1.50,
                2,
            );
            slider_i32(ui, session, "Leaf color", SliderTarget::TreeLeaf, 1..=255);
            slider_i32(ui, session, "Seed", SliderTarget::TreeSeed, 0..=9999);
        }
        GeneratorKind::Helix => {
            axis_row(ui, &mut session.gen_axis);
            slider_f32(
                ui,
                session,
                "Turns",
                SliderTarget::HelixTurns,
                0.25..=12.0,
                2,
            );
            slider_f32(
                ui,
                session,
                "Radius",
                SliderTarget::HelixRadius,
                0.0..=1.0,
                2,
            );
            slider_f32(ui, session, "Tube", SliderTarget::HelixTube, 0.02..=0.90, 2);
            ui.label(RichText::new("Winding").small().color(DIM));
            let mut cw = session.helix_cw;
            row(ui, |ui| {
                if ui.selectable_label(!cw, "CCW").clicked() {
                    cw = false;
                }
                if ui.selectable_label(cw, "CW").clicked() {
                    cw = true;
                }
            });
            session.helix_cw = cw;
        }
        GeneratorKind::Conifer => {
            axis_row(ui, &mut session.gen_axis);
            slider_i32(ui, session, "Tiers", SliderTarget::ConiferTiers, 2..=5);
            slider_f32(
                ui,
                session,
                "Raggedness",
                SliderTarget::ConiferRagged,
                0.0..=1.0,
                2,
            );
            slider_f32(
                ui,
                session,
                "Clump",
                SliderTarget::ConiferClump,
                2.0..=24.0,
                1,
            );
            slider_f32(
                ui,
                session,
                "Trunk",
                SliderTarget::ConiferTrunk,
                0.20..=1.50,
                2,
            );
            slider_f32(
                ui,
                session,
                "Crown",
                SliderTarget::ConiferCrown,
                0.0..=1.0,
                2,
            );
            slider_i32(ui, session, "Needle", SliderTarget::ConiferNeedle, 1..=255);
            slider_i32(ui, session, "Seed", SliderTarget::ConiferSeed, 0..=9999);
        }
    }
}
