use tasty_terminal::Terminal;
use super::{Rect, SplitDirection, SurfaceId};
use super::pane_tree::FocusDirection;
pub use super::surface_layout::SurfaceGroupLayout;

/// Single terminal instance.
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
    /// If lazy init is enabled and terminal hasn't been spawned yet,
    /// this holds the deferred spawn parameters.
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
