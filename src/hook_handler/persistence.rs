//! The process hook registry owns the original file observation and disk revisions.
#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;

use std::path::{Path, PathBuf};

use super::{HookHandlerRegistry, parse_user_handler_section};
use crate::core::file_snapshot::{
    Document, Submit,
    tracker::{Ticket, Tracker},
};

pub(super) struct UserFile {
    path: PathBuf,
    original: toml::Table,
    tracker: Tracker,
}
impl HookHandlerRegistry {
    pub(super) fn load_file(&self, path: &Path) {
        let (original, document) = Document::load_with(path.to_owned(), |text| {
            let text = text.unwrap_or("");
            let original = toml::from_str::<toml::Table>(text).map_err(|e| e.to_string())?;
            let decls = parse_user_handler_section(text).map_err(|e| e.to_string())?;
            let mut inner = self.lock_write();
            for decl in decls {
                super::install_user(&mut inner, decl);
            }
            inner.dirty = true;
            Ok(original)
        });
        document.report_load_error("hook-handlers.toml");
        *self.file.lock().unwrap() = Some(UserFile {
            path: path.to_owned(),
            original,
            tracker: Tracker::new("hook-handlers", document),
        });
    }

    pub(crate) fn submit_file(
        &self,
        sink: Option<Submit>,
        notify: bool,
    ) -> Result<(PathBuf, Ticket), String> {
        let file = self.file.lock().unwrap();
        let file = file
            .as_ref()
            .ok_or("hook handler file was not initialized")?;
        file.tracker.replace_sink(sink);
        let bytes = self.try_export_user_config().and_then(|text| {
            let mut original = file.original.clone();
            let exported = toml::from_str::<toml::Table>(&text).map_err(|e| e.to_string())?;
            original.remove("handler");
            original.extend(exported);
            toml::to_string_pretty(&original)
                .map(String::into_bytes)
                .map_err(|e| e.to_string())
        });
        Ok((
            file.path.clone(),
            file.tracker.update_with_receipt(bytes, notify),
        ))
    }

    pub(crate) fn poll_file(&self) -> Vec<String> {
        self.file
            .lock()
            .unwrap()
            .as_ref()
            .map(|f| f.tracker.poll())
            .unwrap_or_default()
    }
    pub(crate) fn file_status(&self) -> Option<(u64, u64, usize)> {
        self.file
            .lock()
            .unwrap()
            .as_ref()
            .map(|f| f.tracker.status())
    }
    pub(crate) fn before_file_shutdown(&self) {
        if let Some(file) = self.file.lock().unwrap().as_ref() {
            file.tracker.retry_latest_before_shutdown();
        }
    }
    pub(crate) fn finish_file_shutdown(&self) -> Option<(u64, u64)> {
        self.file
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|f| f.tracker.finish_shutdown())
    }
    pub(crate) fn reload_file(&self) -> Result<PathBuf, String> {
        let mut file = self.file.lock().unwrap();
        let previous = file
            .as_ref()
            .ok_or("hook handler file was not initialized")?;
        if previous.tracker.status().2 != 0 {
            return Err("hook handler saves are pending; retry reload after completion".into());
        }
        let path = previous.path.clone();
        let revision = previous.tracker.status().0 + 1;
        let (original, document) = Document::load_with(path.clone(), |text| {
            let text = text.unwrap_or("");
            let original = toml::from_str::<toml::Table>(text).map_err(|e| e.to_string())?;
            let decls = parse_user_handler_section(text).map_err(|e| e.to_string())?;
            self.replace_user_decls(decls);
            Ok(original)
        });
        if let Some(error) = document.load_error() {
            return Err(error);
        }
        *file = Some(UserFile {
            path: path.clone(),
            original,
            tracker: Tracker::loaded("hook-handlers", document, revision),
        });
        Ok(path)
    }
}
