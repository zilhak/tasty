// LogicalPx / PhysicalPx 는 별 leaf crate `tasty-type-geometry` 에 정의되어 있다.
// 본 바이너리의 group import 호환성을 위해 재수출 유지.
// 새 코드는 `tasty_type_geometry::length::*` 직접 import 권장.
pub use tasty_type_geometry::direction::{FocusDirection, SplitDirection};
pub use tasty_type_geometry::length::{self, LogicalPx, PhysicalPx};
pub use tasty_type_geometry::rect::{DividerInfo, PhysicalRect};

// 식별자 alias 는 tasty-utils::id 로 이전됨. 본 모듈은 호환을 위해 재수출 유지.
// 새 코드는 `tasty_utils::id::*` 직접 import 권장.
pub use tasty_utils::id::{PaneId, SurfaceId, TabId, WorkspaceId};

/// Gap in physical pixels between split panes (rendered as a visible border).
pub const PANE_BORDER_WIDTH: PhysicalPx = PhysicalPx(2.0);
/// Gap in physical pixels between split surfaces (within a tab).
pub const SURFACE_BORDER_WIDTH: PhysicalPx = PhysicalPx(1.0);

/// Compute the terminal area rectangle (everything right of the sidebar) in physical pixels.
///
/// This is the single canonical implementation. Both `main.rs` and `gpu.rs` should use this.
pub fn compute_terminal_rect(
    surface_width: PhysicalPx,
    surface_height: PhysicalPx,
    sidebar_width: LogicalPx,
    scale_factor: f32,
) -> PhysicalRect {
    let sw = sidebar_width
        .to_physical(scale_factor)
        .min(surface_width - PhysicalPx(1.0));
    PhysicalRect {
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
