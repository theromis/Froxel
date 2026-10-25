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
