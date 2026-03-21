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
        let pane = Pane::new(pane_id, tab_id, surface_id, cols, rows)?;
        let focused_pane = pane_id;
        Ok(Self {
            id,
            name,
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
        })
    }

    /// Access the pane layout (always valid during normal operation).
    pub fn pane_layout(&self) -> &PaneNode {
        self.pane_layout_opt.as_ref().expect("pane_layout taken")
    }

    /// Access the pane layout mutably.
    pub fn pane_layout_mut(&mut self) -> &mut PaneNode {
        self.pane_layout_opt.as_mut().expect("pane_layout taken")
    }

    /// Temporarily take ownership of the pane layout for structural mutations.
    pub fn take_pane_layout(&mut self) -> PaneNode {
        self.pane_layout_opt.take().expect("pane_layout taken")
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

    /// Split a pane into two by taking ownership (no placeholder PTY created).
    /// Returns the new tree and whether the split was performed.
    pub fn split_pane_owned(
        self,
        pane_id: PaneId,
        direction: SplitDirection,
        new_pane_id: PaneId,
        new_tab_id: TabId,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<(Self, bool)> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id != pane_id {
                    return Ok((PaneNode::Leaf(pane), false));
                }
                let new_pane = Pane::new(new_pane_id, new_tab_id, new_surface_id, cols, rows)?;
                Ok((
                    PaneNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(PaneNode::Leaf(pane)),
                        second: Box::new(PaneNode::Leaf(new_pane)),
                    },
                    true,
                ))
            }
            PaneNode::Split {
                direction: d,
                ratio,
                first,
                second,
            } => {
                let (new_first, found) = first.split_pane_owned(
                    pane_id,
                    direction,
                    new_pane_id,
                    new_tab_id,
                    new_surface_id,
                    cols,
                    rows,
                )?;
                if found {
                    return Ok((
                        PaneNode::Split {
                            direction: d,
                            ratio,
                            first: Box::new(new_first),
                            second,
                        },
                        true,
                    ));
                }
                let (new_second, found) = second.split_pane_owned(
                    pane_id,
                    direction,
                    new_pane_id,
                    new_tab_id,
                    new_surface_id,
                    cols,
                    rows,
                )?;
                Ok((
                    PaneNode::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                    },
                    found,
                ))
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
        let terminal = Terminal::new(cols, rows)?;
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
        let terminal = Terminal::new(cols, rows)?;
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

    /// Get the active tab's panel.
    pub fn active_panel(&self) -> &Panel {
        self.tabs[self.active_tab].panel()
    }

    /// Get the active tab's panel (mutable).
    pub fn active_panel_mut(&mut self) -> &mut Panel {
        self.tabs[self.active_tab].panel_mut()
    }

    /// Split the active panel's focused surface (no placeholder PTY created).
    pub fn split_active_surface(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        let tab = &mut self.tabs[self.active_tab];
        let old_panel = tab.take_panel();
        tab.put_panel(old_panel.split_surface_owned(direction, new_surface_id, cols, rows)?);
        Ok(())
    }

    /// Get the focused terminal (follows through Panel → SurfaceGroup).
    pub fn active_terminal(&self) -> &Terminal {
        self.active_panel().focused_terminal()
    }

    /// Get the focused terminal (mutable).
    pub fn active_terminal_mut(&mut self) -> &mut Terminal {
        self.active_panel_mut().focused_terminal_mut()
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
    pub fn panel(&self) -> &Panel {
        self.panel_opt.as_ref().expect("panel taken")
    }

    /// Access the panel mutably.
    pub fn panel_mut(&mut self) -> &mut Panel {
        self.panel_opt.as_mut().expect("panel taken")
    }

    /// Take ownership of the panel for structural mutations.
    fn take_panel(&mut self) -> Panel {
        self.panel_opt.take().expect("panel taken")
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
    pub fn focused_terminal(&self) -> &Terminal {
        match self {
            Panel::Terminal(node) => &node.terminal,
            Panel::SurfaceGroup(group) => group.focused_terminal(),
        }
    }

    /// Get the focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> &mut Terminal {
        match self {
            Panel::Terminal(node) => &mut node.terminal,
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

    /// Split the focused surface by taking ownership (no placeholder PTY created).
    /// Returns the new Panel.
    pub fn split_surface_owned(
        self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<Self> {
        match self {
            Panel::Terminal(old_node) => {
                let old_surface_id = old_node.id;
                let new_terminal = Terminal::new(cols, rows)?;
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
                Ok(Panel::SurfaceGroup(group))
            }
            Panel::SurfaceGroup(mut group) => {
                group.split_surface(direction, new_surface_id, cols, rows)?;
                Ok(Panel::SurfaceGroup(group))
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
    pub fn layout(&self) -> &SurfaceGroupLayout {
        self.layout_opt.as_ref().expect("layout taken")
    }

    /// Access the layout mutably.
    pub fn layout_mut(&mut self) -> &mut SurfaceGroupLayout {
        self.layout_opt.as_mut().expect("layout taken")
    }

    /// Take ownership of the layout for structural mutations.
    fn take_layout(&mut self) -> SurfaceGroupLayout {
        self.layout_opt.take().expect("layout taken")
    }

    /// Put the layout back.
    fn put_layout(&mut self, layout: SurfaceGroupLayout) {
        self.layout_opt = Some(layout);
    }
}

impl SurfaceGroupNode {
    /// Split the focused surface (no placeholder PTY created).
    pub fn split_surface(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        let old_layout = self.take_layout();
        let (new_layout, _) =
            old_layout.split_owned(self.focused_surface, direction, new_surface_id, cols, rows)?;
        self.put_layout(new_layout);
        self.focused_surface = new_surface_id;
        Ok(())
    }

    /// Compute render rects for all surfaces.
    pub fn compute_rects(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        self.layout().render_regions(rect)
    }

    /// Get the focused terminal.
    pub fn focused_terminal(&self) -> &Terminal {
        self.layout()
            .find_terminal(self.focused_surface)
            .expect("focused surface not found")
    }

    /// Get the focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> &mut Terminal {
        let id = self.focused_surface;
        self.layout_mut()
            .find_terminal_mut(id)
            .expect("focused surface not found")
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
    /// Split a specific surface by ID (takes ownership, no placeholder PTY).
    pub fn split_owned(
        self,
        target_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<(Self, bool)> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if node.id != target_id {
                    return Ok((SurfaceGroupLayout::Single(node), false));
                }
                let new_terminal = Terminal::new(cols, rows)?;
                Ok((
                    SurfaceGroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SurfaceGroupLayout::Single(node)),
                        second: Box::new(SurfaceGroupLayout::Single(SurfaceNode {
                            id: new_surface_id,
                            terminal: new_terminal,
                        })),
                        focus_second: true,
                    },
                    true,
                ))
            }
            SurfaceGroupLayout::Split {
                direction: d,
                ratio,
                first,
                second,
                focus_second,
            } => {
                let (new_first, found) =
                    first.split_owned(target_id, direction, new_surface_id, cols, rows)?;
                if found {
                    return Ok((
                        SurfaceGroupLayout::Split {
                            direction: d,
                            ratio,
                            first: Box::new(new_first),
                            second,
                            focus_second,
                        },
                        true,
                    ));
                }
                let (new_second, found) =
                    second.split_owned(target_id, direction, new_surface_id, cols, rows)?;
                Ok((
                    SurfaceGroupLayout::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                        focus_second,
                    },
                    found,
                ))
            }
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
