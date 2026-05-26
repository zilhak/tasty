// LogicalPx / PhysicalPx 는 별 leaf crate `tasty-type-geometry` 에서 정의되며
// 여기서 재수출된다. 호출자는 `tasty_core::model::LogicalPx` 또는
// `tasty_type_geometry::length::LogicalPx` 어느 쪽으로도 import 가능.
pub use tasty_type_geometry::length::{self, LogicalPx, PhysicalPx};

pub type WorkspaceId = u32;
pub type PaneId = u32;
pub type TabId = u32;
pub type SurfaceId = u32;

/// Gap in physical pixels between split panes (rendered as a visible border).
pub const PANE_BORDER_WIDTH: PhysicalPx = PhysicalPx(2.0);
/// Gap in physical pixels between split surfaces (within a tab).
pub const SURFACE_BORDER_WIDTH: PhysicalPx = PhysicalPx(1.0);

/// A pixel rectangle in physical (device) pixels, used for viewport/scissor calculations.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: PhysicalPx,
    pub y: PhysicalPx,
    pub width: PhysicalPx,
    pub height: PhysicalPx,
}

impl Rect {
    /// Check if a point (x, y) is inside this rectangle.
    pub fn contains(&self, x: PhysicalPx, y: PhysicalPx) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Check if two rects are approximately equal (within 1px tolerance).
    pub fn approx_eq(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() < PhysicalPx(1.0)
            && (self.y - other.y).abs() < PhysicalPx(1.0)
            && (self.width - other.width).abs() < PhysicalPx(1.0)
            && (self.height - other.height).abs() < PhysicalPx(1.0)
    }

    pub fn split(self, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
        self.split_with_gap(direction, ratio, PANE_BORDER_WIDTH)
    }

    pub fn split_with_gap(
        self,
        direction: SplitDirection,
        ratio: f32,
        gap: PhysicalPx,
    ) -> (Rect, Rect) {
        match direction {
            SplitDirection::Vertical => {
                let usable = (self.width - gap).max(PhysicalPx(0.0));
                let first_w = (usable * ratio).floor();
                let second_w = usable - first_w;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: first_w,
                        height: self.height,
                    },
                    Rect {
                        x: self.x + first_w + gap,
                        y: self.y,
                        width: second_w,
                        height: self.height,
                    },
                )
            }
            SplitDirection::Horizontal => {
                let usable = (self.height - gap).max(PhysicalPx(0.0));
                let first_h = (usable * ratio).floor();
                let second_h = usable - first_h;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: self.width,
                        height: first_h,
                    },
                    Rect {
                        x: self.x,
                        y: self.y + first_h + gap,
                        width: self.width,
                        height: second_h,
                    },
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Information about a divider (split border) that the cursor is near.
#[derive(Debug, Clone, Copy)]
pub struct DividerInfo {
    /// The direction of the split this divider belongs to.
    pub direction: SplitDirection,
    /// The rect of the parent split node that owns this divider.
    pub split_rect: Rect,
}

/// Compute the terminal area rectangle (everything right of the sidebar) in physical pixels.
///
/// This is the single canonical implementation. Both `main.rs` and `gpu.rs` should use this.
pub fn compute_terminal_rect(
    surface_width: PhysicalPx,
    surface_height: PhysicalPx,
    sidebar_width: LogicalPx,
    scale_factor: f32,
) -> Rect {
    let sw = sidebar_width
        .to_physical(scale_factor)
        .min(surface_width - PhysicalPx(1.0));
    Rect {
        x: sw,
        y: PhysicalPx(0.0),
        width: (surface_width - sw).max(PhysicalPx(1.0)),
        height: surface_height.max(PhysicalPx(1.0)),
    }
}

pub mod closed_item;
mod diff_panel;
mod empty_surface;
mod image_panel;
mod markdown_panel;
mod pane;
mod pane_tree;
mod surface_layout;
mod surface_trait;
mod tab;
mod terminal_surface;
mod workspace;

pub use closed_item::{ClosedItem, ClosedItemStore};
pub use diff_panel::*;
pub use empty_surface::*;
pub use image_panel::*;
pub use markdown_panel::*;
pub use pane::*;
pub use pane_tree::*;
pub use surface_trait::Surface;
pub use tab::Tab;
pub use terminal_surface::*;
pub use workspace::*;

#[cfg(test)]
mod tests;
