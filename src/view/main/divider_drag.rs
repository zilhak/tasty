//! Pane/Surface 분할 divider 의 드래그 상태.
//!
//! `MainView::dragging_divider` 가 보관. mouse 핸들러에서 사용.

use crate::model::DividerInfo;
use crate::view::ui::View;

/// Tracks an active divider drag operation.
#[derive(Clone, Copy)]
pub(crate) struct DividerDrag {
    pub info: DividerInfo,
    pub sequence: u64,
}

impl super::MainView {
    /// Losing the pointer or entering chrome ends the gesture at its last valid displayed ratio.
    /// Only stale targets and rejected commits roll back a preview.
    pub(super) fn finish_divider_drag(&mut self, engine: &crate::runtime::engine_read::EngineRead<'_>) -> bool {
        let Some(drag) = self.dragging_divider.take() else {
            return false;
        };
        if let Some(commit) = self.state.layout_previews.finish(engine, drag.sequence) {
            self.state.dispatch_intent(
                crate::intent::Intent::CommitDivider(commit).from_user_menu("divider_drag"),
            );
        }
        self.mark_dirty();
        true
    }
}
