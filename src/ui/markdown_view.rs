//! Host-side per-surface state for `MarkdownPanel` rendering. Holds the file's loaded
//! content, the egui_commonmark cache, and scroll offset — none of which belong in the
//! GUI-free `tasty-core` model.

use std::collections::HashMap;

use crate::model::{MarkdownPanel, SurfaceId};

pub struct MarkdownView {
    pub content: String,
    pub scroll_offset: f32,
    pub commonmark_cache: egui_commonmark::CommonMarkCache,
}

impl MarkdownView {
    pub fn new(file_path: &str) -> Self {
        let content =
            std::fs::read_to_string(file_path).unwrap_or_else(|e| format!("Error: {e}"));
        Self {
            content,
            scroll_offset: 0.0,
            commonmark_cache: egui_commonmark::CommonMarkCache::default(),
        }
    }

    pub fn replace_content(&mut self, new_content: String) {
        self.content = new_content;
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
                scroll_offset: 0.0,
                commonmark_cache: egui_commonmark::CommonMarkCache::default(),
            },
        );
        store.drop_view(42);
        assert!(store.views.is_empty());
    }
}
