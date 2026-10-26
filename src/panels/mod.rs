//! Side panels: the tool strip on the left, tool options and the palette
//! around the viewport, scene and render settings on the right.

use egui::{Color32, RichText, Slider, Ui};

use crate::state::*;

/// Secondary labels. Light enough to read on the blue-gray panel
/// (`#2c333e`), still quieter than the primary text.
pub(super) const DIM: Color32 = Color32::from_rgb(196, 206, 220);

/// Frame of both side panels: the dark window background the docks sit on.
pub fn side_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style)
        .fill(style.visuals.extreme_bg_color)
        .inner_margin(egui::Margin::same(DOCK_GAP))
}

/// Width of the tool strip panel: one icon plus the frame margins.
pub const STRIP_WIDTH: f32 = tools::STRIP_ICON + 2.0 * (STRIP_PAD + DOCK_GAP as f32);
const STRIP_PAD: f32 = 6.0;

/// The tool strip on the left: tool icons, then the modes.
pub fn strip(ui: &mut Ui, session: &mut Session) {
    let visuals = ui.visuals().clone();
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .corner_radius(DOCK_RADIUS)
        .inner_margin(egui::Margin::same(STRIP_PAD as i8))
        .show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            egui::ScrollArea::vertical()
                .id_salt("tools")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| tools::strip(ui, session));
        });
}

/// Settings of the current tool in a row above the viewport, and the
/// palette below it. Call after the side panels so both span only the
/// viewport's width.
pub fn around_viewport(ui: &mut Ui, session: &mut Session) {
    let style = ui.style().clone();
    egui::Panel::top("tool-options")
        .resizable(false)
        .show_separator_line(false)
        .frame(side_frame(&style).inner_margin(egui::Margin {
            bottom: 0,
            ..egui::Margin::same(DOCK_GAP)
        }))
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(style.visuals.panel_fill)
                .corner_radius(DOCK_RADIUS)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.set_min_height(24.0);
                        tools::options(ui, session);
                    });
                });
        });
    egui::Panel::bottom("palette-dock")
        .resizable(false)
        .show_separator_line(false)
        .frame(side_frame(&style).inner_margin(egui::Margin {
            top: 0,
            ..egui::Margin::same(DOCK_GAP)
        }))
        .show(ui, |ui| {
            dock_row(
                ui,
                &["Palette"],
                0,
                true,
                |_| {},
                |ui| {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| palette::active_color(ui, session));
                        ui.add_space(4.0);
                        palette::palette(ui, session);
                    });
                },
            );
        });
}

/// Inspector dock on top, scene tree dock at the bottom, on the dark window
/// background like Godot's docks.
pub fn right(ui: &mut Ui, session: &mut Session) {
    outliner::tree_panel(ui, session);
    dock(
        ui,
        &["Inspector"],
        0,
        true,
        |_| {},
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("inspector")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    outliner::object(ui, session);
                    ui.separator();
                    outliner::bone(ui, session);
                    ui.separator();
                    outliner::pose(ui, session);
                    ui.separator();
                    outliner::clips(ui, session);
                    ui.separator();
                    script::script(ui, session);
                    ui.separator();
                    render::document(ui, session);
                    ui.separator();
                    render::export(ui, session);
                    ui.separator();
                    render::render(ui, session);
                    ui.separator();
                    render::light(ui, session);
                    ui.separator();
                    render::selection(ui, session);
                });
        },
    );
    // A drag on a panel field ends with the button; its edits are one undo step.
    if !ui.input(|input| input.pointer.any_down()) {
        session.end_gesture();
    }
}

/// Space between docks, where the window background shows through.
const DOCK_GAP: i8 = 6;
const DOCK_RADIUS: u8 = 4;

/// A dock as tall as its body, for a panel sized by its content.
fn dock_row(
    ui: &mut Ui,
    tabs: &[&str],
    selected: usize,
    open: bool,
    trailing: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) -> Option<usize> {
    dock_impl(ui, tabs, selected, open, false, trailing, body)
}

/// A Godot-style dock: a strip of tabs on a darker band, the selected tab
/// joined to the body below it. `trailing` fills the right end of the strip.
/// Without `open` only the strip shows. Returns the tab that was clicked.
pub(super) fn dock(
    ui: &mut Ui,
    tabs: &[&str],
    selected: usize,
    open: bool,
    trailing: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) -> Option<usize> {
    dock_impl(ui, tabs, selected, open, true, trailing, body)
}

fn dock_impl(
    ui: &mut Ui,
    tabs: &[&str],
    selected: usize,
    open: bool,
    fill: bool,
    trailing: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) -> Option<usize> {
    let visuals = ui.visuals().clone();
    let spacing = ui.spacing().item_spacing;
    let mut picked = None;
    egui::Frame::new()
        .fill(visuals.window_fill)
        .corner_radius(DOCK_RADIUS)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (index, name) in tabs.iter().enumerate() {
                    if dock_tab(ui, name, index == selected).clicked() {
                        picked = Some(index);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    trailing(ui);
                });
            });
            if !open {
                return;
            }
            let corners = egui::CornerRadius {
                nw: if selected == 0 { 0 } else { DOCK_RADIUS },
                ne: DOCK_RADIUS,
                sw: DOCK_RADIUS,
                se: DOCK_RADIUS,
            };
            egui::Frame::new()
                .fill(visuals.panel_fill)
                .corner_radius(corners)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = spacing;
                    if fill {
                        ui.set_min_size(ui.available_size());
                    } else {
                        ui.set_min_width(ui.available_width());
                    }
                    body(ui);
                });
        });
    picked
}

fn dock_tab(ui: &mut Ui, name: &str, selected: bool) -> egui::Response {
    let visuals = ui.visuals().clone();
    let color = if selected {
        visuals.strong_text_color()
    } else {
        DIM
    };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(name.to_owned(), font, color);
    let size = galley.size() + egui::vec2(24.0, 12.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if selected {
        visuals.panel_fill
    } else if response.hovered() {
        visuals.faint_bg_color
    } else {
        Color32::TRANSPARENT
    };
    let corners = egui::CornerRadius {
        nw: DOCK_RADIUS,
        ne: DOCK_RADIUS,
        sw: 0,
        se: 0,
    };
    ui.painter().rect_filled(rect, corners, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

pub(super) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title.to_ascii_uppercase()).small().strong());
}

/// A button `ui_click` can press. Both run [`Session::activate_ui`].
pub(super) fn action_button(
    ui: &mut Ui,
    session: &mut Session,
    id: &str,
    label: &str,
    action: UiAction,
) {
    let resp = ui.button(label);
    run_action(session, id, label, resp, action);
}

/// A checkbox `ui_click` can toggle. The action flips the value.
pub(super) fn action_checkbox(
    ui: &mut Ui,
    session: &mut Session,
    id: &str,
    label: &str,
    on: bool,
    action: UiAction,
) {
    let mut shown = on;
    let resp = ui.checkbox(&mut shown, label);
    run_action(session, id, label, resp, action);
}

fn run_action(
    session: &mut Session,
    id: &str,
    label: &str,
    resp: egui::Response,
    action: UiAction,
) {
    session.note_ui(id, label, resp.rect, action);
    if resp.clicked() {
        let region = session.ui_regions.last().cloned().expect("noted above");
        if let Err(err) = session.activate_ui(&region, None) {
            session.status = err;
        }
    }
}

/// Widgets side by side. In a row that wraps they join that row, so each
/// one can wrap; a nested horizontal block cannot.
pub(super) fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if ui.layout().is_horizontal() {
        add(ui)
    } else {
        ui.horizontal(add).inner
    }
}

pub(super) fn segment<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opts: &[(&str, T)]) {
    row(ui, |ui| {
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 2.0;
        for (name, v) in opts {
            ui.selectable_value(value, *v, *name);
        }
        ui.spacing_mut().item_spacing.x = gap;
    });
}

pub(super) fn slider_i32(
    ui: &mut Ui,
    session: &mut Session,
    label: &str,
    target: SliderTarget,
    range: std::ops::RangeInclusive<i32>,
) {
    let mut value = slider_value(session, target).round() as i32;
    let column = !ui.layout().is_horizontal();
    row(ui, |ui| {
        slider_label(ui, label, column);
        let resp = ui.add(Slider::new(&mut value, range.clone()).show_value(false));
        note_slider(
            session,
            label,
            target,
            resp.rect,
            *range.start() as f32,
            *range.end() as f32,
        );
        if resp.changed() {
            session.write_slider(target, value as f32);
        }
        ui.monospace(format!("{value}"));
    });
}

pub(super) fn slider_f32(
    ui: &mut Ui,
    session: &mut Session,
    label: &str,
    target: SliderTarget,
    range: std::ops::RangeInclusive<f32>,
    digits: usize,
) {
    let mut value = slider_value(session, target);
    let column = !ui.layout().is_horizontal();
    row(ui, |ui| {
        slider_label(ui, label, column);
        let resp = ui.add(Slider::new(&mut value, range.clone()).show_value(false));
        note_slider(
            session,
            label,
            target,
            resp.rect,
            *range.start(),
            *range.end(),
        );
        if resp.changed() {
            session.write_slider(target, value);
        }
        ui.monospace(format!("{value:.digits$}"));
    });
}

/// A slider's name: a fixed column in the Inspector, its own width in the
/// tool options row.
fn slider_label(ui: &mut Ui, label: &str, column: bool) {
    let text = RichText::new(label).small().color(DIM);
    if column {
        ui.add_sized([96.0, 16.0], egui::Label::new(text));
        return;
    }
    // Start a new line when the name, track, and value would not fit.
    let font = egui::TextStyle::Small.resolve(ui.style());
    let name = ui.painter().layout_no_wrap(label.to_owned(), font, DIM);
    let need = name.size().x + ui.spacing().slider_width + 56.0;
    if ui.max_rect().right() - ui.cursor().left() < need {
        ui.end_row();
    }
    ui.add(egui::Label::new(text));
}

pub(super) fn slider_value(session: &Session, target: SliderTarget) -> f32 {
    match target {
        SliderTarget::BrushSize => session.brush_size as f32,
        SliderTarget::FoliageSeed => session.foliage_seed as f32,
        SliderTarget::FoliageDensity => session.foliage_density,
        SliderTarget::TorusTube => session.torus_tube,
        SliderTarget::RoundRadius => session.round_radius,
        SliderTarget::NoiseScale => session.noise_scale,
        SliderTarget::NoiseThreshold => session.noise_threshold,
        SliderTarget::NoiseOctaves => session.noise_octaves as f32,
        SliderTarget::NoiseSeed => session.noise_seed as f32,
        SliderTarget::PerlinLo => session.perlin_lo as f32,
        SliderTarget::PerlinHi => session.perlin_hi as f32,
        SliderTarget::TreeDepth => session.tree_depth as f32,
        SliderTarget::TreeWildness => session.tree_wildness,
        SliderTarget::TreeTrunk => session.tree_trunk,
        SliderTarget::TreeLeaf => session.tree_leaf as f32,
        SliderTarget::TreeSeed => session.tree_seed as f32,
        SliderTarget::HelixTurns => session.helix_turns,
        SliderTarget::HelixRadius => session.helix_radius,
        SliderTarget::HelixTube => session.helix_tube,
        SliderTarget::ConiferTiers => session.conifer_tiers as f32,
        SliderTarget::ConiferRagged => session.conifer_ragged,
        SliderTarget::ConiferClump => session.conifer_clump,
        SliderTarget::ConiferTrunk => session.conifer_trunk,
        SliderTarget::ConiferCrown => session.conifer_crown,
        SliderTarget::ConiferNeedle => session.conifer_needle as f32,
        SliderTarget::ConiferSeed => session.conifer_seed as f32,
        SliderTarget::MaxSamples => session.max_samples as f32,
        SliderTarget::Aperture => session.aperture,
        SliderTarget::FocusDistance => session.focus_distance,
        SliderTarget::MaterialRoughness
        | SliderTarget::MaterialMetallic
        | SliderTarget::MaterialTransparency
        | SliderTarget::MaterialIor
        | SliderTarget::MaterialEmission
        | SliderTarget::MaterialPower
        | SliderTarget::MotionAmount => material_shown(session, target),
        SliderTarget::MaxBounces => session.max_bounces as f32,
        SliderTarget::NeeRis => session.nee_ris as f32,
        SliderTarget::IndirectClamp => session.indirect_clamp,
        SliderTarget::EmaFloor => session.ema_floor,
        SliderTarget::SkyIntensity => session.sky_intensity,
        SliderTarget::HorizonSoftness => session.horizon_softness,
        SliderTarget::SunElevation => session.sun_elevation,
        SliderTarget::SunAzimuth => session.sun_azimuth,
        SliderTarget::SunDisc => session.sun_disc,
        SliderTarget::SunIntensity => session.sun_intensity,
    }
}

pub(super) fn note_slider(
    session: &mut Session,
    label: &str,
    target: SliderTarget,
    rect: egui::Rect,
    min: f32,
    max: f32,
) {
    session.note_ui(
        target.id(),
        label,
        rect,
        UiAction::Slider(SliderSpec { target, min, max }),
    );
}

pub(super) fn material_shown(session: &Session, target: SliderTarget) -> f32 {
    let mat = &session.materials[session.active_slot as usize];
    match target {
        SliderTarget::MaterialRoughness => mat.roughness,
        SliderTarget::MaterialMetallic => mat.metallic,
        SliderTarget::MaterialTransparency => mat.transparency,
        SliderTarget::MaterialIor => mat.ior,
        SliderTarget::MaterialEmission => mat.emission * 100.0,
        SliderTarget::MaterialPower => mat.emission_power as f32,
        SliderTarget::MotionAmount => mat.motion_amount,
        _ => 0.0,
    }
}

pub(super) fn material_slider_f32(
    ui: &mut Ui,
    session: &mut Session,
    label: &str,
    target: SliderTarget,
    range: std::ops::RangeInclusive<f32>,
    digits: usize,
) {
    let mut shown = material_shown(session, target);
    let column = !ui.layout().is_horizontal();
    row(ui, |ui| {
        slider_label(ui, label, column);
        let resp = ui.add(Slider::new(&mut shown, range.clone()).show_value(false));
        note_slider(
            session,
            label,
            target,
            resp.rect,
            *range.start(),
            *range.end(),
        );
        if resp.changed() {
            session.begin_edit();
            session.write_slider(target, shown);
        }
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            session.end_edit();
        }
        ui.monospace(format!("{shown:.digits$}"));
    });
}

pub(super) fn material_slider_i32(
    ui: &mut Ui,
    session: &mut Session,
    label: &str,
    target: SliderTarget,
    range: std::ops::RangeInclusive<i32>,
) {
    let mut shown = material_shown(session, target).round() as i32;
    let column = !ui.layout().is_horizontal();
    row(ui, |ui| {
        slider_label(ui, label, column);
        let resp = ui.add(Slider::new(&mut shown, range.clone()).show_value(false));
        note_slider(
            session,
            label,
            target,
            resp.rect,
            *range.start() as f32,
            *range.end() as f32,
        );
        if resp.changed() {
            session.begin_edit();
            session.write_slider(target, shown as f32);
        }
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            session.end_edit();
        }
        ui.monospace(format!("{shown}"));
    });
}

mod outliner;
mod palette;
mod render;
mod script;
mod tools;
