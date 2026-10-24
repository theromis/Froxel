//! Menu strip at the top of the window, built from [`menu::MENUS`]. It is
//! hidden while the macOS application menu in `native_menu.rs` carries the
//! same commands. `Session::take_keys` handles the shortcuts it lists, so
//! the items only show them.

use crate::menu::{self, Action, Entry};
use crate::state::Session;

fn item(ui: &mut egui::Ui, session: &mut Session, action: Action) {
    let shortcut = ui.ctx().format_shortcut(&action.shortcut());
    let label = if action == Action::Quit {
        "Quit"
    } else {
        action.label()
    };
    if ui
        .add(egui::Button::new(label).shortcut_text(shortcut))
        .clicked()
    {
        action.run(session, &ui.ctx().clone());
    }
}

fn open_recent(ui: &mut egui::Ui, session: &mut Session) {
    ui.menu_button("Open Recent", |ui| {
        let paths = session.recent_paths();
        if paths.is_empty() {
            ui.add_enabled(false, egui::Button::new("No Recent Files"));
        }
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let response = ui.button(name).on_hover_text(path.display().to_string());
            if response.clicked() {
                session.load_path(&path);
            }
        }
    });
}

pub fn show(ui: &mut egui::Ui, session: &mut Session) {
    if menu::native() {
        return;
    }
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            for (index, menu) in menu::MENUS.iter().enumerate() {
                ui.menu_button(menu.title, |ui| {
                    for entry in menu.entries {
                        match entry {
                            Entry::Item(action) => item(ui, session, *action),
                            Entry::OpenRecent => open_recent(ui, session),
                            Entry::Separator => {
                                ui.separator();
                            }
                        }
                    }
                    // Without an application menu, Quit closes File.
                    if index == 0 {
                        item(ui, session, Action::Quit);
                    }
                });
            }
        });
    });
}
