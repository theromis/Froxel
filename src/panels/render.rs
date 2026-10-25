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
    ];
    for (id, label, on, flag) in options {
        action_checkbox(ui, session, id, label, on, UiAction::Export(flag));
    }
    if ui.button("Export mesh…").clicked() {
        session.export_mesh_dialog();
    }
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
