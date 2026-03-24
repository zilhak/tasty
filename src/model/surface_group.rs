use tasty_terminal::Terminal;
use super::{DividerInfo, Rect, SplitDirection, SurfaceId, SURFACE_BORDER_WIDTH};

/// Single terminal instance.
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
}

/// Split within a tab (appears as one tab but renders multiple terminals).
pub struct SurfaceGroupNode {
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    pub(crate) layout_opt: Option<SurfaceGroupLayout>,
    pub focused_surface: SurfaceId,
    /// First surface ID, stored for focus tracking.
    pub(crate) _first_surface: SurfaceId,
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
    pub(crate) fn take_layout(&mut self) -> SurfaceGroupLayout {
        self.layout_opt.take().expect("BUG: layout already taken")
    }

    /// Put the layout back.
    pub(crate) fn put_layout(&mut self, layout: SurfaceGroupLayout) {
        self.layout_opt = Some(layout);
    }
}

impl SurfaceGroupNode {
    /// Close a surface within this group. Returns true if removed.
    pub fn close_surface(&mut self, target_id: SurfaceId) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, found) = old_layout.close_surface(target_id);
        self.put_layout(new_layout);
        if found {
            // If the focused surface was the one we removed, reset focus
            if self.focused_surface == target_id {
                if let Some(first_id) = self.layout().first_surface_id() {
                    self.focused_surface = first_id;
                }
            }
        }
        found
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

    /// Remove a surface from the tree by promoting its sibling.
    /// This is a consuming operation that returns a new layout.
    /// Returns `(new_layout, removed)` where `removed` is true if target was found.
    pub fn close_surface(self, target_id: SurfaceId) -> (Self, bool) {
        match self {
            SurfaceGroupLayout::Single(_) => (self, false), // Can't close the only surface
            SurfaceGroupLayout::Split {
                direction,
                ratio,
                first,
                second,
                focus_second,
            } => {
                let first_is_target =
                    matches!(first.as_ref(), SurfaceGroupLayout::Single(n) if n.id == target_id);
                let second_is_target =
                    matches!(second.as_ref(), SurfaceGroupLayout::Single(n) if n.id == target_id);

                if first_is_target {
                    // Remove first, promote second
                    return (*second, true);
                }
                if second_is_target {
                    // Remove second, promote first
                    return (*first, true);
                }
                // Recurse into children
                let (new_first, found_in_first) = first.close_surface(target_id);
                if found_in_first {
                    return (
                        SurfaceGroupLayout::Split {
                            direction,
                            ratio,
                            first: Box::new(new_first),
                            second,
                            focus_second,
                        },
                        true,
                    );
                }
                let (new_second, found_in_second) = second.close_surface(target_id);
                (
                    SurfaceGroupLayout::Split {
                        direction,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                        focus_second,
                    },
                    found_in_second,
                )
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
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
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
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
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

    /// Visit all terminals (mutable) in this layout tree.
    pub fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal),
    {
        match self {
            SurfaceGroupLayout::Single(node) => f(node.id, &mut node.terminal),
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.for_each_terminal_mut(f);
                second.for_each_terminal_mut(f);
            }
        }
    }

    /// Collect divider rectangles for surface splits.
    pub fn collect_dividers(&self, rect: Rect) -> Vec<Rect> {
        match self {
            SurfaceGroupLayout::Single(_) => vec![],
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let gap = SURFACE_BORDER_WIDTH;
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, gap);
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

    /// Find a divider near the given point within this surface group layout.
    pub fn find_divider_at(&self, x: f32, y: f32, rect: Rect, threshold: f32) -> Option<DividerInfo> {
        match self {
            SurfaceGroupLayout::Single(_) => None,
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                let divider_pos = match direction {
                    SplitDirection::Vertical => r1.x + r1.width,
                    SplitDirection::Horizontal => r1.y + r1.height,
                };
                let cursor_pos = match direction {
                    SplitDirection::Vertical => x,
                    SplitDirection::Horizontal => y,
                };
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
                first.find_divider_at(x, y, r1, threshold)
                    .or_else(|| second.find_divider_at(x, y, r2, threshold))
            }
        }
    }

    /// Update the ratio of the split node whose rect approximately matches `split_rect`.
    pub fn update_ratio_for_rect(&mut self, split_rect: Rect, new_ratio: f32, current_rect: Rect) -> bool {
        match self {
            SurfaceGroupLayout::Single(_) => false,
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                if current_rect.approx_eq(&split_rect) {
                    *ratio = new_ratio.clamp(0.1, 0.9);
                    return true;
                }
                let (r1, r2) = current_rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first.update_ratio_for_rect(split_rect, new_ratio, r1)
                    || second.update_ratio_for_rect(split_rect, new_ratio, r2)
            }
        }
    }

    /// Find which surface contains the given point.
    pub fn find_surface_at(&self, x: f32, y: f32, rect: Rect) -> Option<SurfaceId> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if rect.contains(x, y) {
                    Some(node.id)
                } else {
                    None
                }
            }
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first.find_surface_at(x, y, r1)
                    .or_else(|| second.find_surface_at(x, y, r2))
            }
        }
    }
}
