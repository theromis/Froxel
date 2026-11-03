//! Phosphor glyphs for the phone strip and the desktop tool dock.
//!
//! The family is the vendored subset in `assets/icons` (MIT), installed in
//! `apply_theme`. Hover text stays the English name.

use crate::state::{Tool, ToolMode};

/// Private-use codepoints in the vendored Phosphor subset.
pub mod glyph {
    pub const FOLDER: char = '\u{E24A}';
    pub const CUBE: char = '\u{E1DA}';
    pub const SHAPES: char = '\u{EC5E}';
    pub const PENCIL: char = '\u{E3B4}';
    pub const CAMERA: char = '\u{E10E}';
    pub const BRUSH: char = '\u{E6F0}';
    pub const POLYLINE: char = '\u{E6D2}';
    pub const CIRCLE: char = '\u{E18A}';
    pub const SQUARE: char = '\u{E45E}';
    pub const TEXTURE: char = '\u{E8C4}';
    pub const SPARKLE: char = '\u{E6A2}';
    pub const BUBBLES: char = '\u{E192}';
    pub const MARQUEE: char = '\u{E69A}';
    pub const SELECT_ALL: char = '\u{E746}';
    pub const BUCKET: char = '\u{E392}';
    pub const DROP: char = '\u{E210}';
    pub const ADD: char = '\u{E3D4}';
    pub const REMOVE: char = '\u{E32A}';
    pub const UNDO: char = '\u{E08A}';
    pub const SWAP_H: char = '\u{E0A0}';
    pub const SWAP_V: char = '\u{E098}';
    pub const FLIP: char = '\u{ED6A}';
    pub const OUT: char = '\u{E092}';
}

pub fn font() -> egui::FontId {
    egui::FontId::new(24.0, egui::FontFamily::Name("Phosphor".into()))
}

pub fn tool(tool: Tool) -> char {
    match tool {
        Tool::Brush => glyph::BRUSH,
        Tool::Line => glyph::POLYLINE,
        Tool::Circle => glyph::CIRCLE,
        Tool::Square => glyph::SQUARE,
        Tool::Box => glyph::CUBE,
        Tool::Face => glyph::TEXTURE,
        Tool::Generator => glyph::SPARKLE,
        // The subset has no tube glyph; the desktop strip draws one with
        // [`paint_tool`].
        Tool::Tube => glyph::POLYLINE,
        // The desktop strip draws a dab with [`paint_tool`].
        Tool::Sculpt => glyph::BUBBLES,
        Tool::Mutator => glyph::BUBBLES,
        Tool::Marquee => glyph::MARQUEE,
        Tool::BoxSelect => glyph::SELECT_ALL,
        Tool::FloodSelect => glyph::DROP,
    }
}

pub fn mode(mode: ToolMode) -> char {
    match mode {
        ToolMode::Add => glyph::ADD,
        ToolMode::Remove => glyph::REMOVE,
        ToolMode::Paint => glyph::BUCKET,
    }
}

/// Draw a tool's icon in the middle of a button: its glyph, or for Tube a
/// tapering curve.
pub fn paint_tool(ui: &egui::Ui, response: &egui::Response, tool: Tool, selected: bool) {
    if tool != Tool::Tube && tool != Tool::Sculpt {
        paint(ui, response, self::tool(tool), selected);
        return;
    }
    let color = if selected || response.hovered() {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    if tool == Tool::Sculpt {
        // A dab: a lump rising out of a surface line, inside the brush ring.
        let rect = response.rect.shrink(response.rect.width() * 0.2);
        let c = rect.center();
        let r = rect.width() * 0.5;
        ui.painter()
            .circle_stroke(c, r, egui::Stroke::new(1.5, color));
        let base = c.y + r * 0.35;
        ui.painter().line_segment(
            [
                egui::pos2(c.x - r * 0.8, base),
                egui::pos2(c.x + r * 0.8, base),
            ],
            egui::Stroke::new(2.0, color),
        );
        ui.painter()
            .circle_filled(egui::pos2(c.x, base), r * 0.45, color);
        return;
    }
    let rect = response.rect.shrink(response.rect.width() * 0.22);
    let steps = 12;
    let at = |t: f32| {
        // A quadratic curve from the lower left over the top to the right.
        let a = rect.left_bottom();
        let b = egui::pos2(rect.center().x, rect.top());
        let c = rect.right_center();
        let u = 1.0 - t;
        egui::pos2(
            u * u * a.x + 2.0 * u * t * b.x + t * t * c.x,
            u * u * a.y + 2.0 * u * t * b.y + t * t * c.y,
        )
    };
    for step in 0..steps {
        let t0 = step as f32 / steps as f32;
        let t1 = (step + 1) as f32 / steps as f32;
        let width = 6.0 - 4.5 * t0;
        ui.painter()
            .line_segment([at(t0), at(t1)], egui::Stroke::new(width, color));
        ui.painter().circle_filled(at(t1), width * 0.5, color);
    }
}

/// Draw `glyph` in the middle of a button. Selected or hovered is white.
pub fn paint(ui: &egui::Ui, response: &egui::Response, glyph: char, selected: bool) {
    let color = if selected || response.hovered() {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };
    ui.painter().text(
        response.rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph.to_string(),
        font(),
        color,
    );
}
