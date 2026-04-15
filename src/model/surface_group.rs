use tasty_terminal::Terminal;
use super::{Rect, SurfaceId};
use super::pane_tree::FocusDirection;
use super::surface_trait::Surface;
pub use super::surface_layout::SurfaceGroupLayout;

/// Single terminal instance (Surface type: Terminal).
pub struct TerminalSurface {
    pub id: SurfaceId,
    pub terminal: Terminal,
    /// If lazy init is enabled and terminal hasn't been spawned yet,
    /// this holds the deferred spawn parameters.
    #[allow(dead_code)]
    pub(crate) deferred_spawn: Option<DeferredSpawn>,
}

/// Parameters needed to spawn a PTY later (lazy init).
#[derive(Clone)]
pub(crate) struct DeferredSpawn {
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    pub cols: usize,
    pub rows: usize,
    pub waker: tasty_terminal::Waker,
    pub working_dir: Option<std::path::PathBuf>,
}

impl Surface for TerminalSurface {
    fn type_name(&self) -> &'static str { "Terminal" }

    fn surface_id(&self) -> Option<SurfaceId> { Some(self.id) }

    fn has_terminal(&self) -> bool { true }

    fn focused_terminal(&self) -> Option<&Terminal> { Some(&self.terminal) }

    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> { Some(&mut self.terminal) }

    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        if self.id == surface_id { Some(&self.terminal) } else { None }
    }

    fn find_terminal_surface(&self, surface_id: SurfaceId) -> Option<&TerminalSurface> {
        if self.id == surface_id { Some(self) } else { None }
    }

    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        if self.id == surface_id { Some(&mut self.terminal) } else { None }
    }

    fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        vec![(self.id, &self.terminal, rect)]
    }

    fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        let cols = (rect.width / cell_width).floor().max(1.0) as usize;
        let rows = (rect.height / cell_height).floor().max(1.0) as usize;
        self.terminal.resize(cols, rows);
    }

    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        out.push(&mut self.terminal);
    }

    fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        f(self.id, &mut self.terminal);
    }

    fn as_terminal_surface(&self) -> Option<&TerminalSurface> { Some(self) }
    fn as_terminal_surface_mut(&mut self) -> Option<&mut TerminalSurface> { Some(self) }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": self.terminal.cols(),
            "rows": self.terminal.rows(),
        })
    }
}

/// Split within a tab (appears as one tab but renders multiple terminals).
pub struct SurfaceGroupNode {
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    pub(crate) layout_opt: Option<SurfaceGroupLayout>,
    pub focused_surface: SurfaceId,
    /// First surface ID, stored for focus tracking.
    pub(crate) _first_surface: SurfaceId,
}

impl SurfaceGroupNode {
    #[track_caller]
    pub fn layout(&self) -> &SurfaceGroupLayout {
        self.layout_opt.as_ref().expect("BUG: layout accessed during structural mutation (between take/put)")
    }

    #[track_caller]
    pub fn layout_mut(&mut self) -> &mut SurfaceGroupLayout {
        self.layout_opt.as_mut().expect("BUG: layout accessed during structural mutation (between take/put)")
    }

    #[track_caller]
    pub(crate) fn take_layout(&mut self) -> SurfaceGroupLayout {
        self.layout_opt.take().expect("BUG: layout already taken")
    }

    pub(crate) fn put_layout(&mut self, layout: SurfaceGroupLayout) {
        self.layout_opt = Some(layout);
    }
}

impl SurfaceGroupNode {
    /// Create from a restored layout (no PTY creation needed).
    pub fn from_restored(layout: SurfaceGroupLayout, focused_surface: SurfaceId) -> Self {
        let first_surface = layout.first_surface_id().unwrap_or(0);
        Self {
            layout_opt: Some(layout),
            focused_surface,
            _first_surface: first_surface,
        }
    }

    pub fn close_surface(&mut self, target_id: SurfaceId) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, found) = old_layout.close_surface(target_id);
        self.put_layout(new_layout);
        if found {
            if self.focused_surface == target_id {
                if let Some(first_id) = self.layout().first_surface_id() {
                    self.focused_surface = first_id;
                }
            }
        }
        found
    }

    pub fn compute_rects(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        self.layout().render_regions(rect)
    }

    pub fn focused_terminal(&self) -> Option<&Terminal> {
        let layout = self.layout();
        layout
            .find_terminal(self.focused_surface)
            .or_else(|| layout.first_terminal())
    }

    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        let id = self.focused_surface;
        if self.layout().find_terminal(id).is_none() {
            if let Some(first_id) = self.layout().first_surface_id() {
                self.focused_surface = first_id;
            }
        }
        let id = self.focused_surface;
        self.layout_mut().find_terminal_mut(id)
    }

    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        self.layout_mut().resize_all(rect, cell_width, cell_height);
    }

    pub fn move_focus_forward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 { return; }
        let pos = ids.iter().position(|&id| id == self.focused_surface).unwrap_or(0);
        self.focused_surface = ids[(pos + 1) % ids.len()];
    }

    pub fn move_focus_backward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 { return; }
        let pos = ids.iter().position(|&id| id == self.focused_surface).unwrap_or(0);
        self.focused_surface = ids[(pos + ids.len() - 1) % ids.len()];
    }

    pub fn directional_focus(&self, direction: FocusDirection) -> Option<SurfaceId> {
        self.layout().directional_focus(self.focused_surface, direction)
    }
}

impl Surface for SurfaceGroupNode {
    fn type_name(&self) -> &'static str { "SurfaceGroup" }

    fn surface_id(&self) -> Option<SurfaceId> { None }

    fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.layout().all_surface_ids()
    }

    fn focused_surface_id(&self) -> Option<SurfaceId> {
        Some(self.focused_surface)
    }

    fn has_terminal(&self) -> bool { true }

    fn focused_terminal(&self) -> Option<&Terminal> {
        SurfaceGroupNode::focused_terminal(self)
    }

    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        SurfaceGroupNode::focused_terminal_mut(self)
    }

    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        self.layout().find_terminal(surface_id)
    }

    fn find_terminal_surface(&self, surface_id: SurfaceId) -> Option<&TerminalSurface> {
        self.layout().find_surface_node(surface_id)
    }

    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        self.layout_mut().find_terminal_mut(surface_id)
    }

    fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        self.compute_rects(rect)
    }

    fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        SurfaceGroupNode::resize_all(self, rect, cell_width, cell_height)
    }

    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        self.layout_mut().collect_terminals_mut(out);
    }

    fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        self.layout_mut().for_each_terminal_mut_dyn(f);
    }

    fn as_surface_group(&self) -> Option<&SurfaceGroupNode> { Some(self) }

    fn as_surface_group_mut(&mut self) -> Option<&mut SurfaceGroupNode> { Some(self) }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "SurfaceGroup",
            "focused_surface": self.focused_surface,
            "surfaces": self.layout().all_surface_ids(),
        })
    }
}
