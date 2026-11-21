//! The App Sandbox of the Mac App Store build.
//!
//! The store build (`packaging/macos/appstore.sh`) runs sandboxed; the
//! Developer ID build and every other platform do not, and there every
//! function here does nothing. In the sandbox `HOME` and `TMPDIR` are the
//! app's container, so the support folder, the control token, and the bridge
//! session stay there without any change. Anything else is reachable only
//! when the user picked it in an open or save panel, which lasts until the
//! process exits.
//!
//! Two kinds of security-scoped bookmarks keep access across launches, in
//! `bookmarks.json` in the support folder: the recent files, and the folders
//! granted with File > Grant Folder Access…. [`restore`] resolves them when
//! any `froxel` process starts, the window, `froxel mcp`, and the command-line
//! tools alike, so an MCP client reads and writes inside a granted folder with
//! plain paths.

use std::path::{Path, PathBuf};

/// True in the macOS App Sandbox. The system sets `APP_SANDBOX_CONTAINER_ID`
/// in every sandboxed process. iOS apps are always sandboxed and keep their
/// files in Documents (`state::documents_dir`), so this is false there.
pub fn active() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

/// The folders granted with File > Grant Folder Access… that this process
/// can reach.
pub fn granted_folders() -> Vec<PathBuf> {
    imp::granted_folders()
}

/// Resolve the stored bookmarks and start using them. Call once, first thing.
pub fn restore() {
    if active() {
        imp::restore();
    }
}

/// Keep a bookmark of a folder the user just picked, and use it from now on.
pub fn grant_folder(path: &Path) -> Result<(), String> {
    if !active() {
        return Ok(());
    }
    imp::grant_folder(path)
}

/// Keep bookmarks of exactly these recent files, so Open Recent works after a
/// relaunch. A file is bookmarked while this process can still reach it.
pub fn keep_recent(paths: &[PathBuf]) {
    if active() {
        imp::keep_recent(paths);
    }
}

/// What the sandbox allows, for the MCP server instructions; `None` outside it.
pub fn summary() -> Option<String> {
    if !active() {
        return None;
    }
    let container = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).display().to_string())
        .unwrap_or_else(|| "the app's container".into());
    let folders = granted_folders();
    let granted = if folders.is_empty() {
        "none yet".to_string()
    } else {
        folders
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(format!(
        "This Froxel is the Mac App Store build and runs in the App Sandbox: tools that take a path read and write only inside its container ({container}) and inside the folders the user granted with File > Grant Folder Access… in the window ({granted}). Any other path fails with \"Operation not permitted\"; ask the user to grant its folder, then call the tool again (a running froxel mcp picks up a new grant after reload)."
    ))
}

/// Fails in the sandbox, where the store build has no network entitlements:
/// the control port (`--control`, `froxel mcp --connect` and `--launch`) and
/// the bridge need the Froxel from the dmg or Homebrew.
pub fn check_port(what: &str) -> std::io::Result<()> {
    if active() {
        return Err(std::io::Error::other(format!(
            "the Mac App Store build of Froxel cannot use the {what} port; use the Froxel from the dmg or Homebrew"
        )));
    }
    Ok(())
}

/// An error message with a hint when the sandbox is the likely cause.
pub fn explain(message: &str) -> String {
    if !active() || !denied(message) {
        return message.to_string();
    }
    format!(
        "{message}. The Mac App Store build of Froxel reaches only its container and the folders granted with File > Grant Folder Access…; grant this path's folder and try again."
    )
}

fn denied(message: &str) -> bool {
    message.contains("Operation not permitted")
        || message.contains("Permission denied")
        || message.contains("os error 1)")
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{
        NSData, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURL,
    };
    use serde_json::{json, Value};

    /// The granted folders this process started accessing.
    static FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    #[derive(Clone)]
    struct Mark {
        path: PathBuf,
        data: Vec<u8>,
    }

    #[derive(Default)]
    struct Store {
        folders: Vec<Mark>,
        files: Vec<Mark>,
    }

    fn store_path() -> Option<PathBuf> {
        crate::lighting::support_file("FROXEL_BOOKMARKS", "bookmarks.json")
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(text: &str) -> Option<Vec<u8>> {
        if text.len() % 2 != 0 {
            return None;
        }
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
            .collect()
    }

    fn marks(value: &Value) -> Vec<Mark> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(Mark {
                            path: PathBuf::from(item.get("path")?.as_str()?),
                            data: unhex(item.get("bookmark")?.as_str()?)?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn load() -> Store {
        let Some(text) = store_path().and_then(|path| std::fs::read_to_string(path).ok()) else {
            return Store::default();
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return Store::default();
        };
        Store {
            folders: marks(&value["folders"]),
            files: marks(&value["files"]),
        }
    }

    fn save(store: &Store) {
        let Some(path) = store_path() else {
            return;
        };
        let list = |marks: &[Mark]| -> Value {
            marks
                .iter()
                .map(|mark| json!({"path": mark.path.display().to_string(), "bookmark": hex(&mark.data)}))
                .collect()
        };
        let text =
            json!({"version": 1, "folders": list(&store.folders), "files": list(&store.files)});
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&path, format!("{text:#}\n")) {
            eprintln!("could not write {}: {err}", path.display());
        }
    }

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    fn bookmark(path: &Path) -> Result<Vec<u8>, String> {
        url(path)
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::WithSecurityScope,
                None,
                None,
            )
            .map(|data| data.to_vec())
            .map_err(|err| {
                format!(
                    "could not keep access to {}: {}",
                    path.display(),
                    err.localizedDescription()
                )
            })
    }

    /// Resolve a bookmark and start accessing it for the rest of the process.
    /// Returns the current path and, when the bookmark is stale, a fresh one.
    fn open(mark: &Mark) -> Option<(PathBuf, Option<Vec<u8>>)> {
        let data = NSData::with_bytes(&mark.data);
        let mut stale = Bool::NO;
        // SAFETY: `stale` is a valid pointer for the call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope
                    | NSURLBookmarkResolutionOptions::WithoutUI,
                None,
                &mut stale,
            )
        }
        .ok()?;
        // SAFETY: a URL resolved from a security-scoped bookmark. Access is kept
        // until the process exits, so it is never stopped.
        if !unsafe { url.startAccessingSecurityScopedResource() } {
            return None;
        }
        let path = PathBuf::from(url.path()?.to_string());
        let fresh = if stale.as_bool() {
            bookmark(&path).ok()
        } else {
            None
        };
        Some((path, fresh))
    }

    fn reopen(marks: &[Mark], changed: &mut bool) -> Vec<Mark> {
        let mut kept = Vec::new();
        for mark in marks {
            match open(mark) {
                Some((path, fresh)) => {
                    *changed |= fresh.is_some() || path != mark.path;
                    kept.push(Mark {
                        path,
                        data: fresh.unwrap_or_else(|| mark.data.clone()),
                    });
                }
                // A folder or file that is gone, or whose bookmark no longer resolves.
                None => *changed = true,
            }
        }
        kept
    }

    pub fn restore() {
        let store = load();
        let mut changed = false;
        let folders = reopen(&store.folders, &mut changed);
        let files = reopen(&store.files, &mut changed);
        if let Ok(mut list) = FOLDERS.lock() {
            *list = folders.iter().map(|mark| mark.path.clone()).collect();
        }
        if changed {
            save(&Store { folders, files });
        }
    }

    pub fn granted_folders() -> Vec<PathBuf> {
        FOLDERS.lock().map(|list| list.clone()).unwrap_or_default()
    }

    pub fn grant_folder(path: &Path) -> Result<(), String> {
        let data = bookmark(path)?;
        let mut store = load();
        store.folders.retain(|mark| mark.path != path);
        store.folders.push(Mark {
            path: path.to_path_buf(),
            data,
        });
        save(&store);
        if let Ok(mut list) = FOLDERS.lock() {
            if !list.iter().any(|folder| folder == path) {
                list.push(path.to_path_buf());
            }
        }
        Ok(())
    }

    pub fn keep_recent(paths: &[PathBuf]) {
        let mut store = load();
        let before = store.files.len();
        store.files.retain(|mark| paths.contains(&mark.path));
        let mut changed = store.files.len() != before;
        for path in paths {
            if store.files.iter().any(|mark| &mark.path == path) {
                continue;
            }
            if let Ok(data) = bookmark(path) {
                store.files.push(Mark {
                    path: path.clone(),
                    data,
                });
                changed = true;
            }
        }
        if changed {
            save(&store);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::{Path, PathBuf};

    pub fn restore() {}

    pub fn granted_folders() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn grant_folder(_path: &Path) -> Result<(), String> {
        Ok(())
    }

    pub fn keep_recent(_paths: &[PathBuf]) {}
}

#[cfg(test)]
#[path = "../tests/sandbox.rs"]
mod tests;
