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

/// Widths the Inspector panel can be dragged to. Every Inspector row must
/// fit the narrowest (`tests/panels.rs`).
pub const INSPECTOR_WIDTH: std::ops::RangeInclusive<f32> = 280.0..=400.0;

/// The Inspector's side panel, as the window shows it.
pub fn inspector_panel(style: &egui::Style) -> egui::Panel {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(300.0)
        .size_range(INSPECTOR_WIDTH)
        .frame(side_frame(style))
}

/// Where the docks landed in one frame, in window points. The layout test
/// reads it; the window does not.
#[cfg_attr(not(test), allow(dead_code))]
pub struct Docks {
    pub strip: egui::Rect,
    pub inspector: egui::Rect,
    pub tool_options: egui::Rect,
    pub palette: egui::Rect,
    pub viewport: egui::Rect,
    pub inspector_fit: InspectorFit,
}

/// The tool strip, the Inspector, the tool options, the palette, and the
/// viewport `central` draws, in the window's remaining space. The order
/// matters: the side panels go first so the rows around the viewport span
/// only its width.
pub fn desktop(
    ui: &mut Ui,
    session: &mut Session,
    central: impl FnOnce(&mut Ui, &mut Session),
) -> Docks {
    let style = ui.style().clone();
    let strip = egui::Panel::left("tools")
        .resizable(false)
        .exact_size(STRIP_WIDTH)
        .frame(side_frame(&style))
        .show(ui, |ui| self::strip(ui, session))
        .response
        .rect;
    let inspector = inspector_panel(&style).show(ui, |ui| right(ui, session));
    let (tool_options, palette) = around_viewport(ui, session);
    let viewport = egui::CentralPanel::default()
        .show(ui, |ui| central(ui, session))
        .response
        .rect;
    Docks {
        strip,
        inspector: inspector.response.rect,
        tool_options,
        palette,
        viewport,
        inspector_fit: inspector.inner,
    }
}

/// Settings of the current tool in a row above the viewport, and the
/// palette below it. Call after the side panels so both span only the
/// viewport's width. Returns the two docks' rectangles.
pub fn around_viewport(ui: &mut Ui, session: &mut Session) -> (egui::Rect, egui::Rect) {
    let style = ui.style().clone();
    let tool_options = egui::Panel::top("tool-options")
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
        })
        .response
        .rect;
    let palette = egui::Panel::bottom("palette-dock")
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
        })
        .response
        .rect;
    (tool_options, palette)
}

/// Inspector dock on top, scene tree dock at the bottom, on the dark window
/// background like Godot's docks. Returns how far the widest section
/// reaches past the panel.
pub fn right(ui: &mut Ui, session: &mut Session) -> InspectorFit {
    let mut fit = InspectorFit::default();
    outliner::tree_panel(ui, session);
    dock(
        ui,
        &["Inspector"],
        0,
        true,
        |_| {},
        |ui| {
            // Both axes: a vertical-only scroll area grows to its widest row,
            // which pushes the panel's edge under the viewport. A row too wide
            // for the panel scrolls sideways instead.
            let scroll = egui::ScrollArea::both()
                .id_salt("inspector")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = ui.max_rect().width();
                    for (index, (name, show)) in INSPECTOR_SECTIONS.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let used = ui
                            .scope(|ui| {
                                ui.set_max_width(width);
                                show(ui, session);
                            })
                            .response
                            .rect
                            .width();
                        if used - width > fit.overflow {
                            fit = InspectorFit {
                                overflow: used - width,
                                section: name,
                            };
                        }
                    }
                });
            fit.overflow = fit
                .overflow
                .max(scroll.content_size.x - scroll.inner_rect.width());
        },
    );
    // A drag on a panel field ends with the button; its edits are one undo step.
    if !ui.input(|input| input.pointer.any_down()) {
        session.end_gesture();
    }
    fit
}

/// How far the Inspector's widest section reaches past the panel, and its
/// name; 0 when every row fits.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct InspectorFit {
    pub overflow: f32,
    pub section: &'static str,
}

/// The Inspector's sections, top to bottom, with separators between them.
type Section = fn(&mut Ui, &mut Session);
const INSPECTOR_SECTIONS: &[(&str, Section)] = &[
    ("outliner::object", outliner::object),
    ("outliner::bone", outliner::bone),
    ("outliner::pose", outliner::pose),
    ("outliner::clips", outliner::clips),
    ("script::script", script::script),
    ("render::document", |ui, session| {
        render::document(ui, session)
    }),
    ("render::export", render::export),
    ("render::render", render::render),
    ("render::light", render::light),
    ("render::selection", render::selection),
    ("render::smooth", render::smooth),
    ("render::break_end", render::break_end),
    ("stone::protect", stone::protect),
    ("render::scatter", render::scatter),
    ("stone::relief", stone::relief),
    ("stone::joints", stone::joints),
    ("stone::weather", stone::weather),
    ("stone::bark", stone::bark),
    ("stone::planks", stone::planks),
    ("stone::cavity", stone::cavity),
    ("foliage::foliage", foliage::foliage),
    ("stone::growth", stone::growth),
    ("stone::sweep", stone::sweep),
    ("stone::trim", stone::trim),
    ("organic::loft", organic::loft),
    ("organic::deform", organic::deform),
    ("silhouette::measure", silhouette::measure),
    ("silhouette::pixelate", silhouette::pixelate),
    ("silhouette::carve", silhouette::carve),
    ("silhouette::paint", silhouette::paint),
    ("silhouette::paint_views", silhouette::paint_views),
    ("silhouette::decal", silhouette::decal),
    ("silhouette::fit", silhouette::fit),
    ("silhouette::optimize", silhouette::optimize),
    ("silhouette::orient", silhouette::orient),
    ("silhouette::compare", silhouette::compare),
    ("edit::split", edit::split),
    ("edit::recolor", edit::recolor),
    ("edit::clear", edit::clear),
    ("edit::intersect", edit::intersect),
    ("edit::stroke", edit::stroke),
    ("edit::plates", edit::plates),
    ("edit::components", edit::components),
    ("edit::deterrace", edit::deterrace),
    ("edit::reduce", edit::reduce),
    ("edit::parts", edit::parts),
    ("import::import", import::import),
    ("stone::markers", stone::markers),
];

#[cfg(test)]
#[path = "../../tests/panels.rs"]
mod tests;

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

/// Widgets side by side. In a row that wraps they move to the next line
/// together when they do not fit ([`keep_together`]).
pub(super) fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if ui.layout().main_wrap() {
        let id = ui.next_auto_id();
        keep_together(ui, id, add)
    } else if ui.layout().is_horizontal() {
        add(ui)
    } else {
        ui.horizontal(add).inner
    }
}

/// A group of widgets that wraps as one in a wrapped row: when it does not
/// fit after what is already on the line, it starts the next line. Its
/// width comes from the previous frame. [`row`] uses it in a wrapped
/// layout, so a label never stays behind its slider.
fn keep_together<R>(ui: &mut Ui, id_salt: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.scope_id().with(("keep-together", id_salt));
    let width: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
    let line_started = ui.cursor().left() > ui.max_rect().left() + 0.5;
    if line_started && ui.max_rect().right() - ui.cursor().left() < width {
        ui.end_row();
    }
    let inner = ui.horizontal(add);
    let width = inner.response.rect.width();
    ui.data_mut(|data| data.insert_temp(id, width));
    inner.inner
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
/// tool options row (where [`row`] wraps the name, track, and value as one).
fn slider_label(ui: &mut Ui, label: &str, column: bool) {
    let text = RichText::new(label).small().color(DIM);
    if column {
        ui.add_sized([96.0, 16.0], egui::Label::new(text));
        return;
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
        SliderTarget::TubeStart => session.tube_start,
        SliderTarget::TubeEnd => session.tube_end,
        SliderTarget::TubeSecond => session.tube_second as f32,
        SliderTarget::TubePeriod => session.tube_period,
        SliderTarget::TubeSectors => session.tube_sectors as f32,
        SliderTarget::TubeAspect => session.tube_aspect,
        SliderTarget::TubeRoll => session.tube_roll,
        SliderTarget::SmoothIterations => session.smooth_iterations as f32,
        SliderTarget::SmoothFillAt => session.smooth_fill_at as f32,
        SliderTarget::SmoothRemoveAt => session.smooth_remove_at as f32,
        SliderTarget::ExportAo => session.export.ao_strength,
        SliderTarget::ExportAoRadius => session.export.ao_radius as f32,
        SliderTarget::BakeOcclusion
        | SliderTarget::BakeDistance
        | SliderTarget::BakeRays
        | SliderTarget::BakeSunEnergy
        | SliderTarget::BakeAmbientEnergy
        | SliderTarget::BakeSunSoftness => {
            let bake = session.export.bake.clone().unwrap_or_default();
            match target {
                SliderTarget::BakeOcclusion => bake.occlusion,
                SliderTarget::BakeDistance => bake.distance,
                SliderTarget::BakeRays => bake.rays as f32,
                SliderTarget::BakeSunEnergy => bake.sun_energy,
                SliderTarget::BakeAmbientEnergy => bake.ambient_energy,
                _ => bake.sun_softness,
            }
        }
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

mod edit;
mod foliage;
mod import;
mod organic;
mod outliner;
mod palette;
mod render;
mod script;
mod silhouette;
mod stone;
mod tools;
