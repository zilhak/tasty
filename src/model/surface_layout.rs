use tasty_terminal::Terminal;
use super::{DividerInfo, Rect, SplitDirection, SurfaceId, SURFACE_BORDER_WIDTH};
use super::pane_tree::FocusDirection;
use super::surface_group::SurfaceNode;

/// Which side of a split we descended into while building a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathSide {
    First,
    Second,
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
                    None,
                )
            }
            SurfaceGroupLayout::Single(node) => {
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
    pub fn close_surface(self, target_id: SurfaceId) -> (Self, bool) {
        match self {
            SurfaceGroupLayout::Single(_) => (self, false),
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
                    return (*second, true);
                }
                if second_is_target {
                    return (*first, true);
                }
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

    pub fn first_terminal(&self) -> Option<&Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => Some(&node.terminal),
            SurfaceGroupLayout::Split { first, .. } => first.first_terminal(),
        }
    }

    pub fn first_surface_id(&self) -> Option<SurfaceId> {
        match self {
            SurfaceGroupLayout::Single(node) => Some(node.id),
            SurfaceGroupLayout::Split { first, .. } => first.first_surface_id(),
        }
    }

    pub fn find_terminal(&self, id: SurfaceId) -> Option<&Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if node.id == id { Some(&node.terminal) } else { None }
            }
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.find_terminal(id).or_else(|| second.find_terminal(id))
            }
        }
    }

    pub fn find_terminal_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if node.id == id { Some(&mut node.terminal) } else { None }
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

    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            SurfaceGroupLayout::Single(node) => vec![(node.id, &node.terminal, rect)],
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                let mut result = first.render_regions(r1);
                result.extend(second.render_regions(r2));
                result
            }
        }
    }

    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            SurfaceGroupLayout::Single(node) => {
                let cols = ((rect.width - 4.0) / cell_width).floor().max(1.0) as usize;
                let rows = ((rect.height - 4.0) / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first.resize_all(r1, cell_width, cell_height);
                second.resize_all(r2, cell_width, cell_height);
            }
        }
    }

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

    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            SurfaceGroupLayout::Single(node) => out.push(&mut node.terminal),
            SurfaceGroupLayout::Split { first, second, .. } => {
                first.collect_terminals_mut(out);
                second.collect_terminals_mut(out);
            }
        }
    }

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

    pub fn collect_dividers(&self, rect: Rect) -> Vec<Rect> {
        match self {
            SurfaceGroupLayout::Single(_) => vec![],
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let gap = SURFACE_BORDER_WIDTH;
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, gap);
                let divider = match direction {
                    SplitDirection::Vertical => Rect {
                        x: r1.x + r1.width, y: rect.y, width: gap, height: rect.height,
                    },
                    SplitDirection::Horizontal => Rect {
                        x: rect.x, y: r1.y + r1.height, width: rect.width, height: gap,
                    },
                };
                let mut result = vec![divider];
                result.extend(first.collect_dividers(r1));
                result.extend(second.collect_dividers(r2));
                result
            }
        }
    }

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

    pub fn directional_focus(&self, current_id: SurfaceId, direction: FocusDirection) -> Option<SurfaceId> {
        let mut path: Vec<(SplitDirection, PathSide, &SurfaceGroupLayout)> = Vec::new();
        if !self.build_path_to(current_id, &mut path) {
            return None;
        }

        for (split_dir, side, sibling) in path.iter().rev() {
            if Self::direction_matches_split(*split_dir, direction) {
                let want_first = Self::direction_wants_first(direction);
                let currently_first = *side == PathSide::First;
                if currently_first != want_first {
                    return Some(sibling.edge_leaf(direction));
                }
            }
        }
        None
    }

    fn build_path_to<'a>(
        &'a self,
        target_id: SurfaceId,
        path: &mut Vec<(SplitDirection, PathSide, &'a SurfaceGroupLayout)>,
    ) -> bool {
        match self {
            SurfaceGroupLayout::Single(node) => node.id == target_id,
            SurfaceGroupLayout::Split { direction, first, second, .. } => {
                path.push((*direction, PathSide::First, second.as_ref()));
                if first.build_path_to(target_id, path) {
                    return true;
                }
                path.pop();

                path.push((*direction, PathSide::Second, first.as_ref()));
                if second.build_path_to(target_id, path) {
                    return true;
                }
                path.pop();

                false
            }
        }
    }

    fn edge_leaf(&self, direction: FocusDirection) -> SurfaceId {
        match self {
            SurfaceGroupLayout::Single(node) => node.id,
            SurfaceGroupLayout::Split { first, second, .. } => match direction {
                FocusDirection::Left | FocusDirection::Up => second.edge_leaf(direction),
                FocusDirection::Right | FocusDirection::Down => first.edge_leaf(direction),
            },
        }
    }

    fn direction_matches_split(split: SplitDirection, dir: FocusDirection) -> bool {
        match dir {
            FocusDirection::Left | FocusDirection::Right => split == SplitDirection::Vertical,
            FocusDirection::Up | FocusDirection::Down => split == SplitDirection::Horizontal,
        }
    }

    fn direction_wants_first(dir: FocusDirection) -> bool {
        matches!(dir, FocusDirection::Left | FocusDirection::Up)
    }

    pub fn find_surface_at(&self, x: f32, y: f32, rect: Rect) -> Option<SurfaceId> {
        match self {
            SurfaceGroupLayout::Single(node) => {
                if rect.contains(x, y) { Some(node.id) } else { None }
            }
            SurfaceGroupLayout::Split { direction, ratio, first, second, .. } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first.find_surface_at(x, y, r1)
                    .or_else(|| second.find_surface_at(x, y, r2))
            }
        }
    }
}
