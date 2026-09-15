use serde_json::json;

use crate::i18n::t;
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

pub fn handle_notification_list(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let notifications: Vec<_> = engine
        .notifications
        .all()
        .rev()
        .map(|n| {
            json!({
                "id": n.id,
                "title": n.title,
                "body": n.body,
                "workspace_id": n.source_workspace,
                "surface_id": n.source_surface,
                "read": n.read,
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(latest_notifications(notifications)))
}

/// Preserve the single-engine contract: newest creation first, at most 50.
/// Coalescing updates an entry in place without assigning it a new creation ID.
/// Apply the same cap after merging engines, never 50 times the window count.
pub(crate) fn latest_notifications(mut rows: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    rows.sort_by_key(|row| std::cmp::Reverse(row["id"].as_u64()));
    rows.truncate(50);
    rows
}

pub fn handle_notification_create(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(t("notification.default_title"))
        .to_string();
    let body = params
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let surface_id = match super::params::optional_u32(params, "surface_id", &id) {
        Ok(v) => v.unwrap_or(0),
        Err(e) => return e,
    };

    // workspace_id 결정 — CLAUDE.md "포커스 독립성" 원칙: IPC는 사용자 포커스에
    // 의존하지 않아야 한다. workspace_id를 명시하지 않으면 다음 순으로 결정.
    let ws_param = match super::params::optional_u32(params, "workspace_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ws_id = match ws_param {
        Some(v) => v,
        None => {
            // 1. surface_id가 주어지면 그 surface가 속한 워크스페이스로 라우팅
            if surface_id > 0 {
                if let Some((idx, _)) = engine.find_workspace_index_for_surface(surface_id) {
                    engine.workspaces[idx].id
                } else {
                    return JsonRpcResponse::invalid_params(
                        id,
                        format!(
                            "surface_id {} not found; cannot determine workspace",
                            surface_id
                        ),
                    );
                }
            // 2. 워크스페이스가 정확히 1개면 자동 사용 (호환성 폴백, 1버전 유지 예정)
            } else if engine.workspaces.len() == 1 {
                tracing::warn!(
                    "notification.create called without 'workspace_id' or 'surface_id'; \
                     auto-routing to the only workspace. \
                     This fallback will be removed in a future version."
                );
                engine.workspaces[0].id
            } else {
                return JsonRpcResponse::invalid_params(
                    id,
                    "Missing required 'workspace_id' parameter \
                     (focus-independent IPC requires explicit workspace_id, or surface_id \
                     to route by ownership)",
                );
            }
        }
    };
    // mutate 는 Core::apply 단일 진입점 — handler 는 read 후 enqueue.
    // cascade (notifications.add + host event enqueue) 는
    // App.cascade_notification_pushed 가 처리.
    state.dispatch_intent(
        crate::core::intent::DomainIntent::PushNotification {
            ws_id,
            surface_id,
            title,
            body,
            source: "host".to_string(),
        }
        .from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "created": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_merge_has_one_global_limit_and_matches_creation_order() {
        let (state_a, mut a) = crate::state::tests::test_state();
        let (state_b, mut b) = crate::state::tests::test_state();
        let ids = crate::core::state::IdGenerator::new();
        a.notifications =
            crate::notification::NotificationStore::with_counter(0, ids.notification_counter());
        b.notifications =
            crate::notification::NotificationStore::with_counter(0, ids.notification_counter());
        for i in 0..120 {
            let store = if i % 2 == 0 {
                &mut a.notifications
            } else {
                &mut b.notifications
            };
            store.add(1, 1, format!("entry-{i}"), String::new());
        }
        let mut rows = Vec::new();
        for (state, engine) in [(&state_b, &b), (&state_a, &a)] {
            rows.extend(
                handle_notification_list(state, engine, json!(1))
                    .result
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            );
        }
        let merged = latest_notifications(rows);
        assert_eq!(merged.len(), 50);
        let titles: Vec<_> = merged
            .iter()
            .map(|n| n["title"].as_str().unwrap())
            .collect();
        let expected: Vec<_> = (70..120).rev().map(|i| format!("entry-{i}")).collect();
        assert_eq!(titles, expected);
        let ids: std::collections::BTreeSet<_> =
            merged.iter().map(|n| n["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), 50);
    }
}
