//! Tool buttons and the generator form.

use egui::{RichText, Ui};

use super::*;

pub(super) fn tools(ui: &mut Ui, session: &mut Session) {
    section(ui, "Tool");
    const TOOLS: [(Tool, &str, &str); 11] = [
        (Tool::Brush, "Brush", "V"),
        (Tool::Line, "Line", "L"),
        (Tool::Circle, "Circle", ""),
        (Tool::Square, "Square", ""),
        (Tool::Box, "Box", "B"),
        (Tool::Face, "Face", "F"),
        (Tool::Generator, "Generator", ""),
        (Tool::Mutator, "Mutator", "U"),
        (Tool::Marquee, "Marquee", "M"),
        (Tool::BoxSelect, "Box Select", ""),
        (Tool::FloodSelect, "Flood", ""),
    ];
    egui::Grid::new("tools")
        .num_columns(2)
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for (i, (tool, label, key)) in TOOLS.iter().enumerate() {
                let tip = if key.is_empty() {
                    (*label).to_string()
                } else {
                    format!("{label} ({key})")
                };
                let on = session.tool == *tool;
                let resp = ui
                    .add_sized([86.0, 26.0], egui::Button::new(*label).selected(on))
                    .on_hover_text(tip);
                let rect = resp.rect;
                let clicked = resp.clicked();
                let id = format!("tool.{}", label.to_ascii_lowercase().replace(' ', "-"));
                session.note_ui(&id, label, rect, UiAction::Tool(*tool));
                if clicked {
                    session.tool = *tool;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });

    section(ui, "Mode");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (name, mode) in [
            ("Add", ToolMode::Add),
            ("Remove", ToolMode::Remove),
            ("Paint", ToolMode::Paint),
        ] {
            let resp = ui.selectable_value(&mut session.tool_mode, mode, name);
            let rect = resp.rect;
            session.note_ui(
                &format!("mode.{}", name.to_ascii_lowercase()),
                name,
                rect,
                UiAction::Mode(mode),
            );
        }
    });

    if session.tool != Tool::Face && session.tool != Tool::Generator {
        section(ui, "Brush");
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
        ui.horizontal(|ui| {
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
        ui.label(RichText::new(summary).small().monospace().color(DIM));
    }

    match session.tool {
        Tool::Line => {
            section(ui, "Line");
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
            section(ui, "Circle");
            ui.horizontal(|ui| {
                ui.toggle_value(&mut session.circle_fill, "Fill");
                ui.toggle_value(&mut session.circle_even, "Even");
            });
        }
        Tool::Square => {
            section(ui, "Square");
            ui.horizontal(|ui| {
                ui.toggle_value(&mut session.square_fill, "Fill");
                ui.toggle_value(&mut session.square_even, "Even");
            });
        }
        Tool::Box => {
            section(ui, "Box");
            ui.toggle_value(&mut session.box_fill, "Fill");
        }
        Tool::Face => {
            section(ui, "Face");
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
            ui.horizontal(|ui| {
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
            section(ui, "Generator");
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
        Tool::Mutator => {
            section(ui, "Mutator");
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
            ui.horizontal(|ui| {
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
            ui.horizontal(|ui| {
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
