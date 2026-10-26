//! Phone and tablet layout.
//!
//! The viewport fills the screen. A top bar carries the document name and
//! the scene sheet. A bottom strip carries the tool, the mode, the brush
//! size, the colour, and undo, drawn with Phosphor icons. Hover text is
//! the English name. The tool grid, the tool's own settings, the palette,
//! and the files open as sheets joined to that strip; names inside those
//! sheets stay words. One finger edits, two orbit and pinch, three pan
//! (see `viewport`). A long-press on the color swatch, then a tap on a
//! voxel, copies that voxel's palette index. Desktop Alt-click does the
//! same without the long-press.
//!
//! At 700 points and wider the desktop Tools and Palette dock sits on the
//! left, and the strip does not repeat it. Render mode drops the dock so the
//! picture is full width.
//!
//! It is on for iOS and Android builds. `FROXEL_MOBILE_UI=1` turns it on for
//! a desktop build too, to try it with a mouse.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText, Ui, Vec2};

use crate::icons::{self, glyph};
use crate::state::{
    Axis, BrushShape, EditorMode, FaceExtrude, FaceWith, GeneratorKind, LineMode, RenderMode,
    Session, Tool, ToolMode, UiAction,
};
use crate::viewport::{self, ViewCache};

/// Smallest comfortable touch target, in points.
const TOUCH: f32 = 44.0;
const GAP: f32 = 4.0;
const UNDO_W: f32 = 56.0;
/// Portrait phones stay on the strip. A landscape tablet is wide enough for
/// the real tools dock.
const WIDE: f32 = 700.0;

/// Secondary labels, the same quiet blue-gray as the desktop docks.
const DIM: Color32 = Color32::from_rgb(196, 206, 220);

const TOOLS: [Tool; 11] = [
    Tool::Brush,
    Tool::Line,
    Tool::Circle,
    Tool::Square,
    Tool::Box,
    Tool::Face,
    Tool::Generator,
    Tool::Mutator,
    Tool::Marquee,
    Tool::BoxSelect,
    Tool::FloodSelect,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sheet {
    #[default]
    None,
    Tools,
    Options,
    Color,
    Scene,
}

pub struct MobileUi {
    sheet: Sheet,
    /// Object-mode axis buttons flip instead of rotating.
    flip: bool,
    /// A long-press already redid; the release must not undo as well.
    redo_held: bool,
    /// A long-press on the color swatch armed sampling; the release must not
    /// also open the palette.
    sample_held: bool,
    /// Last active slot copied into `recent`.
    tracked_slot: u8,
    recent: Vec<u8>,
    /// Last frame's viewport, so a press there can dismiss a sheet before
    /// this frame lays the panels out.
    viewport: egui::Rect,
}

impl Default for MobileUi {
    fn default() -> Self {
        Self {
            sheet: Sheet::None,
            flip: false,
            redo_held: false,
            sample_held: false,
            tracked_slot: 0,
            recent: Vec::new(),
            viewport: egui::Rect::NOTHING,
        }
    }
}

impl MobileUi {
    /// Phone controls that `activate_ui` cannot run: the sheets, undo, and
    /// object rotate or flip. Returns false when the id belongs to a session
    /// action such as mode or editor.
    pub fn activate(&mut self, id: &str, session: &mut Session) -> bool {
        if id == "mobile.scene" {
            self.sheet = if self.sheet == Sheet::Scene {
                Sheet::None
            } else {
                Sheet::Scene
            };
            return true;
        }
        if id == "mobile.tool" {
            self.sheet = tool_tap(self.sheet);
            return true;
        }
        if id == "mobile.color" {
            self.sheet = if self.sheet == Sheet::Color {
                Sheet::None
            } else {
                Sheet::Color
            };
            return true;
        }
        if id == "mobile.undo" {
            session.undo();
            return true;
        }
        if id == "mobile.flip" {
            self.flip = !self.flip;
            return true;
        }
        if let Some(axis) = axis_of(id) {
            if self.flip {
                session.flip_object(axis);
            } else {
                session.rotate_object(axis, false);
            }
            return true;
        }
        if let Some(slug) = id.strip_prefix("mobile.tools.") {
            if let Some(tool) = tool_from_slug(slug) {
                let (next, sheet) = pick_tool(session.tool, tool);
                session.tool = next;
                self.sheet = sheet;
                return true;
            }
        }
        if let Some(slot) = id.strip_prefix("mobile.palette.") {
            if let Ok(slot) = slot.parse::<u8>() {
                if slot != 0 {
                    session.active_slot = slot;
                    remember_slot(&mut self.recent, slot);
                    self.sheet = Sheet::None;
                }
                return true;
            }
        }
        false
    }
}

pub fn active() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
        || std::env::var_os("FROXEL_MOBILE_UI").is_some()
}

/// Draws the whole frame and returns the viewport's rectangle.
pub fn show(
    ui: &mut Ui,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    session: &mut Session,
    cache: &mut ViewCache,
    mobile: &mut MobileUi,
) -> egui::Rect {
    let ctx = ui.ctx().clone();
    if mobile.tracked_slot != session.active_slot {
        if session.active_slot != 0 {
            remember_slot(&mut mobile.recent, session.active_slot);
        }
        mobile.tracked_slot = session.active_slot;
    }
    dismiss_on_press(&ctx, mobile);
    let full = ctx.viewport_rect();
    let safe = ctx.content_rect();
    let wide = safe.width() >= WIDE;
    if wide && matches!(mobile.sheet, Sheet::Tools | Sheet::Options | Sheet::Color) {
        mobile.sheet = Sheet::None;
    }
    let top_inset = (safe.min.y - full.min.y).max(0.0);
    let bottom_inset = (full.max.y - safe.max.y).max(0.0);
    let side = (safe.min.x - full.min.x)
        .max(full.max.x - safe.max.x)
        .max(0.0);
    let window = ui.visuals().window_fill;
    let panel = ui.visuals().panel_fill;
    let edge = 8.0_f32.max(side);

    egui::Panel::top("mobile_top")
        .resizable(false)
        .show_separator_line(false)
        .frame(fill_frame(window, pad(edge, edge, top_inset + 4.0, 6.0)))
        .show(ui, |ui| title_bar(ui, session, mobile));

    egui::Panel::bottom("mobile_strip")
        .resizable(false)
        .show_separator_line(false)
        .frame(fill_frame(window, pad(edge, edge, 6.0, bottom_inset + 6.0)))
        .show(ui, |ui| strip(ui, session, mobile, wide));

    if mobile.sheet != Sheet::None {
        let max_h = (safe.height() * 0.46).clamp(200.0, 440.0);
        // A bottom panel remembers last frame's height. A scroll area inside
        // it then shrinks to that, so the sheet stays one row forever.
        // The height is fixed up front instead.
        egui::Panel::bottom("mobile_sheet")
            .resizable(false)
            .exact_size(sheet_height(mobile.sheet, max_h))
            .show_separator_line(false)
            .frame(fill_frame(panel, pad(edge, edge, 8.0, 4.0)))
            .show(ui, |ui| sheet(ui, session, mobile, max_h));
    }

    if wide && session.render_mode != RenderMode::Render {
        let frame = crate::panels::side_frame(ui.style());
        egui::Panel::left("tools")
            .resizable(false)
            .exact_size(crate::panels::STRIP_WIDTH)
            .frame(frame)
            .show(ui, |ui| crate::panels::strip(ui, session));
        crate::panels::around_viewport(ui, session);
    }

    let mut rect = egui::Rect::NOTHING;
    egui::CentralPanel::default()
        .frame(egui::Frame::new())
        .show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            let chips = chip_rects(area);
            let block: &[egui::Rect] = if wide { &[] } else { &chips };
            rect = viewport::show(&ctx, ui, render_state, session, cache, false, block);
            if !wide {
                paint_chips(ui, session, chips);
            }
        });
    mobile.viewport = rect;
    rect
}

fn dismiss_on_press(ctx: &egui::Context, mobile: &mut MobileUi) {
    let pos = ctx.input(|input| {
        if input.pointer.primary_pressed() {
            input.pointer.interact_pos().or(input.pointer.hover_pos())
        } else {
            None
        }
    });
    if pos.is_some_and(|at| mobile.viewport.contains(at)) {
        mobile.sheet = Sheet::None;
    }
}

fn fill_frame(fill: Color32, margin: egui::Margin) -> egui::Frame {
    egui::Frame::new().fill(fill).inner_margin(margin)
}

fn pad(left: f32, right: f32, top: f32, bottom: f32) -> egui::Margin {
    egui::Margin {
        left: px(left),
        right: px(right),
        top: px(top),
        bottom: px(bottom),
    }
}

fn px(value: f32) -> i8 {
    value.round().clamp(0.0, 120.0) as i8
}

fn title_bar(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let scene = mobile.sheet == Sheet::Scene;
        if icon_button(
            ui,
            session,
            "mobile.scene",
            "Scene",
            UiAction::None,
            scene,
            Vec2::splat(TOUCH),
            glyph::FOLDER,
        )
        .clicked()
        {
            mobile.sheet = if scene { Sheet::None } else { Sheet::Scene };
        }
        let width = ui.available_width();
        ui.add_sized(
            [width, TOUCH],
            egui::Label::new(RichText::new(session.document_label()).strong()).truncate(),
        );
    });
    if !session.status.is_empty() {
        ui.add(egui::Label::new(RichText::new(&session.status).small().color(DIM)).truncate());
    }
}

fn strip(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, wide: bool) {
    ui.set_min_height(TOUCH);
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(GAP, 0.0);
        if wide {
            editor_icons(ui, session, Vec2::splat(TOUCH));
            render_icons(ui, session, Vec2::splat(TOUCH));
        }
        if session.render_mode == RenderMode::Render {
            samples_save(ui, session);
        } else if session.editor_mode == EditorMode::Object {
            axes(ui, session, mobile);
            undo_button(ui, session, mobile);
        } else {
            if !wide {
                model_tools(ui, session, mobile);
            } else {
                let spare = (ui.available_width() - UNDO_W - GAP).max(0.0);
                ui.add_space(spare);
            }
            undo_button(ui, session, mobile);
        }
    });
}

fn model_tools(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi) {
    let fitted = model_strip((ui.available_width() - 2.0).max(0.0));
    // `fitted.undo` is the width the caller still has to spend on Undo.
    let _reserved = fitted.undo;
    let tool = session.tool;
    let open = matches!(mobile.sheet, Sheet::Tools | Sheet::Options);
    if icon_button(
        ui,
        session,
        "mobile.tool",
        tool.name(),
        UiAction::Tool(tool),
        open,
        Vec2::new(fitted.tool, TOUCH),
        icons::tool(tool),
    )
    .clicked()
    {
        mobile.sheet = tool_tap(mobile.sheet);
    }
    mode_segment(ui, fitted.modes, session);
    if let Some(width) = fitted.size {
        let mut size = session.brush_size;
        stepper(ui, width, &mut size, 1, 63, |_| 1);
        session.brush_size = size;
    }
    color_button(ui, fitted.swatch, session, mobile);
}

fn mode_segment(ui: &mut Ui, width: f32, session: &mut Session) {
    fit(ui, width, |ui| {
        ui.style_mut().spacing.button_padding = egui::vec2(2.0, 2.0);
        let each = (width - GAP * 2.0) / 3.0;
        let size = Vec2::new(each, TOUCH);
        if mode_button(
            ui,
            session,
            "mobile.mode.add",
            "Add",
            ToolMode::Add,
            size,
            icons::mode(ToolMode::Add),
        ) {
            session.tool_mode = ToolMode::Add;
        }
        if mode_button(
            ui,
            session,
            "mobile.mode.remove",
            "Remove",
            ToolMode::Remove,
            size,
            icons::mode(ToolMode::Remove),
        ) {
            session.tool_mode = ToolMode::Remove;
        }
        if mode_button(
            ui,
            session,
            "mobile.mode.paint",
            "Paint",
            ToolMode::Paint,
            size,
            icons::mode(ToolMode::Paint),
        ) {
            session.tool_mode = ToolMode::Paint;
        }
    });
}

fn axes(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi) {
    let width = ui.available_width();
    let each = ((width - UNDO_W - GAP * 4.0) / 4.0).max(1.0);
    let size = Vec2::new(each, TOUCH);
    let flip = mobile.flip;
    for (id, label, axis) in [
        ("mobile.axis.x", "X", Axis::X),
        ("mobile.axis.y", "Y", Axis::Y),
        ("mobile.axis.z", "Z", Axis::Z),
    ] {
        let glyph = match axis {
            Axis::X => glyph::SWAP_H,
            Axis::Y => glyph::SWAP_V,
            Axis::Z => glyph::OUT,
        };
        if icon_button(ui, session, id, label, UiAction::None, false, size, glyph).clicked() {
            if flip {
                session.flip_object(axis);
            } else {
                session.rotate_object(axis, false);
            }
        }
    }
    if icon_button(
        ui,
        session,
        "mobile.flip",
        "Flip",
        UiAction::None,
        flip,
        size,
        glyph::FLIP,
    )
    .clicked()
    {
        mobile.flip = !mobile.flip;
    }
}

fn samples_save(ui: &mut Ui, session: &mut Session) {
    let width = ui.available_width();
    let save = if width >= 360.0 { 120.0 } else { 72.0 };
    let label = width >= 420.0;
    let label_w = if label { 72.0 } else { 0.0 };
    let gaps = GAP * if label { 2.0 } else { 1.0 };
    let step_w = (width - save - label_w - gaps).clamp(1.0, 180.0);
    if label {
        ui.add_sized([label_w, TOUCH], egui::Label::new("Samples"));
    }
    let mut samples = session.max_samples;
    stepper(ui, step_w, &mut samples, 16, 8192, |value| {
        if value >= 1024 {
            256
        } else if value >= 256 {
            64
        } else {
            16
        }
    });
    session.max_samples = samples;
    let save_label = if save >= 120.0 { "Save Render" } else { "Save" };
    if ui
        .add_sized([save, TOUCH], egui::Button::new(save_label))
        .clicked()
    {
        session.save_render();
    }
}

fn stepper(
    ui: &mut Ui,
    width: f32,
    value: &mut i32,
    min: i32,
    max: i32,
    step: impl Fn(i32) -> i32,
) {
    fit(ui, width, |ui| {
        let button = if width >= 116.0 { 40.0 } else { 32.0 };
        let number = (width - button * 2.0 - GAP * 2.0).max(28.0);
        if ui
            .add_sized([button, TOUCH], egui::Button::new("−"))
            .clicked()
        {
            *value = (*value - step(*value)).max(min);
        }
        ui.add_sized(
            [number, TOUCH],
            egui::Label::new(RichText::new(value.to_string()).monospace()),
        );
        if ui
            .add_sized([button, TOUCH], egui::Button::new("+"))
            .clicked()
        {
            *value = (*value + step(*value)).min(max);
        }
    });
}

fn fit(ui: &mut Ui, width: f32, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, TOUCH),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(Vec2::new(width, TOUCH));
            ui.spacing_mut().item_spacing.x = GAP;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            body(ui);
        },
    );
}

fn color_button(ui: &mut Ui, width: f32, session: &mut Session, mobile: &mut MobileUi) {
    let [r, g, b, _] = session.palette[session.active_slot as usize];
    let open = mobile.sheet == Sheet::Color;
    let armed = session.sample_armed;
    let swatch = egui::Button::new("")
        .fill(Color32::from_rgb(r, g, b))
        .stroke(if armed {
            egui::Stroke::new(3.0, Color32::WHITE)
        } else {
            egui::Stroke::NONE
        })
        .selected(open && !armed);
    let tip = if armed {
        "Tap a voxel, or tap here to cancel"
    } else {
        "Color. Long-press, then tap a voxel to pick its color"
    };
    let response = ui.add_sized([width, TOUCH], swatch).on_hover_text(tip);
    session.note_ui(
        "mobile.color",
        "Color",
        response.rect,
        UiAction::Palette(session.active_slot),
    );
    if response.long_touched() && !mobile.sample_held {
        session.sample_armed = true;
        mobile.sample_held = true;
        mobile.sheet = Sheet::None;
        session.status = "Tap a voxel to pick its color.".into();
    }
    if response.clicked() && !mobile.sample_held {
        if session.sample_armed {
            session.sample_armed = false;
            session.status.clear();
        } else {
            mobile.sheet = if open { Sheet::None } else { Sheet::Color };
        }
    }
    if !ui.input(|input| input.pointer.any_down()) {
        mobile.sample_held = false;
    }
}

fn undo_button(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi) {
    let response = icon_button(
        ui,
        session,
        "mobile.undo",
        "Undo",
        UiAction::None,
        false,
        Vec2::new(UNDO_W, TOUCH),
        glyph::UNDO,
    )
    .on_hover_text("Long-press to redo");
    if response.long_touched() && !mobile.redo_held {
        session.redo();
        mobile.redo_held = true;
    }
    if response.clicked() && !mobile.redo_held {
        session.undo();
    }
    if !ui.input(|input| input.pointer.any_down()) {
        mobile.redo_held = false;
    }
}

fn touch(text: &str, on: bool) -> egui::Button<'_> {
    egui::Button::new(text)
        .selected(on)
        .min_size(Vec2::new(TOUCH, TOUCH))
}

fn sheet(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, max_h: f32) {
    ui.set_min_width(ui.available_width());
    match mobile.sheet {
        Sheet::Tools => tools_sheet(ui, session, mobile, max_h),
        Sheet::Options => options_sheet(ui, session, max_h),
        Sheet::Color => color_sheet(ui, session, mobile, max_h),
        Sheet::Scene => scene_sheet(ui, session, mobile, max_h),
        Sheet::None => {}
    }
}

fn sheet_height(sheet: Sheet, max_h: f32) -> f32 {
    let tools = 6.0 * (TOUCH + GAP) + 24.0;
    match sheet {
        Sheet::Tools => tools.min(max_h),
        Sheet::Options => 300.0_f32.min(max_h),
        Sheet::Color | Sheet::Scene => max_h,
        Sheet::None => 0.0,
    }
}

fn scrolling(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show(ui, body);
}

fn tools_sheet(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, _max_h: f32) {
    scrolling(ui, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let each = (ui.available_width() - GAP) / 2.0;
        egui::Grid::new("mobile_tools")
            .spacing(Vec2::splat(GAP))
            .show(ui, |ui| {
                for (index, tool) in TOOLS.iter().enumerate() {
                    let on = session.tool == *tool;
                    let response = ui
                        .add_sized([each, TOUCH], egui::Button::new(tool.name()).selected(on))
                        .on_hover_text(tool.name());
                    session.note_ui(
                        &format!("mobile.tools.{}", tool_slug(*tool)),
                        tool.name(),
                        response.rect,
                        UiAction::Tool(*tool),
                    );
                    if response.clicked() {
                        let (next, sheet) = pick_tool(session.tool, *tool);
                        session.tool = next;
                        mobile.sheet = sheet;
                    }
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
    });
}

fn options_sheet(ui: &mut Ui, session: &mut Session, _max_h: f32) {
    scrolling(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        caption(ui, session.tool.name());
        if let Some(hint) = select_hint(session.tool) {
            caption(ui, hint);
        }
        match session.tool {
            Tool::Generator => generator_options(ui, session),
            Tool::Face => face_options(ui, session),
            _ => brush_options(ui, session),
        }
        match session.tool {
            Tool::Line => {
                caption(ui, "Line");
                choices(
                    ui,
                    &mut session.line_mode,
                    &[
                        ("Straight", LineMode::Straight),
                        ("Project", LineMode::Project),
                    ],
                );
            }
            Tool::Circle => {
                caption(ui, "Circle");
                ui.horizontal(|ui| {
                    flag(ui, &mut session.circle_fill, "Fill");
                    flag(ui, &mut session.circle_even, "Even");
                });
            }
            Tool::Square => {
                caption(ui, "Square");
                ui.horizontal(|ui| {
                    flag(ui, &mut session.square_fill, "Fill");
                    flag(ui, &mut session.square_even, "Even");
                });
            }
            Tool::Box => {
                caption(ui, "Box");
                flag(ui, &mut session.box_fill, "Fill");
            }
            Tool::Mutator => mutator_options(ui, session),
            _ => {}
        }
    });
}

fn brush_options(ui: &mut Ui, session: &mut Session) {
    caption(ui, "Size");
    let mut size = session.brush_size;
    stepper(ui, 160.0, &mut size, 1, 63, |_| 1);
    session.brush_size = size;
    caption(ui, "Shape");
    choices(
        ui,
        &mut session.brush_shape,
        &[
            ("Square", BrushShape::Square),
            ("Circle", BrushShape::Circle),
        ],
    );
    choices(ui, &mut session.brush_3d, &[("2D", false), ("3D", true)]);
}

fn face_options(ui: &mut Ui, session: &mut Session) {
    caption(ui, "Extrude");
    choices(
        ui,
        &mut session.face_extrude,
        &[
            ("Color", FaceExtrude::Color),
            ("Geometry", FaceExtrude::Geometry),
        ],
    );
    caption(ui, "With");
    choices(
        ui,
        &mut session.face_with,
        &[
            ("Geometry", FaceWith::Geometry),
            ("Palette", FaceWith::Palette),
        ],
    );
    caption(ui, "Connectivity");
    choices(
        ui,
        &mut session.face_eight,
        &[("4-side", false), ("8-side", true)],
    );
}

fn generator_options(ui: &mut Ui, session: &mut Session) {
    let kinds = [
        GeneratorKind::Ellipsoid,
        GeneratorKind::RoundedBox,
        GeneratorKind::Cylinder,
        GeneratorKind::Cone,
        GeneratorKind::Pyramid,
        GeneratorKind::Torus,
        GeneratorKind::Perlin,
        GeneratorKind::Tree,
        GeneratorKind::Helix,
        GeneratorKind::Conifer,
    ];
    let each = (ui.available_width() - GAP) / 2.0;
    egui::Grid::new("mobile_generator")
        .spacing(Vec2::splat(GAP))
        .show(ui, |ui| {
            for (index, kind) in kinds.iter().enumerate() {
                let on = session.generator == *kind;
                if ui
                    .add_sized([each, TOUCH], egui::Button::new(kind.name()).selected(on))
                    .clicked()
                {
                    session.generator = *kind;
                }
                if index % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

fn mutator_options(ui: &mut Ui, session: &mut Session) {
    caption(ui, "Foliage seed");
    let mut seed = session.foliage_seed;
    stepper(ui, 160.0, &mut seed, 1, 255, |_| 1);
    session.foliage_seed = seed;
    flag_set(ui, session, "Use active palette slot");
    caption(ui, "Density");
    ui.add(egui::Slider::new(&mut session.foliage_density, 0.0..=1.0));
}

fn flag_set(ui: &mut Ui, session: &mut Session, label: &str) {
    if ui.add(touch(label, false)).clicked() {
        session.foliage_seed = session.active_slot.max(1) as i32;
    }
}

fn choices<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, opts: &[(&str, T)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (name, option) in opts {
            if ui.add(touch(name, *value == *option)).clicked() {
                *value = *option;
            }
        }
    });
}

fn flag(ui: &mut Ui, value: &mut bool, label: &str) {
    if ui.add(touch(label, *value)).clicked() {
        *value = !*value;
    }
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(DIM));
}

fn color_sheet(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, _max_h: f32) {
    if mobile.recent.is_empty() && session.active_slot != 0 {
        remember_slot(&mut mobile.recent, session.active_slot);
    }
    let cell = palette_cell(ui.available_width());
    if !mobile.recent.is_empty() {
        caption(ui, "Recent");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(6.0);
            for slot in mobile.recent.clone() {
                swatch(ui, session, mobile, slot, cell);
            }
        });
    }
    caption(ui, "Palette");
    scrolling(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(6.0);
        let cell = palette_cell(ui.available_width());
        egui::Grid::new("mobile_palette")
            .spacing(Vec2::splat(6.0))
            .show(ui, |ui| {
                // Slot 0 is empty space in .vox, so the grid starts at 1.
                for slot in 1..=255u8 {
                    swatch(ui, session, mobile, slot, cell);
                    if slot % 8 == 0 {
                        ui.end_row();
                    }
                }
            });
    });
}

fn swatch(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, slot: u8, cell: f32) {
    let [r, g, b, _] = session.palette[slot as usize];
    let selected = session.active_slot == slot;
    let button = egui::Button::new("")
        .fill(Color32::from_rgb(r, g, b))
        .stroke(if selected {
            egui::Stroke::new(3.0, Color32::WHITE)
        } else {
            egui::Stroke::NONE
        });
    let response = ui.add_sized([cell, cell], button).on_hover_text("Color");
    session.note_ui(
        &format!("mobile.palette.{slot}"),
        &format!("Color {slot}"),
        response.rect,
        UiAction::Palette(slot),
    );
    if response.clicked() {
        session.active_slot = slot;
        remember_slot(&mut mobile.recent, slot);
        mobile.sheet = Sheet::None;
    }
}

fn scene_sheet(ui: &mut Ui, session: &mut Session, mobile: &mut MobileUi, _max_h: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        if ui.add(touch("New", false)).clicked() {
            session.new_document();
            mobile.sheet = Sheet::None;
        }
        if ui.add(touch("Save", false)).clicked() {
            session.save();
        }
    });
    scrolling(ui, |ui| {
        let saved = vox_files(&documents());
        if !saved.is_empty() {
            caption(ui, "Your files");
            for path in saved {
                if file_row(ui, &path) {
                    session.load_path(&path);
                    mobile.sheet = Sheet::None;
                }
            }
        }
        caption(ui, "Examples");
        for path in examples() {
            if file_row(ui, &path) {
                session.load_path(&path);
                // Examples ship inside the app and are read-only: saving
                // writes a new file to Documents.
                session.path = None;
                mobile.sheet = Sheet::None;
            }
        }
        caption(ui, "Objects");
        for index in 0..session.objects.len() {
            object_row(ui, session, index);
        }
    });
}

fn object_row(ui: &mut Ui, session: &mut Session, index: usize) {
    let hidden = session.objects[index].hidden;
    let name = session.objects[index].name.clone();
    let selected = index == session.selected;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let eye = if hidden { "Show" } else { "Hide" };
        if ui
            .add_sized([72.0, TOUCH], egui::Button::new(eye))
            .clicked()
        {
            if let Err(err) = session.set_object(index, None, None, Some(!hidden)) {
                session.status = err;
            }
        }
        let text = if hidden {
            RichText::new(name).color(DIM)
        } else if selected {
            RichText::new(name).strong()
        } else {
            RichText::new(name)
        };
        if ui
            .add_sized(
                [ui.available_width(), TOUCH],
                egui::Button::new(text).selected(selected),
            )
            .clicked()
        {
            session.select_object(index);
        }
    });
}

fn file_row(ui: &mut Ui, path: &Path) -> bool {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.add_sized([ui.available_width(), TOUCH], egui::Button::new(name))
        .clicked()
}

fn chip_rects(viewport: egui::Rect) -> [egui::Rect; 2] {
    let width = ((viewport.width() - 24.0) / 2.0).clamp(1.0, 176.0);
    let size = Vec2::new(width, 52.0);
    let y = viewport.top() + 8.0;
    let left = egui::Rect::from_min_size(egui::pos2(viewport.left() + 8.0, y), size);
    let right = egui::Rect::from_min_size(egui::pos2(viewport.right() - 8.0 - width, y), size);
    [left, right]
}

fn paint_chips(ui: &mut Ui, session: &mut Session, chips: [egui::Rect; 2]) {
    chip_frame(ui, chips[0], |ui| {
        let each = ((ui.available_width() - 2.0) / 2.0).max(1.0);
        ui.horizontal(|ui| editor_icons(ui, session, Vec2::new(each, TOUCH)));
    });
    chip_frame(ui, chips[1], |ui| {
        let each = ((ui.available_width() - 2.0) / 2.0).max(1.0);
        ui.horizontal(|ui| render_icons(ui, session, Vec2::new(each, TOUCH)));
    });
}

fn chip_frame(ui: &mut Ui, rect: egui::Rect, body: impl FnOnce(&mut Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let fill = ui.visuals().extreme_bg_color;
        let stroke = ui.visuals().widgets.inactive.bg_stroke.color;
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui.spacing_mut().item_spacing.x = 2.0;
                body(ui);
            });
    });
}

struct ModelStrip {
    tool: f32,
    modes: f32,
    size: Option<f32>,
    swatch: f32,
    undo: f32,
}

fn model_strip(width: f32) -> ModelStrip {
    let swatch = 44.0;
    let undo = UNDO_W;
    let size = if width >= 520.0 { Some(116.0) } else { None };
    let gaps = GAP * if size.is_some() { 4.0 } else { 3.0 };
    let rest = (width - gaps - swatch - undo - size.unwrap_or(0.0)).max(0.0);
    // Three mode targets share the wider part of the strip.
    let mut modes = 186.0_f32.min(rest * 0.72);
    let mut tool = rest - modes;
    if tool < 76.0 {
        tool = 76.0_f32.min(rest);
        modes = (rest - tool).max(0.0);
    }
    ModelStrip {
        tool,
        modes,
        size,
        swatch,
        undo,
    }
}

fn icon_button(
    ui: &mut Ui,
    session: &mut Session,
    id: &str,
    label: &str,
    action: UiAction,
    selected: bool,
    size: Vec2,
    glyph: char,
) -> egui::Response {
    let response = ui
        .add_sized(
            size,
            egui::Button::new("").min_size(size).selected(selected),
        )
        .on_hover_text(label);
    icons::paint(ui, &response, glyph, selected);
    session.note_ui(id, label, response.rect, action);
    response
}

fn mode_button(
    ui: &mut Ui,
    session: &mut Session,
    id: &str,
    label: &str,
    mode: ToolMode,
    size: Vec2,
    glyph: char,
) -> bool {
    let on = session.tool_mode == mode;
    icon_button(
        ui,
        session,
        id,
        label,
        UiAction::Mode(mode),
        on,
        size,
        glyph,
    )
    .clicked()
}

fn editor_icons(ui: &mut Ui, session: &mut Session, size: Vec2) {
    let model = session.editor_mode == EditorMode::Model;
    if icon_button(
        ui,
        session,
        "mobile.editor.model",
        "Model",
        UiAction::Editor(EditorMode::Model),
        model,
        size,
        glyph::CUBE,
    )
    .clicked()
    {
        session.editor_mode = EditorMode::Model;
    }
    let object = session.editor_mode == EditorMode::Object;
    if icon_button(
        ui,
        session,
        "mobile.editor.object",
        "Object",
        UiAction::Editor(EditorMode::Object),
        object,
        size,
        glyph::SHAPES,
    )
    .clicked()
    {
        session.editor_mode = EditorMode::Object;
    }
}

fn render_icons(ui: &mut Ui, session: &mut Session, size: Vec2) {
    let edit = session.render_mode == RenderMode::Edit;
    if icon_button(
        ui,
        session,
        "mobile.render.edit",
        "Edit",
        UiAction::Render(RenderMode::Edit),
        edit,
        size,
        glyph::PENCIL,
    )
    .clicked()
    {
        session.render_mode = RenderMode::Edit;
    }
    let render = session.render_mode == RenderMode::Render;
    if icon_button(
        ui,
        session,
        "mobile.render.render",
        "Render",
        UiAction::Render(RenderMode::Render),
        render,
        size,
        glyph::CAMERA,
    )
    .clicked()
    {
        session.render_mode = RenderMode::Render;
    }
}

fn tool_slug(tool: Tool) -> &'static str {
    match tool {
        Tool::Brush => "brush",
        Tool::Line => "line",
        Tool::Circle => "circle",
        Tool::Square => "square",
        Tool::Box => "box",
        Tool::Face => "face",
        Tool::Generator => "generator",
        Tool::Mutator => "mutator",
        Tool::Marquee => "marquee",
        Tool::BoxSelect => "box-select",
        Tool::FloodSelect => "flood",
    }
}

fn tool_from_slug(slug: &str) -> Option<Tool> {
    TOOLS.into_iter().find(|tool| tool_slug(*tool) == slug)
}

fn axis_of(id: &str) -> Option<Axis> {
    match id {
        "mobile.axis.x" => Some(Axis::X),
        "mobile.axis.y" => Some(Axis::Y),
        "mobile.axis.z" => Some(Axis::Z),
        _ => None,
    }
}

fn select_hint(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Marquee => Some("Drag a rectangle."),
        Tool::BoxSelect => Some("Drag a box."),
        Tool::FloodSelect => Some("Select connected cells."),
        _ => None,
    }
}

fn palette_cell(width: f32) -> f32 {
    ((width - 6.0 * 7.0) / 8.0).clamp(32.0, 48.0)
}

/// First tap opens the tool grid, the next opens that tool's settings, and
/// the one after that closes the sheet.
fn tool_tap(sheet: Sheet) -> Sheet {
    match sheet {
        Sheet::Tools => Sheet::Options,
        Sheet::Options => Sheet::None,
        Sheet::None | Sheet::Color | Sheet::Scene => Sheet::Tools,
    }
}

/// Picking the tool that is already current opens its settings. A different
/// tool becomes current and the sheet closes so the stroke can start.
fn pick_tool(current: Tool, picked: Tool) -> (Tool, Sheet) {
    if picked == current {
        (current, Sheet::Options)
    } else {
        (picked, Sheet::None)
    }
}

fn remember_slot(recent: &mut Vec<u8>, slot: u8) {
    if slot == 0 {
        return;
    }
    recent.retain(|kept| *kept != slot);
    recent.insert(0, slot);
    recent.truncate(8);
}

fn vox_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vox"))
        })
        .collect();
    files.sort();
    files
}

/// The app's own folder for saved files.
fn documents() -> PathBuf {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        crate::state::documents_dir()
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        std::env::temp_dir().join("froxel-mobile")
    }
}

/// Example models: next to the executable in an app bundle, or the
/// repository's `examples/` when run from a checkout.
fn examples() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        found = vox_files(&dir);
    }
    if found.is_empty() {
        found = vox_files(Path::new("examples"));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_button_cycles_sheets() {
        assert_eq!(tool_tap(Sheet::None), Sheet::Tools);
        assert_eq!(tool_tap(Sheet::Color), Sheet::Tools);
        assert_eq!(tool_tap(Sheet::Scene), Sheet::Tools);
        assert_eq!(tool_tap(Sheet::Tools), Sheet::Options);
        assert_eq!(tool_tap(Sheet::Options), Sheet::None);
    }

    #[test]
    fn picking_the_current_tool_opens_options() {
        let (same, same_sheet) = pick_tool(Tool::Brush, Tool::Brush);
        assert!(same == Tool::Brush && same_sheet == Sheet::Options);
        let (other, other_sheet) = pick_tool(Tool::Brush, Tool::Box);
        assert!(other == Tool::Box && other_sheet == Sheet::None);
    }

    #[test]
    fn recent_slots_keep_eight_unique() {
        let mut recent = Vec::new();
        for slot in 1..=10 {
            remember_slot(&mut recent, slot);
        }
        assert_eq!(recent, vec![10, 9, 8, 7, 6, 5, 4, 3]);
        remember_slot(&mut recent, 5);
        assert_eq!(recent, vec![5, 10, 9, 8, 7, 6, 4, 3]);
        remember_slot(&mut recent, 0);
        assert_eq!(recent[0], 5);
    }

    #[test]
    fn tool_control_opens_the_tool_sheet() {
        let mut ui = MobileUi::default();
        let mut session = Session::new();
        assert!(ui.activate("mobile.tool", &mut session));
        assert_eq!(ui.sheet, Sheet::Tools);
        assert!(ui.activate("mobile.tool", &mut session));
        assert_eq!(ui.sheet, Sheet::Options);
        assert!(ui.activate("mobile.scene", &mut session));
        assert_eq!(ui.sheet, Sheet::Scene);
        assert!(ui.activate("mobile.color", &mut session));
        assert_eq!(ui.sheet, Sheet::Color);
        assert!(ui.activate("mobile.tools.line", &mut session));
        assert!(session.tool == Tool::Line && ui.sheet == Sheet::None);
        assert!(ui.activate("mobile.tools.line", &mut session));
        assert_eq!(ui.sheet, Sheet::Options);
        assert!(ui.activate("mobile.flip", &mut session));
        assert!(ui.flip);
        assert!(!ui.activate("mobile.editor.model", &mut session));
    }

    #[test]
    fn model_strip_fits_the_width() {
        for width in [320.0, 390.0, 430.0, 520.0, 800.0] {
            let strip = model_strip(width);
            let gaps = GAP * if strip.size.is_some() { 4.0 } else { 3.0 };
            let sum = strip.tool
                + strip.modes
                + strip.size.unwrap_or(0.0)
                + strip.swatch
                + strip.undo
                + gaps;
            assert!(
                (sum - width).abs() < 0.01,
                "{width} laid out {sum}, tool {} modes {}",
                strip.tool,
                strip.modes
            );
            assert!(strip.tool > 0.0 && strip.modes > 0.0);
        }
    }
}
