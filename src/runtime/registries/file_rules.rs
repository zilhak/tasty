//! One observed document for detector, extension priority and handler user contributions.
#[cfg(test)]
mod tests;

use crate::core::file_snapshot::{Document, Submit, tracker::Tracker};
use crate::file::{format::FileFormatRegistry, handler::FileHandlerRegistry};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) struct FileRules {
    path: PathBuf,
    state: Mutex<State>,
}
struct State {
    #[cfg(any(feature = "gui", test))]
    table: toml::Table,
    tracker: Tracker,
    sink: Option<Submit>,
}
impl FileRules {
    pub(crate) fn load(
        path: &Path,
        format: &FileFormatRegistry,
        handler: &FileHandlerRegistry,
    ) -> Self {
        let (_table, document, _) = read(path, format, handler);
        document.report_load_error("file-handlers.toml");
        Self {
            path: path.into(),
            state: Mutex::new(State {
                #[cfg(any(feature = "gui", test))]
                table: _table,
                tracker: Tracker::new("file:file-handlers.toml", document),
                sink: None,
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| {
            tracing::warn!(path = %self.path.display(), "file rules lock poisoned; retaining current state");
            poisoned.into_inner()
        })
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn install_sink(&self, sink: Submit) {
        let mut state = self.lock();
        state.tracker.install_sink(sink.clone());
        state.sink = Some(sink);
    }
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn submit(&self, format: &FileFormatRegistry, handler: &FileHandlerRegistry) {
        let mut state = self.lock();
        let exported = crate::file::handler::save::export_combined_user_config(format, handler);
        let result = (|| -> Result<Vec<u8>, String> {
            let mut exported: toml::Table = toml::from_str(&exported).map_err(|e| e.to_string())?;
            for section in ["detector", "extension_priority", "handler"] {
                state.table.remove(section);
                if let Some(value) = exported.remove(section) {
                    state.table.insert(section.into(), value);
                }
            }
            toml::to_string_pretty(&state.table)
                .map(String::into_bytes)
                .map_err(|e| e.to_string())
        })();
        state.tracker.update(result);
    }
    pub(crate) fn poll(&self) -> Vec<String> {
        self.lock().tracker.poll()
    }
    pub(crate) fn before_shutdown(&self) {
        self.lock().tracker.retry_latest_before_shutdown();
    }
    pub(crate) fn finish_shutdown(&self) -> Option<(u64, u64)> {
        self.lock().tracker.finish_shutdown()
    }
    pub(crate) fn status(&self) -> (u64, u64, usize) {
        self.lock().tracker.status()
    }
    pub(crate) fn dirty_age_us(&self) -> Option<u64> {
        self.lock().tracker.dirty_age_us()
    }
    pub(crate) fn reload(
        &self,
        format: &FileFormatRegistry,
        handler: &FileHandlerRegistry,
    ) -> Result<Vec<tasty_file_handler::RejectedUserHandler>, String> {
        let mut state = self.lock();
        if state.tracker.status().2 != 0 {
            return Err(
                "file handler saves are still pending; retry reload after completion".into(),
            );
        }
        let (_table, document, rejected) = read(&self.path, format, handler);
        if let Some(error) = document.load_error() {
            return Err(error);
        }
        let tracker = Tracker::loaded(
            "file:file-handlers.toml",
            document,
            state.tracker.status().0 + 1,
        );
        if let Some(sink) = &state.sink {
            tracker.install_sink(sink.clone());
        }
        #[cfg(any(feature = "gui", test))]
        {
            state.table = _table;
        }
        state.tracker = tracker;
        Ok(rejected)
    }
}
fn read(
    path: &Path,
    format: &FileFormatRegistry,
    handler: &FileHandlerRegistry,
) -> (
    toml::Table,
    Arc<Document>,
    Vec<tasty_file_handler::RejectedUserHandler>,
) {
    let mut rejected = Vec::new();
    let (table, document) = Document::load_with::<toml::Table>(path.into(), |text| {
        let text = text.unwrap_or("");
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        FileHandlerRegistry::validate_user_config_text(text)?;
        format.replace_user_config_text(text)?;
        rejected = handler.replace_user_config_text(text)?;
        Ok(table)
    });
    (table, document, rejected)
}
