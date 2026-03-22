use tasty_terminal::{Terminal, Waker};
use super::{PaneId, PaneNode, Pane, SurfaceId, TabId, WorkspaceId};

/// Workspace - one sidebar item. Contains a PaneLayout (binary split tree of Panes).
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    pane_layout_opt: Option<PaneNode>,
    pub focused_pane: PaneId,
}

impl Workspace {
    /// Create a workspace with one default Pane containing one Tab with one Terminal.
    pub fn new(
        id: WorkspaceId,
        name: String,
        cols: usize,
        rows: usize,
        pane_id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        waker: Waker,
    ) -> anyhow::Result<Self> {
        Self::new_with_shell(id, name, cols, rows, pane_id, tab_id, surface_id, None, waker)
    }

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
        waker: Waker,
    ) -> anyhow::Result<Self> {
        let pane = Pane::new_with_shell(pane_id, tab_id, surface_id, cols, rows, shell, waker)?;
        let focused_pane = pane_id;
        Ok(Self {
            id,
            name,
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

    /// Temporarily take ownership of the pane layout for structural mutations.
    /// MUST be followed by `put_pane_layout()`. Panics if already taken.
    #[track_caller]
    pub fn take_pane_layout(&mut self) -> PaneNode {
        self.pane_layout_opt.take().expect("BUG: pane_layout already taken")
    }

    /// Put the pane layout back after structural mutations.
    pub fn put_pane_layout(&mut self, layout: PaneNode) {
        self.pane_layout_opt = Some(layout);
    }
}
