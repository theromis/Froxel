//! Script editor for the active object.
//!
//! The source is stored as it is typed. Parameters come from the last
//! successful run. Regenerate is explicit unless Auto is on, in which case
//! a parameter edit reruns after a short pause.

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
                if let Err(err) = session.regenerate_script(index) {
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
        let _ = session.redeclare_script(index);
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

    if ui.button("Regenerate").clicked() {
        session.script_due = None;
        if let Err(err) = session.regenerate_script(index) {
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
    }
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
