use tasty_terminal::{Terminal, Waker};
use super::{
    DividerInfo, PaneId, Panel, Rect, SplitDirection, SurfaceId, SurfaceNode, TabId,
};

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

    /// Remove a pane from the tree by promoting its sibling.
    /// Returns true if the pane was found and removed.
    /// Returns false for root leaf (can't close the only pane).
    pub fn close_pane(&mut self, target_id: PaneId) -> bool {
        match self {
            PaneNode::Leaf(_) => false, // Can't close the root pane
            PaneNode::Split {
                first, second, ..
            } => {
                // Check if first child is the target leaf
                let first_is_target =
                    matches!(first.as_ref(), PaneNode::Leaf(p) if p.id == target_id);
                let second_is_target =
                    matches!(second.as_ref(), PaneNode::Leaf(p) if p.id == target_id);

                if first_is_target {
                    // Remove first, promote second
                    let old = std::mem::replace(
                        self,
                        PaneNode::Leaf(Pane {
                            id: 0,
                            tabs: vec![],
                            active_tab: 0,
                        }),
                    );
                    if let PaneNode::Split { second, .. } = old {
                        *self = *second;
                    }
                    return true;
                }
                if second_is_target {
                    let old = std::mem::replace(
                        self,
                        PaneNode::Leaf(Pane {
                            id: 0,
                            tabs: vec![],
                            active_tab: 0,
                        }),
                    );
                    if let PaneNode::Split { first, .. } = old {
                        *self = *first;
                    }
                    return true;
                }
                // Recurse into children
                first.close_pane(target_id) || second.close_pane(target_id)
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

    /// Collect divider rectangles (the gap between split panes).
    /// Each returned Rect is the thin strip that should be drawn as a border.
    pub fn collect_dividers(&self, rect: Rect) -> Vec<Rect> {
        match self {
            PaneNode::Leaf(_) => vec![],
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let gap = super::PANE_BORDER_WIDTH;
                let (r1, r2) = rect.split(*direction, *ratio);
                // The divider sits in the gap between r1 and r2
                let divider = match direction {
                    SplitDirection::Vertical => Rect {
                        x: r1.x + r1.width,
                        y: rect.y,
                        width: gap,
                        height: rect.height,
                    },
                    SplitDirection::Horizontal => Rect {
                        x: rect.x,
                        y: r1.y + r1.height,
                        width: rect.width,
                        height: gap,
                    },
                };
                let mut result = vec![divider];
                result.extend(first.collect_dividers(r1));
                result.extend(second.collect_dividers(r2));
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

    /// Visit all terminals (mutable) in this PaneNode tree, calling `f(surface_id, &mut terminal)` on each.
    pub fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal),
    {
        match self {
            PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    tab.panel_mut().for_each_terminal_mut(f);
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.for_each_terminal_mut(f);
                second.for_each_terminal_mut(f);
            }
        }
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

    /// Find a divider near the given point. Returns divider info if the cursor
    /// is within `threshold` pixels of a split border.
    pub fn find_divider_at(&self, x: f32, y: f32, rect: Rect, threshold: f32) -> Option<DividerInfo> {
        match self {
            PaneNode::Leaf(_) => None,
            PaneNode::Split { direction, ratio, first, second } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                let divider_pos = match direction {
                    SplitDirection::Vertical => r1.x + r1.width,
                    SplitDirection::Horizontal => r1.y + r1.height,
                };
                let cursor_pos = match direction {
                    SplitDirection::Vertical => x,
                    SplitDirection::Horizontal => y,
                };
                // Check if cursor is within threshold of this divider
                // and within the perpendicular bounds
                let in_bounds = match direction {
                    SplitDirection::Vertical => y >= rect.y && y < rect.y + rect.height,
                    SplitDirection::Horizontal => x >= rect.x && x < rect.x + rect.width,
                };
                if in_bounds && (cursor_pos - divider_pos).abs() < threshold {
                    return Some(DividerInfo {
                        direction: *direction,
                        split_rect: rect,
                    });
                }
                // Recurse into children
                first.find_divider_at(x, y, r1, threshold)
                    .or_else(|| second.find_divider_at(x, y, r2, threshold))
            }
        }
    }

    /// Update the ratio of the split node whose rect approximately matches `split_rect`.
    /// Returns true if a matching split was found and updated.
    pub fn update_ratio_for_rect(&mut self, split_rect: Rect, new_ratio: f32, current_rect: Rect) -> bool {
        match self {
            PaneNode::Leaf(_) => false,
            PaneNode::Split { direction, ratio, first, second } => {
                if current_rect.approx_eq(&split_rect) {
                    *ratio = new_ratio.clamp(0.1, 0.9);
                    return true;
                }
                let (r1, r2) = current_rect.split(*direction, *ratio);
                first.update_ratio_for_rect(split_rect, new_ratio, r1)
                    || second.update_ratio_for_rect(split_rect, new_ratio, r2)
            }
        }
    }
}

/// A screen region with its own independent tab bar.
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

impl Pane {
    /// Create a Pane with a custom shell.
    pub fn new_with_shell(
        id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::new_with_shell_args(cols, rows, shell, shell_args, waker)?;
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

    /// Add a new tab with a custom shell.
    pub fn add_tab_with_shell(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
    ) -> anyhow::Result<()> {
        let terminal = Terminal::new_with_shell_args(cols, rows, shell, shell_args, waker)?;
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

    /// Split the active panel's focused surface with a custom shell.
    pub fn split_active_surface_with_shell(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
    ) -> anyhow::Result<()> {
        // Pre-create the new terminal before any structural mutation.
        // If Terminal::new fails, panel is untouched.
        let new_terminal = Terminal::new_with_shell_args(cols, rows, shell, shell_args, waker)?;
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

    /// Close the tab at the given index. Returns false if the tab can't be closed
    /// (e.g., it's the last tab).
    pub fn close_tab(&mut self, tab_index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return false; // Can't close last tab
        }
        if tab_index < self.tabs.len() {
            self.tabs.remove(tab_index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            true
        } else {
            false
        }
    }

    /// Close the currently active tab. Returns false if it's the last tab.
    pub fn close_active_tab(&mut self) -> bool {
        self.close_tab(self.active_tab)
    }

    /// Get the focused terminal (follows through Panel -> SurfaceGroup).
    pub fn active_terminal(&self) -> Option<&Terminal> {
        self.active_panel()?.focused_terminal()
    }

    /// Get the focused terminal (mutable).
    pub fn active_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.active_panel_mut()?.focused_terminal_mut()
    }

    /// Find a terminal by surface ID across all tabs (immutable).
    pub fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        for tab in &self.tabs {
            if let Some(t) = tab.panel().find_terminal(surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by surface ID across all tabs (mutable).
    pub fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        for tab in &mut self.tabs {
            if let Some(t) = tab.panel_mut().find_terminal_mut(surface_id) {
                return Some(t);
            }
        }
        None
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
    pub(crate) fn take_panel(&mut self) -> Panel {
        self.panel_opt.take().expect("BUG: panel already taken")
    }

    /// Put the panel back after structural mutations.
    pub(crate) fn put_panel(&mut self, panel: Panel) {
        self.panel_opt = Some(panel);
    }
}
