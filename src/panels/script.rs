//! Script editor for the active object.
//!
//! The source is stored as it is typed. Parameters come from the last
//! successful run. Regenerate is explicit unless Auto is on, in which case
//! a parameter edit reruns after a short pause. Runs happen on a worker
//! thread; the panel shows the elapsed time and a Cancel button meanwhile.

use std::time::Duration;

use egui::{Color32, RichText, Slider, TextEdit, Ui};

use super::*;
use crate::script::{ParamValue, ScriptParam};

const DEBOUNCE: f64 = 0.35;

pub(super) fn script(ui: &mut Ui, session: &mut Session) {
    if let Some((index, due)) = session.script_due {
        let now = ui.input(|input| input.time);
        if now >= due {
            session.script_due = None;
            if session
                .objects
                .get(index)
                .is_some_and(|object| object.script.auto)
            {
                if let Err(err) = session.start_script(index, false) {
                    session.script_error = err;
                }
            }
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }

    section(ui, "Script");
    let index = session.selected;
    let carries = session
        .objects
        .get(index)
        .is_some_and(|object| !object.script.source.is_empty());
    if carries {
        ui.label(
            RichText::new("This object carries a script. It runs when you regenerate.")
                .small()
                .color(DIM),
        );
    }

    let mut source = session
        .objects
        .get(index)
        .map(|object| object.script.source.clone())
        .unwrap_or_default();
    let editor = TextEdit::multiline(&mut source)
        .font(egui::TextStyle::Monospace)
        .code_editor()
        .desired_rows(10)
        .desired_width(f32::INFINITY);
    let resp = ui.add(editor);
    if resp.gained_focus() {
        session.begin_edit();
    }
    if resp.changed() {
        if let Some(object) = session.objects.get_mut(index) {
            object.script.source = source;
        }
    }
    if resp.lost_focus() {
        if let Err(err) = session.start_script(index, true) {
            session.script_error = err;
        }
        session.end_edit();
    }

    let mut auto = session
        .objects
        .get(index)
        .is_some_and(|object| object.script.auto);
    if ui.checkbox(&mut auto, "Auto").changed() {
        session.begin_edit();
        if let Some(object) = session.objects.get_mut(index) {
            object.script.auto = auto;
        }
        session.end_edit();
    }

    let params: Vec<ScriptParam> = session
        .objects
        .get(index)
        .map(|object| object.script.params.clone())
        .unwrap_or_default();
    for param in &params {
        param_row(ui, session, index, param);
    }

    if let Some((running, seconds)) = session.script_running() {
        ui.horizontal(|ui| {
            ui.spinner();
            let what = session
                .objects
                .get(running)
                .map(|object| object.name.as_str())
                .unwrap_or("script");
            ui.label(RichText::new(format!("Running {what}… {seconds:.1} s")).small());
            if ui.button("Cancel").clicked() {
                session.cancel_script();
            }
        });
    } else if ui.button("Regenerate").clicked() {
        session.script_due = None;
        if let Err(err) = session.start_script(index, false) {
            session.script_error = err;
        }
    }
    if !session.script_error.is_empty() {
        ui.label(
            RichText::new(&session.script_error)
                .small()
                .color(Color32::from_rgb(255, 150, 140)),
        );
    }
}

fn param_row(ui: &mut Ui, session: &mut Session, index: usize, param: &ScriptParam) {
    if let ParamValue::Floats {
        values,
        width,
        min,
        max,
        max_len,
    } = &param.value
    {
        list_row(
            ui,
            session,
            index,
            param,
            values,
            *width,
            (*min, *max),
            *max_len,
        );
        return;
    }
    match param.value {
        ParamValue::Float { value, min, max } => {
            let mut shown = value;
            ui.horizontal(|ui| {
                ui.add_sized(
                    [96.0, 16.0],
                    egui::Label::new(RichText::new(&param.name).small().color(DIM)),
                );
                let resp = ui.add(Slider::new(&mut shown, min..=max).show_value(false));
                if resp.changed() {
                    session.begin_edit();
                    session.write_script_float(index, &param.name, shown);
                    schedule(ui, session, index);
                }
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    session.end_edit();
                }
                ui.monospace(format!("{shown:.2}"));
            });
        }
        ParamValue::Int { value, min, max } => {
            let mut shown = value;
            ui.horizontal(|ui| {
                ui.add_sized(
                    [96.0, 16.0],
                    egui::Label::new(RichText::new(&param.name).small().color(DIM)),
                );
                let resp = ui.add(Slider::new(&mut shown, min..=max).show_value(false));
                if resp.changed() {
                    session.begin_edit();
                    session.write_script_int(index, &param.name, shown);
                    schedule(ui, session, index);
                }
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    session.end_edit();
                }
                ui.monospace(format!("{shown}"));
            });
        }
        ParamValue::Bool(value) => {
            let mut shown = value;
            if ui.checkbox(&mut shown, &param.name).changed() {
                session.begin_edit();
                session.write_script_bool(index, &param.name, shown);
                session.end_edit();
                schedule(ui, session, index);
            }
        }
        ParamValue::Color(mut rgb) => {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [96.0, 16.0],
                    egui::Label::new(RichText::new(&param.name).small().color(DIM)),
                );
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    session.begin_edit();
                    session.write_script_color(index, &param.name, rgb);
                    session.end_edit();
                    schedule(ui, session, index);
                }
            });
        }
        ParamValue::Floats { .. } => {}
    }
}

/// A list parameter as one line of text: `1, 2, 3`, or points split by `;`
/// as in `0 4; 8 2`. It is applied when the field loses focus; a value that
/// does not parse, is too long, or is out of range is reported and kept in
/// the field for fixing.
#[allow(clippy::too_many_arguments)]
fn list_row(
    ui: &mut Ui,
    session: &mut Session,
    index: usize,
    param: &ScriptParam,
    values: &[f32],
    width: u8,
    (min, max): (f32, f32),
    max_len: u32,
) {
    let id = ui.make_persistent_id(("script-list", index, &param.name));
    let entries = values.len() / usize::from(width);
    ui.label(
        RichText::new(format!(
            "{}  {entries}/{max_len}, {min}..={max}",
            param.name
        ))
        .small()
        .color(DIM),
    );
    let mut text = ui
        .data_mut(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| format_list(values, width));
    let resp = ui.add(
        TextEdit::singleline(&mut text)
            .id(id.with("edit"))
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY),
    );
    if resp.changed() {
        ui.data_mut(|data| data.insert_temp(id, text.clone()));
    }
    if resp.lost_focus() && ui.data_mut(|data| data.get_temp::<String>(id)).is_some() {
        let applied = parse_list(&text, width).and_then(|parsed| {
            session.begin_edit();
            let result = session.write_script_floats(index, &param.name, parsed);
            session.end_edit();
            result
        });
        match applied {
            Ok(()) => {
                ui.data_mut(|data| data.remove::<String>(id));
                session.script_error.clear();
                schedule(ui, session, index);
            }
            Err(err) => session.script_error = format!("`{}`: {err}", param.name),
        }
    }
}

fn format_list(values: &[f32], width: u8) -> String {
    values
        .chunks(usize::from(width))
        .map(|point| {
            point
                .iter()
                .map(|value| format!("{value}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(if width == 1 { ", " } else { "; " })
}

/// Numbers split by commas, spaces, or `;`. Points are read `width` numbers
/// at a time, so `0 4; 8 2` and `0, 4, 8, 2` are the same two points.
fn parse_list(text: &str, width: u8) -> Result<Vec<f32>, String> {
    let mut out = Vec::new();
    for word in text
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .filter(|word| !word.is_empty())
    {
        let value: f32 = word
            .parse()
            .map_err(|_| format!("`{word}` is not a number"))?;
        if !value.is_finite() {
            return Err(format!("`{word}` is not finite"));
        }
        out.push(value);
    }
    if !out.len().is_multiple_of(usize::from(width)) {
        return Err(format!("expected points of {width} numbers"));
    }
    Ok(out)
}

fn schedule(ui: &Ui, session: &mut Session, index: usize) {
    if session
        .objects
        .get(index)
        .is_some_and(|object| object.script.auto)
    {
        let now = ui.input(|input| input.time);
        session.script_due = Some((index, now + DEBOUNCE));
    }
}
