//! Surface 의 "busy" 상태 폴링/조회. `refresh_busy_surfaces` 는 매 tick 에 호출되어
//! 각 PTY 의 foreground process 를 비교해 `engine.busy_surfaces` 를 갱신한다.

use super::AppState;

impl AppState {
    /// Recompute `engine.busy_surfaces` by polling every PTY's foreground
    /// process. Returns true if the set changed (caller should redraw).
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        let mut busy: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for ws in &mut self.engine.workspaces {
            ws.pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, terminal| {
                    if terminal.is_busy() {
                        busy.insert(sid);
                    }
                });
        }
        let changed = self.engine.busy_surfaces != busy;
        self.engine.busy_surfaces = busy;
        changed
    }

    /// Whether the given surface is currently running a non-shell foreground
    /// program (cached value from the last `refresh_busy_surfaces` poll).
    pub fn is_surface_busy(&self, surface_id: u32) -> bool {
        self.engine.busy_surfaces.contains(&surface_id)
    }

    /// Whether any surface in the given list is busy.
    pub fn any_busy(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|sid| self.engine.busy_surfaces.contains(sid))
    }

    /// Number of busy surfaces among the given list.
    pub fn busy_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.engine.busy_surfaces.contains(sid))
            .count()
    }

}
