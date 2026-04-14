//! Linux file clipboard using text/uri-list.
//! TODO: Implement using GTK clipboard + text/uri-list + x-special/gnome-copied-files

use super::FileClipboardOp;

pub fn set_file_clipboard(_paths: &[&str], _op: FileClipboardOp) -> Result<(), String> {
    tracing::warn!("File clipboard not yet implemented on Linux");
    Err("Not implemented".to_string())
}

pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    tracing::warn!("File clipboard not yet implemented on Linux");
    Ok(None)
}
