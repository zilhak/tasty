use tasty_terminal::Waker;
use super::{PaneId, PaneNode, Pane, SurfaceId, TabId, WorkspaceId};

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
    /// Create a workspace with a custom shell.
    pub fn new_with_shell(
        id: WorkspaceId,
        name: String,
        cols: usize,
        rows: usize,
        pane_id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
    ) -> anyhow::Result<Self> {
        let pane = Pane::new_with_shell(pane_id, tab_id, surface_id, cols, rows, shell, shell_args, waker)?;
        let focused_pane = pane_id;
        Ok(Self {
            id,
            name,
            subtitle: String::new(),
            description: String::new(),
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
        })
    }

    /// Access the pane layout (always valid during normal operation).
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout(&self) -> &PaneNode {
        self.pane_layout_opt.as_ref().expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

    /// Access the pane layout mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout_mut(&mut self) -> &mut PaneNode {
        self.pane_layout_opt.as_mut().expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

}
