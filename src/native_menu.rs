//! macOS application menu bar, built from [`menu::MENUS`]. The window then
//! shows no menu strip; items dispatch through [`Action::run`].
//!
//! Set `FROXEL_NO_NATIVE_MENU` to leave the process menu untouched (the
//! in-window strip and egui shortcuts then carry these commands).

use std::path::PathBuf;
use std::str::FromStr;

use muda::accelerator::Accelerator;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSUserDefaults};

use crate::menu::{self, Action, Entry};
use crate::state::Session;

/// AppKit appends Writing Tools, AutoFill, Start Dictation, and Emoji &
/// Symbols to a menu titled Edit. None of them apply to voxels. Registered
/// defaults are not stored, and a `defaults write` still overrides them.
/// Call before the application starts.
pub fn suppress_system_edit_items() {
    let keys = [
        // Writing Tools and AutoFill (macOS 15 and later).
        ("NSAutoFillSystemInsertMenuEnabled", false),
        ("NSMenuDoesNotAutomaticallyInsertWritingToolsItems", true),
        ("NSDisabledDictationMenuItem", true),
        ("NSDisabledCharacterPaletteMenuItem", true),
    ];
    let names: Vec<_> = keys
        .iter()
        .map(|(key, _)| NSString::from_str(key))
        .collect();
    let values: Vec<_> = keys
        .iter()
        .map(|(_, value)| NSNumber::new_bool(*value))
        .collect();
    let names: Vec<&NSString> = names.iter().map(|name| &**name).collect();
    let values: Vec<&AnyObject> = values.iter().map(|value| value.as_ref()).collect();
    let defaults = NSDictionary::<NSString, AnyObject>::from_slices(&names, &values);
    // SAFETY: every value is an NSNumber, a property-list type.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

pub struct NativeMenu {
    _menu: Menu,
    _submenus: Vec<Submenu>,
    recent: Submenu,
    _predefined: Vec<PredefinedMenuItem>,
    _items: Vec<MenuItem>,
    recent_items: Vec<MenuItem>,
    recent_paths: Vec<PathBuf>,
}

fn accel(action: Action) -> Option<Accelerator> {
    let shortcut = action.shortcut();
    let mut text = String::new();
    if shortcut.modifiers.command {
        text.push_str("CMD+");
    }
    if shortcut.modifiers.shift {
        text.push_str("SHIFT+");
    }
    text.push_str(shortcut.logical_key.name());
    Accelerator::from_str(&text).ok()
}

fn item(action: Action) -> MenuItem {
    MenuItem::with_id(
        format!("mv-{}", action.id()),
        action.label(),
        true,
        accel(action),
    )
}

fn recent_empty() -> MenuItem {
    let empty = MenuItem::with_id("mv-recent-empty", "No Recent Files", true, None);
    empty.set_enabled(false);
    empty
}

impl NativeMenu {
    pub fn install() -> Self {
        let menu = Menu::new();
        let app = Submenu::new("Froxel", true);
        let mut predefined = vec![
            PredefinedMenuItem::hide(Some("Hide Froxel")),
            PredefinedMenuItem::hide_others(Some("Hide Others")),
            PredefinedMenuItem::show_all(Some("Show All")),
            PredefinedMenuItem::separator(),
        ];
        for item in &predefined {
            let _ = app.append(item);
        }
        // The predefined Quit terminates the app without a close request,
        // which would skip the unsaved-changes prompt.
        let quit = item(Action::Quit);
        let _ = app.append(&quit);
        let _ = menu.append(&app);

        let mut items = vec![quit];
        let mut submenus = vec![app];
        let recent = Submenu::new("Open Recent", true);
        let empty = recent_empty();
        let _ = recent.append(&empty);
        for spec in menu::MENUS {
            let submenu = Submenu::new(spec.title, true);
            for entry in spec.entries {
                match entry {
                    Entry::Item(action) => {
                        let entry = item(*action);
                        let _ = submenu.append(&entry);
                        items.push(entry);
                    }
                    Entry::OpenRecent => {
                        let _ = submenu.append(&recent);
                    }
                    Entry::Separator => {
                        let separator = PredefinedMenuItem::separator();
                        let _ = submenu.append(&separator);
                        predefined.push(separator);
                    }
                }
            }
            let _ = menu.append(&submenu);
            submenus.push(submenu);
        }

        menu.init_for_nsapp();
        Self {
            _menu: menu,
            _submenus: submenus,
            recent,
            _predefined: predefined,
            _items: items,
            recent_items: vec![empty],
            recent_paths: Vec::new(),
        }
    }

    pub fn poll(&mut self, session: &mut Session, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let Some(id) = ev.id.as_ref().strip_prefix("mv-") else {
                continue;
            };
            if let Some(rest) = id.strip_prefix("recent-") {
                if let Ok(index) = rest.parse::<usize>() {
                    if let Some(path) = session.recent_paths().get(index).cloned() {
                        session.load_path(&path);
                    }
                    ctx.request_repaint();
                }
                continue;
            }
            if let Some(action) = Action::from_id(id) {
                action.run(session, ctx);
            }
        }
        self.sync_recent(session);
    }

    fn sync_recent(&mut self, session: &Session) {
        let paths = session.recent_paths();
        if paths == self.recent_paths {
            return;
        }
        while self.recent.remove_at(0).is_some() {}
        self.recent_items.clear();
        if paths.is_empty() {
            let empty = recent_empty();
            let _ = self.recent.append(&empty);
            self.recent_items.push(empty);
        } else {
            for (index, path) in paths.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let entry = MenuItem::with_id(format!("mv-recent-{index}"), name, true, None);
                let _ = self.recent.append(&entry);
                self.recent_items.push(entry);
            }
        }
        self.recent_paths = paths;
    }
}
