//! Explorer bookmarks storage.
//! Persisted to `~/.tasty/bookmarks.json`.
//! All explorer panels share the same bookmarks.

use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct BookmarkEntry {
    pub name: String,
    pub path: String,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Bookmarks {
    pub entries: Vec<BookmarkEntry>,
}

impl Bookmarks {
    fn file_path() -> Option<PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().join(".tasty").join("bookmarks.json"))
    }

    pub fn load() -> Self {
        Self::file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::file_path() {
            if let Ok(json) = serde_json::to_string_pretty(self) {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("failed to save bookmarks: {e}");
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn add(&mut self, name: String, path: String) {
        self.entries.retain(|b| b.path != path);
        self.entries.push(BookmarkEntry { name, path });
        self.save();
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, path: &str) {
        self.entries.retain(|b| b.path != path);
        self.save();
    }

    #[allow(dead_code)]
    pub fn is_bookmarked(&self, path: &str) -> bool {
        self.entries.iter().any(|b| b.path == path)
    }
}
