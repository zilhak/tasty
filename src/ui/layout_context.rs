/// Layout context carrying pane/surface/workspace geometry for scope-based
/// visibility and clamping.  Consumed by PopupManager, ToastManager, and any
/// future system that needs to resolve scope → screen rect.
pub struct LayoutContext {
    pub active_workspace: usize,
    /// (pane_id, rect) for all visible panes.
    pub pane_rects: Vec<(u32, egui::Rect)>,
    /// (surface_id, rect) for all visible surfaces.
    pub surface_rects: Vec<(u32, egui::Rect)>,
    /// (pane_id, active_tab_index) for each pane.
    pub active_tabs: Vec<(u32, usize)>,
}
