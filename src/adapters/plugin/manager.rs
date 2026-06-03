//! Plugin 생명주기 매니저.
//!
//! 호스트의 부팅 시 한 번 만들어지고, `App`이 유일한 인스턴스를 보유한다.
//! - 부팅 시 `discover_and_start()`로 `~/.tasty/plugins/`를 스캔하여 활성 plugin 모두 spawn
//! - 매 메인 루프 tick에서 `pump()` 호출 → plugin 알림 처리 + 헬스체크 + 재시작
//! - 종료 시 `shutdown_all()`

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::json;
use tasty_plugin_protocol::{HandleChannelMessage, SharedBufferCreateResult, SharedBufferId};
#[cfg(unix)]
use tasty_shm::PeerPid;
use tasty_shm::SharedMemory;

use tasty_plugin_protocol::host_port::SurfaceRegistry;

use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::send_response;
use crate::plugin::handle_channel::HandleListener;
use crate::plugin::host_cmd::{HostCmd, SurfaceHandles};
use crate::plugin::ipc_namespace::IpcNamespaceRegistry;
use crate::plugin::listener::HostListener;
use crate::plugin::manifest::{EventHookDecl, HookMode, IpcHookDecl, Permission, PluginPackage};
use crate::plugin::process::PluginProcess;
use crate::plugin::protocol::{
    self, IpcCallResult, PluginEvent, PluginRequest, PluginResponse, SurfaceResult,
};
use crate::plugin::registry_state::PluginsConfig;

pub(super) const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const PING_INTERVAL: Duration = Duration::from_secs(15);
pub(super) const RESTART_FAILURE_WINDOW: Duration = Duration::from_secs(10);
pub(super) const RESTART_FAILURE_LIMIT: usize = 3;

/// IPC 응답을 최종적으로 어디로 회신해야 하는지를 식별. 호스트 외부 caller(CLI/사용자)는
/// `Local`, 다른 plugin이면 `Plugin`.
pub(super) enum FinalCaller {
    Local {
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    Plugin {
        caller_plugin_id: String,
        call_id: u64,
    },
}

/// pending host→plugin request의 종류. 응답 수신 시 어떤 후처리를 할지 식별.
pub(super) enum PendingRequestKind {
    SurfaceCreate {
        surface_id: u32,
    },
    SurfaceEvent {
        surface_id: u32,
    },
    SurfaceRestore {
        surface_id: u32,
    },
    /// 단계 G: 단축키 매칭으로 plugin command가 트리거된 경우. 응답은
    /// SurfaceResult 형태로 surface tree/display_name을 갱신할 수 있다.
    CommandInvoke {
        surface_id: u32,
    },
    /// popup.open IPC 응답 대기. 응답은 [`PopupOpenResult`] — 초기 tree.
    PopupOpen {
        instance_id: u64,
    },
    /// popup.event IPC 응답 대기. 응답은 [`PopupEventResult`] — 갱신 tree + close 신호.
    PopupEvent {
        instance_id: u64,
    },
    /// 그 외 (host.hello / ping / 등) — 응답 무시.
    Other,
    /// Client IPC 요청을 plugin namespace로 forward한 경우. plugin이 응답을 주면
    /// 보관한 response_tx로 client에 회신한다.
    NamespaceInvoke {
        plugin_id: String,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    /// 다른 plugin이 보낸 IpcCall이 namespace 메서드인 경우. target plugin이 응답을
    /// 주면 caller plugin에 `ipc.result`로 회신한다.
    PluginToPluginNamespace {
        /// forward 받은 target plugin (응답을 주는 쪽).
        plugin_id: String,
        /// 호출한 plugin (응답을 받을 쪽).
        caller_plugin_id: String,
        /// caller plugin이 ipc.call 시점에 발급한 call_id.
        call_id: u64,
    },
    /// extension의 pre-IPC hook을 dispatch한 뒤 응답 대기. extension이 응답을 주면
    /// (transform이면 payload 교체, filter면 차단 결정) 그 결과로 target plugin에
    /// 실제 ipc.invoke를 보낸다. post-hook이 있으면 함께 전달해 두 번째 phase에 사용.
    ExtensionPreIpcHook {
        target_plugin_id: String,
        extension_plugin_id: String,
        method: String,
        params: serde_json::Value,
        pre_hook_mode: HookMode,
        final_caller: FinalCaller,
        post_hook: Option<IpcHookDecl>,
        /// hook 응답이 도착해야 하는 시각. 지나면 timeout으로 처리.
        deadline: Instant,
    },
    /// extension의 post-IPC hook을 dispatch한 뒤 응답 대기. extension이 응답을 주면
    /// (transform이면 payload 교체, filter면 ignored — post는 차단 무의미)
    /// 그 결과로 caller에 최종 응답.
    ExtensionPostIpcHook {
        extension_plugin_id: String,
        method: String,
        post_hook_mode: HookMode,
        /// target plugin의 응답을 그대로 들고 온 것. Ok면 result, Err면 (msg, code).
        target_outcome: TargetOutcome,
        final_caller: FinalCaller,
        deadline: Instant,
    },
    /// pre-hook 없이 target에 직접 ipc.invoke한 뒤, 매칭 post-hook이 있어
    /// 응답이 오면 post-hook으로 chain해야 하는 경우의 pending.
    NamespaceInvokeWithPostHook {
        target_plugin_id: String,
        method: String,
        extension_plugin_id: String,
        post_hook_decl: IpcHookDecl,
        final_caller: FinalCaller,
    },
    /// debug 빌드 한정 — `debug.extension.invoke_hook`이 보낸 hook 응답을 그대로
    /// caller(local CLI)에 회신.
    #[cfg(debug_assertions)]
    DebugExtensionInvokeHook {
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        original_id: serde_json::Value,
    },
    /// extension의 pre-event hook을 dispatch한 뒤 응답 대기. 응답이 오면
    /// (transform이면 envelope.payload 교체, filter면 fan-out 차단)
    /// event_bus.fan_out으로 진행. post_event가 있으면 함께 둔다.
    ExtensionPreEventHook {
        publisher_plugin_id: String,
        extension_plugin_id: String,
        envelope: tasty_plugin_protocol::EventEnvelope,
        pre_hook_mode: HookMode,
        post_hook: Option<super::manifest::EventHookDecl>,
        deadline: Instant,
    },
    /// extension의 post-event hook을 dispatch한 뒤 응답 대기. event는 이미 fan-out 됐으므로
    /// post 응답은 observe로만 의미가 있다 (transform/filter는 ignore).
    ExtensionPostEventHook {
        extension_plugin_id: String,
        event_key: String,
        deadline: Instant,
    },
}

/// 연속 실패 hook의 backoff 윈도우. 3회 연속 실패 후 60초 동안 hook 우회.
pub(super) const HOOK_FAIL_BACKOFF: Duration = Duration::from_secs(60);
pub(super) const HOOK_FAIL_LIMIT: u8 = 3;

/// (ext_id, method) 단위 hook 실패 추적. consecutive_failures가 HOOK_FAIL_LIMIT에 도달하면
/// backoff_until로 설정해 그 동안 hook을 우회한다.
#[derive(Debug, Clone, Default)]
pub(super) struct HookFailureState {
    pub(super) consecutive_failures: u8,
    pub(super) backoff_until: Option<Instant>,
}

/// target plugin의 ipc.invoke 응답 결과. post-hook 진입 시 보존해 둔다.
pub(super) enum TargetOutcome {
    Ok(serde_json::Value),
    Err { message: String, code: i32 },
}

/// extension hook 응답에서 추출한 결정.
pub(super) enum HookOutcome {
    /// payload를 새 값으로 교체 (transform).
    Modified(serde_json::Value),
    /// 차단 (filter).
    Block,
    /// 그 외 — observe, filter pass, transform no-op, 응답 누락, 파싱 실패.
    Pass,
}

/// `ExtensionHookResult` JSON에서 outcome 추출. 에러/누락은 fail-open(`Pass`).
pub(super) fn parse_hook_result(resp: &PluginResponse) -> HookOutcome {
    if resp.error.is_some() {
        return HookOutcome::Pass;
    }
    let result = match &resp.result {
        Some(v) => v,
        None => return HookOutcome::Pass,
    };
    if let Some(v) = result.get("modified_payload") {
        if !v.is_null() {
            return HookOutcome::Modified(v.clone());
        }
    }
    if let Some(pass) = result.get("pass").and_then(|p| p.as_bool()) {
        if !pass {
            return HookOutcome::Block;
        }
    }
    HookOutcome::Pass
}

pub(super) struct RemoteSurfaceEntry {
    pub(super) plugin_id: String,
    pub(super) handles: SurfaceHandles,
}

pub struct PluginManager {
    pub packages: Vec<PluginPackage>,
    pub processes: HashMap<String, PluginProcess>,
    pub config: PluginsConfig,
    pub(super) waker: tasty_terminal::waker_factory::SharedWakerFactory,
    pub(super) listener: Option<HostListener>,
    /// 보조 핸들 채널 listener. shared buffer 핸들 전송에 사용. Windows에서는 02c까지
    /// `None`으로 유지된다 (HandleListener::bind가 Unsupported를 반환).
    pub(super) handle_listener: Option<HandleListener>,
    pub log_dir: PathBuf,
    pub(super) next_request_id: AtomicU64,
    pub(super) last_ping: Instant,
    /// plugin id → 최근 spawn 실패 timestamps. 짧은 시간 내 반복 실패하면 자동 disable.
    pub(super) spawn_failures: HashMap<String, Vec<Instant>>,
    /// 자동 disable되어 사용자가 수동 enable하기 전까지 더 이상 spawn 시도 안 함.
    pub(super) auto_disabled: std::collections::HashSet<String>,
    /// hello 받은 plugin의 surface_kinds를 등록하기 위한 registry 핸들. None이면
    /// registry 등록 동작이 비활성 (헤드리스/테스트).
    pub surface_registry: Option<Arc<dyn SurfaceRegistry>>,
    /// 이미 registry에 등록된 plugin id (hello를 여러 번 받아도 1회만 등록).
    pub(crate) registered_plugins: std::collections::HashSet<String>,
    /// registry create/restore closure가 새 RemoteSurface 등록을 보내는 채널.
    pub(crate) host_cmd_tx: Sender<HostCmd>,
    pub(super) host_cmd_rx: Receiver<HostCmd>,
    /// surface_id → RemoteSurface handle. 라이프사이클 동안 유지.
    pub(super) surfaces: HashMap<u32, RemoteSurfaceEntry>,
    /// host → plugin 요청 ID → 종류. 응답 수신 시 후처리 dispatch용.
    pub(super) pending_requests: HashMap<u64, PendingRequestKind>,
    /// 각 plugin에 grant된 권한. 매니페스트 + plugins.toml의 granted를 교집합한 결과.
    /// `Arc`로 공유하여 CallerContext가 동시 호출 시 안전.
    pub(super) plugin_permissions: HashMap<String, Arc<HashSet<Permission>>>,
    /// plugin이 보낸 IpcCall을 호스트의 main loop에서 라우팅 처리하기 위해 모으는 큐.
    /// `App::process_plugin_ipc_calls()`가 매 tick에 비운다.
    pub(super) pending_plugin_calls: Vec<PendingPluginCall>,
    /// plugin이 매니페스트로 선언한 단축키 command 일람. plugin
    /// enable/disable/install/remove 시 갱신됨.
    pub command_registry: super::command_registry::PluginCommandRegistry,
    /// plugin이 매니페스트로 선언한 IPC namespace prefix 일람. plugin이
    /// 실행 중일 때만 등록되며, 호스트 IPC dispatcher가 namespace 메서드를
    /// 어느 plugin에 forward할지 해결할 때 조회한다.
    pub ipc_namespaces: IpcNamespaceRegistry,
    /// plugin id → (buffer id → 매핑 영역). 호스트가 `host.shared_buffer.create`로
    /// 발급한 영역의 매핑 유지(=OS region keep-alive)와 dirty 수신 시 lookup용.
    /// plugin process가 종료/재시작되면 해당 plugin 슬롯이 통째로 drop되어
    /// 매핑이 해제된다.
    pub(super) plugin_buffers: HashMap<String, HashMap<SharedBufferId, SharedMemory>>,
    /// 호스트 전체에서 단조 증가하는 shared buffer id. plugin 간 충돌 회피 + 디버그
    /// 추적을 단순화하기 위해 글로벌 카운터로 둔다.
    pub(super) next_buffer_id: AtomicU64,
    /// Plugin extension 상태 추적. `[extends]` 블록을 선언한 plugin들의
    /// active/pending/disabled/conflict 상태를 보관한다. PR 4/5에서 event/IPC
    /// hook dispatch 시 `active_extension_for_target`을 조회한다.
    pub extensions: super::extension_registry::ExtensionRegistry,
    /// (ext_id, method) 단위 hook 실패 추적. 3회 연속 실패하면 60초간 backoff.
    pub(super) hook_failures: HashMap<(String, String), HookFailureState>,
    /// Event Bus 1.0 라우터. 호스트 본문과 plugin 간 broadcast 이벤트를 fan-out.
    pub event_bus: super::event_bus::EventBus,
    /// 호스트가 발화하는 envelope의 `meta.trace_id` 카운터.
    pub(super) event_trace_seq: AtomicU64,
    /// 현재 열려 있는 popup 인스턴스. host가 발급한 `instance_id`를 키로 사용.
    pub(super) popup_instances: HashMap<u64, PopupInstance>,
    /// 다음 popup `instance_id`. 1부터 시작해 단조 증가.
    pub(super) next_popup_instance_id: u64,
    /// 파일 형식 식별 시스템. plugin enable/disable 시 detector 추가/제거.
    /// 호스트 본문이 CoreState 와 같은 Arc 를 공유.
    pub file_format: Arc<crate::file::format::FileFormatRegistry>,
    /// 파일 핸들러 시스템. plugin enable/disable 시 handler 추가/제거.
    pub file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    /// i18n namespace 등록 trait. None 이면 등록 skip (headless/test).
    pub i18n_registrar: Option<Arc<dyn tasty_plugin_protocol::host_port::I18nNamespaceRegistrar>>,
}

/// 호스트가 추적 중인 popup 인스턴스 한 건. plugin process가 죽으면 함께 제거된다.
#[derive(Debug, Clone)]
pub struct PopupInstance {
    pub plugin_id: String,
    pub popup_id: String,
    pub contribute: super::manifest::PopupContribute,
    /// plugin이 마지막으로 보낸 UI 트리. 아직 open 응답 전이면 `None`.
    pub tree: Option<tasty_plugin_protocol::ui_tree::UiNode>,
}

/// plugin → host IPC 호출 한 건. 라우팅 후 결과를 plugin에 회신해야 함.
#[derive(Debug, Clone)]
pub struct PendingPluginCall {
    pub plugin_id: String,
    pub call_id: u64,
    pub method: String,
    pub params: serde_json::Value,
    pub permissions: Arc<HashSet<Permission>>,
}

/// `[[contributes.popup]]` 한 항목 + 소유 plugin id. 호스트의 popup 라우터가
/// trigger 매칭에 사용하기 위한 평탄 뷰.
#[derive(Debug, Clone)]
pub struct PluginPopupEntry {
    pub plugin_id: String,
    pub contribute: crate::plugin::manifest::PopupContribute,
}

mod buffer;
mod events;
mod ipc_dispatch;
mod lifecycle;
mod popup;
mod pump;
mod queries;
mod response;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::process::PluginProcess;

    fn empty_waker() -> tasty_terminal::waker_factory::SharedWakerFactory {
        // headless 환경에서 PluginManager가 사용하는 waker — 실제 wake는 no-op로 충분.
        Arc::new(tasty_terminal::waker_factory::NoopWakerFactory)
    }

    /// validate_namespace_call의 분기를 직접 검증하기 위한 mgr 초기화.
    /// process는 spawn하지 않고 ipc_namespaces와 plugin_permissions만 직접 채운다.
    fn mgr_with_namespace_owner(owner: &str, prefix: &str) -> PluginManager {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.ipc_namespaces
            .register(owner, prefix)
            .expect("test prefix should be unique");
        mgr
    }

    #[test]
    fn validate_namespace_call_method_not_found() {
        let mgr = PluginManager::new(empty_waker());
        let err = mgr
            .validate_namespace_call("nope.method", None)
            .unwrap_err();
        assert_eq!(err.0, -32601);
    }

    #[test]
    fn validate_namespace_call_local_caller_allowed_when_target_running() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        // process 가짜 entry 삽입 — request 전송은 안 한다, 검증만.
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        let id = mgr
            .validate_namespace_call("codex.spawn", None)
            .expect("local caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    #[test]
    fn validate_namespace_call_target_not_running() {
        let mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        let err = mgr
            .validate_namespace_call("codex.spawn", None)
            .unwrap_err();
        assert_eq!(err.0, -32002);
    }

    #[test]
    fn validate_namespace_call_self_invocation_rejected() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.codex"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("its own namespace"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_without_grant_denied() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        // caller plugin은 ipc.invoke:codex 권한 없음
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::SurfaceRead]),
        );
        let err = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .unwrap_err();
        assert_eq!(err.0, -32001);
        assert!(err.1.contains("permission_denied"));
        assert!(err.1.contains("ipc.invoke:codex"));
    }

    #[test]
    fn validate_namespace_call_plugin_caller_with_grant_allowed() {
        let mut mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        mgr.processes
            .insert("com.example.codex".into(), stub_process());
        mgr.set_plugin_permissions(
            "com.example.helper",
            HashSet::from([Permission::IpcInvoke("codex".into())]),
        );
        let id = mgr
            .validate_namespace_call("codex.spawn", Some("com.example.helper"))
            .expect("granted caller should pass");
        assert_eq!(id, "com.example.codex");
    }

    /// validate 만 보는 테스트용 stub. PluginProcess는 process.rs의 cfg(test)
    /// 헬퍼를 위임 호출한다.
    fn stub_process() -> PluginProcess {
        PluginProcess::stub_for_test("stub")
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_zero_size() {
        let mut mgr = PluginManager::new(empty_waker());
        mgr.processes.insert("com.example.x".into(), stub_process());
        let err = mgr
            .create_shared_buffer_for("com.example.x", 1, 0)
            .unwrap_err();
        assert!(err.contains("size must be > 0"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_unknown_plugin() {
        let mut mgr = PluginManager::new(empty_waker());
        let err = mgr
            .create_shared_buffer_for("com.example.ghost", 1, 4096)
            .unwrap_err();
        assert!(err.contains("not running"), "got: {err}");
    }

    fn make_response(result: Option<serde_json::Value>, error: Option<&str>) -> PluginResponse {
        PluginResponse {
            id: 1,
            result,
            error: error.map(String::from),
            error_code: None,
        }
    }

    #[test]
    fn parse_hook_result_pass_on_missing_result() {
        let resp = make_response(None, None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn parse_hook_result_pass_on_error() {
        let resp = make_response(Some(serde_json::json!({"pass": false})), Some("boom"));
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn parse_hook_result_block_on_pass_false() {
        let resp = make_response(Some(serde_json::json!({"pass": false})), None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Block));
    }

    #[test]
    fn parse_hook_result_modified_payload_takes_precedence() {
        let resp = make_response(
            Some(serde_json::json!({"modified_payload": {"x": 1}, "pass": false})),
            None,
        );
        match parse_hook_result(&resp) {
            HookOutcome::Modified(v) => assert_eq!(v, serde_json::json!({"x": 1})),
            other => panic!(
                "expected Modified, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parse_hook_result_pass_when_modified_null() {
        let resp = make_response(Some(serde_json::json!({"modified_payload": null})), None);
        assert!(matches!(parse_hook_result(&resp), HookOutcome::Pass));
    }

    #[test]
    fn record_hook_failure_triggers_backoff_after_limit() {
        let mut mgr = PluginManager::new(empty_waker());
        let ext = "com.example.ext";
        let method = "codex.spawn";
        assert!(!mgr.is_hook_in_backoff(ext, method));
        for _ in 0..HOOK_FAIL_LIMIT {
            mgr.record_hook_failure(ext, method);
        }
        assert!(mgr.is_hook_in_backoff(ext, method));
    }

    #[test]
    fn record_hook_success_resets_counter() {
        let mut mgr = PluginManager::new(empty_waker());
        let ext = "com.example.ext";
        let method = "codex.spawn";
        mgr.record_hook_failure(ext, method);
        mgr.record_hook_failure(ext, method);
        mgr.record_hook_success(ext, method);
        // 추가로 (HOOK_FAIL_LIMIT - 1)회 실패만으로는 backoff 진입 금지.
        for _ in 0..(HOOK_FAIL_LIMIT - 1) {
            mgr.record_hook_failure(ext, method);
        }
        assert!(!mgr.is_hook_in_backoff(ext, method));
    }

    #[test]
    fn find_active_ipc_hooks_returns_none_when_no_extension() {
        let mgr = mgr_with_namespace_owner("com.example.codex", "codex");
        assert!(
            mgr.find_active_ipc_hooks("com.example.codex", "codex.spawn")
                .is_none()
        );
    }

    #[test]
    fn find_active_event_hooks_returns_none_when_no_extension() {
        let mgr = PluginManager::new(empty_waker());
        assert!(
            mgr.find_active_event_hooks("com.example.foo", "foo.bar")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_shared_buffer_rejects_when_handle_listener_missing() {
        // stub mgr는 handle_listener를 bind하지 않으므로 즉시 거절되어야 한다.
        let mut mgr = PluginManager::new(empty_waker());
        mgr.processes.insert("com.example.x".into(), stub_process());
        assert!(mgr.handle_listener.is_none());
        let err = mgr
            .create_shared_buffer_for("com.example.x", 1, 4096)
            .unwrap_err();
        assert!(err.contains("handle channel not available"), "got: {err}");
    }
}
