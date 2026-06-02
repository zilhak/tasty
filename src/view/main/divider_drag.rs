//! Pane/Surface 분할 divider 의 드래그 상태.
//!
//! `MainView::dragging_divider` 가 보관. mouse 핸들러에서 사용.

use crate::model::DividerInfo;

/// Tracks an active divider drag operation.
#[derive(Clone, Copy)]
pub(crate) struct DividerDrag {
    pub info: DividerInfo,
    pub kind: DividerDragKind,
}

#[derive(Clone, Copy)]
pub(crate) enum DividerDragKind {
    /// Dragging a pane-level split divider.
    Pane,
    /// Dragging a surface-level split divider (within a tab).
    Surface,
}
