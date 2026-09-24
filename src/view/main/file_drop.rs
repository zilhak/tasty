//! 외부 파일 드롭. hover 경로는 안내에, 완료 경로는 프레임 끝의 DispatchFile 처리에 사용한다.
//! DroppedFile에 좌표가 없어 보관 중인 cursor_position으로 대상을 고른다.

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
        // OS가 취소 이벤트를 보내지 않아도 hover 표시를 지운다.
        self.state.drop_hover = None;
        self.base.dirty = true;
    }

    /// 쌓인 파일을 DispatchFile로 보낸다. 터미널 영역 밖이면 안내하고 무시한다.
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
            let scale_factor = self.base.gpu.scale_factor();
            let engine = &mut self.core_state;
            // best-effort focus 이동. drop 좌표에 pane/surface 가 없으면 현재 focus 유지.
            let _pane_focus =
                self.state
                    .focus_pane_at_position(engine, x, y, terminal_rect, scale_factor);
            let _surface_focus =
                self.state
                    .focus_surface_at_position(engine, x, y, terminal_rect, scale_factor);
        }
        for path in drops {
            self.state.dispatch_intent(
                crate::core::intent::DomainIntent::DispatchFile {
                    target: crate::file::format::FileTarget::new(path),
                    depth: crate::file::format::DetectDepth::Deep,
                    origin_surface_id: None,
                    dispatch_origin: crate::file::dispatch::FileDispatchOrigin::User,
                    ignore_size_limit: false,
                }
                .from_user_menu("file_drop"),
            );
        }
    }
}
