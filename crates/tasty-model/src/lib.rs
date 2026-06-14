//! Tasty workspace / pane / tab / surface 도메인 모델.
//!
//! image_panel / markdown_panel / popup_kind / toast_kind 등은 본 바이너리의
//! gui 컴포넌트 API surface. headless 빌드 (`tasty --no-default-features`) 에선
//! 호출자가 cfg(gui) 로 차단되지만, library crate 표면 자체는 GUI 무관이라
//! dead_code 침묵은 본 crate 에서 적용한다.
#![allow(dead_code, unused_imports)]

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

// LogicalPx / PhysicalPx 는 별 leaf crate `tasty-type-geometry` 에 정의되어 있다.
// 본 바이너리의 group import 호환성을 위해 재수출 유지.
// 새 코드는 `tasty_type_geometry::length::*` 직접 import 권장.
pub use tasty_type_geometry::direction::{FocusDirection, SplitDirection};
pub use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
pub use tasty_type_geometry::rect::{DividerInfo, PhysicalRect};

// 식별자 alias 는 tasty-utils::id 로 이전됨. 본 모듈은 호환을 위해 재수출 유지.
// 새 코드는 `tasty_utils::id::*` 직접 import 권장.
pub use tasty_utils::id::{PaneId, SurfaceId, TabId, WorkspaceId};

/// Gap in physical pixels between split panes (rendered as a visible border).
pub const PANE_BORDER_WIDTH: PhysicalPx = PhysicalPx(2.0);
/// Gap in physical pixels between split surfaces (within a tab).
pub const SURFACE_BORDER_WIDTH: PhysicalPx = PhysicalPx(1.0);

/// Compute the terminal area rectangle (everything right of the sidebar, below the
/// titlebar) in physical pixels.
///
/// `top_inset` reserves space at the top for the custom titlebar (CSD). It is `0` until
/// the titlebar is actually drawn, making the inset a no-op.
///
/// This is the single canonical implementation. Both `main.rs` and `gpu.rs` should use this.
pub fn compute_terminal_rect(
    surface_width: PhysicalPx,
    surface_height: PhysicalPx,
    sidebar_width: LogicalPx,
    top_inset: PhysicalPx,
    scale_factor: f32,
) -> PhysicalRect {
    let sw = sidebar_width
        .to_physical(scale_factor)
        .min(surface_width - PhysicalPx(1.0));
    PhysicalRect {
        x: sw,
        y: top_inset,
        width: (surface_width - sw).max(PhysicalPx(1.0)),
        height: (surface_height - top_inset).max(PhysicalPx(1.0)),
    }
}

mod attach_mapping;
mod attached_surface;
mod binary_tree;
pub mod closed_item;
mod empty_surface;
mod image_panel;
mod markdown_panel;
mod pane;
mod pane_tree;
pub mod popup_kind;
mod surface_layout;
mod surface_trait;
mod tab;
mod terminal_surface;
pub mod toast_kind;
mod workspace;

pub use attach_mapping::{WorkspaceAttachMapping, WorkspaceAttachTarget};
pub use attached_surface::*;
pub use binary_tree::BinaryTree;
pub use closed_item::{ClosedItem, ClosedItemStore};
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
