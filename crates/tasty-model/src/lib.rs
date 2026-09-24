#![forbid(unsafe_code)]

//! GUI에 의존하지 않는 workspace·pane·tab·surface 모델과 UI 분류 타입.

/// `Surface::as_any` / `as_any_mut` 구현을 한 줄로 채우는 매크로.
///
/// ```ignore
/// impl Surface for MyPanel {
///     tasty_model::impl_surface_any!();
///     // ... 다른 메서드들 ...
/// }
/// ```
#[macro_export]
macro_rules! impl_surface_any {
    () => {
        fn as_any(&self) -> &dyn ::std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
            self
        }
    };
}

// 호환 import를 위한 재수출. 새 코드는 tasty_type_geometry::length를 직접 사용한다.
pub use tasty_type_geometry::direction::{FocusDirection, SplitDirection};
pub use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
pub use tasty_type_geometry::rect::{DividerInfo, LogicalRect, PhysicalRect};

// 호환 import를 위한 재수출. 새 코드는 tasty_utils::id를 직접 사용한다.
pub use tasty_utils::id::{
    NORMAL_CATEGORY_ID, PaneId, SurfaceId, TabId, WorkspaceCategoryId, WorkspaceId,
};

/// pane 사이 간격의 논리 두께. BinaryTree::border_width에서 배율을 적용해 물리 두께로 바꾼다.
pub const PANE_BORDER_WIDTH: LogicalPx = LogicalPx(2.0);

/// surface 사이 간격의 물리 두께. 배율과 무관하게 1 device px을 유지한다.
/// 별도의 디자인 의도가 확인된 값이라는 뜻은 아니며 현재 좌표계 계약을 나타낸다.
pub const SURFACE_BORDER_WIDTH: PhysicalPx = PhysicalPx(1.0);

/// Compute the terminal area rectangle (everything right of the sidebar, below the
/// titlebar) in physical pixels.
///
/// `top_inset` reserves space at the top for the custom titlebar (CSD). It is `0` until
/// the titlebar is actually drawn, making the inset a no-op.
///
/// `bottom_inset` reserves space at the bottom of the work column for the StatusBar
/// (`adapters::ui::status_bar`). Like `top_inset`, a value of `0` makes it a no-op.
///
pub fn compute_terminal_rect(
    surface_width: PhysicalPx,
    surface_height: PhysicalPx,
    sidebar_width: LogicalPx,
    top_inset: PhysicalPx,
    bottom_inset: PhysicalPx,
    scale_factor: f32,
) -> PhysicalRect {
    let sw = sidebar_width
        .to_physical(scale_factor)
        .min(surface_width - PhysicalPx(1.0));
    PhysicalRect {
        x: sw,
        y: top_inset,
        width: (surface_width - sw).max(PhysicalPx(1.0)),
        height: (surface_height - top_inset - bottom_inset).max(PhysicalPx(1.0)),
    }
}

mod attach_mapping;
mod attach_mesh_surface;
pub mod banner_kind;
mod binary_tree;
pub mod closed_item;
mod dag_graph_surface;
mod empty_surface;
mod explorer_panel;
mod nav_state;
mod pane;
mod pane_tree;
pub mod popup_kind;
mod surface_layout;
mod surface_trait;
mod tab;
mod terminal_surface;
pub mod toast_kind;
mod workspace;
mod workspace_category;

pub use attach_mapping::{WorkspaceAttachMapping, WorkspaceAttachTarget};
pub use attach_mesh_surface::AttachMeshSurface;
pub use binary_tree::BinaryTree;
pub use closed_item::{ClosedItem, ClosedItemStore};
pub use dag_graph_surface::*;
pub use empty_surface::*;
pub use explorer_panel::*;
pub use nav_state::NavState;
pub use pane::*;
pub use pane_tree::*;
pub use surface_trait::Surface;
pub use tab::Tab;
pub use terminal_surface::*;
pub use workspace::*;
pub use workspace_category::*;

#[cfg(test)]
mod tests;
