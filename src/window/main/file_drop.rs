//! 외부 → Tasty 방향 drag&drop 수신.
//!
//! winit `WindowEvent::{HoveredFile, HoveredFileCancelled, DroppedFile}` 셋이
//! `MainWindow::handle_event` 에서 이 함수들로 라우팅된다.
//!
//! - hover 단계: `state.drop_hover` 에 path 누적 (overlay 렌더용).
//! - drop 단계: `state.pending_file_drops` 큐에 push. frame end 에서 drain →
//!   `file_dispatch::dispatch_file_target` 으로 보낸다.
//!
//! winit `DroppedFile` 은 좌표를 주지 않으므로 `MainWindow.cursor_position` 을
//! frame end 라우팅 시점에 활용한다.

use std::path::PathBuf;

use crate::state::DropHoverState;

use super::MainWindow;

impl MainWindow {
    pub(crate) fn handle_hovered_file(&mut self, path: PathBuf) {
        let cursor = self
            .cursor_position
            .map(|p| (p.x as f32, p.y as f32));
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
}
