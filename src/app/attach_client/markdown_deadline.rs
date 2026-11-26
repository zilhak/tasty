//! 회신이 마감까지 오지 않거나 연결이 바뀌어 회신이 오지 않을 markdown 원문 요청을 실패로 끝낸다.
//! 마감 상태는 `tasty_remote::markdown_requests`가 갖고, 여기서는 대기 표시를 내리고 plugin에 실패를 보낸다.

use super::resources::{
    markdown_content_failure, push_markdown_changed, push_markdown_content_result,
};
use crate::app::App;
use crate::state::attach_pending::AttachPending;
use serde_json::Value;
use std::time::Instant;
use tasty_remote::client_session::{AttachClientSession, ClientSessionState, SessionState};
use tasty_remote::connection::ConnectionEpoch;
use tasty_remote::markdown_requests::MARKDOWN_CONTENT_REPLY_TIMEOUT;

/// 마감이 지나 끝낸 원문 요청.
pub(super) struct Expired {
    epoch: ConnectionEpoch,
    request_id: u64,
    local_surface_id: u32,
}

/// 연결된 세션에서 마감이 지난 요청을 꺼낸다. 연결이 없는 세션의 요청은 끊김 경로가 이미
/// request_id=0으로 취소했으므로 실패를 다시 보내지 않고 버린다.
pub(super) fn take_expired(sessions: &mut [AttachClientSession], now: Instant) -> Vec<Expired> {
    let mut expired = Vec::new();
    for sess in sessions {
        if sess.state.phase != SessionState::Connected {
            sess.state.markdown_requests.forget_all();
            continue;
        }
        let epoch = sess.transport.frame_tx.epoch();
        for (request_id, local_surface_id) in sess.state.markdown_requests.take_due(now) {
            // 에이전트 요청 표시도 회신 대신 여기서 소비한다.
            sess.state.agent_requests.take_markdown(request_id);
            expired.push(Expired {
                epoch: epoch.clone(),
                request_id,
                local_surface_id,
            });
        }
    }
    expired
}

/// 끝낸 요청에 건 사용자 대기를 모든 상태에서 내리고 plugin에 보낼 실패 결과를 돌려준다.
pub(super) fn settle_expired<'a>(
    expired: &[Expired],
    states: impl Iterator<Item = &'a mut AttachPending>,
) -> Vec<Value> {
    for pending in states {
        for request in expired {
            pending.query_result(&request.epoch, request.request_id, false);
        }
    }
    expired.iter().map(Expired::failure).collect()
}

/// 교체된 연결로 보낸 원문 요청을 같은 request_id 실패로 끝낸 뒤 문서마다 변경 신호를 보낸다.
/// 옛 연결의 회신은 오지 않고, 그 id를 기다리는 plugin은 변경 신호를 받아도 다시 요청하지 않으므로
/// 실패가 먼저 가야 한다. 원문은 바로 다시 받지 않고 plugin이 재요청이나 stale 표시를 고른다.
pub(super) fn end_replaced_connection_requests(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    state: &mut ClientSessionState,
) {
    for (request_id, local) in state.markdown_requests.take_all() {
        let failure = markdown_content_failure(local, request_id, "mirror workspace disconnected");
        push_markdown_content_result(plugin_manager, &failure);
    }
    for &local in &state.markdown_locals {
        push_markdown_changed(plugin_manager, local);
    }
}

impl Expired {
    /// plugin에 보낼 실패 결과. 사유는 다른 host 사유처럼 번역하지 않는다.
    fn failure(&self) -> Value {
        let reason = format!(
            "remote did not answer within {} s",
            MARKDOWN_CONTENT_REPLY_TIMEOUT.as_secs()
        );
        markdown_content_failure(self.local_surface_id, self.request_id, &reason)
    }
}

impl App {
    /// 마감이 지난 원문 요청을 실패로 끝낸다. 대기 표시는 같은 회차의 `sync_attach_waits`가 다시 계산한다.
    pub(super) fn expire_markdown_content_requests(&mut self, now: Instant) {
        let expired = take_expired(&mut self.remote.sessions, now);
        if expired.is_empty() {
            return;
        }
        for request in &expired {
            tracing::warn!(
                "markdown_content_request 회신 없음 (local surface {}, request {})",
                request.local_surface_id,
                request.request_id
            );
        }
        // park는 창 상태를 통째로 옮기므로 parked 상태에 남은 대기도 내린다.
        let failures = settle_expired(
            &expired,
            self.engines_mut()
                .sessions()
                .map(|(state, _)| &mut state.attach_pending),
        );
        for failure in &failures {
            push_markdown_content_result(&mut self.plugin_manager, failure);
        }
    }
}

#[cfg(test)]
#[path = "markdown_deadline_tests.rs"]
mod tests;
