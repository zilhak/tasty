use crate::terminal::Terminal;

pub type WorkspaceId = u32;
pub type PaneId = u32;
pub type TabId = u32;
pub type SurfaceId = u32;

/// A pixel rectangle used for viewport/scissor calculations.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn split(self, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
        match direction {
            SplitDirection::Vertical => {
                let first_w = (self.width * ratio).floor();
                let second_w = self.width - first_w;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: first_w,
                        height: self.height,
                    },
                    Rect {
                        x: self.x + first_w,
                        y: self.y,
                        width: second_w,
                        height: self.height,
                    },
                )
            }
            SplitDirection::Horizontal => {
                let first_h = (self.height * ratio).floor();
                let second_h = self.height - first_h;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: self.width,
                        height: first_h,
                    },
                    Rect {
                        x: self.x,
                        y: self.y + first_h,
                        width: self.width,
                        height: second_h,
                    },
                )
            }
        }
    }
}

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
    ) -> anyhow::Result<Self> {
        Self::new_with_shell(id, name, cols, rows, pane_id, tab_id, surface_id, None)
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
    ) -> anyhow::Result<Self> {
        let pane = Pane::new_with_shell(pane_id, tab_id, surface_id, cols, rows, shell)?;
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

/// Binary tree of Panes - physical screen splits.
/// Each leaf is a Pane with its own independent tab bar.
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// Split the target pane in-place. The new `Pane` must be pre-created by the caller
    /// (so PTY creation happens before any structural mutation).
    ///
    /// Returns `Ok(new_pane)` on failure (target not found), `Ok(())` on success,
    /// encoded as `Result<Option<Pane>, !>` - actually just uses `Option<Pane>` return.
    ///
    /// API: returns `Some(new_pane)` if NOT found (caller can decide what to do),
    /// returns `None` if found and split was performed.
    pub fn split_pane_in_place(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Option<Pane> {
        match self {
            PaneNode::Leaf(pane) if pane.id == target_id => {
                let new_pane_id = new_pane.id;
                // Replace self (Leaf(orig)) with Leaf(new_pane); returns Leaf(orig).
                let original_leaf = std::mem::replace(self, PaneNode::Leaf(new_pane));
                // Replace self (Leaf(new_pane)) with the final Split; returns Leaf(new_pane).
                let new_leaf = std::mem::replace(
                    self,
                    PaneNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(original_leaf),
                        // placeholder - replaced immediately below
                        second: Box::new(PaneNode::Leaf(Pane {
                            id: new_pane_id,
                            tabs: vec![],
                            active_tab: 0,
                        })),
                    },
                );
                // Put the real new leaf into second.
                if let PaneNode::Split { second, .. } = self {
                    *second = Box::new(new_leaf);
                }
                None // success
            }
            PaneNode::Leaf(_) => Some(new_pane), // not found, return pane back
            PaneNode::Split { first, second, .. } => {
                // Try first; if not found, new_pane is returned and we try second.
                let remaining = first.split_pane_in_place(target_id, direction, new_pane);
                if let Some(pane) = remaining {
                    second.split_pane_in_place(target_id, direction, pane)
                } else {
                    None // success in first
                }
            }
        }
    }

    /// Return a reference to the first (leftmost/topmost) pane in the tree.
    pub fn first_pane(&self) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) => Some(pane),
            PaneNode::Split { first, .. } => first.first_pane(),
        }
    }

    /// Compute pixel rectangles for each Pane given a total rect.
    pub fn compute_rects(&self, rect: Rect) -> Vec<(PaneId, Rect)> {
        match self {
            PaneNode::Leaf(pane) => vec![(pane.id, rect)],
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                let mut result = first.compute_rects(r1);
                result.extend(second.compute_rects(r2));
                result
            }
        }
    }

    /// Find a Pane by ID (immutable).
    pub fn find_pane(&self, id: PaneId) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.find_pane(id).or_else(|| second.find_pane(id))
            }
        }
    }

    /// Find a Pane by ID (mutable).
    pub fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                if let Some(p) = first.find_pane_mut(id) {
                    Some(p)
                } else {
                    second.find_pane_mut(id)
                }
            }
        }
    }

    /// Collect references to all terminals in this tree.
    pub fn all_terminals(&self) -> Vec<&Terminal> {
        match self {
            PaneNode::Leaf(pane) => pane.all_terminals(),
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_terminals();
                result.extend(second.all_terminals());
                result
            }
        }
    }

    /// Collect mutable references to all terminals in this tree.
    pub fn all_terminals_mut(&mut self) -> Vec<&mut Terminal> {
        match self {
            PaneNode::Leaf(pane) => pane.all_terminals_mut(),
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_terminals_mut();
                result.extend(second.all_terminals_mut());
                result
            }
        }
    }

    /// Process all terminals. Returns true if any changed.
    pub fn process_all(&mut self) -> bool {
        let mut changed = false;
        for terminal in self.all_terminals_mut() {
            if terminal.process() {
                changed = true;
            }
        }
        changed
    }

    /// Get all pane IDs in order.
    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        match self {
            PaneNode::Leaf(pane) => vec![pane.id],
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_pane_ids();
                result.extend(second.all_pane_ids());
                result
            }
        }
    }

    /// Move focus to the next pane (by ID order). Returns the new focused PaneId if changed.
    pub fn next_pane_id(&self, current: PaneId) -> PaneId {
        let ids = self.all_pane_ids();
        if ids.len() <= 1 {
            return current;
        }
        let pos = ids.iter().position(|&id| id == current).unwrap_or(0);
        ids[(pos + 1) % ids.len()]
    }

    /// Move focus to the previous pane (by ID order). Returns the new focused PaneId if changed.
    pub fn prev_pane_id(&self, current: PaneId) -> PaneId {
        let ids = self.all_pane_ids();
        if ids.len() <= 1 {
            return current;
        }
        let pos = ids.iter().position(|&id| id == current).unwrap_or(0);
        ids[(pos + ids.len() - 1) % ids.len()]
    }
}

/// A screen region with its own independent tab bar.
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

impl Pane {
    /// Create a Pane with one Tab containing a single terminal.
    pub fn new(
        id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<Self> {
        Self::new_with_shell(id, tab_id, surface_id, cols, rows, None)
    }

    /// Create a Pane with a custom shell.
    pub fn new_with_shell(
        id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::new_with_shell(cols, rows, shell)?;
        let tab = Tab {
            id: tab_id,
            name: "Shell".to_string(),
            panel_opt: Some(Panel::Terminal(SurfaceNode {
                id: surface_id,
                terminal,
            })),
        };
        Ok(Self {
            id,
            tabs: vec![tab],
            active_tab: 0,
        })
    }

    /// Add a new tab with a single terminal.
    pub fn add_tab(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        self.add_tab_with_shell(tab_id, surface_id, cols, rows, None)
    }

    /// Add a new tab with a custom shell.
    pub fn add_tab_with_shell(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
    ) -> anyhow::Result<()> {
        let terminal = Terminal::new_with_shell(cols, rows, shell)?;
        let tab = Tab {
            id: tab_id,
            name: format!("Shell {}", self.tabs.len() + 1),
            panel_opt: Some(Panel::Terminal(SurfaceNode {
                id: surface_id,
                terminal,
            })),
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        Ok(())
    }

    /// Get the active tab's panel. Returns None if tabs are empty.
    pub fn active_panel(&self) -> Option<&Panel> {
        if self.tabs.is_empty() { return None; }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        Some(self.tabs[idx].panel())
    }

    /// Get the active tab's panel (mutable). Returns None if tabs are empty.
    pub fn active_panel_mut(&mut self) -> Option<&mut Panel> {
        if self.tabs.is_empty() { return None; }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        Some(self.tabs[idx].panel_mut())
    }

    /// Split the active panel's focused surface.
    pub fn split_active_surface(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        self.split_active_surface_with_shell(direction, new_surface_id, cols, rows, None)
    }

    /// Split the active panel's focused surface with a custom shell.
    pub fn split_active_surface_with_shell(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
    ) -> anyhow::Result<()> {
        // Pre-create the new terminal before any structural mutation.
        // If Terminal::new fails, panel is untouched.
        let new_terminal = Terminal::new_with_shell(cols, rows, shell)?;
        if self.tabs.is_empty() {
            return Ok(()); // nothing to split
        }
        let active = self.active_tab.min(self.tabs.len() - 1);
        let tab = &mut self.tabs[active];
        // take/put is safe here: split_surface_with_terminal is infallible.
        let old_panel = tab.take_panel();
        tab.put_panel(old_panel.split_surface_with_terminal(direction, new_surface_id, new_terminal));
        Ok(())
    }

    /// Get the focused terminal (follows through Panel -> SurfaceGroup).
    pub fn active_terminal(&self) -> Option<&Terminal> {
        self.active_panel()?.focused_terminal()
    }

    /// Get the focused terminal (mutable).
    pub fn active_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.active_panel_mut()?.focused_terminal_mut()
    }

    /// Switch to next tab.
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to previous tab.
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Collect all terminals from all tabs in this Pane.
    pub fn all_terminals(&self) -> Vec<&Terminal> {
        let mut result = Vec::new();
        for tab in &self.tabs {
            tab.panel().collect_terminals(&mut result);
        }
        result
    }

    /// Collect all terminals (mutable) from all tabs in this Pane.
    pub fn all_terminals_mut(&mut self) -> Vec<&mut Terminal> {
        let mut result = Vec::new();
        for tab in &mut self.tabs {
            tab.panel_mut().collect_terminals_mut(&mut result);
        }
        result
    }
}

/// One tab in a Pane's tab bar. Maps to a Panel.
pub struct Tab {
    pub id: TabId,
    pub name: String,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    panel_opt: Option<Panel>,
}

impl Tab {
    /// Access the panel (always valid during normal operation).
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn panel(&self) -> &Panel {
        self.panel_opt.as_ref().expect("BUG: panel accessed during structural mutation (between take/put)")
    }

    /// Access the panel mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn panel_mut(&mut self) -> &mut Panel {
        self.panel_opt.as_mut().expect("BUG: panel accessed during structural mutation (between take/put)")
    }

    /// Take ownership of the panel for structural mutations.
    /// MUST be followed by `put_panel()`. Panics if already taken.
    #[track_caller]
    fn take_panel(&mut self) -> Panel {
        self.panel_opt.take().expect("BUG: panel already taken")
    }

    /// Put the panel back after structural mutations.
    fn put_panel(&mut self, panel: Panel) {
        self.panel_opt = Some(panel);
    }
}

/// Content type within a Tab.
pub enum Panel {
    /// A single terminal instance.
    Terminal(SurfaceNode),
    /// A split within a tab - appears as ONE tab but renders multiple terminals.
    SurfaceGroup(SurfaceGroupNode),
}

impl Panel {
    /// Get the focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) => Some(&node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal(),
        }
    }

    /// Get the focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) => Some(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal_mut(),
        }
    }

    /// Collect all terminals in this panel.
    pub fn collect_terminals<'a>(&'a self, out: &mut Vec<&'a Terminal>) {
        match self {
            Panel::Terminal(node) => out.push(&node.terminal),
            Panel::SurfaceGroup(group) => group.layout().collect_terminals(out),
        }
    }

    /// Collect all terminals (mutable) in this panel.
    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            Panel::Terminal(node) => out.push(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().collect_terminals_mut(out),
        }
    }

    /// Get render regions for this panel within the given rect.
    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            Panel::Terminal(node) => vec![(node.id, &node.terminal, rect)],
            Panel::SurfaceGroup(group) => group.compute_rects(rect),
        }
    }

    /// Resize all terminals in this panel.
    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            Panel::Terminal(node) => {
                let cols = ((rect.width - 4.0) / cell_width).floor().max(1.0) as usize;
                let rows = ((rect.height - 4.0) / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            Panel::SurfaceGroup(group) => group.resize_all(rect, cell_width, cell_height),
        }
    }

    /// Split the focused surface. Takes a pre-created terminal (infallible).
    /// Called from Pane::split_active_surface after pre-creation succeeds.
    pub fn split_surface_with_terminal(
        self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        new_terminal: Terminal,
    ) -> Self {
        match self {
            Panel::Terminal(old_node) => {
                let old_surface_id = old_node.id;
                let group = SurfaceGroupNode {
                    layout_opt: Some(SurfaceGroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SurfaceGroupLayout::Single(old_node)),
                        second: Box::new(SurfaceGroupLayout::Single(SurfaceNode {
                            id: new_surface_id,
                            terminal: new_terminal,
                        })),
                        focus_second: true,
                    }),
                    focused_surface: new_surface_id,
                    _first_surface: old_surface_id,
                };
                Panel::SurfaceGroup(group)
            }
            Panel::SurfaceGroup(mut group) => {
                // Pre-built terminal: wrap into SurfaceNode and use infallible split.
                let new_node = SurfaceNode { id: new_surface_id, terminal: new_terminal };
                let target = group.focused_surface;
                let old_layout = group.take_layout();
                let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
                group.put_layout(new_layout);
                group.focused_surface = new_surface_id;
                Panel::SurfaceGroup(group)
            }
        }
    }
}

/// Single terminal instance.
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
}

/// Split within a tab (appears as one tab but renders multiple terminals).
pub struct SurfaceGroupNode {
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    layout_opt: Option<SurfaceGroupLayout>,
    pub focused_surface: SurfaceId,
    /// First surface ID, stored for focus tracking.
    _first_surface: SurfaceId,
}

impl SurfaceGroupNode {
    /// Access the layout (always valid during normal operation).
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn layout(&self) -> &SurfaceGroupLayout {
        self.layout_opt.as_ref().expect("BUG: layout accessed during structural mutation (between take/put)")
    }

    /// Access the layout mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn layout_mut(&mut self) -> &mut SurfaceGroupLayout {
        self.layout_opt.as_mut().expect("BUG: layout accessed during structural mutation (between take/put)")
    }

    /// Take ownership of the layout for structural mutations.
    /// MUST be followed by `put_layout()`. Panics if already taken.
    #[track_caller]
    fn take_layout(&mut self) -> SurfaceGroupLayout {
        self.layout_opt.take().expect("BUG: layout already taken")
    }

    /// Put the layout back.
    fn put_layout(&mut self, layout: SurfaceGroupLayout) {
        self.layout_opt = Some(layout);
    }
}

impl SurfaceGroupNode {
    /// Split the focused surface.
    pub fn split_surface(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        // Pre-create the new terminal BEFORE any structural mutation.
        // If Terminal::new fails, layout_opt is untouched.
        let new_node = SurfaceNode {
            id: new_surface_id,
            terminal: Terminal::new(cols, rows)?,
        };
        let target = self.focused_surface;
        // take/put is safe here: split_with_node is infallible (no error path),
        // so put_layout always runs.
        let old_layout = self.take_layout();
        let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
        self.put_layout(new_layout);
        self.focused_surface = new_surface_id;
        Ok(())
    }

    /// Compute render rects for all surfaces.
    pub fn compute_rects(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        self.layout().render_regions(rect)
    }

    /// Get the focused terminal, falling back to the first terminal if the focus ID is stale.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        let layout = self.layout();
        layout
            .find_terminal(self.focused_surface)
            .or_else(|| layout.first_terminal())
    }

    /// Get the focused terminal (mutable), falling back to the first terminal if stale.
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        let id = self.focused_surface;
        if self.layout().find_terminal(id).is_none() {
            // Reset focused_surface to first available.
            if let Some(first_id) = self.layout().first_surface_id() {
                self.focused_surface = first_id;
            }
        }
        let id = self.focused_surface;
        self.layout_mut().find_terminal_mut(id)
    }

    /// Resize all surfaces.
    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        self.layout_mut().resize_all(rect, cell_width, cell_height);
    }

    /// Move focus forward among surfaces.
    pub fn move_focus_forward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + 1) % ids.len()];
    }

    /// Move focus backward among surfaces.
    pub fn move_focus_backward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + ids.len() - 1) % ids.len()];
    }
}

pub enum SurfaceGroupLayout {
    Single(SurfaceNode),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<SurfaceGroupLayout>,
        second: Box<SurfaceGroupLayout>,
        /// Which branch has focus: false = first, true = second
        focus_second: bool,
    },
}

impl SurfaceGroupLayout {
    /// Split a specific surface by taking ownership (infallible structural mutation).
    /// The new `SurfaceNode` must be pre-created by the caller.
    /// Returns `(new_layout, found)`.
    pub fn split_with_node(
        self,
        target_id: SurfaceId,
        direction: SplitDirection,
        new_node: SurfaceNode,
    ) -> (Self, Option<SurfaceNode>) {
        match self {
            SurfaceGroupLayout::Single(node) if node.id == target_id => {
                (
                    SurfaceGroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SurfaceGroupLayout::Single(node)),
                        second: Box::new(SurfaceGroupLayout::Single(new_node)),
                        focus_second: true,
                    },
                    None, // success - new_node consumed
                )
            }
            SurfaceGroupLayout::Single(node) => {
                // not found - return new_node back
                (SurfaceGroupLayout::Single(node), Some(new_node))
            }
            SurfaceGroupLayout::Split {
                direction: d,
                ratio,
                first,
                second,
                focus_second,
            } => {
                let (new_first, remaining) =
                    first.split_with_node(target_id, direction, new_node);
                if let Some(node) = remaining {
                    let (new_second, still_remaining) =
                        second.split_with_node(target_id, direction, node);
                    (
                        SurfaceGroupLayout::Split {
                            direction: d,
                            ratio,
                            first: Box::new(new_first),
                            second: Box::new(new_second),
                            focus_second,
                        },
                        still_remaining,
                    )
                } else {
                    (
                        SurfaceGroupLayout::Split {
                            direction: d,
                            ratio,
                            first: Box::new(new_first),
                            second,
                            focus_second,
                        },
                        None,
                    )
                }
            }
        }
    }

    /// Return the first (leftmost) terminal in the tree.
    pub fn first_terminal(&self) -> Option<&Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => Some(&node.terminal),
            SurfaceGroupLayout::Split { first, .. } => first.first_terminal(),
        }
    }

    /// Return the first (leftmost) surface ID in the tree.
    pub fn first_surface_id(&self) -> Option<SurfaceId> {
        match self {
            SurfaceGroupLayout::Single(node) => Some(node.id),
            SurfaceGroupLayout::Split { first, .. } => first.first_surface_id(),
        }
    }

    /// Find a terminal by surface ID.
    pub fn find_terminal(&self, id: SurfaceId) -> Option<&Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if node.id == id {
                    Some(&node.terminal)
                } else {
                    None
                }
            }
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.find_terminal(id).or_else(|| second.find_terminal(id))
            }
        }
    }

    /// Find a terminal by surface ID (mutable).
    pub fn find_terminal_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if node.id == id {
                    Some(&mut node.terminal)
                } else {
                    None
                }
            }
            SurfaceGroupLayout::Split { first, second, .. } => {
                if let Some(t) = first.find_terminal_mut(id) {
                    Some(t)
                } else {
                    second.find_terminal_mut(id)
                }
            }
        }
    }

    /// Compute render regions.
    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            SurfaceGroupLayout::Single(node) => vec![(node.id, &node.terminal, rect)],
            SurfaceGroupLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                let mut result = first.render_regions(r1);
                result.extend(second.render_regions(r2));
                result
            }
        }
    }

    /// Resize all terminals.
    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            SurfaceGroupLayout::Single(node) => {
                let cols = ((rect.width - 4.0) / cell_width).floor().max(1.0) as usize;
                let rows = ((rect.height - 4.0) / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            SurfaceGroupLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                first.resize_all(r1, cell_width, cell_height);
                second.resize_all(r2, cell_width, cell_height);
            }
        }
    }

    /// Collect all surface IDs in order.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            SurfaceGroupLayout::Single(node) => vec![node.id],
            SurfaceGroupLayout::Split { first, second, .. } => {
                let mut result = first.all_surface_ids();
                result.extend(second.all_surface_ids());
                result
            }
        }
    }

    /// Collect all terminals.
    pub fn collect_terminals<'a>(&'a self, out: &mut Vec<&'a Terminal>) {
        match self {
            SurfaceGroupLayout::Single(node) => out.push(&node.terminal),
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.collect_terminals(out);
                second.collect_terminals(out);
            }
        }
    }

    /// Collect all terminals (mutable).
    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            SurfaceGroupLayout::Single(node) => out.push(&mut node.terminal),
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.collect_terminals_mut(out);
                second.collect_terminals_mut(out);
            }
        }
    }

    /// Process all terminals. Returns true if any changed.
    pub fn process_all(&mut self) -> bool {
        match self {
            SurfaceGroupLayout::Single(node) => node.terminal.process(),
            SurfaceGroupLayout::Split { first, second, .. } => {
                let a = first.process_all();
                let b = second.process_all();
                a || b
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}
