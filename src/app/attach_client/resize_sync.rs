//! mirror 크기 요청의 응답 마감을 처리한다. 마감이 지난 첫 요청은 한 번 다시 보내고,
//! 다시 보낸 요청까지 실패하면 실패 목록에 올린다. 대기 상태 자체는 `tasty_remote::resize_sync`가 갖는다.

use crate::app::App;
use crate::ipc::stream::{StreamControl, StreamTag};
use std::time::Instant;
use tasty_remote::client_session::{AttachClientSession, SessionState};
use tasty_remote::resize_sync::Resend;

impl App {
    /// 마감이 지난 요청을 처리하고 가장 이른 다음 마감에 루프를 깨우도록 타이머를 맞춘다.
    /// 연결이 없는 세션은 응답이 올 수 없으므로 대기를 버린다. 재연결 설치가 크기를 다시 맞춘다.
    pub(crate) fn poll_attach_resize_sync(&mut self, now: Instant) {
        for sess in &mut self.remote.sessions {
            if sess.state.phase != SessionState::Connected {
                sess.state.resize_sync.forget_pending();
                continue;
            }
            let failed_before: Vec<u32> = sess.state.resize_sync.failed().collect();
            for resend in sess.state.resize_sync.take_due(now) {
                if send_resize(sess, resend) {
                    sess.state.resize_sync.note_resent(resend, now);
                }
            }
            for remote in sess.state.resize_sync.failed() {
                if !failed_before.contains(&remote) {
                    tracing::warn!(
                        "attach resize: mirror workspace {} 의 원격 surface {remote} 크기 동기화가 자동 재시도 뒤에도 응답을 받지 못했다",
                        sess.state.local_workspace
                    );
                }
            }
        }
        let next = self
            .remote
            .sessions
            .iter()
            .filter_map(|sess| sess.state.resize_sync.next_deadline())
            .min();
        crate::app::timers::sync_attach_resize_timer(&mut self.timers, next, now);
    }
}

/// 같은 크기 요청을 다시 큐에 넣는다. 큐가 닫혔으면 연결 종료가 따로 처리되므로 기다리지 않는다.
fn send_resize(sess: &mut AttachClientSession, resend: Resend) -> bool {
    let payload = match serde_json::to_vec(&StreamControl::ClientResize {
        surface_id: resend.surface_id,
        cols: resend.cols,
        rows: resend.rows,
    }) {
        Ok(payload) => payload,
        Err(e) => {
            tracing::warn!("attach resize: 재전송 요청을 직렬화하지 못했다: {e}");
            return false;
        }
    };
    match sess.send_frame(StreamTag::Control, payload) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("attach resize: 전송 큐가 닫혀 재전송하지 못했다: {e}");
            false
        }
    }
}
