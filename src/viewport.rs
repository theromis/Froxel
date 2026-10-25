//! Voxel viewport. The mesh is drawn by wgpu into a texture; egui shows that
//! texture and the HUD on top. Picking still uses the CPU ray, with the same
//! camera the shader uses.

use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3, Vec4};

use crate::gpu::{self, SceneGpu, SharedGpu};
use crate::state::{
    Cell, EditorMode, Grab, LineMode, ObjectBand, RenderMode, Session, Tool, ToolDrag, UiAction,
    Volume,
};

pub struct ViewCache {
    gpu: Option<SharedGpu>,
    texture: Option<egui::TextureId>,
    anchor: Option<egui::Pos2>,
    trace: Option<crate::trace_gpu::Tracer>,
    trace_texture: Option<egui::TextureId>,
    trace_epoch: u64,
    /// Two or more fingers are down, or were since the last time none were:
    /// the touch moves the camera and edits nothing.
    touch_gesture: bool,
}

impl ViewCache {
    pub fn new() -> Self {
        Self {
            gpu: None,
            texture: None,
            anchor: None,
            trace: None,
            trace_texture: None,
            trace_epoch: 0,
            touch_gesture: false,
        }
    }

    fn ensure(&mut self, rs: &eframe::egui_wgpu::RenderState) {
        if self.gpu.is_some() {
            return;
        }
        let scene = SceneGpu::new(&rs.device);
        let view = gpu::egui_view(scene.color_texture());
        let id = rs.renderer.write().register_native_texture(
            &rs.device,
            &view,
            eframe::wgpu::FilterMode::Linear,
        );
        let shared = Arc::new(Mutex::new(scene));
        rs.renderer
            .write()
            .callback_resources
            .insert(shared.clone());
        self.gpu = Some(shared);
        self.texture = Some(id);
    }
}

/// `hud` draws the desktop chips. `block` is what the phone paints itself:
/// a press there orbits nothing and edits nothing.
pub fn show(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    session: &mut Session,
    cache: &mut ViewCache,
    hud: bool,
    block: &[egui::Rect],
) -> egui::Rect {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, egui::Sense::click_and_drag());
    session.note_ui("viewport", "Viewport", rect, UiAction::Viewport);

    let left =
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(168.0, 32.0));
    let motion_chip =
        egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 48.0), egui::vec2(118.0, 28.0));
    let right = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 178.0, rect.top() + 10.0),
        egui::vec2(168.0, 32.0),
    );
    let pointer = ctx.input(|i| i.pointer.clone());
    let pointed = pointer.hover_pos().or(pointer.interact_pos());
    let blocked = pointed.is_some_and(|p| block.iter().any(|area| area.contains(p)));
    let on_hud = blocked
        || (hud
            && pointer
                .hover_pos()
                .is_some_and(|p| left.contains(p) || right.contains(p) || motion_chip.contains(p)));
    let hover = pointer.hover_pos().filter(|p| rect.contains(*p) && !on_hud);
    let space = ctx.input(|i| i.key_down(egui::Key::Space));
    let scroll = if hover.is_some() {
        ctx.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };
    // Keep casting while a stroke is dragged past the widget edge.
    // A fast drag can arrive as press plus release, with the end position
    // only on the release frame. Keep aiming at that position.
    let aiming = pointer.button_down(egui::PointerButton::Primary)
        || pointer.button_released(egui::PointerButton::Primary);
    let aim = hover.or_else(|| pointer.latest_pos().filter(|_| aiming));
    let ray = aim.map(|p| ray_through(session, rect, p));
    if pointer.button_pressed(egui::PointerButton::Primary) && hover.is_some() {
        cache.anchor = hover;
    }
    session.hover_gizmo =
        if session.editor_mode == EditorMode::Object && session.render_mode == RenderMode::Edit {
            hover.and_then(|pos| gizmo_under(session, rect, pos))
        } else {
            None
        };
    let shift = ctx.input(|i| i.modifiers.shift);
    let drag = tool_drag(session, rect, cache.anchor, aim, shift);
    let gesturing = touch_gesture(ctx, session, cache, hover.is_some() || pointer.any_down());
    if !gesturing {
        session.pointer(
            hover.is_some(),
            pointer.button_down(egui::PointerButton::Primary),
            pointer.button_pressed(egui::PointerButton::Primary) && hover.is_some(),
            pointer.button_released(egui::PointerButton::Primary),
            pointer.button_down(egui::PointerButton::Secondary),
            pointer.button_pressed(egui::PointerButton::Secondary) && hover.is_some(),
            pointer.button_down(egui::PointerButton::Middle),
            pointer.button_pressed(egui::PointerButton::Middle) && hover.is_some(),
            space,
            glam::Vec2::new(pointer.delta().x, pointer.delta().y),
            scroll,
            ray,
            &drag,
            ctx.input(|i| i.modifiers.command),
        );
    }
    if pointer.button_released(egui::PointerButton::Primary) {
        cache.anchor = None;
    }

    if session.render_mode != RenderMode::Render {
        session.cancel_pending_render("left Render mode");
    }
    let painter = ui.painter().with_clip_rect(rect);
    if session.render_mode == RenderMode::Render
        && paint_gpu(ctx, render_state, session, cache, rect, &painter)
    {
    } else if session.render_mode == RenderMode::Render {
        session.trace_on_gpu = false;
        advance_trace(ctx, session, rect);
        if let Some(image) = &session.render_preview {
            let rgba = crate::trace::pixels(image, session);
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &rgba,
            );
            let handle = ctx.load_texture("froxel-trace", color, egui::TextureOptions::LINEAR);
            painter.image(
                handle.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            painter.text(
                rect.left_bottom() + egui::vec2(12.0, -18.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{} / {}   {}×{}",
                    image.samples,
                    session.max_samples.max(1),
                    image.width,
                    image.height
                ),
                egui::FontId::proportional(13.0),
                egui::Color32::from_rgb(244, 246, 250),
            );
        }
    } else if let Some(rs) = render_state {
        session.trace_on_gpu = false;
        cache.ensure(rs);
        if let (Some(gpu), Some(texture)) = (&cache.gpu, cache.texture) {
            let ppp = ctx.pixels_per_point();
            let width = (rect.width() * ppp).round().max(1.0) as u32;
            let height = (rect.height() * ppp).round().max(1.0) as u32;
            let aspect = rect.width() / rect.height().max(1.0);
            let view_proj = gpu_view_proj(session, aspect);
            let mut scene = gpu.lock().expect("voxel viewport");
            if scene.resize(&rs.device, width, height) {
                let texture_handle = scene.color_texture().clone();
                drop(scene);
                let view = gpu::egui_view(&texture_handle);
                rs.renderer.write().update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    &view,
                    eframe::wgpu::FilterMode::Linear,
                    texture,
                );
                scene = gpu.lock().expect("voxel viewport");
            }
            let time = ctx.input(|input| input.time) as f32;
            scene.sync(&rs.device, &rs.queue, session, view_proj, time);
            drop(scene);
            painter.image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            // prepare() draws the texture before egui samples it. paint() is empty.
            painter.add(gpu::paint_callback(rect));
        }
    } else {
        session.trace_on_gpu = false;
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(22, 24, 28));
    }

    let show_band = pointer.button_down(egui::PointerButton::Primary)
        && ((session.editor_mode == EditorMode::Model && session.tool == Tool::Marquee)
            || session.grab == Grab::ObjectMarquee);
    if show_band {
        if let (Some(a), Some(b)) = (cache.anchor, aim) {
            paint_band(&painter, a, b);
        }
    }

    if session.editor_mode == EditorMode::Object && session.render_mode == RenderMode::Edit {
        paint_object_outlines(ui.painter().with_clip_rect(rect), session, rect);
        paint_gizmo(ui.painter().with_clip_rect(rect), session, rect);
    }
    paint_bones(ui.painter().with_clip_rect(rect), session, rect);

    if session.active().cells.is_empty() {
        let color = egui::Color32::from_rgb(214, 220, 230);
        if hud {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "empty",
                egui::FontId::proportional(14.0),
                color,
            );
        } else {
            let galley = painter.layout(
                "One finger draws. Two orbit and pinch. Three pan.".to_owned(),
                egui::FontId::proportional(15.0),
                color,
                (rect.width() - 32.0).max(40.0),
            );
            painter.galley(rect.center() - galley.size() / 2.0, galley, color);
        }
    }

    if session.motion_preview {
        ctx.request_repaint();
    }
    if matches!(session.hover, Some(_))
        && matches!(
            session.tool,
            Tool::Brush | Tool::Marquee | Tool::BoxSelect | Tool::FloodSelect
        )
    {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if !hud {
        return rect;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(motion_chip), |ui| {
        hud_frame(ui, |ui| {
            ui.checkbox(&mut session.motion_preview, "Motion");
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        hud_frame(ui, |ui| {
            let model = ui.selectable_value(&mut session.editor_mode, EditorMode::Model, "Model");
            let model_rect = model.rect;
            session.note_ui(
                "editor.model",
                "Model",
                model_rect,
                UiAction::Editor(EditorMode::Model),
            );
            let object =
                ui.selectable_value(&mut session.editor_mode, EditorMode::Object, "Object");
            let object_rect = object.rect;
            session.note_ui(
                "editor.object",
                "Object",
                object_rect,
                UiAction::Editor(EditorMode::Object),
            );
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        hud_frame(ui, |ui| {
            let edit = ui.selectable_value(&mut session.render_mode, RenderMode::Edit, "Edit");
            let edit_rect = edit.rect;
            session.note_ui(
                "render.edit",
                "Edit",
                edit_rect,
                UiAction::Render(RenderMode::Edit),
            );
            let render =
                ui.selectable_value(&mut session.render_mode, RenderMode::Render, "Render");
            let render_rect = render.rect;
            session.note_ui(
                "render.render",
                "Render",
                render_rect,
                UiAction::Render(RenderMode::Render),
            );
        });
    });

    rect
}

fn hud_frame(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    let chip = egui::Color32::from_rgb(70, 80, 96);
    let label = egui::Color32::from_rgb(244, 246, 250);
    let accent = egui::Color32::from_rgb(69, 145, 251);
    egui::Frame::NONE
        .fill(chip)
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(176, 188, 206),
        ))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(label);
            visuals.selection.bg_fill = accent;
            visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
            visuals.widgets.inactive.fg_stroke.color = label;
            visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
            visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
            visuals.widgets.noninteractive.fg_stroke.color = label;
            visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(92, 104, 124);
            visuals.widgets.active.weak_bg_fill = accent;
            visuals.widgets.active.bg_fill = accent;
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| body(ui));
        });
}

fn paint_gpu(
    ctx: &egui::Context,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    session: &mut Session,
    cache: &mut ViewCache,
    rect: egui::Rect,
    painter: &egui::Painter,
) -> bool {
    let Some(rs) = render_state else {
        return false;
    };
    let ppp = ctx.pixels_per_point();
    let viewport = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    let size = preview_pixels(viewport, true);
    if cache.trace.is_none() {
        cache.trace = Some(crate::trace_gpu::Tracer::new(&rs.device));
    }
    let frame = cache
        .trace
        .as_mut()
        .expect("tracer")
        .advance(&rs.device, &rs.queue, session, size);
    if !frame.gpu {
        return false;
    }
    session.trace_on_gpu = true;
    if cache.trace_epoch != frame.epoch || cache.trace_texture.is_none() {
        let existing = cache.trace_texture;
        let id = {
            let view = &gpu::egui_view(cache.trace.as_ref().expect("tracer").color_texture());
            if let Some(id) = existing {
                rs.renderer.write().update_egui_texture_from_wgpu_texture(
                    &rs.device,
                    view,
                    eframe::wgpu::FilterMode::Linear,
                    id,
                );
                id
            } else {
                rs.renderer.write().register_native_texture(
                    &rs.device,
                    view,
                    eframe::wgpu::FilterMode::Linear,
                )
            }
        };
        cache.trace_texture = Some(id);
        cache.trace_epoch = frame.epoch;
    }
    if let Some(id) = cache.trace_texture {
        painter.image(
            id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        painter.text(
            rect.left_bottom() + egui::vec2(12.0, -18.0),
            egui::Align2::LEFT_BOTTOM,
            format!(
                "{} / {}   {}×{}",
                frame.samples,
                session.max_samples.max(1),
                frame.size[0],
                frame.size[1]
            ),
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgb(244, 246, 250),
        );
    }
    if frame.repaint {
        ctx.request_repaint();
    }
    if crate::denoise::available() {
        if let Err(err) = cache
            .trace
            .as_mut()
            .expect("tracer")
            .finish_oidn(&rs.device, &rs.queue, session)
        {
            session.status = err;
        }
    }
    if let Some(path) = session.take_finished_render(frame.samples) {
        match cache
            .trace
            .as_ref()
            .expect("tracer")
            .save_png(&rs.device, &rs.queue, &path, session)
        {
            Ok(()) => {
                let file = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                session.status = format!("Saved render to {file}.");
            }
            Err(err) => session.status = err,
        }
    }
    true
}

/// The on-screen render uses the panel's own pixels, so the model is the same
/// size as in the edit view. A fixed 960×800 stretched into the panel is not.
fn preview_pixels(viewport: [u32; 2], gpu: bool) -> [u32; 2] {
    crate::trace::view_size(viewport, if gpu { 1280 } else { 192 })
}

fn advance_trace(ctx: &egui::Context, session: &mut Session, rect: egui::Rect) {
    let ppp = ctx.pixels_per_point();
    let viewport = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    let size = preview_pixels(viewport, false);
    let key = crate::trace::frame_key(session, size);
    let size_mismatch = session
        .render_preview
        .as_ref()
        .is_none_or(|image| !image.matches(size[0], size[1]));
    let fresh = size_mismatch || session.render_key != key;
    if fresh {
        session.render_preview = Some(crate::trace::Image::new(size[0], size[1]));
        session.render_key = key;
    }
    let cap = session.render_sample_cap();
    let mut image = session.render_preview.take().expect("preview");
    if image.samples < cap {
        crate::trace::trace_rows(&mut image, session, 6);
        ctx.request_repaint();
    }
    let samples = image.samples;
    session.render_preview = Some(image);
    if let Some(path) = session.take_finished_render(samples) {
        let image = session.render_preview.clone().expect("preview");
        session.write_saved_render(&path, &image);
    }
}

/// OpenGL-style projection. The ray caster unprojects with this matrix.
fn projection(aspect: f32) -> Mat4 {
    Mat4::perspective_rh(50_f32.to_radians(), aspect.max(0.05), 0.08, 4000.0)
}

fn view_matrix(session: &Session) -> Mat4 {
    let eye = session.eye();
    let forward = (session.target - eye).normalize_or_zero();
    let up = if forward.dot(Vec3::Z).abs() > 0.95 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    Mat4::look_at_rh(eye, session.target, up)
}

/// Same view as [`projection`], with z mapped into wgpu's 0..1 clip range.
fn gpu_view_proj(session: &Session, aspect: f32) -> Mat4 {
    let clip = Mat4::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 1.0),
    );
    clip * projection(aspect) * view_matrix(session)
}

/// Two fingers orbit and pinch to zoom, three or more pan. A stroke that the
/// first finger began is undone when the second lands, and nothing edits
/// again until every finger has lifted. Returns true while that lasts.
fn touch_gesture(
    ctx: &egui::Context,
    session: &mut Session,
    cache: &mut ViewCache,
    inside: bool,
) -> bool {
    let touch = ctx.multi_touch().filter(|_| inside || cache.touch_gesture);
    if let Some(touch) = touch {
        if !cache.touch_gesture {
            session.cancel_stroke();
            cache.anchor = None;
            cache.touch_gesture = true;
        }
        let delta = glam::Vec2::new(touch.translation_delta.x, touch.translation_delta.y);
        if touch.num_touches >= 3 {
            session.touch_view(glam::Vec2::ZERO, touch.zoom_delta, delta);
        } else {
            session.touch_view(delta, touch.zoom_delta, glam::Vec2::ZERO);
        }
        ctx.request_repaint();
        return true;
    }
    if cache.touch_gesture && !ctx.input(|i| i.pointer.any_down() || i.any_touches()) {
        cache.touch_gesture = false;
    }
    cache.touch_gesture
}

fn tool_drag(
    session: &Session,
    rect: egui::Rect,
    anchor: Option<egui::Pos2>,
    aim: Option<egui::Pos2>,
    shift: bool,
) -> ToolDrag {
    let Some(anchor) = anchor else {
        return ToolDrag {
            rays: Vec::new(),
            face_steps: 1,
            marquee: None,
            shift,
            object_band: None,
        };
    };
    let Some(aim) = aim else {
        return ToolDrag {
            rays: Vec::new(),
            face_steps: 1,
            marquee: None,
            shift,
            object_band: None,
        };
    };
    let face_steps = if let Some((cell, axis, sign)) = session.stroke_face() {
        face_drag_steps(session, rect, anchor, aim, cell, axis, sign)
    } else {
        1
    };
    let rays = if session.tool == Tool::Line && session.line_mode == LineMode::Project {
        sample_rays(session, rect, anchor, aim)
    } else {
        Vec::new()
    };
    let marquee = if session.tool == Tool::Marquee {
        Some(marquee_cells(session, rect, anchor, aim))
    } else {
        None
    };
    let object_band = if session.editor_mode == EditorMode::Object {
        Some(objects_in_rect(session, rect, anchor, aim))
    } else {
        None
    };
    ToolDrag {
        rays,
        face_steps,
        marquee,
        shift,
        object_band,
    }
}

fn paint_band(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2) {
    let band = egui::Rect::from_two_pos(a, b);
    painter.rect_filled(
        band,
        0.0,
        egui::Color32::from_rgba_unmultiplied(255, 220, 80, 28),
    );
    painter.rect_stroke(
        band,
        0.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 220, 80)),
        egui::StrokeKind::Inside,
    );
}

fn objects_in_rect(
    session: &Session,
    rect: egui::Rect,
    a: egui::Pos2,
    b: egui::Pos2,
) -> ObjectBand {
    let min_x = a.x.min(b.x);
    let max_x = a.x.max(b.x);
    let min_y = a.y.min(b.y);
    let max_y = a.y.max(b.y);
    if max_x - min_x < 2.0 && max_y - min_y < 2.0 {
        return ObjectBand {
            click: true,
            hits: Vec::new(),
        };
    }
    let mut hits = Vec::new();
    for (index, volume) in session.objects.iter().enumerate() {
        let Some((sx0, sy0, sx1, sy1)) = screen_box(session, rect, volume) else {
            continue;
        };
        if sx0 <= max_x && sx1 >= min_x && sy0 <= max_y && sy1 >= min_y {
            hits.push(index);
        }
    }
    ObjectBand { click: false, hits }
}

fn screen_box(
    session: &Session,
    rect: egui::Rect,
    volume: &Volume,
) -> Option<(f32, f32, f32, f32)> {
    let (min, dims) = crate::state::resolved_box(volume)?;
    let shift = volume.translation;
    let lo = Vec3::new(
        (min.0 + shift.0) as f32,
        (min.1 + shift.1) as f32,
        (min.2 + shift.2) as f32,
    );
    let hi = Vec3::new(
        (min.0 + shift.0 + dims[0]) as f32,
        (min.1 + shift.1 + dims[1]) as f32,
        (min.2 + shift.2 + dims[2]) as f32,
    );
    let mut sx0 = f32::MAX;
    let mut sy0 = f32::MAX;
    let mut sx1 = f32::MIN;
    let mut sy1 = f32::MIN;
    let mut any = false;
    for corner in 0..8 {
        let point = Vec3::new(
            if corner & 1 == 0 { lo.x } else { hi.x },
            if corner & 2 == 0 { lo.y } else { hi.y },
            if corner & 4 == 0 { lo.z } else { hi.z },
        );
        let Some(screen) = project_point(session, rect, point) else {
            continue;
        };
        sx0 = sx0.min(screen.x);
        sy0 = sy0.min(screen.y);
        sx1 = sx1.max(screen.x);
        sy1 = sy1.max(screen.y);
        any = true;
    }
    any.then_some((sx0, sy0, sx1, sy1))
}

fn paint_object_outlines(painter: egui::Painter, session: &Session, rect: egui::Rect) {
    for index in &session.object_selection {
        let Some(volume) = session.objects.get(*index) else {
            continue;
        };
        let primary = *index == session.selected;
        let color = if primary {
            egui::Color32::from_rgb(255, 214, 90)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 214, 90, 170)
        };
        let width = if primary { 1.5_f32 } else { 1.0 };
        paint_box(
            &painter,
            session,
            rect,
            volume,
            egui::Stroke::new(width, color),
        );
    }
}

fn paint_box(
    painter: &egui::Painter,
    session: &Session,
    rect: egui::Rect,
    volume: &Volume,
    stroke: egui::Stroke,
) {
    let Some((min, dims)) = crate::state::resolved_box(volume) else {
        return;
    };
    let shift = volume.translation;
    let lo = [
        (min.0 + shift.0) as f32,
        (min.1 + shift.1) as f32,
        (min.2 + shift.2) as f32,
    ];
    let hi = [
        (min.0 + shift.0 + dims[0]) as f32,
        (min.1 + shift.1 + dims[1]) as f32,
        (min.2 + shift.2 + dims[2]) as f32,
    ];
    let corner = |mask: u8| {
        project_point(
            session,
            rect,
            Vec3::new(
                if mask & 1 == 0 { lo[0] } else { hi[0] },
                if mask & 2 == 0 { lo[1] } else { hi[1] },
                if mask & 4 == 0 { lo[2] } else { hi[2] },
            ),
        )
    };
    for axis in 0..3u8 {
        let bit = 1u8 << axis;
        for mask in 0..8u8 {
            if mask & bit != 0 {
                continue;
            }
            let Some(a) = corner(mask) else { continue };
            let Some(b) = corner(mask | bit) else {
                continue;
            };
            painter.line_segment([a, b], stroke);
        }
    }
}

fn marquee_cells(session: &Session, rect: egui::Rect, a: egui::Pos2, b: egui::Pos2) -> Vec<Cell> {
    let min_x = a.x.min(b.x);
    let max_x = a.x.max(b.x);
    let min_y = a.y.min(b.y);
    let max_y = a.y.max(b.y);
    // A click, not a drag, clears the selection.
    if max_x - min_x < 2.0 && max_y - min_y < 2.0 {
        return Vec::new();
    }
    let aspect = rect.width() / rect.height().max(1.0);
    let vp = projection(aspect) * view_matrix(session);
    let mut picked = Vec::new();
    for cell in session.active().cells.keys().copied() {
        let center = Vec3::new(
            cell.0 as f32 + 0.5,
            cell.1 as f32 + 0.5,
            cell.2 as f32 + 0.5,
        );
        let Some(ndc) = to_ndc(vp, center) else {
            continue;
        };
        let sx = rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width();
        let sy = rect.min.y + (1.0 - (ndc.y * 0.5 + 0.5)) * rect.height();
        if sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y {
            picked.push(cell);
        }
    }
    picked
}

fn sample_rays(
    session: &Session,
    rect: egui::Rect,
    a: egui::Pos2,
    b: egui::Pos2,
) -> Vec<(Vec3, Vec3)> {
    let steps = ((a.x - b.x).abs().max((a.y - b.y).abs()) * 2.0).clamp(1.0, 256.0) as i32;
    let mut rays = Vec::with_capacity(steps as usize + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let p = egui::pos2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        rays.push(ray_through(session, rect, p));
    }
    rays
}

fn face_drag_steps(
    session: &Session,
    rect: egui::Rect,
    anchor: egui::Pos2,
    aim: egui::Pos2,
    cell: crate::state::Cell,
    axis: i32,
    sign: i32,
) -> i32 {
    let aspect = rect.width() / rect.height().max(1.0);
    let vp = projection(aspect) * view_matrix(session);
    let center = Vec3::new(
        cell.0 as f32 + 0.5,
        cell.1 as f32 + 0.5,
        cell.2 as f32 + 0.5,
    );
    let normal = match axis {
        0 => Vec3::new(sign as f32, 0.0, 0.0),
        1 => Vec3::new(0.0, sign as f32, 0.0),
        _ => Vec3::new(0.0, 0.0, sign as f32),
    };
    let Some(a) = to_ndc(vp, center) else {
        return 1;
    };
    let Some(b) = to_ndc(vp, center + normal) else {
        return 1;
    };
    let dir = b - a;
    let len2 = dir.dot(dir);
    if len2 < 1e-8 {
        return 1;
    }
    let drag = glam::Vec2::new(
        (aim.x - anchor.x) / rect.width().max(1.0) * 2.0,
        -(aim.y - anchor.y) / rect.height().max(1.0) * 2.0,
    );
    ((drag.x * dir.x + drag.y * dir.y) / len2).round() as i32
}

fn paint_gizmo(painter: egui::Painter, session: &Session, rect: egui::Rect) {
    let Some(center) = object_center(session) else {
        return;
    };
    let Some(origin) = project_point(session, rect, center) else {
        return;
    };
    let colors = [
        egui::Color32::from_rgb(220, 70, 70),
        egui::Color32::from_rgb(70, 190, 90),
        egui::Color32::from_rgb(70, 140, 230),
    ];
    for axis in 0..3i32 {
        let mut tip = center;
        tip[axis as usize] += 4.0;
        let Some(end) = project_point(session, rect, tip) else {
            continue;
        };
        let hovered = session.hover_gizmo == Some(axis);
        painter.line_segment(
            [origin, end],
            egui::Stroke::new(if hovered { 4.0 } else { 2.5_f32 }, colors[axis as usize]),
        );
    }
}

fn gizmo_under(session: &Session, rect: egui::Rect, pos: egui::Pos2) -> Option<i32> {
    let center = object_center(session)?;
    let origin = project_point(session, rect, center)?;
    let mut best: Option<(f32, i32)> = None;
    for axis in 0..3i32 {
        let mut tip = center;
        tip[axis as usize] += 4.0;
        let Some(end) = project_point(session, rect, tip) else {
            continue;
        };
        let distance = distance_to_segment(pos, origin, end);
        if distance <= 10.0 && best.is_none_or(|(so_far, _)| distance < so_far) {
            best = Some((distance, axis));
        }
    }
    best.map(|(_, axis)| axis)
}

fn object_center(session: &Session) -> Option<Vec3> {
    let (min, max) = session.bounds()?;
    let translation = session.active().translation;
    Some(Vec3::new(
        (min.0 + max.0) as f32 * 0.5 + 0.5 + translation.0 as f32,
        (min.1 + max.1) as f32 * 0.5 + 0.5 + translation.1 as f32,
        (min.2 + max.2) as f32 * 0.5 + 0.5 + translation.2 as f32,
    ))
}

fn paint_bones(painter: egui::Painter, session: &Session, rect: egui::Rect) {
    if session.skeleton.bones.is_empty() {
        return;
    }
    // Joints follow the preview pose; a broken chain falls back to the pivots.
    let joints: Vec<Vec3> = crate::rig::posed_joints(&session.skeleton, &session.pose)
        .map(|joints| joints.into_iter().map(|(point, _)| point).collect())
        .unwrap_or_else(|_| {
            session
                .skeleton
                .bones
                .iter()
                .map(|bone| Vec3::from_array(bone.pivot))
                .collect()
        });
    for (index, joint) in joints.iter().enumerate() {
        let Some(at) = project_point(session, rect, *joint) else {
            continue;
        };
        let selected = session.selected_bone == Some(index);
        let color = if selected {
            egui::Color32::from_rgb(255, 196, 64)
        } else {
            egui::Color32::from_rgb(120, 170, 220)
        };
        if let Some(parent) = session.skeleton.parent_index(index) {
            if let Some(from) = project_point(session, rect, joints[parent]) {
                painter.line_segment(
                    [from, at],
                    egui::Stroke::new(if selected { 2.5_f32 } else { 1.5_f32 }, color),
                );
            }
        }
        painter.circle_filled(at, if selected { 5.0 } else { 3.5 }, color);
    }
}

fn project_point(session: &Session, rect: egui::Rect, point: Vec3) -> Option<egui::Pos2> {
    let aspect = rect.width() / rect.height().max(1.0);
    let ndc = to_ndc(projection(aspect) * view_matrix(session), point)?;
    Some(egui::pos2(
        rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.min.y + (1.0 - (ndc.y * 0.5 + 0.5)) * rect.height(),
    ))
}

fn distance_to_segment(point: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let length = ab.length_sq();
    if length < 1e-4 {
        return (point - a).length();
    }
    let t = ((point - a).dot(ab) / length).clamp(0.0, 1.0);
    (point - (a + ab * t)).length()
}

fn to_ndc(vp: Mat4, p: Vec3) -> Option<glam::Vec2> {
    let c = vp * Vec4::new(p.x, p.y, p.z, 1.0);
    if c.w <= 0.05 {
        return None;
    }
    Some(glam::Vec2::new(c.x / c.w, c.y / c.w))
}

pub(crate) fn ray_through(session: &Session, rect: egui::Rect, pos: egui::Pos2) -> (Vec3, Vec3) {
    let aspect = rect.width() / rect.height().max(1.0);
    let inv = (projection(aspect) * view_matrix(session)).inverse();
    let ndc_x = (pos.x - rect.min.x) / rect.width() * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y - rect.min.y) / rect.height() * 2.0;
    let unproject = |z: f32| {
        let p = inv * Vec4::new(ndc_x, ndc_y, z, 1.0);
        Vec3::new(p.x, p.y, p.z) / p.w
    };
    let a = unproject(0.0);
    let b = unproject(1.0);
    (a, b - a)
}

#[cfg(test)]
#[path = "../tests/viewport.rs"]
mod tests;
