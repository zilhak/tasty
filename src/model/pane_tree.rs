use tasty_terminal::Terminal;
use super::{
    DividerInfo, Pane, PaneId, Rect, SplitDirection, SurfaceId,
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
                    if let Some(panel) = tab.panel_mut_if_initialized() {
                        panel.for_each_terminal_mut(f);
                    }
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

    /// Collect all surface IDs across all panes in this tree.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            PaneNode::Leaf(pane) => pane.all_surface_ids(),
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_surface_ids();
                result.extend(second.all_surface_ids());
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

