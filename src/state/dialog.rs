//! File and message dialogs.
//!
//! The desktop uses native dialogs through `rfd`. Phones and tablets have
//! none: a save goes to the app's Documents folder under the suggested name,
//! open is the app's own file screen, and an unsaved document is saved
//! rather than asked about.

use std::path::PathBuf;

/// The answer to "save before closing?". Phones only ever save.
#[cfg_attr(any(target_os = "ios", target_os = "android"), allow(dead_code))]
pub(super) enum CloseAnswer {
    Save,
    DontSave,
    Cancel,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod imp {
    use super::CloseAnswer;
    use std::path::PathBuf;

    const SAVE: &str = "Save";
    const DONT_SAVE: &str = "Don't Save";
    const CANCEL: &str = "Cancel";

    pub fn pick_open(filter: &str, extensions: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter(filter, extensions)
            .pick_file()
    }

    pub fn pick_save(
        title: Option<&str>,
        filter: &str,
        extensions: &[&str],
        file_name: &str,
    ) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .add_filter(filter, extensions)
            .set_file_name(file_name);
        if let Some(title) = title {
            dialog = dialog.set_title(title);
        }
        dialog.save_file()
    }

    pub fn ask_close(document: &str) -> CloseAnswer {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!("Save changes to {document} before closing?"))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                SAVE.into(),
                DONT_SAVE.into(),
                CANCEL.into(),
            ))
            .show();
        match answer {
            rfd::MessageDialogResult::Custom(label) if label == SAVE => CloseAnswer::Save,
            rfd::MessageDialogResult::Yes => CloseAnswer::Save,
            rfd::MessageDialogResult::Custom(label) if label == DONT_SAVE => CloseAnswer::DontSave,
            rfd::MessageDialogResult::No => CloseAnswer::DontSave,
            _ => CloseAnswer::Cancel,
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
mod imp {
    use super::CloseAnswer;
    use std::path::PathBuf;

    pub fn pick_open(_filter: &str, _extensions: &[&str]) -> Option<PathBuf> {
        None
    }

    pub fn pick_save(
        _title: Option<&str>,
        _filter: &str,
        _extensions: &[&str],
        file_name: &str,
    ) -> Option<PathBuf> {
        Some(crate::state::documents_dir().join(file_name))
    }

    pub fn ask_close(_document: &str) -> CloseAnswer {
        CloseAnswer::Save
    }
}

pub(super) fn pick_open(filter: &str, extensions: &[&str]) -> Option<PathBuf> {
    imp::pick_open(filter, extensions)
}

pub(super) fn pick_save(
    title: Option<&str>,
    filter: &str,
    extensions: &[&str],
    file_name: &str,
) -> Option<PathBuf> {
    imp::pick_save(title, filter, extensions, file_name)
}

pub(super) fn ask_close(document: &str) -> CloseAnswer {
    imp::ask_close(document)
}
