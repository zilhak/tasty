//! `file_picker.trigger` IPC(TODO 21, ADR-0058) — plugin 이 host 소유 `file_picker`
//! popup(ADR-0053, `adapters::ui::popup::file_picker`)을 열도록 트리거한다.
//!
//! **popup 확정을 기다리지 않고** `request_id` 만 즉시 회신한다(비동기 accept) —
//! 실제 확정/취소 결과는 `src/app/dispatch/file_picker.rs` 가 확정 지점에서
//! `"file_picker.result"` 이벤트로 plugin 에 push 한다. `git_viewer.query`
//! (ADR-0056)와 동일한 "즉시 ack + 이벤트 push" shape.
//!
//! # 동시성 정책(TODO 21이 위임받은 결정)
//!
//! `file_picker` popup 은 단일 인스턴스만 존재한다(`AppState::dialogs.file_picker:
//! Option<..>`). 이미 열려 있는 상태에서 두 번째 `file_picker.trigger` 가 들어오면
//! **거부**한다(즉시 JSON-RPC 에러) — "이전 요청을 대체"하는 정책은 채택하지 않았다.
//! 이 트리거 핸들러는 `state`/`engine` 만 받고 `PluginManager` 접근권이 없어(이
//! 코드베이스의 확립된 관례 — IPC 핸들러는 `CoreState` 에 pending 을 큐잉하고
//! `App` 레벨 dispatch 가 `plugin_manager` 로 실제 이벤트를 emit 한다, `git_viewer`/
//! `list_dir` forward 와 동형), "대체" 정책을 택하면 밀려난 이전 요청의 plugin 에게
//! 즉시 취소를 통지할 방법이 없다 — 그 plugin 의 pending-map 항목이 응답을 영영
//! 받지 못한 채 무기한 남는다(`host.call` 자체는 이미 즉시 반환했으므로 타임아웃
//! 크래시는 아니지만, ADR-0058 이 세운 "모든 트리거는 정확히 하나의 결과를 받는다"
//! 계약을 조용히 깬다). 거부는 두 번째 plugin 의 `host.call` 이 즉시 에러로 끝나
//! 그 자리에서 재시도 여부를 판단할 수 있게 하므로 이 계약을 지킨다.

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use crate::state::{AppState, FilePickerRequester};

/// `file_picker.trigger { filters?: string[] }` 요청.
#[derive(Deserialize)]
struct FilePickerTriggerReq {
    /// 확장자 필터(점 없이, 예: `["md", "markdown"]`). 비면 필터 없음.
    #[serde(default)]
    filters: Vec<String>,
}

/// `file_picker.trigger` — `file_picker` popup 을 열고 `request_id` 만 즉시
/// 회신한다. 이미 popup 이 열려 있으면 거부(위 모듈 doc "동시성 정책" 참고).
pub fn handle_trigger(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if state.dialogs.file_picker.is_some() {
        return JsonRpcResponse::error(
            id,
            -32000,
            "file_picker popup is already open — retry after it closes",
        );
    }

    let req: FilePickerTriggerReq = match serde_json::from_value(params.clone()) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };

    let request_id = crate::core::next_file_picker_trigger_request_id();
    let requester = match caller {
        CallerContext::Plugin { plugin_id, .. } => Some(FilePickerRequester {
            plugin_id: plugin_id.clone(),
            request_id,
        }),
        // Local/Agent 가 직접 호출(예: 디버깅/CLI) — 이벤트를 받을 plugin 이 없다.
        CallerContext::Local | CallerContext::Agent { .. } => None,
    };

    crate::adapters::ui::popup::file_picker::open(state, engine, requester, req.filters);

    JsonRpcResponse::success(id, json!({ "request_id": request_id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tasty_memory::MemoryStorage;

    fn make_test_state() -> (AppState, crate::core::CoreState) {
        let term_waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, term_waker).unwrap();
        let preset_store = Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let state = AppState::new(&mut engine, preset_store, memory);
        (state, engine)
    }

    fn plugin_caller(id: &str) -> CallerContext {
        CallerContext::Plugin {
            plugin_id: id.to_string(),
            permissions: Arc::new(Default::default()),
        }
    }

    #[test]
    fn trigger_from_plugin_records_requester() {
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({}),
        );
        let result = resp.result.expect("trigger should succeed");
        let request_id = result["request_id"].as_u64().expect("request_id");

        let data = state
            .dialogs
            .file_picker
            .as_ref()
            .expect("popup should be open");
        let req = data.requester.as_ref().expect("requester recorded");
        assert_eq!(req.plugin_id, "com.tasty.markdown");
        assert_eq!(req.request_id, request_id);
    }

    #[test]
    fn trigger_from_local_leaves_requester_none() {
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &CallerContext::Local,
            json!(1),
            &json!({}),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert!(data.requester.is_none());
    }

    /// 동시성 정책(TODO 21 위임 결정) — 이미 popup 이 열려 있으면 두 번째 trigger 를
    /// 거부한다("대체" 아님). 근거는 이 파일 모듈 doc 참고.
    #[test]
    fn second_trigger_while_open_is_rejected() {
        let (mut state, mut engine) = make_test_state();
        let first = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({}),
        );
        assert!(first.result.is_some());

        let second = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.other"),
            json!(2),
            &json!({}),
        );
        assert!(second.result.is_none());
        assert!(second.error.is_some());

        // 첫 요청의 requester 는 그대로 살아 있다 — 대체되지 않았다.
        let req = state
            .dialogs
            .file_picker
            .as_ref()
            .and_then(|d| d.requester.as_ref())
            .expect("first requester still present");
        assert_eq!(req.plugin_id, "com.tasty.markdown");
    }

    #[test]
    fn trigger_passes_filters_through_to_popup_state() {
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "filters": ["md", "markdown"] }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert_eq!(data.filters, vec!["md".to_string(), "markdown".to_string()]);
    }
}
