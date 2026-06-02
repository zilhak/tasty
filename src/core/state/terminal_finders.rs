use tasty_terminal::Terminal;

use super::CoreState;

impl CoreState {
    /// Check if this engine owns a surface with the given id (any kind, not just terminal).
    pub fn has_surface(&self, surface_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.all_surface_ids().contains(&surface_id))
    }

    /// Check if this engine owns a workspace with the given id.
    pub fn has_workspace(&self, workspace_id: u32) -> bool {
        self.workspaces.iter().any(|ws| ws.id == workspace_id)
    }

    /// Check if this engine owns a pane with the given id.
    pub fn has_pane(&self, pane_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.pane_layout().all_pane_ids().contains(&pane_id))
    }

    /// Find a terminal by surface ID (immutable). `TerminalStore` 가 source of truth.
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.terminals.get(surface_id)
    }

    /// Find a terminal by surface ID (mutable). `TerminalStore` 가 source of truth.
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        self.terminals.get_mut(surface_id)
    }
}
