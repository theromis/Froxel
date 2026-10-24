//! Palette grid and the active color.

use egui::{Color32, RichText, Ui};

use super::*;

/// The active slot and its color picker, kept above the scrolling grid.
pub(super) fn active_color(ui: &mut Ui, session: &mut Session) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Active").small().color(DIM));
        ui.monospace(format!("{}", session.active_slot));
        let mut rgb = session.color(session.active_slot);
        let resp = ui.color_edit_button_srgb(&mut rgb);
        if resp.changed() {
            session.begin_edit();
            session.set_color(session.active_slot, rgb);
        }
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            session.end_edit();
        }
    });
}

pub(super) fn palette(ui: &mut Ui, session: &mut Session) {
    let cell = 14.0;
    let gap = 2.0;
    let pitch = cell + gap;
    let cols = 8i32;
    let rows = 32i32;
    let w = cols as f32 * cell + (cols - 1) as f32 * gap;
    let h = rows as f32 * cell + (rows - 1) as f32 * gap;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(rect);
    let cell_of = |p: egui::Pos2| -> (i32, i32) {
        let col = ((p.x - rect.min.x) / pitch).floor() as i32;
        let row = ((p.y - rect.min.y) / pitch).floor() as i32;
        (col.clamp(0, cols - 1), row.clamp(0, rows - 1))
    };
    for row in 0..rows {
        for col in 0..cols {
            let slot = crate::state::palette_slot_at(col, row);
            let r = egui::Rect::from_min_size(
                egui::pos2(
                    rect.min.x + col as f32 * pitch,
                    rect.min.y + row as f32 * pitch,
                ),
                egui::vec2(cell, cell),
            );
            if slot == 0 {
                continue;
            }
            let c = session.palette[slot as usize];
            painter.rect_filled(
                r,
                1.0,
                Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
            );
            session.note_ui(
                &format!("palette.{slot}"),
                &format!("{slot}"),
                r,
                UiAction::Palette(slot),
            );
            let selected = session.palette_slots.contains(&slot);
            let active = slot == session.active_slot;
            if selected || active {
                painter.rect_stroke(
                    r,
                    1.0,
                    egui::Stroke::new(
                        if active { 1.5_f32 } else { 1.0_f32 },
                        if active {
                            Color32::from_rgb(255, 210, 70)
                        } else {
                            Color32::from_white_alpha(180)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
    if let Some(((c0, r0), (c1, r1))) = session.palette_drag() {
        let min_c = c0.min(c1);
        let max_c = c0.max(c1);
        let min_r = r0.min(r1);
        let max_r = r0.max(r1);
        let band = egui::Rect::from_min_max(
            egui::pos2(
                rect.min.x + min_c as f32 * pitch,
                rect.min.y + min_r as f32 * pitch,
            ),
            egui::pos2(
                rect.min.x + max_c as f32 * pitch + cell,
                rect.min.y + max_r as f32 * pitch + cell,
            ),
        );
        painter.rect_stroke(
            band,
            0.0,
            egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 220, 80)),
            egui::StrokeKind::Outside,
        );
    }
    if resp.secondary_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let (col, row) = cell_of(p);
            session.note_palette_anchor(crate::state::palette_slot_at(col, row));
        }
    }
    let anchor = session.palette_anchor;
    let can_paste_color = session.has_color_clipboard();
    let voxels = !session.picked.is_empty();
    let count = session.palette_action_count(anchor);
    resp.context_menu(|ui| {
        if ui.button("Copy color").clicked() {
            session.copy_palette_color(anchor);
            ui.close();
        }
        if ui
            .add_enabled(can_paste_color, egui::Button::new("Paste color"))
            .clicked()
        {
            session.paste_palette_color(anchor);
            ui.close();
        }
        ui.separator();
        if ui.button("Select voxels by color").clicked() {
            session.select_voxels_by_color(anchor);
            ui.close();
        }
        if ui
            .add_enabled(
                voxels && count == 1,
                egui::Button::new("Paint voxels with selected color"),
            )
            .clicked()
        {
            session.paint_picked_with_color(anchor);
            ui.close();
        }
        if ui
            .add_enabled(
                voxels && count >= 2,
                egui::Button::new("Paint voxels with random selected colors"),
            )
            .clicked()
        {
            session.paint_picked_with_random_colors(anchor);
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(count >= 3, egui::Button::new("Make gradient"))
            .clicked()
        {
            session.make_palette_gradient(anchor);
            ui.close();
        }
    });
    if ui.input(|i| i.pointer.primary_pressed()) && resp.hovered() {
        if let Some(p) = resp.interact_pointer_pos() {
            let shift = ui.input(|i| i.modifiers.shift);
            session.begin_palette_drag(cell_of(p), shift);
        }
    }
    if session.palette_drag().is_some() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            session.update_palette_drag(cell_of(p));
        }
    }
    if session.palette_drag().is_some() && ui.input(|i| i.pointer.primary_released()) {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            session.update_palette_drag(cell_of(p));
        }
        session.finish_palette_drag();
    }
}
