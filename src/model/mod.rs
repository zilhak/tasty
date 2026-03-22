use tasty_terminal::{Terminal, Waker};

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
    /// Check if a point (x, y) is inside this rectangle.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Check if two rects are approximately equal (within 1px tolerance).
    pub fn approx_eq(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() < 1.0
            && (self.y - other.y).abs() < 1.0
            && (self.width - other.width).abs() < 1.0
            && (self.height - other.height).abs() < 1.0
    }

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
pub fn compute_terminal_rect(surface_width: f32, surface_height: f32, sidebar_width: f32, scale_factor: f32) -> Rect {
    let sw = (sidebar_width * scale_factor).min(surface_width - 1.0);
    Rect {
        x: sw,
        y: 0.0,
        width: (surface_width - sw).max(1.0),
        height: surface_height.max(1.0),
    }
}

mod workspace;
mod pane;
mod panel;
mod surface_group;

pub use workspace::*;
pub use pane::*;
pub use panel::*;
pub use surface_group::*;

#[cfg(test)]
mod tests;
