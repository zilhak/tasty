//! Windows file clipboard using CF_HDROP.
//! TODO: Implement using CF_HDROP + Preferred DropEffect

use super::FileClipboardOp;

pub fn set_file_clipboard(_paths: &[&str], _op: FileClipboardOp) -> Result<(), String> {
    tracing::warn!("File clipboard not yet implemented on Windows");
    Err("Not implemented".to_string())
}

pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    tracing::warn!("File clipboard not yet implemented on Windows");
    Ok(None)
}
