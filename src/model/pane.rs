use tasty_terminal::{Terminal, Waker};
use super::{
    DividerInfo, ExplorerPanel, MarkdownPanel, PaneId, Panel, Rect, SplitDirection, SurfaceId,
    SurfaceNode, TabId,
};

/// Directional focus movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Which side of a split we descended into while building a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathSide {
    First,
    Second,
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

    /// Move focus in a direction based on the split tree structure.
    /// Returns the pane_id to focus, or None if movement is not possible.
    pub fn directional_focus(&self, current_pane_id: PaneId, direction: FocusDirection) -> Option<PaneId> {
        // path entries: (split_direction, side_we_went, sibling_subtree)
        let mut path: Vec<(SplitDirection, PathSide, &PaneNode)> = Vec::new();
        if !self.build_path_to(current_pane_id, &mut path) {
            return None;
        }

        // Walk the path backwards looking for a split that can be crossed
        for (split_dir, side, sibling) in path.iter().rev() {
            if Self::direction_matches_split(*split_dir, direction) {
                let want_first = Self::direction_wants_first(direction);
                let currently_first = *side == PathSide::First;
                // We can cross if we're on the opposite side from where we want to go
                if currently_first != want_first {
                    return Some(sibling.edge_leaf(direction));
                }
            }
        }
        None
    }

    /// Build the path from root to the pane with the given id.
    /// Returns true if found, populating `path` with (split_dir, side, sibling) entries.
    fn build_path_to<'a>(
        &'a self,
        target_id: PaneId,
        path: &mut Vec<(SplitDirection, PathSide, &'a PaneNode)>,
    ) -> bool {
        match self {
            PaneNode::Leaf(pane) => pane.id == target_id,
            PaneNode::Split { direction, first, second, .. } => {
                // Try first subtree
                path.push((*direction, PathSide::First, second.as_ref()));
                if first.build_path_to(target_id, path) {
                    return true;
                }
                path.pop();

                // Try second subtree
                path.push((*direction, PathSide::Second, first.as_ref()));
                if second.build_path_to(target_id, path) {
                    return true;
                }
                path.pop();

                false
            }
        }
    }

    /// Find the edge leaf in the direction of movement within this subtree.
    /// - Moving Left  → rightmost leaf (closest to the left edge we're crossing from)
    /// - Moving Right → leftmost leaf
    /// - Moving Up    → bottommost leaf
    /// - Moving Down  → topmost leaf
    fn edge_leaf(&self, direction: FocusDirection) -> PaneId {
        match self {
            PaneNode::Leaf(pane) => pane.id,
            PaneNode::Split { first, second, .. } => match direction {
                // Left/Up: we want the "far" end of the sibling, so go to second (right/bottom)
                FocusDirection::Left | FocusDirection::Up => second.edge_leaf(direction),
                // Right/Down: we want the near end, so go to first (left/top)
                FocusDirection::Right | FocusDirection::Down => first.edge_leaf(direction),
            },
        }
    }

    /// Returns true if this split direction is relevant for the given movement direction.
    /// Vertical split (left|right children) → relevant for Left/Right.
    /// Horizontal split (top|bottom children) → relevant for Up/Down.
    fn direction_matches_split(split: SplitDirection, dir: FocusDirection) -> bool {
        match dir {
            FocusDirection::Left | FocusDirection::Right => split == SplitDirection::Vertical,
            FocusDirection::Up | FocusDirection::Down => split == SplitDirection::Horizontal,
        }
    }

    /// Returns true if this direction targets the "first" child of a split.
    /// Left/Up target first (left/top). Right/Down target second (right/bottom).
    fn direction_wants_first(dir: FocusDirection) -> bool {
        matches!(dir, FocusDirection::Left | FocusDirection::Up)
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
                deferred_spawn: None,
            })),
            deferred_spawn: None,
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
                deferred_spawn: None,
            })),
            deferred_spawn: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        Ok(())
    }

    /// Add a new tab without changing the active tab. Used by IPC/CLI.
    pub fn add_tab_background_with_shell(
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
                deferred_spawn: None,
            })),
            deferred_spawn: None,
        };
        self.tabs.push(tab);
        // Do NOT change self.active_tab
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

    /// Split a specific surface by ID (cross-tab search). Does NOT change focus.
    pub fn split_surface_by_id_with_shell(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
    ) -> anyhow::Result<()> {
        let new_terminal = Terminal::new_with_shell_args(cols, rows, shell, shell_args, waker)?;
        for tab in &mut self.tabs {
            if tab.panel().find_terminal(target_surface_id).is_some() {
                let old_panel = tab.take_panel();
                tab.put_panel(old_panel.split_surface_by_id_with_terminal(
                    target_surface_id, direction, new_surface_id, new_terminal,
                ));
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
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

    /// Switch to tab by index (0-based). Returns true if switched.
    pub fn goto_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() && index != self.active_tab {
            self.active_tab = index;
            true
        } else {
            false
        }
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

    /// Add a Markdown viewer tab.
    pub fn add_markdown_tab(&mut self, tab_id: TabId, panel_id: u32, file_path: String) {
        let name = file_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Markdown")
            .to_string();
        let panel = Panel::Markdown(MarkdownPanel::new(panel_id, file_path));
        let tab = Tab {
            id: tab_id,
            name,
            panel_opt: Some(panel),
        deferred_spawn: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Add a file explorer tab.
    pub fn add_explorer_tab(&mut self, tab_id: TabId, panel_id: u32, root_path: String) {
        let name = root_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Explorer")
            .to_string();
        let panel = Panel::Explorer(ExplorerPanel::new(panel_id, root_path));
        let tab = Tab {
            id: tab_id,
            name,
            panel_opt: Some(panel),
        deferred_spawn: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Get the active tab (mutable). Returns None if tabs are empty.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        if self.tabs.is_empty() {
            return None;
        }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        Some(&mut self.tabs[idx])
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
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations
    /// or when lazy_pty_init is enabled and the tab hasn't been focused yet.
    panel_opt: Option<Panel>,
    /// When lazy_pty_init is enabled, stores parameters to spawn PTY on first access.
    pub(crate) deferred_spawn: Option<super::surface_group::DeferredSpawn>,
}

impl Tab {
    /// Access the panel. If lazy init is pending, spawns the terminal first.
    #[track_caller]
    pub fn panel(&self) -> &Panel {
        self.panel_opt.as_ref().expect("BUG: panel accessed during structural mutation or before lazy init")
    }

    /// Ensure the panel is initialized (lazy spawn if needed). Returns true if spawned.
    pub fn ensure_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.panel_opt.is_some() || self.deferred_spawn.is_none() {
            return false;
        }
        let spawn = self.deferred_spawn.take().unwrap();
        let shell_ref = spawn.shell.as_deref();
        let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
        match Terminal::new_with_shell_args(spawn.cols, spawn.rows, shell_ref, &shell_args, spawn.waker) {
            Ok(terminal) => {
                self.panel_opt = Some(Panel::Terminal(SurfaceNode {
                    id: surface_id,
                    terminal,
                    deferred_spawn: None,
                }));
                true
            }
            Err(e) => {
                tracing::error!("lazy PTY init failed: {e}");
                false
            }
        }
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
