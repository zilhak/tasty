//! 플러그인이 호스트 파일 피커를 연다. request_id를 즉시 반환하고
//! 선택·취소 결과는 file_picker.result 이벤트로 나중에 보낸다(ADR-0036).
//! 이미 열린 피커는 대체하지 않고 두 번째 요청을 거절한다.
//! 대체하면 기존 요청자에게 취소를 통지할 수 없어 결과를 기다리는 요청이 남기 때문이다.

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use crate::state::{AppState, FilePickerRequester};

/// `file_picker.trigger { filters?: string[], owner_popup_instance?: u64, start_dir?: string,
/// origin_surface_id?: u32 }` 요청.
#[derive(Deserialize)]
struct FilePickerTriggerReq {
    /// 확장자 필터(점 없이, 예: `["md", "markdown"]`). 비면 필터 없음.
    #[serde(default)]
    filters: Vec<String>,
    /// 플러그인 팝업 안에서 열면 자기 인스턴스를 지정해 부모·자식으로 연결한다.
    /// 팝업 밖에서 여는 경우에는 생략한다.
    #[serde(default)]
    owner_popup_instance: Option<u64>,
    /// 시작 디렉토리. 로컬 출발이면 절대경로여야 하고 디렉토리가 아니면 홈으로 폴백한다.
    /// 원격(mirror) 출발이면 원격 경로 문자열을 그대로 서버에 보낸다. 생략하면 출발
    /// surface 의 cwd, 그것도 없으면 홈.
    #[serde(default)]
    start_dir: Option<String>,
    /// 피커를 띄운 **로컬** surface id. 로컬/원격 판정을 이 surface 의 workspace 로 한다
    /// (생략하면 활성 workspace).
    #[serde(default)]
    origin_surface_id: Option<u32>,
}

pub fn handle_trigger(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    // 결과는 플러그인에게만 전달된다. 외부 호출은 결과 없이 사용자 포커스만 바꾸므로 거절한다.
    let requester_plugin = match caller {
        CallerContext::Plugin { plugin_id, .. } => plugin_id.clone(),
        CallerContext::Local | CallerContext::Agent { .. } => {
            return JsonRpcResponse::error(
                id,
                -32016,
                "method 'file_picker.trigger' answers only a plugin caller: the picked path \
                 is pushed to the calling plugin, so a CLI or agent caller would only take \
                 the user's input focus",
            );
        }
    };

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
    let requester = Some(FilePickerRequester {
        plugin_id: requester_plugin,
        request_id,
        owner_popup_instance: req.owner_popup_instance,
    });

    use crate::adapters::ui::popup::file_picker::FilePickerStart;
    let start = match req.start_dir {
        Some(dir) => FilePickerStart {
            dir: Some(dir),
            origin_surface_id: req.origin_surface_id,
        },
        None => FilePickerStart::from_surface(engine, req.origin_surface_id),
    };
    crate::adapters::ui::popup::file_picker::open(state, engine, requester, req.filters, start);

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
        assert_eq!(req.owner_popup_instance, None);
    }

    #[test]
    fn trigger_records_the_declared_owner_popup_instance() {
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "owner_popup_instance": 42 }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        let req = data.requester.as_ref().expect("requester recorded");
        assert_eq!(req.owner_popup_instance, Some(42));
        assert!(state.plugin_popup_has_open_child(42));
        assert!(!state.plugin_popup_has_open_child(43));
    }

    #[test]
    fn trigger_from_a_non_plugin_caller_is_refused_without_opening_the_popup() {
        let agent = CallerContext::Agent {
            agent_id: "child:1".to_string(),
            permissions: Arc::new(Default::default()),
        };
        for caller in [CallerContext::Local, agent] {
            let (mut state, mut engine) = make_test_state();
            let resp = handle_trigger(&mut state, &mut engine, &caller, json!(1), &json!({}));
            let err = resp.error.expect("non-plugin caller must be refused");
            assert_eq!(err.code, -32016, "{caller:?}");
            assert!(
                state.dialogs.file_picker.is_none(),
                "{caller:?} opened the picker"
            );
            assert!(
                state.take_pending_intents().is_empty(),
                "{caller:?} queued a popup intent"
            );
        }
    }

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

    fn home() -> String {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().to_string_lossy().into_owned())
            .expect("home dir")
    }

    #[test]
    fn local_start_dir_is_used_when_it_is_a_directory() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tasty_fp_start_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "start_dir": dir.to_string_lossy() }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert!(data.mirror_ws_id.is_none());
        assert_eq!(data.current_dir, dir.to_string_lossy());
        std::fs::remove_dir_all(&dir).expect("cleanup temp dir");
    }

    #[test]
    fn missing_local_start_dir_falls_back_to_home() {
        let (mut state, mut engine) = make_test_state();
        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "start_dir": "/definitely/not/a/tasty/dir" }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert_eq!(data.current_dir, home());
    }

    // 활성 workspace는 로컬로 두어 출발 surface 대신 활성 대상을 잘못 읽으면 드러나게 한다.
    fn push_background_mirror(engine: &mut crate::core::CoreState) -> (u32, u32) {
        let (ws_id, surface_id) = (9_000u32, 9_003u32);
        let mut ws = crate::model::Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            9_001,
            9_002,
            surface_id,
        );
        ws.mirror = true;
        engine.workspaces.push(ws);
        (ws_id, surface_id)
    }

    #[test]
    fn origin_surface_decides_remote_even_when_active_workspace_is_local() {
        let (mut state, mut engine) = make_test_state();
        let (ws_id, sid) = push_background_mirror(&mut engine);
        assert!(!engine.workspaces[state.active_workspace].mirror);

        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "start_dir": "/srv/remote/proj", "origin_surface_id": sid }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert_eq!(data.mirror_ws_id, Some(ws_id));
        assert_eq!(data.current_dir, "/srv/remote/proj");
        let forward = engine
            .pending_list_dir_forward
            .last()
            .expect("원격 조회가 큐잉된다");
        assert_eq!(
            forward.dir, "/srv/remote/proj",
            "원격 경로는 stat 없이 그대로 간다"
        );
    }

    #[test]
    fn origin_without_start_dir_uses_the_pushed_remote_cwd() {
        let (mut state, mut engine) = make_test_state();
        let (ws_id, sid) = push_background_mirror(&mut engine);
        engine.set_mirror_surface_cwd(sid, Some("/srv/pushed".to_string()));

        let resp = handle_trigger(
            &mut state,
            &mut engine,
            &plugin_caller("com.tasty.markdown"),
            json!(1),
            &json!({ "origin_surface_id": sid }),
        );
        assert!(resp.result.is_some());
        let data = state.dialogs.file_picker.as_ref().expect("popup open");
        assert_eq!(data.mirror_ws_id, Some(ws_id));
        assert_eq!(data.current_dir, "/srv/pushed");
    }
}

#[cfg(test)]
#[path = "file_picker_scope_tests.rs"]
mod scope_tests;
