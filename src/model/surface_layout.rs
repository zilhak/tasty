use super::FocusDirection;
use super::surface_trait::Surface;
use super::terminal_surface::TerminalSurface;
use super::{
    BinaryTree, DividerInfo, PhysicalPx, PhysicalRect, SURFACE_BORDER_WIDTH, SplitDirection,
    SurfaceId,
};
use std::any::Any;

/// A surface and its screen region, returned by `surface_regions()`.
pub struct SurfaceRegion<'a> {
    pub id: SurfaceId,
    pub rect: PhysicalRect,
    pub surface: &'a dyn Surface,
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

impl BinaryTree for SurfaceLayout {
    type Id = SurfaceId;
    const BORDER_WIDTH: PhysicalPx = SURFACE_BORDER_WIDTH;

    fn split_parts(&self) -> Option<(SplitDirection, f32, &Self, &Self)> {
        match self {
            SurfaceLayout::Leaf(_) => None,
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => Some((*direction, *ratio, first, second)),
        }
    }

    fn split_parts_mut(&mut self) -> Option<(SplitDirection, &mut f32, &mut Self, &mut Self)> {
        match self {
            SurfaceLayout::Leaf(_) => None,
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => Some((*direction, ratio, &mut **first, &mut **second)),
        }
    }

    fn leaf_id(&self) -> Option<SurfaceId> {
        match self {
            SurfaceLayout::Leaf(s) => s.surface_id(),
            _ => None,
        }
    }
}

impl SurfaceLayout {
    /// Helper: get the surface ID of a Leaf node.
    fn leaf_surface_id(surface: &dyn Surface) -> SurfaceId {
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
            SurfaceLayout::Leaf(surface) if Self::leaf_surface_id(&*surface) == target_id => (
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
        // `Box<dyn Surface>` 를 다시 TerminalSurface 로 복원 — Any downcast 사용.
        let remaining_node = remaining.and_then(|s| {
            (s as Box<dyn Any>)
                .downcast::<TerminalSurface>()
                .ok()
                .map(|b| *b)
        });
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
                let first_is_target = matches!(first.as_ref(), SurfaceLayout::Leaf(s) if Self::leaf_surface_id(&**s) == target_id);
                let second_is_target = matches!(second.as_ref(), SurfaceLayout::Leaf(s) if Self::leaf_surface_id(&**s) == target_id);

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
                if Self::leaf_surface_id(&**surface) == target_id {
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

    /// Find a leaf surface by ID (any type, not just Terminal).
    pub fn find_surface(&self, id: SurfaceId) -> Option<&dyn Surface> {
        match self {
            SurfaceLayout::Leaf(surface) => {
                if Self::leaf_surface_id(&**surface) == id {
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
                if Self::leaf_surface_id(&**surface) == id {
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

    // ─── BinaryTree alias (외부 caller 0 변경 보장) ────────────────────────
    //
    // 외부 호출처가 `crate::model::BinaryTree` 를 import 하지 않으므로
    // 동명 메서드는 UFCS 위임으로 보존하고, 이름 다른 id-시리즈도
    // alias 로 노출한다. UFCS (`<Self as BinaryTree>::method`) 필수 —
    // `self.method(...)` 로 호출하면 inherent 가 재선택되어 무한 재귀.

    pub fn first_surface_id(&self) -> Option<SurfaceId> {
        <Self as BinaryTree>::first_id(self)
    }

    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        <Self as BinaryTree>::all_ids(self)
    }

    pub fn compute_rects(&self, rect: PhysicalRect) -> Vec<(SurfaceId, PhysicalRect)> {
        <Self as BinaryTree>::compute_rects(self, rect)
    }

    pub fn collect_dividers(&self, rect: PhysicalRect) -> Vec<PhysicalRect> {
        <Self as BinaryTree>::collect_dividers(self, rect)
    }

    pub fn find_divider_at(
        &self,
        x: f32,
        y: f32,
        rect: PhysicalRect,
        threshold: f32,
    ) -> Option<DividerInfo> {
        <Self as BinaryTree>::find_divider_at(self, x, y, rect, threshold)
    }

    pub fn update_ratio_for_rect(
        &mut self,
        split_rect: PhysicalRect,
        new_ratio: f32,
        current_rect: PhysicalRect,
    ) -> bool {
        <Self as BinaryTree>::update_ratio_for_rect(self, split_rect, new_ratio, current_rect)
    }

    pub fn directional_focus(
        &self,
        current_id: SurfaceId,
        direction: FocusDirection,
    ) -> Option<SurfaceId> {
        <Self as BinaryTree>::directional_focus(self, current_id, direction)
    }
}
