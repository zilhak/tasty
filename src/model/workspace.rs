use super::{Pane, PaneId, PaneNode, SurfaceId, TabId, WorkspaceId};

/// Workspace - one sidebar item. Contains a PaneLayout (binary split tree of Panes).
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    pane_layout_opt: Option<PaneNode>,
    pub focused_pane: PaneId,
}

impl Workspace {
    /// Create a workspace with a TerminalSurface marker. Caller must have already
    /// `engine.terminals.insert(surface_id, terminal)` for the spawned Terminal.
    pub fn new_with_terminal_marker(
        id: WorkspaceId,
        name: String,
        pane_id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
    ) -> Self {
        let pane = Pane::new_with_terminal_marker(pane_id, tab_id, surface_id);
        let focused_pane = pane_id;
        Self {
            id,
            name,
            subtitle: String::new(),
            description: String::new(),
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
        }
    }

    /// Access the pane layout (always valid during normal operation).
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout(&self) -> &PaneNode {
        self.pane_layout_opt
            .as_ref()
            .expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

    /// Access the pane layout mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout_mut(&mut self) -> &mut PaneNode {
        self.pane_layout_opt
            .as_mut()
            .expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

    /// Create a workspace from a pre-built Pane (for non-terminal surface types).
    pub fn new_with_pane(id: WorkspaceId, name: String, pane: Pane) -> Self {
        let focused_pane = pane.id;
        Self {
            id,
            name,
            subtitle: String::new(),
            description: String::new(),
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
        }
    }

    /// Create a workspace from a restored pane layout (no PTY creation needed).
    pub fn from_restored(
        id: WorkspaceId,
        name: String,
        subtitle: String,
        pane_layout: PaneNode,
        focused_pane: PaneId,
    ) -> Self {
        Self {
            id,
            name,
            subtitle,
            description: String::new(),
            pane_layout_opt: Some(pane_layout),
            focused_pane,
        }
    }

    /// Collect all surface IDs in this workspace.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.pane_layout().all_surface_ids()
    }

    /// Produce a JSON tree representation of this workspace.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let panes: Vec<_> = self
            .pane_layout()
            .all_pane_ids()
            .iter()
            .filter_map(|&pid| self.pane_layout().find_pane(pid))
            .map(|pane| {
                let mut p = pane.to_tree_json();
                p["focused"] = serde_json::json!(pane.id == self.focused_pane);
                p
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "panes": panes,
        })
    }
}
