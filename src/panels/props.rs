//! Panel widgets drawn from a tool argument's schema property, the way
//! Blender's `layout.prop` draws an RNA property: the label, tooltip,
//! slider range, and digits come from `ops::schema` ([`crate::ops::prop_ui`]),
//! so a panel only says which arguments go where. `docs/op-properties.md`
//! is the design.

use egui::{RichText, Slider, Ui};

use super::{note_slider, row, slider_value, DIM};
use crate::ops::{prop_ui, PropUi};
use crate::state::{Session, SliderTarget};

/// A toggle for a tool argument in [`Session::op_args`]: a boolean, or an
/// enum with `x-ui.on`, which is that item when on and left out (its
/// default) when off.
pub(super) fn op_toggle(ui: &mut Ui, session: &mut Session, tool: &str, key: &str) {
    let prop = prop_ui(tool, key);
    let current = session.op_arg(prop.tool, key);
    let mut on = match prop.on {
        Some(item) => current == item,
        None => current == true,
    };
    if ui
        .toggle_value(&mut on, prop.label)
        .on_hover_text(prop.tooltip())
        .changed()
    {
        let value = match (on, prop.on) {
            (false, _) => None,
            (true, Some(item)) => Some(item.into()),
            (true, None) => Some(true.into()),
        };
        session.set_op_arg(prop.tool, key, value);
    }
}

/// One button per item of an enum argument in [`Session::op_args`],
/// labelled with the item. `none` adds a first button that leaves the
/// argument out (the Tube tool's Solid, which is `palette`, not `palettes`).
pub(super) fn op_choice(
    ui: &mut Ui,
    session: &mut Session,
    tool: &str,
    key: &str,
    none: Option<&str>,
) {
    let prop = prop_ui(tool, key);
    let current = session.op_arg(prop.tool, key);
    let mut picked: Option<Option<&str>> = None;
    row(ui, |ui| {
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 2.0;
        let choices = none
            .map(|label| (label.to_string(), None))
            .into_iter()
            .chain(
                prop.items
                    .iter()
                    .map(|item| (title_case(item), Some(*item))),
            );
        for (label, item) in choices {
            let selected = match item {
                Some(item) => current == item,
                None => current.is_null(),
            };
            if ui
                .selectable_label(selected, label)
                .on_hover_text(prop.tooltip())
                .clicked()
            {
                picked = Some(item);
            }
        }
        ui.spacing_mut().item_spacing.x = gap;
    });
    if let Some(item) = picked {
        session.set_op_arg(prop.tool, key, item.map(Into::into));
    }
}

/// `stripes` as a button label: `Stripes`.
fn title_case(item: &str) -> String {
    let mut chars = item.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A slider for a number argument over its soft range, with the schema's
/// digits (whole steps for an integer), writing `target`.
pub(super) fn op_slider(
    ui: &mut Ui,
    session: &mut Session,
    tool: &str,
    key: &str,
    target: SliderTarget,
) {
    let prop = prop_ui(tool, key);
    slider(ui, session, prop.label, &prop, target);
}

/// [`op_slider`] under another label, for an argument a tool shows more
/// than once (the Tube tool's Start and End radius).
pub(super) fn op_slider_as(
    ui: &mut Ui,
    session: &mut Session,
    tool: &str,
    key: &str,
    label: &str,
    target: SliderTarget,
) {
    let prop = prop_ui(tool, key);
    slider(ui, session, label, &prop, target);
}

fn slider(ui: &mut Ui, session: &mut Session, label: &str, prop: &PropUi, target: SliderTarget) {
    let (min, max) = prop
        .soft
        .unwrap_or_else(|| panic!("`{}` argument `{}` has no soft range", prop.tool, prop.key));
    let (min, max) = (min as f32, max as f32);
    let digits = if prop.integer { 0 } else { prop.digits };
    let mut value = slider_value(session, target);
    if prop.integer {
        value = value.round();
    }
    let column = !ui.layout().is_horizontal();
    row(ui, |ui| {
        let text = RichText::new(label).small().color(DIM);
        let name = if column {
            ui.add_sized([96.0, 16.0], egui::Label::new(text))
        } else {
            ui.add(egui::Label::new(text))
        };
        name.on_hover_text(prop.tooltip());
        let mut slider = Slider::new(&mut value, min..=max).show_value(false);
        if prop.integer {
            slider = slider.integer();
        }
        let resp = ui.add(slider).on_hover_text(prop.tooltip());
        note_slider(session, label, target, resp.rect, min, max);
        if resp.changed() {
            session.write_slider(target, value);
        }
        ui.monospace(format!("{value:.digits$}"));
    });
}

/// A grid row's label for a tool argument a panel struct edits: the
/// schema `title`, small and dim, with the hint as its tooltip.
pub(super) fn prop_label(ui: &mut Ui, tool: &str, key: &str) {
    let prop = prop_ui(tool, key);
    ui.label(RichText::new(prop.label).small().color(DIM))
        .on_hover_text(prop.tooltip());
}

/// A slider over the argument's soft range, with its decimals.
pub(super) fn prop_slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    tool: &str,
    key: &str,
    value: &mut N,
) -> egui::Response {
    let prop = prop_ui(tool, key);
    let (min, max) = soft_range(&prop);
    let mut slider = Slider::new(value, N::from_f64(min)..=N::from_f64(max));
    if let Some(decimals) = prop.decimals {
        slider = slider.max_decimals(decimals);
    }
    ui.add(slider)
}

/// A drag value kept inside the argument's soft range, with its speed and
/// decimals.
pub(super) fn prop_drag<N: egui::emath::Numeric>(
    ui: &mut Ui,
    tool: &str,
    key: &str,
    value: &mut N,
) -> egui::Response {
    let prop = prop_ui(tool, key);
    let (min, max) = soft_range(&prop);
    let mut drag = egui::DragValue::new(value).range(min..=max);
    if let Some(speed) = prop.speed {
        drag = drag.speed(speed);
    }
    if let Some(decimals) = prop.decimals {
        drag = drag.max_decimals(decimals);
    }
    ui.add(drag)
}

/// A checkbox for a flag a panel struct keeps, labelled and explained by
/// the argument it sets.
pub(super) fn prop_check(ui: &mut Ui, tool: &str, key: &str, value: &mut bool) -> egui::Response {
    let prop = prop_ui(tool, key);
    ui.checkbox(value, prop.label).on_hover_text(prop.tooltip())
}

fn soft_range(prop: &PropUi) -> (f64, f64) {
    prop.soft
        .unwrap_or_else(|| panic!("`{}` argument `{}` has no soft range", prop.tool, prop.key))
}
