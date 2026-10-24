//! File and Edit commands shared by the macOS menu bar in `native_menu.rs`
//! and the in-window strip in `menu_bar.rs`. Both menus are built from
//! [`MENUS`], so labels, order, and shortcuts cannot drift apart.

use egui::{Key, KeyboardShortcut, Modifiers};

use crate::state::Session;

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    ExportMesh,
    Close,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    NewObject,
    Duplicate,
    Deselect,
}

pub enum Entry {
    Item(Action),
    OpenRecent,
    Separator,
}

pub struct Menu {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

/// Quit is not listed: on macOS it lives in the application menu, and the
/// in-window strip appends it to File.
pub const MENUS: &[Menu] = &[
    Menu {
        title: "File",
        entries: &[
            Entry::Item(Action::New),
            Entry::Item(Action::Open),
            Entry::OpenRecent,
            Entry::Separator,
            Entry::Item(Action::Save),
            Entry::Item(Action::SaveAs),
            Entry::Separator,
            Entry::Item(Action::ExportMesh),
            Entry::Separator,
            Entry::Item(Action::Close),
        ],
    },
    Menu {
        title: "Edit",
        entries: &[
            Entry::Item(Action::Undo),
            Entry::Item(Action::Redo),
            Entry::Separator,
            Entry::Item(Action::Cut),
            Entry::Item(Action::Copy),
            Entry::Item(Action::Paste),
            Entry::Item(Action::Delete),
            Entry::Item(Action::NewObject),
            Entry::Item(Action::Duplicate),
            Entry::Item(Action::Deselect),
        ],
    },
];

const ALL: [Action; 16] = [
    Action::New,
    Action::Open,
    Action::Save,
    Action::SaveAs,
    Action::ExportMesh,
    Action::Close,
    Action::Quit,
    Action::Undo,
    Action::Redo,
    Action::Cut,
    Action::Copy,
    Action::Paste,
    Action::Delete,
    Action::NewObject,
    Action::Duplicate,
    Action::Deselect,
];

/// True when the macOS application menu carries these commands. It then
/// owns their shortcuts and the window shows no menu strip. Set
/// `FROXEL_NO_NATIVE_MENU` to use the in-window strip instead.
pub fn native() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("FROXEL_NO_NATIVE_MENU").is_none()
}

impl Action {
    #[cfg(target_os = "macos")]
    pub fn id(self) -> &'static str {
        match self {
            Action::New => "new",
            Action::Open => "open",
            Action::Save => "save",
            Action::SaveAs => "save-as",
            Action::ExportMesh => "export",
            Action::Close => "close",
            Action::Quit => "quit",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::Cut => "cut",
            Action::Copy => "copy",
            Action::Paste => "paste",
            Action::Delete => "delete",
            Action::NewObject => "new-object",
            Action::Duplicate => "duplicate",
            Action::Deselect => "deselect",
        }
    }

    #[cfg(target_os = "macos")]
    pub fn from_id(id: &str) -> Option<Action> {
        ALL.into_iter().find(|action| action.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New",
            Action::Open => "Open…",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::ExportMesh => "Export Mesh…",
            Action::Close => "Close",
            Action::Quit => "Quit Froxel",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Delete => "Delete",
            Action::NewObject => "New Object",
            Action::Duplicate => "Duplicate",
            Action::Deselect => "Deselect",
        }
    }

    pub fn shortcut(self) -> KeyboardShortcut {
        let (modifiers, key) = match self {
            Action::New => (CMD, Key::N),
            Action::Open => (CMD, Key::O),
            Action::Save => (CMD, Key::S),
            Action::SaveAs => (CMD_SHIFT, Key::S),
            Action::ExportMesh => (CMD_SHIFT, Key::E),
            Action::Close => (CMD, Key::W),
            Action::Quit => (CMD, Key::Q),
            Action::Undo => (CMD, Key::Z),
            Action::Redo => (CMD_SHIFT, Key::Z),
            Action::Cut => (CMD, Key::X),
            Action::Copy => (CMD, Key::C),
            Action::Paste => (CMD, Key::V),
            Action::Delete => (CMD, Key::Backspace),
            Action::NewObject => (CMD_SHIFT, Key::N),
            Action::Duplicate => (CMD, Key::D),
            Action::Deselect => (CMD_SHIFT, Key::D),
        };
        KeyboardShortcut::new(modifiers, key)
    }

    pub fn run(self, session: &mut Session, ctx: &egui::Context) {
        match self {
            Action::New => session.new_document(),
            Action::Open => session.open_dialog(),
            Action::Save => session.save(),
            Action::SaveAs => session.save_as(),
            Action::ExportMesh => session.export_mesh_dialog(),
            Action::Close | Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Undo => session.undo(),
            Action::Redo => session.redo(),
            Action::Cut => session.cut_picked(),
            Action::Copy => session.copy_picked(),
            Action::Paste => session.paste_clipboard(),
            Action::Delete => session.delete_picked(),
            Action::NewObject => session.new_object(),
            Action::Duplicate => session.duplicate_object(),
            Action::Deselect => session.deselect(),
        }
        ctx.request_repaint();
    }
}

/// Consumes the first menu shortcut pressed this frame. Shortcuts with
/// more modifiers go first, because egui matches Cmd+Z on Cmd+Shift+Z.
pub fn consume_shortcut(input: &mut egui::InputState) -> Option<Action> {
    let mut actions = ALL;
    actions.sort_by_key(|action| std::cmp::Reverse(action.shortcut().modifiers.shift));
    actions
        .into_iter()
        .find(|action| input.consume_shortcut(&action.shortcut()))
}
