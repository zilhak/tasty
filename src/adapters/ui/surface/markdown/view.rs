//! Host-side per-surface state for `MarkdownPanel` rendering. Holds the file's loaded
//! content, the load outcome (for the load-fail / empty chrome states), the file's base
//! directory (to resolve relative image paths), and scroll offset — none of which belong
//! in the GUI-free `tasty-core` model.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::model::{MarkdownPanel, SurfaceId};

pub struct MarkdownView {
    /// Raw markdown source. Empty when the load failed (see `load_error`) or the file is
    /// genuinely empty — the renderer distinguishes the two via `load_error`.
    pub content: String,
    /// `Some(message)` when the initial read (or a reload) failed. Drives the peach
    /// "Failed to load" chrome instead of leaking a raw `Error:` string into the body.
    pub load_error: Option<String>,
    /// The markdown file's parent directory, used to resolve relative image paths.
    pub base_dir: Option<PathBuf>,
    /// 향후 scroll position persistence 시 사용 — 현 시점 read 0.
    #[allow(dead_code)] // 구조체 필드 — 향후 scroll persistence 용 보존, 현재 미read
    pub scroll_offset: f32,
}

impl MarkdownView {
    pub fn new(file_path: &str) -> Self {
        let (content, load_error) = match std::fs::read_to_string(file_path) {
            Ok(text) => (text, None),
            Err(e) => (String::new(), Some(e.to_string())),
        };
        Self {
            content,
            load_error,
            base_dir: std::path::Path::new(file_path)
                .parent()
                .map(|p| p.to_path_buf()),
            scroll_offset: 0.0,
        }
    }

    /// Replace the body with freshly-read content (an external change cleared the error).
    pub fn replace_content(&mut self, new_content: String) {
        self.content = new_content;
        self.load_error = None;
    }
}

#[derive(Default)]
pub struct MarkdownViewStore {
    views: HashMap<SurfaceId, MarkdownView>,
}

impl MarkdownViewStore {
    /// Get the view for `panel`, polling the file for an external mtime change and
    /// refreshing `content` if the panel reports a reload. Creates the view on first
    /// access using the panel's current `file_path`.
    pub fn get_or_init(&mut self, panel: &mut MarkdownPanel) -> &mut MarkdownView {
        let view = self
            .views
            .entry(panel.id)
            .or_insert_with(|| MarkdownView::new(&panel.file_path));
        if let Some(new_content) = panel.poll_reload() {
            view.replace_content(new_content);
        }
        view
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_view_removes_entry() {
        let mut store = MarkdownViewStore::default();
        // Insert directly to avoid needing a real file
        store.views.insert(
            42,
            MarkdownView {
                content: String::new(),
                load_error: None,
                base_dir: None,
                scroll_offset: 0.0,
            },
        );
        store.drop_view(42);
        assert!(store.views.is_empty());
    }

    #[test]
    fn new_on_missing_file_records_error_not_body() {
        let view = MarkdownView::new("\0nonexistent-path-for-test");
        assert!(view.load_error.is_some());
        assert!(view.content.is_empty());
    }
}
