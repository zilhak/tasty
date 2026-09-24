//! mirror 문서 원문을 원격에 요청한다. request_id를 즉시 반환하고
//! 결과는 markdown_mirror.content_result 이벤트로 해당 플러그인에게만 보낸다.
//! markdown prefix는 플러그인이 소유하므로 호스트가 받을 별도 namespace를 쓴다.

use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use super::params::require_u32;
use crate::core::{CoreState, PendingMarkdownContentForward};

/// 로컬 mirror surface ID로 원문을 요청한다. 원격 ID 변환은 attach 세션 매핑을 사용한다.
/// 대상 세션이 없으면 App의 요청 처리 단계에서 플러그인에 ok:false 결과를 보낸다.
pub fn handle_content_request(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_u32(params, "surface_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    // 에이전트 요청의 잘림 알림은 사용자에게 띄우지 않는다.
    let agent_origin = params
        .get("agent_origin")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let request_id = crate::core::next_markdown_content_request_id();
    engine
        .pending_markdown_content_forward
        .push(PendingMarkdownContentForward {
            local_surface_id: surface_id,
            request_id,
            agent_origin,
        });
    JsonRpcResponse::success(id, json!({ "request_id": request_id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_request_queues_and_returns_nonzero_request_id() {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).unwrap();
        let resp = handle_content_request(&mut engine, json!(1), &json!({ "surface_id": 7 }));
        let rid = resp
            .result
            .as_ref()
            .and_then(|r| r.get("request_id"))
            .and_then(|v| v.as_u64())
            .expect("request_id");
        // 0은 요청 취소 신호로 예약된 값이다.
        assert_ne!(rid, 0);
        assert_eq!(engine.pending_markdown_content_forward.len(), 1);
        assert_eq!(
            engine.pending_markdown_content_forward[0].local_surface_id,
            7
        );
        assert_eq!(engine.pending_markdown_content_forward[0].request_id, rid);
        assert!(
            !engine.pending_markdown_content_forward[0].agent_origin,
            "칸이 없으면 plugin 자신의 요청이다"
        );
    }

    #[test]
    fn content_request_carries_the_agent_origin() {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).unwrap();
        let resp = handle_content_request(
            &mut engine,
            json!(1),
            &json!({ "surface_id": 7, "agent_origin": true }),
        );
        assert!(resp.error.is_none());
        assert!(engine.pending_markdown_content_forward[0].agent_origin);
    }

    #[test]
    fn content_request_without_surface_id_is_invalid_params() {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).unwrap();
        let resp = handle_content_request(&mut engine, json!(1), &json!({}));
        assert!(resp.error.is_some());
        assert!(engine.pending_markdown_content_forward.is_empty());
    }
}
