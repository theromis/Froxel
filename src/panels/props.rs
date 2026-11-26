//! Panel widgets drawn from a tool argument's schema property, the way
//! Blender's `layout.prop` draws an RNA property: the label, tooltip,
//! slider range, and digits come from `ops::schema` ([`crate::ops::prop_ui`]),
//! so a panel only says which arguments go where. `docs/op-properties.md`
//! is the design.

use egui::{RichText, Slider, Ui};

use super::{note_slider, row, slider_value, DIM};
use crate::ops::{prop_ui, PropUi};
use crate::state::{Session, SliderTarget};

/// A toggle for a boolean argument or an enum argument with `x-ui.on`:
/// on is that item, off is the default.
pub(super) fn op_toggle(ui: &mut Ui, tool: &str, key: &str, value: &mut bool) -> egui::Response {
    let prop = prop_ui(tool, key);
    ui.toggle_value(value, prop.label)
        .on_hover_text(prop.tooltip())
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
