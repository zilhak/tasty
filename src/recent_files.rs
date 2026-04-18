//! Recent files storage for markdown and HTML open dialogs.
//! In-memory cache owned by `AppState.recent_files`. Persisted to
//! `~/.tasty/recent_files.json` on every mutation.

use std::path::PathBuf;

const MAX_ENTRIES: usize = 10;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct RecentFiles {
    pub markdown: Vec<String>,
    pub html: Vec<String>,
}

impl RecentFiles {
    fn file_path() -> Option<PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".tasty").join("recent_files.json"))
    }

    /// Load from disk. Called once at app startup and cached in `AppState.recent_files`.
    pub fn load() -> Self {
        Self::file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Some(path) = Self::file_path() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("failed to save recent files: {e}");
                }
            }
        }
    }

    pub fn add_markdown(&mut self, path: String) {
        self.markdown.retain(|p| p != &path);
        self.markdown.insert(0, path);
        self.markdown.truncate(MAX_ENTRIES);
        self.save();
    }

    pub fn add_html(&mut self, url: String) {
        self.html.retain(|u| u != &url);
        self.html.insert(0, url);
        self.html.truncate(MAX_ENTRIES);
        self.save();
    }
}
