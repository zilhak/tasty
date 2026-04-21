use tasty_terminal::Terminal;
use super::{PhysicalPx, Rect, SurfaceId};
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

    fn is_gpu_rendered(&self) -> bool { true }

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
        let cols = (rect.width / cell_width).floor().max(PhysicalPx(1.0)).value() as usize;
        let rows = (rect.height / cell_height).floor().max(PhysicalPx(1.0)).value() as usize;
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
    fn take_terminal_surface(self: Box<Self>) -> Option<TerminalSurface> { Some(*self) }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": self.terminal.cols(),
            "rows": self.terminal.rows(),
        })
    }
}
