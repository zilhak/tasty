//! `completion_strategy.*` IPC 핸들러 — 완료 판정 전략 레지스트리 조회 (TODO80 §B).
//!
//! `hook_handler.list`(`src/adapters/ipc/handler/hook_handler.rs`)를 미러링한다.
//! 상태는 `crate::completion_strategy` 전역 싱글턴이라 engine/state 를 받지 않는다.
//! reload/dispatch 대응물은 없다 — 전략은 "발화" 대상이 아니라 판정 함수이고,
//! user config 재로드는 아직 노출하지 않는다(Settings UI CRUD 표면 없음).
//!
//! **불가침 원칙 2·3**: 조회는 에이전트 작업이라 IPC+CLI 양면 노출. list 는 전
//! 범위(비활성 포함) 조회 — 사용자 포커스/상태에 부수효과 없음.

use serde_json::json;

use crate::completion_strategy::{self, CompletionStrategyKind};
use tasty_ipc::protocol::JsonRpcResponse;

/// `completion_strategy.list` — 등록된 모든 완료 판정 전략(비활성 포함, 포커스
/// 독립·전 범위).
///
/// 각 항목: id / priority / owner / kind(poll/push, poll 은 poll_method 요약,
/// push 는 notify_via+timeout_ms) / disabled / display_name_i18n_key /
/// default_for_methods.
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
