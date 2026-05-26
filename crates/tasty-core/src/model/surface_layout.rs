use super::pane_tree::FocusDirection;
use super::surface_trait::Surface;
use super::terminal_surface::TerminalSurface;
use super::{
    DividerInfo, PhysicalPx, PhysicalRect, SURFACE_BORDER_WIDTH, SplitDirection, SurfaceId,
};
use tasty_terminal::Terminal;

/// A surface and its screen region, returned by `surface_regions()`.
pub struct SurfaceRegion<'a> {
    pub id: SurfaceId,
    pub rect: PhysicalRect,
    pub surface: &'a dyn Surface,
}

/// Which side of a split we descended into while building a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathSide {
    First,
    Second,
}

pub enum SurfaceLayout {
    /// A single surface leaf node. Can be any surface type (Terminal, Markdown, Explorer, etc.).
    Leaf(Box<dyn Surface>),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<SurfaceLayout>,
        second: Box<SurfaceLayout>,
        /// Which branch has focus: false = first, true = second
        focus_second: bool,
    },
}

impl SurfaceLayout {
    /// Helper: get the surface ID of a Leaf node.
    fn leaf_id(surface: &dyn Surface) -> SurfaceId {
        surface
            .surface_id()
            .expect("BUG: Leaf surface must have an ID")
    }

    /// Split a specific surface by taking ownership (infallible structural mutation).
    /// Accepts any Surface type (Terminal, Markdown, Explorer, Html, etc.).
    pub fn split_with_surface(
        self,
        target_id: SurfaceId,
        direction: SplitDirection,
        new_surface: Box<dyn Surface>,
    ) -> (Self, Option<Box<dyn Surface>>) {
        match self {
            SurfaceLayout::Leaf(surface) if Self::leaf_id(&*surface) == target_id => (
                SurfaceLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(SurfaceLayout::Leaf(surface)),
                    second: Box::new(SurfaceLayout::Leaf(new_surface)),
                    focus_second: true,
                },
                None,
            ),
            SurfaceLayout::Leaf(surface) => (SurfaceLayout::Leaf(surface), Some(new_surface)),
            SurfaceLayout::Split {
                direction: d,
                ratio,
                first,
                second,
                focus_second,
            } => {
                let (new_first, remaining) =
                    first.split_with_surface(target_id, direction, new_surface);
                if let Some(node) = remaining {
                    let (new_second, still_remaining) =
                        second.split_with_surface(target_id, direction, node);
                    (
                        SurfaceLayout::Split {
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
                        SurfaceLayout::Split {
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

    /// Split a specific surface with a TerminalSurface node (convenience wrapper).
    pub fn split_with_node(
        self,
        target_id: SurfaceId,
        direction: SplitDirection,
        new_node: TerminalSurface,
    ) -> (Self, Option<TerminalSurface>) {
        let (result, remaining) = self.split_with_surface(target_id, direction, Box::new(new_node));
        let remaining_node = remaining.and_then(|s| s.take_terminal_surface());
        (result, remaining_node)
    }

    /// Remove a surface from the tree by promoting its sibling.
    pub fn close_surface(self, target_id: SurfaceId) -> (Self, bool) {
        match self {
            SurfaceLayout::Leaf(_) => (self, false),
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                focus_second,
            } => {
                let first_is_target = matches!(first.as_ref(), SurfaceLayout::Leaf(s) if Self::leaf_id(&**s) == target_id);
                let second_is_target = matches!(second.as_ref(), SurfaceLayout::Leaf(s) if Self::leaf_id(&**s) == target_id);

                if first_is_target {
                    return (*second, true);
                }
                if second_is_target {
                    return (*first, true);
                }
                let (new_first, found_in_first) = first.close_surface(target_id);
                if found_in_first {
                    return (
                        SurfaceLayout::Split {
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
                    SurfaceLayout::Split {
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

    /// Replace a leaf surface by ID with a new surface. Returns true if found and replaced.
    pub fn replace_surface(&mut self, target_id: SurfaceId, new_surface: Box<dyn Surface>) -> bool {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if Self::leaf_id(&**surface) == target_id {
                    *surface = new_surface;
                    true
                } else {
                    false
                }
            }
            SurfaceLayout::Split { first, second, .. } => {
                if first.contains_surface(target_id) {
                    first.replace_surface(target_id, new_surface)
                } else {
                    second.replace_surface(target_id, new_surface)
                }
            }
        }
    }

    /// Check if a surface ID exists in this layout.
    pub fn contains_surface(&self, id: SurfaceId) -> bool {
        match self {
            SurfaceLayout::Leaf(surface) => surface.surface_id() == Some(id),
            SurfaceLayout::Split { first, second, .. } => {
                first.contains_surface(id) || second.contains_surface(id)
            }
        }
    }

    pub fn first_terminal(&self) -> Option<&Terminal> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.focused_terminal(),
            SurfaceLayout::Split { first, .. } => first.first_terminal(),
        }
    }

    pub fn first_surface_id(&self) -> Option<SurfaceId> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.surface_id(),
            SurfaceLayout::Split { first, .. } => first.first_surface_id(),
        }
    }

    pub fn find_terminal(&self, id: SurfaceId) -> Option<&Terminal> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.find_terminal(id),
            SurfaceLayout::Split { first, second, .. } => {
                first.find_terminal(id).or_else(|| second.find_terminal(id))
            }
        }
    }

    pub fn find_surface_node(&self, id: SurfaceId) -> Option<&TerminalSurface> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.find_terminal_surface(id),
            SurfaceLayout::Split { first, second, .. } => first
                .find_surface_node(id)
                .or_else(|| second.find_surface_node(id)),
        }
    }

    /// Find a leaf surface by ID (any type, not just Terminal).
    pub fn find_surface(&self, id: SurfaceId) -> Option<&dyn Surface> {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if Self::leaf_id(&**surface) == id {
                    Some(&**surface)
                } else {
                    None
                }
            }
            SurfaceLayout::Split { first, second, .. } => {
                first.find_surface(id).or_else(|| second.find_surface(id))
            }
        }
    }

    /// Find a mutable reference to a leaf surface by ID (any type).
    pub fn find_leaf_mut(&mut self, id: SurfaceId) -> Option<&mut Box<dyn Surface>> {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if Self::leaf_id(&**surface) == id {
                    Some(surface)
                } else {
                    None
                }
            }
            SurfaceLayout::Split { first, second, .. } => {
                if first.contains_surface(id) {
                    first.find_leaf_mut(id)
                } else {
                    second.find_leaf_mut(id)
                }
            }
        }
    }

    pub fn find_terminal_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.find_terminal_mut(id),
            SurfaceLayout::Split { first, second, .. } => {
                if let Some(t) = first.find_terminal_mut(id) {
                    Some(t)
                } else {
                    second.find_terminal_mut(id)
                }
            }
        }
    }

    /// Collect regions for all surfaces with their Surface trait references.
    pub fn surface_regions(&self, rect: PhysicalRect) -> Vec<SurfaceRegion<'_>> {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if let Some(id) = surface.surface_id() {
                    vec![SurfaceRegion {
                        id,
                        rect,
                        surface: &**surface,
                    }]
                } else {
                    vec![]
                }
            }
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                let mut result = first.surface_regions(r1);
                result.extend(second.surface_regions(r2));
                result
            }
        }
    }

    pub fn resize_all(&mut self, rect: PhysicalRect, cell_width: f32, cell_height: f32) {
        match self {
            SurfaceLayout::Leaf(surface) => {
                surface.resize_all(rect, cell_width, cell_height);
            }
            SurfaceLayout::Split {
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

    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            SurfaceLayout::Leaf(surface) => surface.surface_id().into_iter().collect(),
            SurfaceLayout::Split { first, second, .. } => {
                let mut result = first.all_surface_ids();
                result.extend(second.all_surface_ids());
                result
            }
        }
    }

    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            SurfaceLayout::Leaf(surface) => {
                surface.collect_terminals_mut(out);
            }
            SurfaceLayout::Split { first, second, .. } => {
                first.collect_terminals_mut(out);
                second.collect_terminals_mut(out);
            }
        }
    }

    pub fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal) + ?Sized,
    {
        match self {
            SurfaceLayout::Leaf(surface) => {
                surface.for_each_terminal_mut(&mut |id, t| f(id, t));
            }
            SurfaceLayout::Split { first, second, .. } => {
                first.for_each_terminal_mut(f);
                second.for_each_terminal_mut(f);
            }
        }
    }

    /// Visit every leaf Surface (Terminal/Empty/Markdown/etc.) for read-only inspection.
    /// 닫기 경로에서 leaf 들의 `scrollback_persist_id` 를 추출해 디스크 정리하는 용도로
    /// 쓰인다.
    pub fn for_each_surface(&self, f: &mut dyn FnMut(&dyn Surface)) {
        match self {
            SurfaceLayout::Leaf(surface) => f(&**surface),
            SurfaceLayout::Split { first, second, .. } => {
                first.for_each_surface(f);
                second.for_each_surface(f);
            }
        }
    }

    /// Object-safe version of for_each_terminal_mut (uses &mut dyn FnMut).
    pub fn for_each_terminal_mut_dyn(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        match self {
            SurfaceLayout::Leaf(surface) => {
                surface.for_each_terminal_mut(f);
            }
            SurfaceLayout::Split { first, second, .. } => {
                first.for_each_terminal_mut_dyn(f);
                second.for_each_terminal_mut_dyn(f);
            }
        }
    }

    pub fn collect_dividers(&self, rect: PhysicalRect) -> Vec<PhysicalRect> {
        match self {
            SurfaceLayout::Leaf(_) => vec![],
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let gap = SURFACE_BORDER_WIDTH;
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, gap);
                let divider = match direction {
                    SplitDirection::Vertical => PhysicalRect {
                        x: r1.x + r1.width,
                        y: rect.y,
                        width: gap,
                        height: rect.height,
                    },
                    SplitDirection::Horizontal => PhysicalRect {
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

    pub fn find_divider_at(
        &self,
        x: f32,
        y: f32,
        rect: PhysicalRect,
        threshold: f32,
    ) -> Option<DividerInfo> {
        match self {
            SurfaceLayout::Leaf(_) => None,
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                let divider_pos = match direction {
                    SplitDirection::Vertical => (r1.x + r1.width).value(),
                    SplitDirection::Horizontal => (r1.y + r1.height).value(),
                };
                let cursor_pos = match direction {
                    SplitDirection::Vertical => x,
                    SplitDirection::Horizontal => y,
                };
                let in_bounds = match direction {
                    SplitDirection::Vertical => {
                        y >= rect.y.value() && y < (rect.y + rect.height).value()
                    }
                    SplitDirection::Horizontal => {
                        x >= rect.x.value() && x < (rect.x + rect.width).value()
                    }
                };
                if in_bounds && (cursor_pos - divider_pos).abs() < threshold {
                    return Some(DividerInfo {
                        direction: *direction,
                        split_rect: rect,
                    });
                }
                first
                    .find_divider_at(x, y, r1, threshold)
                    .or_else(|| second.find_divider_at(x, y, r2, threshold))
            }
        }
    }

    pub fn update_ratio_for_rect(
        &mut self,
        split_rect: PhysicalRect,
        new_ratio: f32,
        current_rect: PhysicalRect,
    ) -> bool {
        match self {
            SurfaceLayout::Leaf(_) => false,
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                if current_rect.approx_eq(&split_rect) {
                    *ratio = new_ratio.clamp(0.1, 0.9);
                    return true;
                }
                let (r1, r2) =
                    current_rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first.update_ratio_for_rect(split_rect, new_ratio, r1)
                    || second.update_ratio_for_rect(split_rect, new_ratio, r2)
            }
        }
    }

    pub fn directional_focus(
        &self,
        current_id: SurfaceId,
        direction: FocusDirection,
    ) -> Option<SurfaceId> {
        let mut path: Vec<(SplitDirection, PathSide, &SurfaceLayout)> = Vec::new();
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
        path: &mut Vec<(SplitDirection, PathSide, &'a SurfaceLayout)>,
    ) -> bool {
        match self {
            SurfaceLayout::Leaf(surface) => Self::leaf_id(&**surface) == target_id,
            SurfaceLayout::Split {
                direction,
                first,
                second,
                ..
            } => {
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
            SurfaceLayout::Leaf(surface) => Self::leaf_id(&**surface),
            SurfaceLayout::Split { first, second, .. } => match direction {
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

    pub fn find_surface_at(&self, x: f32, y: f32, rect: PhysicalRect) -> Option<SurfaceId> {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    surface.surface_id()
                } else {
                    None
                }
            }
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split_with_gap(*direction, *ratio, SURFACE_BORDER_WIDTH);
                first
                    .find_surface_at(x, y, r1)
                    .or_else(|| second.find_surface_at(x, y, r2))
            }
        }
    }
}
