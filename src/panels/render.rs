//! Document stats, render, light, and selection.

use egui::{RichText, Ui};

use super::*;

pub(super) fn document(ui: &mut Ui, session: &Session) {
    section(ui, "Document");
    ui.label(RichText::new(session.document_label()).small().color(DIM));
    section(ui, "Statistics");
    if session.voxel_count() == 0 {
        ui.label(RichText::new("empty").color(DIM));
    } else if let Some((x, y, z)) = session.size_xyz() {
        stat(ui, "Voxels", &format_count(session.voxel_count()));
        stat(ui, "Size", &format!("{x} × {y} × {z}"));
    }
}

pub(super) fn stat(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [56.0, 16.0],
            egui::Label::new(RichText::new(label).color(DIM)),
        );
        ui.monospace(value);
    });
}

pub(super) fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Options for File > Export Mesh, the same ones `export_mesh` takes.
pub(super) fn export(ui: &mut Ui, session: &mut Session) {
    section(ui, "Export");
    let options = [
        (
            "export.merge",
            "Merge faces",
            session.export.merge,
            ExportFlag::Merge,
        ),
        (
            "export.vertex-colors",
            "Vertex colors",
            session.export.vertex_colors,
            ExportFlag::VertexColors,
        ),
        (
            "export.hidden",
            "Hidden objects",
            session.export.export_hidden,
            ExportFlag::Hidden,
        ),
        (
            "export.split",
            "One mesh per object",
            session.export.split_objects,
            ExportFlag::Split,
        ),
        (
            "export.atlas",
            "Colour atlas",
            session.export.color_atlas,
            ExportFlag::Atlas,
        ),
    ];
    for (id, label, on, flag) in options {
        action_checkbox(ui, session, id, label, on, UiAction::Export(flag));
    }
    slider_f32(
        ui,
        session,
        "Occlusion",
        SliderTarget::ExportAo,
        0.0..=1.0,
        2,
    );
    ui.add_enabled_ui(session.export.ao_strength > 0.0, |ui| {
        slider_i32(
            ui,
            session,
            "AO radius",
            SliderTarget::ExportAoRadius,
            0..=crate::export::occlusion::MAX_RADIUS as i32,
        );
        ui.horizontal(|ui| {
            ui.add_sized(
                [64.0, 16.0],
                egui::Label::new(RichText::new("AO target").small().color(DIM)),
            );
            let target = &mut session.export.ao_target;
            egui::ComboBox::from_id_salt("ao-target")
                .selected_text(target.name())
                .show_ui(ui, |ui| {
                    for choice in [
                        crate::export::AoTarget::Auto,
                        crate::export::AoTarget::Vertex,
                        crate::export::AoTarget::Texture,
                        crate::export::AoTarget::Both,
                    ] {
                        ui.selectable_value(target, choice, choice.name());
                    }
                });
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Export mesh…").clicked() {
            session.export_mesh_dialog();
        }
        if ui
            .button("Export models…")
            .on_hover_text("Export .vox files on disk to .glb with these options")
            .clicked()
        {
            session.export_models_dialog();
        }
        if ui
            .button("Export variants…")
            .on_hover_text("Bake the script once per parameter set in a variants JSON file and write a .vox, a .glb with these options, and a manifest per set (MCP export_variants)")
            .clicked()
        {
            session.export_variants_dialog();
        }
    });
}

pub(super) fn render(ui: &mut Ui, session: &mut Session) {
    section(ui, "Render");
    ui.checkbox(&mut session.stochastic_gi, "Stochastic Sampling GI")
        .on_hover_text("On: bounce light and sample emissive faces. Off: sun and sky only.");
    ui.horizontal(|ui| {
        ui.add_sized(
            [64.0, 16.0],
            egui::Label::new(RichText::new("Glass").small().color(DIM)),
        );
        egui::ComboBox::from_id_salt("glass")
            .selected_text(match session.glass_mode {
                GlassMode::Probabilistic => "Probabilistic",
                GlassMode::Absorption => "Absorption",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut session.glass_mode,
                    GlassMode::Probabilistic,
                    "Probabilistic",
                );
                ui.selectable_value(&mut session.glass_mode, GlassMode::Absorption, "Absorption");
            });
    });
    slider_i32(ui, session, "Samples", SliderTarget::MaxSamples, 16..=8192);
    section(ui, "Camera");
    ui.checkbox(&mut session.orthographic, "Orthographic")
        .on_hover_text("Parallel projection in the viewport, the tracer, and saved images");
    ui.checkbox(&mut session.aces, "ACES tonemap");
    ui.checkbox(&mut session.denoise, "Denoise")
        .on_hover_text(crate::denoise::hover());
    ui.label(
        egui::RichText::new(crate::denoise::name())
            .small()
            .color(DIM),
    );
    slider_f32(
        ui,
        session,
        "Aperture",
        SliderTarget::Aperture,
        0.0..=3.0,
        2,
    );
    ui.add_enabled_ui(session.aperture > 0.0, |ui| {
        slider_f32(
            ui,
            session,
            "Focus",
            SliderTarget::FocusDistance,
            1.0..=500.0,
            0,
        );
    });
    if ui.button("Save render…").clicked() {
        session.save_render();
    }
    ui.horizontal(|ui| {
        if ui
            .button("Save preview…")
            .on_hover_text("The fast lit raster of the viewport, from this camera angle")
            .clicked()
        {
            session.save_still("render.preview");
        }
        if ui
            .button("Save model view…")
            .on_hover_text("Flat palette colours, no light, from this camera angle")
            .clicked()
        {
            session.save_still("render.flat");
        }
    });
    let show_bones = session.show_bones;
    action_checkbox(
        ui,
        session,
        "render.bones",
        "Bones on saved render",
        show_bones,
        UiAction::ShowBones,
    );

    ui.separator();
    section(ui, "Material");
    ui.label(
        RichText::new(format!("Index {}", session.active_slot))
            .small()
            .color(DIM),
    );
    let mut ty = session.materials[session.active_slot as usize].ty;
    let before = ty;
    egui::ComboBox::from_id_salt("material")
        .selected_text(ty.name())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut ty, MaterialType::Diffuse, "Diffuse");
            ui.selectable_value(&mut ty, MaterialType::Metal, "Metal");
            ui.selectable_value(&mut ty, MaterialType::Emissive, "Emissive");
            ui.selectable_value(&mut ty, MaterialType::Glass, "Glass");
        });
    if ty != before {
        session.begin_edit();
        session.material_mut().ty = ty;
        session.end_edit();
    }
    let mut motion = session.materials[session.active_slot as usize].motion;
    let motion_before = motion;
    egui::ComboBox::from_id_salt("motion")
        .selected_text(motion.label())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut motion, Motion::None, Motion::None.label());
            ui.selectable_value(&mut motion, Motion::Sway, Motion::Sway.label());
            ui.selectable_value(&mut motion, Motion::Bob, Motion::Bob.label());
        });
    if motion != motion_before {
        session.begin_edit();
        session.material_mut().motion = motion;
        session.end_edit();
        session.mesh_epoch = session.mesh_epoch.wrapping_add(1);
    }
    material_slider_f32(
        ui,
        session,
        "Amount",
        SliderTarget::MotionAmount,
        0.0..=4.0,
        2,
    );
    match ty {
        MaterialType::Diffuse => {}
        MaterialType::Metal => {
            material_slider_f32(
                ui,
                session,
                "Roughness",
                SliderTarget::MaterialRoughness,
                0.0..=1.0,
                2,
            );
            material_slider_f32(
                ui,
                session,
                "Metallic",
                SliderTarget::MaterialMetallic,
                0.0..=1.0,
                2,
            );
        }
        MaterialType::Glass => {
            material_slider_f32(
                ui,
                session,
                "Roughness",
                SliderTarget::MaterialRoughness,
                0.0..=1.0,
                2,
            );
            material_slider_f32(
                ui,
                session,
                "Transparency",
                SliderTarget::MaterialTransparency,
                0.0..=1.0,
                2,
            );
            material_slider_f32(ui, session, "IoR", SliderTarget::MaterialIor, 1.0..=3.0, 2);
        }
        MaterialType::Emissive => {
            material_slider_f32(
                ui,
                session,
                "Emission",
                SliderTarget::MaterialEmission,
                0.0..=100.0,
                0,
            );
            material_slider_i32(ui, session, "Power", SliderTarget::MaterialPower, 1..=5);
        }
    }
}

pub(super) fn light(ui: &mut Ui, session: &mut Session) {
    section(ui, "Path tracer");
    slider_i32(ui, session, "Bounces", SliderTarget::MaxBounces, 1..=8);
    ui.add_enabled_ui(session.stochastic_gi, |ui| {
        ui.checkbox(&mut session.power_weighted_nee, "Power-weighted NEE");
        slider_i32(ui, session, "RIS samples", SliderTarget::NeeRis, 1..=32);
    });
    slider_f32(
        ui,
        session,
        "Indirect clamp",
        SliderTarget::IndirectClamp,
        0.0..=32.0,
        1,
    );
    slider_f32(
        ui,
        session,
        "Temporal smooth",
        SliderTarget::EmaFloor,
        0.0..=1.0,
        3,
    );

    section(ui, "Sky");
    ui.checkbox(&mut session.atmospheric_sky, "Atmospheric sky")
        .on_hover_text(
            "Nishita single-scatter sky (Rayleigh and Mie), lit by the sun's colour and direction. Off: a flat horizon gradient.",
        );
    slider_f32(
        ui,
        session,
        "Intensity",
        SliderTarget::SkyIntensity,
        0.0..=4.0,
        2,
    );
    ui.add_enabled_ui(!session.atmospheric_sky, |ui| {
        slider_f32(
            ui,
            session,
            "Horizon",
            SliderTarget::HorizonSoftness,
            0.0..=15.0,
            1,
        );
    });

    section(ui, "Sun");
    slider_f32(
        ui,
        session,
        "Elevation",
        SliderTarget::SunElevation,
        -90.0..=90.0,
        0,
    );
    slider_f32(
        ui,
        session,
        "Azimuth",
        SliderTarget::SunAzimuth,
        0.0..=360.0,
        0,
    );
    slider_f32(
        ui,
        session,
        "Disc size",
        SliderTarget::SunDisc,
        0.1..=30.0,
        1,
    );
    slider_f32(
        ui,
        session,
        "Intensity",
        SliderTarget::SunIntensity,
        0.0..=10.0,
        2,
    );
    ui.horizontal(|ui| {
        ui.add_sized(
            [96.0, 16.0],
            egui::Label::new(RichText::new("Color").small().color(DIM)),
        );
        let mut rgb = [
            (session.sun_color[0] * 255.0) as u8,
            (session.sun_color[1] * 255.0) as u8,
            (session.sun_color[2] * 255.0) as u8,
        ];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            session.sun_color = [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ];
        }
    });
}

pub(super) fn selection(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0;
    let object = session.editor_mode == EditorMode::Object;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Rotate 90°");
        axis_buttons(ui, "CCW", |axis| {
            if object {
                session.rotate_object(axis, false);
            } else {
                session.rotate_selection(axis, false);
            }
        });
        ui.label(RichText::new("CCW (right-hand rule)").small().color(DIM));
        axis_buttons(ui, "CW", |axis| {
            if object {
                session.rotate_object(axis, true);
            } else {
                session.rotate_selection(axis, true);
            }
        });
        ui.label(RichText::new("CW").small().color(DIM));
        ui.separator();
        section(ui, "Flip");
        axis_buttons(ui, "Flip", |axis| {
            if object {
                session.flip_object(axis);
            } else {
                session.flip_selection(axis);
            }
        });
        let scope = if object || session.picked.is_empty() {
            "Whole object"
        } else {
            "Selection"
        };
        ui.label(
            RichText::new(format!("{scope} · {} selected", session.picked.len()))
                .small()
                .color(DIM),
        );
        let hint = if object {
            "Drag empty space to select. X Y Z rotate the selection. Shift flips."
        } else {
            "⌘-drag moves it. Paste lands on the hovered face."
        };
        ui.label(RichText::new(hint).small().color(DIM));
    });
}

/// MCP `smooth`: the majority filter over the selection's box, or the
/// whole active object.
pub(super) fn smooth(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Smooth");
        slider_i32(ui, session, "Passes", SliderTarget::SmoothIterations, 1..=8);
        slider_i32(ui, session, "Fill at", SliderTarget::SmoothFillAt, 1..=27);
        slider_i32(ui, session, "Remove at", SliderTarget::SmoothRemoveAt, -1..=25);
        let scope = if session.picked.is_empty() {
            "Smooth object"
        } else {
            "Smooth selection"
        };
        if ui.button(scope).clicked() {
            session.smooth_picked();
        }
        ui.label(
            RichText::new(
                "Fills pits and removes spikes, keeping colours. It also removes anything one cell thick.",
            )
            .small()
            .color(DIM),
        );
    });
}

/// MCP `break_end`: a jagged break across a plane, with splinters.
pub(super) fn break_end(ui: &mut Ui, session: &mut Session) {
    let enabled = session.voxel_count() > 0 && session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Break");
        let panel = &mut session.break_panel;
        egui::Grid::new("break-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                let label = |ui: &mut Ui, text: &str| {
                    ui.label(RichText::new(text).small().color(DIM));
                };
                label(ui, "Point");
                ui.horizontal(|ui| {
                    for value in &mut panel.point {
                        ui.add(egui::DragValue::new(value).speed(0.25).max_decimals(1));
                    }
                });
                ui.end_row();
                label(ui, "Normal");
                ui.horizontal(|ui| {
                    for value in &mut panel.normal {
                        ui.add(
                            egui::DragValue::new(value)
                                .speed(0.02)
                                .range(-1.0..=1.0)
                                .max_decimals(2),
                        );
                    }
                });
                ui.end_row();
                ui.label("");
                ui.horizontal(|ui| {
                    for (name, normal) in [
                        ("+X", [1.0, 0.0, 0.0]),
                        ("−X", [-1.0, 0.0, 0.0]),
                        ("+Y", [0.0, 1.0, 0.0]),
                        ("−Y", [0.0, -1.0, 0.0]),
                        ("+Z", [0.0, 0.0, 1.0]),
                        ("−Z", [0.0, 0.0, -1.0]),
                    ] {
                        if ui.small_button(name).clicked() {
                            panel.normal = normal;
                        }
                    }
                });
                ui.end_row();
                label(ui, "Depth");
                ui.add(egui::Slider::new(&mut panel.depth, 0.0..=32.0).max_decimals(1));
                ui.end_row();
                label(ui, "Splinters");
                ui.add(egui::Slider::new(&mut panel.splinters, 0..=512));
                ui.end_row();
                label(ui, "Length");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut panel.length[0])
                            .range(1.0..=64.0)
                            .speed(0.2),
                    );
                    ui.add(
                        egui::DragValue::new(&mut panel.length[1])
                            .range(1.0..=64.0)
                            .speed(0.2),
                    );
                });
                ui.end_row();
                label(ui, "Thickness");
                ui.add(egui::Slider::new(&mut panel.thickness, 0.5..=4.0).max_decimals(1));
                ui.end_row();
                label(ui, "Taper");
                ui.add(egui::Slider::new(&mut panel.taper, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Rim bias");
                ui.add(egui::Slider::new(&mut panel.rim_bias, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Bark slot");
                ui.add(egui::DragValue::new(&mut panel.bark).range(0..=255))
                    .on_hover_text("0 keeps the cells' colour");
                ui.end_row();
                label(ui, "Sapwood");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.sapwood)
                        .hint_text("e.g. 11, 12")
                        .desired_width(120.0),
                );
                ui.end_row();
                label(ui, "Tip slot");
                ui.add(egui::DragValue::new(&mut panel.tips).range(0..=255))
                    .on_hover_text("0 uses the sapwood");
                ui.end_row();
                label(ui, "Seed");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui.button("Point at selection").clicked() {
                session.break_point_at_selection();
            }
            if ui.button("Break").clicked() {
                session.break_from_panel();
            }
        });
    });
}

/// MCP `scatter`: small pieces that settle into a heap.
pub(super) fn scatter(ui: &mut Ui, session: &mut Session) {
    let enabled = session.editor_mode == EditorMode::Model;
    ui.add_enabled_ui(enabled, |ui| {
        section(ui, "Scatter");
        let panel = &mut session.scatter_panel;
        egui::Grid::new("scatter-panel")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                let label = |ui: &mut Ui, text: &str| {
                    ui.label(RichText::new(text).small().color(DIM));
                };
                label(ui, "Pieces");
                ui.add(egui::Slider::new(&mut panel.count, 0..=2000));
                ui.end_row();
                label(ui, "Kinds");
                ui.horizontal(|ui| {
                    for (on, name) in panel
                        .kinds
                        .iter_mut()
                        .zip(["Shard", "Chip", "Pebble", "Chunk"])
                    {
                        ui.toggle_value(on, name);
                    }
                });
                ui.end_row();
                label(ui, "Size");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut panel.size[0]).range(1..=16));
                    ui.add(egui::DragValue::new(&mut panel.size[1]).range(1..=16));
                });
                ui.end_row();
                label(ui, "Palettes");
                ui.add(
                    egui::TextEdit::singleline(&mut panel.palettes)
                        .hint_text("active slot")
                        .desired_width(120.0),
                );
                ui.end_row();
                label(ui, "Clump");
                ui.add(egui::Slider::new(&mut panel.clump, 0.0..=1.0).max_decimals(2));
                ui.end_row();
                label(ui, "Seed");
                ui.add(egui::DragValue::new(&mut panel.seed).range(0..=999_999));
                ui.end_row();
            });
        let scope = if session.picked.is_empty() {
            "Scatter over object"
        } else {
            "Scatter over selection"
        };
        if ui.button(scope).clicked() {
            session.scatter_from_panel();
        }
        ui.label(
            RichText::new("Pieces fall from the top of the box and rest on what is below.")
                .small()
                .color(DIM),
        );
    });
}

pub(super) fn axis_buttons(ui: &mut Ui, _which: &str, mut act: impl FnMut(Axis)) {
    ui.horizontal(|ui| {
        if ui.button("X").clicked() {
            act(Axis::X);
        }
        if ui.button("Y").clicked() {
            act(Axis::Y);
        }
        if ui.button("Z").clicked() {
            act(Axis::Z);
        }
    });
}
