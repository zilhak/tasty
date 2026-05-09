//! Host-side per-surface state for `ExplorerPanel` rendering. Holds GUI-bound
//! state that does not belong in the GUI-free `tasty-core` model: selection
//! sets, scroll offsets, address-bar editing buffer, focus tracking, refresh
//! timer, and preview content cache.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::model::{ExplorerPanel, SurfaceId, is_previewable_file};

pub struct ExplorerView {
    /// The last clicked/navigated item — used for right-side preview.
    pub selected_file: Option<String>,
    /// All currently selected items (for multi-selection & clipboard).
    pub selected_files: HashSet<String>,
    /// Anchor point for Shift+click range selection.
    pub selection_anchor: Option<String>,
    /// Cached text content for the preview pane.
    pub file_content: Option<String>,
    /// Whether the previewed file should be rendered as Markdown.
    pub is_markdown: bool,
    /// Preview pane scroll offset.
    pub scroll_offset: f32,
    /// Tree pane scroll offset (reserved for future use; egui ScrollArea
    /// stores its own state, but we keep this for parity with the previous model).
    pub tree_scroll_offset: f32,
    /// Address bar editing buffer.
    pub address_bar_text: String,
    /// Whether the address bar is actively being edited (has focus).
    pub address_bar_editing: bool,
    /// Whether the file preview pane is visible.
    pub show_preview: bool,
    /// Tree panel width ratio (0.0..1.0) when preview is shown. Default 0.35.
    pub tree_ratio: f32,
    /// 직전 프레임에 이 surface가 focused였는지 추적. 포커스 획득 시 트리 갱신용.
    pub was_focused: bool,
    /// 마지막 자동 갱신 시각. focused 상태에서 2초마다 증분 갱신.
    pub last_refresh: Instant,
}

impl ExplorerView {
    pub fn new(panel: &ExplorerPanel) -> Self {
        Self {
            selected_file: None,
            selected_files: HashSet::new(),
            selection_anchor: None,
            file_content: None,
            is_markdown: false,
            scroll_offset: 0.0,
            tree_scroll_offset: 0.0,
            address_bar_text: panel.root_path.clone(),
            address_bar_editing: false,
            show_preview: true,
            tree_ratio: 0.35,
            was_focused: false,
            last_refresh: Instant::now(),
        }
    }

    /// Single-click select: clear all selections, select one item, set anchor.
    pub fn select_single(&mut self, path: &str) {
        self.selected_files.clear();
        self.selected_files.insert(path.to_string());
        self.selection_anchor = Some(path.to_string());
        self.set_preview(path);
    }

    /// Ctrl/Cmd+click: toggle one item in the selection set.
    pub fn toggle_select(&mut self, path: &str) {
        if self.selected_files.contains(path) {
            self.selected_files.remove(path);
        } else {
            self.selected_files.insert(path.to_string());
        }
        self.selection_anchor = Some(path.to_string());
        self.set_preview(path);
    }

    /// Shift+click: range select from anchor to target (inclusive).
    /// `visible_paths` must be the current ordered list of visible tree nodes.
    pub fn range_select(&mut self, target: &str, visible_paths: &[String]) {
        let anchor = self
            .selection_anchor
            .clone()
            .unwrap_or_else(|| target.to_string());
        let anchor_idx = visible_paths.iter().position(|p| p == &anchor);
        let target_idx = visible_paths.iter().position(|p| p == target);
        if let (Some(a), Some(t)) = (anchor_idx, target_idx) {
            let (start, end) = if a <= t { (a, t) } else { (t, a) };
            self.selected_files.clear();
            for p in &visible_paths[start..=end] {
                self.selected_files.insert(p.clone());
            }
        }
        self.set_preview(target);
    }

    /// Select all visible items.
    pub fn select_all(&mut self, visible_paths: &[String]) {
        self.selected_files.clear();
        for p in visible_paths {
            self.selected_files.insert(p.clone());
        }
    }

    /// Update the right-side preview for the given path.
    fn set_preview(&mut self, path: &str) {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        self.is_markdown = ext == "md" || ext == "markdown";
        self.selected_file = Some(path.to_string());
        self.scroll_offset = 0.0;

        // Only load preview for files (not directories)
        let is_dir = std::path::Path::new(path).is_dir();
        if !is_dir && is_previewable_file(path, &ext) {
            self.file_content = std::fs::read_to_string(path).ok();
        } else {
            self.file_content = None;
        }
    }

    /// 디렉터리 이동 후 호출. address bar/selection/preview를 새 루트 기준으로 리셋.
    pub fn reset_after_navigate(&mut self, new_path: &str) {
        self.address_bar_text = new_path.to_string();
        self.selected_file = None;
        self.selected_files.clear();
        self.selection_anchor = None;
        self.file_content = None;
        self.is_markdown = false;
        self.scroll_offset = 0.0;
    }
}

#[derive(Default)]
pub struct ExplorerViewStore {
    views: HashMap<SurfaceId, ExplorerView>,
}

impl ExplorerViewStore {
    /// Get the view for `panel`, creating it on first access using the panel's
    /// current `root_path` as the initial address bar value.
    pub fn get_or_init(&mut self, panel: &ExplorerPanel) -> &mut ExplorerView {
        self.views
            .entry(panel.id)
            .or_insert_with(|| ExplorerView::new(panel))
    }

    pub fn get(&self, sid: SurfaceId) -> Option<&ExplorerView> {
        self.views.get(&sid)
    }

    pub fn get_mut(&mut self, sid: SurfaceId) -> Option<&mut ExplorerView> {
        self.views.get_mut(&sid)
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> ExplorerPanel {
        // Use cwd for the temporary panel — the directory must exist for `new`.
        let cwd = std::env::current_dir().unwrap().to_string_lossy().to_string();
        ExplorerPanel::new(1, cwd)
    }

    #[test]
    fn new_view_uses_panel_root_path_for_address_bar() {
        let panel = make_panel();
        let view = ExplorerView::new(&panel);
        assert_eq!(view.address_bar_text, panel.root_path);
        assert!(view.show_preview);
        assert_eq!(view.tree_ratio, 0.35);
        assert!(view.selected_files.is_empty());
    }

    #[test]
    fn select_single_replaces_selection_and_sets_anchor() {
        let mut view = ExplorerView::new(&make_panel());
        view.selected_files.insert("foo".into());
        view.selected_files.insert("bar".into());
        view.select_single("baz");
        assert_eq!(view.selected_files.len(), 1);
        assert!(view.selected_files.contains("baz"));
        assert_eq!(view.selection_anchor.as_deref(), Some("baz"));
    }

    #[test]
    fn toggle_select_adds_then_removes() {
        let mut view = ExplorerView::new(&make_panel());
        view.toggle_select("a");
        assert!(view.selected_files.contains("a"));
        view.toggle_select("a");
        assert!(!view.selected_files.contains("a"));
    }

    #[test]
    fn range_select_picks_inclusive_range() {
        let mut view = ExplorerView::new(&make_panel());
        view.selection_anchor = Some("b".into());
        let visible = vec!["a".to_string(), "b".into(), "c".into(), "d".into()];
        view.range_select("d", &visible);
        assert_eq!(view.selected_files.len(), 3);
        assert!(view.selected_files.contains("b"));
        assert!(view.selected_files.contains("c"));
        assert!(view.selected_files.contains("d"));
    }

    #[test]
    fn drop_view_removes_entry() {
        let mut store = ExplorerViewStore::default();
        let panel = make_panel();
        store.get_or_init(&panel);
        assert!(store.get(panel.id).is_some());
        store.drop_view(panel.id);
        assert!(store.get(panel.id).is_none());
    }

    #[test]
    fn reset_after_navigate_clears_selection() {
        let mut view = ExplorerView::new(&make_panel());
        view.selected_files.insert("x".into());
        view.selected_file = Some("x".into());
        view.is_markdown = true;
        view.reset_after_navigate("/new/path");
        assert_eq!(view.address_bar_text, "/new/path");
        assert!(view.selected_files.is_empty());
        assert!(view.selected_file.is_none());
        assert!(!view.is_markdown);
    }
}
