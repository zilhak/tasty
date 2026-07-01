//! 외부 → Tasty 방향 drag&drop 수신.
//!
//! winit `WindowEvent::{HoveredFile, HoveredFileCancelled, DroppedFile}` 셋이
//! `MainView::handle_event` 에서 이 함수들로 라우팅된다.
//!
//! - hover 단계: `state.drop_hover` 에 path 누적 (overlay 렌더용).
//! - drop 단계: `state.pending_file_drops` 큐에 push. frame end 에서 drain →
//!   `DomainIntent::DispatchFile` 발화로 보낸다.
//!
//! winit `DroppedFile` 은 좌표를 주지 않으므로 `MainView.cursor_position` 을
//! frame end 라우팅 시점에 활용한다.

use std::path::PathBuf;

use crate::adapters::ui::ToastScope;
use crate::state::DropHoverState;
use tasty_type_geometry::length::PhysicalPx;

use super::MainView;

impl MainView {
    pub(crate) fn handle_hovered_file(&mut self, path: PathBuf) {
        let cursor = self.cursor_position.map(|p| (p.x as f32, p.y as f32));
        match self.state.drop_hover.as_mut() {
            Some(s) => s.paths.push(path),
            None => {
                self.state.drop_hover = Some(DropHoverState {
                    paths: vec![path],
                    cursor,
                });
            }
        }
        self.base.dirty = true;
    }

    pub(crate) fn handle_hovered_file_cancelled(&mut self) {
        if self.state.drop_hover.take().is_some() {
            self.base.dirty = true;
        }
    }

    pub(crate) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.state.pending_file_drops.push(path);
        // OS 가 DroppedFile 후 HoveredFileCancelled 를 보장하지 않을 수 있으므로
        // 명시 해제. 다중 파일 drop 의 경우 첫 DroppedFile 에서 정리되고 이후
        // pending_file_drops 만 누적.
        self.state.drop_hover = None;
        self.base.dirty = true;
    }

    /// frame end 에서 호출. 큐를 비우고 각 파일을 `DispatchFile(Deep)` Intent 로
    /// 발화. 좌표는 `cursor_position` 기준 — terminal_rect 외부면 toast 후 무시.
    pub(crate) fn process_pending_file_drops(&mut self) {
        let drops = std::mem::take(&mut self.state.pending_file_drops);
        if drops.is_empty() {
            return;
        }
        let Some(pos) = self.cursor_position else {
            tracing::warn!(
                count = drops.len(),
                "file drop with no cursor position — ignored",
            );
            return;
        };
        let terminal_rect = self.compute_terminal_rect();
        let (x, y) = (pos.x as f32, pos.y as f32);
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            self.state
                .toasts
                .push_info(crate::i18n::t("file_drop.outside"), ToastScope::Window);
            return;
        }
        {
            let engine = &mut self.core_state;
            // best-effort focus 이동. drop 좌표에 pane/surface 가 없으면 현재 focus 유지.
            let _pane_focus = self
                .state
                .focus_pane_at_position(engine, x, y, terminal_rect);
            let _surface_focus = self
                .state
                .focus_surface_at_position(engine, x, y, terminal_rect);
        }
        for path in drops {
            self.state.dispatch_intent(
                crate::core::intent::DomainIntent::DispatchFile {
                    target: crate::file::format::FileTarget::new(path),
                    depth: crate::file::format::DetectDepth::Deep,
                    origin_surface_id: None,
                    ignore_size_limit: false,
                }
                .from_user_menu("file_drop"),
            );
        }
    }
}
