//! 완료 전략 전역 레지스트리를 조회한다. 설정 재로드나 직접 실행 API는 제공하지 않는다.

use serde_json::json;

use crate::completion_strategy::{self, CompletionStrategyKind};
use tasty_ipc::protocol::JsonRpcResponse;

/// 비활성 전략을 포함해 모두 반환한다. 사용자 포커스와 상태를 바꾸지 않는다.
pub fn handle_list(id: serde_json::Value) -> JsonRpcResponse {
    let items: Vec<_> = completion_strategy::global()
        .all_strategies_including_disabled()
        .into_iter()
        .map(|s| {
            let (kind, poll_method, notify_via, timeout_ms) = match &s.kind {
                CompletionStrategyKind::Poll(spec) => {
                    ("poll", Some(spec.poll_method.clone()), None, None)
                }
                CompletionStrategyKind::Push {
                    notify_via,
                    timeout_ms,
                } => (
                    "push",
                    None,
                    Some(notify_via.as_str().to_string()),
                    Some(*timeout_ms),
                ),
            };
            json!({
                "id": s.id.as_str(),
                "priority": s.priority,
                "owner": s.owner.prefix(),
                "kind": kind,
                "poll_method": poll_method,
                "notify_via": notify_via,
                "timeout_ms": timeout_ms,
                "disabled": s.disabled,
                "display_name_i18n_key": s.display_name_i18n_key,
                "default_for_methods": s.default_for_methods,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "strategies": items }))
}
