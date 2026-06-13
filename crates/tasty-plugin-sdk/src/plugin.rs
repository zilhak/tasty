//! Plugin trait — plugin 작성자가 구현하는 진입점.
//!
//! 사용 예:
//!
//! ```ignore
//! struct MyExplorer;
//!
//! impl Plugin for MyExplorer {
//!     fn id(&self) -> &str { "com.example.explorer" }
//!     fn version(&self) -> &str { "0.1.0" }
//!     fn surface_kinds(&self) -> Vec<&str> { vec!["explorer"] }
//!
//!     fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
//!         // initial tree
//!         SurfaceResult { tree: Some(my_tree()), display_name: Some("Files".into()) }
//!     }
//!
//!     fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult {
//!         // event 처리 후 새 tree 반환
//!         SurfaceResult { tree: Some(my_tree()), display_name: None }
//!     }
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     tasty_plugin_sdk::run(MyExplorer)
//! }
//! ```

use std::path::PathBuf;

use serde_json::Value;
pub use tasty_plugin_protocol::SurfaceResult;
use tasty_plugin_protocol::ui_tree::UiEvent;
pub use tasty_plugin_protocol::{PopupEventResult, PopupOpenResult};

use crate::host::HostHandle;

#[derive(Debug, Clone)]
pub struct SurfaceCreateCtx {
    pub surface_id: u32,
    pub kind: String,
    /// 호스트가 source surface 로부터 carry 한 시작 cwd. None 이면 plugin 측에서
    /// params 우선순위 또는 자체 fallback (예: home dir) 으로 결정.
    /// Surface cwd invariant — `docs/architecture/invariants/surface-cwd.md`.
    pub cwd: Option<PathBuf>,
    pub params: Value,
}

#[derive(Debug, Clone)]
pub struct SurfaceEventCtx {
    pub surface_id: u32,
    pub event: UiEvent,
}

#[derive(Debug, Clone)]
pub struct SurfaceRestoreCtx {
    pub surface_id: u32,
    pub kind: String,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct SurfaceSnapshotCtx {
    pub surface_id: u32,
}

/// 매니페스트 `[[contributes.commands]]`로 등록한 command가 사용자 단축키
/// 매칭으로 호출됐을 때 전달되는 컨텍스트.
#[derive(Debug, Clone)]
pub struct CommandInvokeCtx {
    pub surface_id: u32,
    pub command_id: String,
}

/// `popup.open` 콜백 컨텍스트.
#[derive(Debug, Clone)]
pub struct PopupOpenCtx {
    /// 매니페스트 `[[contributes.popup]]`의 `id` (plugin 내 로컬 id).
    pub popup_id: String,
    /// 같은 popup_id의 여러 인스턴스를 구분하기 위해 호스트가 발급한 id.
    pub instance_id: u64,
    /// trigger 시점에 호스트가 알 수 있던 컨텍스트 (event payload 등). 없으면 Null.
    pub context: Value,
}

/// `popup.event` 콜백 컨텍스트.
#[derive(Debug, Clone)]
pub struct PopupEventCtx {
    pub instance_id: u64,
    pub event: UiEvent,
}

/// `popup.closed` 콜백 컨텍스트. fire-and-forget이므로 반환값 없음.
#[derive(Debug, Clone)]
pub struct PopupClosedCtx {
    pub instance_id: u64,
    pub reason: tasty_plugin_protocol::PopupCloseReason,
}

/// `extension.invoke_hook` 컨텍스트. extension plugin이 hook 호출을 받았을 때.
#[derive(Clone)]
pub struct ExtensionHookCtx {
    /// event(이벤트 발화 가로채기) / ipc(IPC 호출 가로채기).
    pub kind: tasty_plugin_protocol::ExtensionHookKind,
    /// pre(흐름 시작 전) / post(흐름 종료 후).
    pub phase: tasty_plugin_protocol::ExtensionHookPhase,
    /// transform(payload 수정) / filter(차단 결정) / observe(관찰).
    pub mode: tasty_plugin_protocol::ExtensionHookMode,
    /// 매칭된 hook의 대상 — event key 또는 IPC method.
    pub target: String,
    /// 가공/관찰 대상 payload (envelope.payload, IPC params, IPC result 중 하나).
    pub payload: Value,
    pub host: HostHandle,
}

impl std::fmt::Debug for ExtensionHookCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionHookCtx")
            .field("kind", &self.kind)
            .field("phase", &self.phase)
            .field("mode", &self.mode)
            .field("target", &self.target)
            .field("payload", &self.payload)
            .field("host", &"HostHandle { .. }")
            .finish()
    }
}

/// extension hook의 응답. host에 `ExtensionHookResult`로 직렬화되어 전달된다.
///
/// 헬퍼 생성자(`Self::pass`, `Self::block`, `Self::transformed`, `Self::observed`)를
/// 사용하면 mode와 의미가 맞지 않는 잘못된 조합을 피하기 쉽다.
#[derive(Debug, Clone, Default)]
pub struct ExtensionHookOutcome {
    pub modified_payload: Option<Value>,
    pub pass: Option<bool>,
}

impl ExtensionHookOutcome {
    /// observe / filter pass / transform no-op 모두에 사용할 수 있는 "그대로 통과" 결과.
    pub fn pass() -> Self {
        Self::default()
    }

    /// filter mode에서 흐름을 차단.
    pub fn block() -> Self {
        Self {
            pass: Some(false),
            modified_payload: None,
        }
    }

    /// transform mode에서 payload를 새 값으로 교체.
    pub fn transformed(payload: Value) -> Self {
        Self {
            pass: None,
            modified_payload: Some(payload),
        }
    }

    pub(crate) fn into_proto(self) -> tasty_plugin_protocol::ExtensionHookResult {
        tasty_plugin_protocol::ExtensionHookResult {
            modified_payload: self.modified_payload,
            pass: self.pass,
        }
    }
}

/// 매니페스트 `[[contributes.ipc_namespace]]`로 점유한 prefix의 메서드가
/// IPC 라우터로부터 forward됐을 때 plugin에 전달되는 컨텍스트.
#[derive(Clone)]
pub struct IpcMethodCtx {
    /// 호스트가 받은 원본 메서드 이름 (예: "codex.spawn"). plugin은 이걸로
    /// 내부 dispatch한다.
    pub method: String,
    pub params: Value,
    /// caller가 plugin이면 그 plugin id, CLI/사용자면 `None`.
    pub caller_plugin_id: Option<String>,
    /// 호스트 IPC 메서드를 동기로 호출할 수 있는 핸들. `clone()`해 자기 스레드로
    /// 옮길 수도 있다.
    pub host: HostHandle,
}

impl std::fmt::Debug for IpcMethodCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcMethodCtx")
            .field("method", &self.method)
            .field("params", &self.params)
            .field("caller_plugin_id", &self.caller_plugin_id)
            .field("host", &"HostHandle { .. }")
            .finish()
    }
}

/// `handle_ipc_method`에서 반환할 에러. JSON-RPC 에러 코드와 메시지를 담아
/// 호스트가 원 caller에게 그대로 전달한다.
#[derive(Debug, Clone)]
pub struct IpcMethodError {
    pub message: String,
    /// JSON-RPC 에러 코드. 기본 -32000 (server error).
    pub code: i32,
}

impl IpcMethodError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: -32000,
        }
    }

    pub fn with_code(message: impl Into<String>, code: i32) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }

    pub fn not_found(method: &str) -> Self {
        Self {
            message: format!("method '{method}' not found"),
            code: -32601,
        }
    }

    pub fn not_implemented() -> Self {
        Self {
            message: "method not implemented".into(),
            code: -32601,
        }
    }

    pub fn invalid_params(msg: &str) -> Self {
        Self {
            message: format!("invalid params: {msg}"),
            code: -32602,
        }
    }
}

/// Plugin 핸들러 안에서 SDK 호출이 실패하면 `?` 한 번으로 IPC 응답까지
/// 흘려보낼 수 있게 자동 변환을 제공한다. JSON-RPC 코드는 server error(-32000).
impl From<crate::error::PluginError> for IpcMethodError {
    fn from(err: crate::error::PluginError) -> Self {
        IpcMethodError::new(err.to_string())
    }
}

/// Plugin 생명주기 진입점. 모든 메서드는 동기적으로 호출되고, plugin 로직은
/// 내부에서 자유롭게 thread/async runtime을 사용할 수 있다.
pub trait Plugin: Send + 'static {
    /// 매니페스트 id와 일치해야 함. SDK가 hello 송신 시 사용.
    fn id(&self) -> &str;

    /// hello에 포함할 plugin 자체 버전.
    fn version(&self) -> &str {
        "0.0.0"
    }

    /// `surface.create`에 응답. 초기 tree와 display_name 반환.
    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult;

    /// `surface.event`에 응답. tree가 None이면 호스트는 이전 tree 유지.
    fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult;

    /// `surface.restore`에 응답. 영속화된 데이터로부터 surface 복원.
    fn restore_surface(&mut self, _ctx: SurfaceRestoreCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    /// `surface.snapshot` — 영속화할 데이터 반환. 기본 구현은 null.
    fn snapshot_surface(&mut self, _ctx: SurfaceSnapshotCtx) -> Value {
        Value::Null
    }

    /// `surface.destroy` — 호스트가 surface를 닫을 때 호출. 자원 해제용.
    fn destroy_surface(&mut self, _surface_id: u32) {}

    /// `command.invoke` — 매니페스트로 등록한 command가 사용자 단축키 매칭으로
    /// 호출됨. tree가 None이면 호스트는 이전 tree 유지. 기본 구현은 no-op.
    fn handle_command(&mut self, _ctx: CommandInvokeCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    /// `ipc.invoke` — 매니페스트 `[[contributes.ipc_namespace]]`로 점유한 prefix에
    /// 해당하는 IPC 메서드 호출이 호스트로부터 forward됨. plugin은 method 이름으로
    /// 자체 dispatch하고 JSON 결과 또는 [`IpcMethodError`]를 반환한다.
    /// 기본 구현은 `not_implemented` 에러.
    fn handle_ipc_method(&mut self, _ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        Err(IpcMethodError::not_implemented())
    }

    /// `event.dispatch` — [`BusHandle::subscribe`]로 등록한 패턴이 매칭되는
    /// 이벤트가 fan-out돼 도착했을 때 호출된다. fire-and-forget이라 반환값은 없다.
    /// 기본 구현은 no-op.
    fn on_event(&mut self, _ctx: EventDispatchCtx) {}

    /// `popup.open` — 매니페스트 `[[contributes.popup]]`로 contribute한 popup의
    /// 새 인스턴스가 열림. plugin은 초기 UI tree를 [`PopupOpenResult`]에 담아 반환.
    /// 기본 구현은 빈 트리.
    fn open_popup(&mut self, _ctx: PopupOpenCtx) -> PopupOpenResult {
        PopupOpenResult { tree: None }
    }

    /// `popup.event` — popup 인스턴스 위에서 사용자 이벤트 발생. plugin은
    /// 갱신된 트리(없으면 None)와 자체 닫기 신호(`close=true`)를 반환한다.
    /// 기본 구현은 변경 없음.
    fn handle_popup_event(&mut self, _ctx: PopupEventCtx) -> PopupEventResult {
        PopupEventResult {
            tree: None,
            close: false,
        }
    }

    /// `popup.closed` — popup 인스턴스가 닫혔음을 통보. fire-and-forget.
    /// plugin은 인스턴스별 자체 상태를 정리한다. 기본 구현은 no-op.
    fn on_popup_closed(&mut self, _ctx: PopupClosedCtx) {}

    /// `extension.invoke_hook` — 이 plugin이 다른 plugin(target)의 IPC 또는 이벤트
    /// 흐름을 가로채는 extension일 때, host가 매니페스트 `[[extends.*]]` 항목에 매칭되는
    /// 시점에 호출한다.
    ///
    /// 반환값은 [`ExtensionHookOutcome`]. mode에 맞는 헬퍼를 사용하면 안전:
    /// - `transform` → `ExtensionHookOutcome::transformed(new_payload)` 또는 `pass()`
    /// - `filter` → `block()` 또는 `pass()`
    /// - `observe` → `pass()` (반환값은 호스트가 무시)
    ///
    /// 기본 구현은 `pass()` — extension이 아니거나 hook을 처리하지 않는 plugin은
    /// 안전하게 통과시킨다.
    fn handle_extension_hook(&mut self, _ctx: ExtensionHookCtx) -> ExtensionHookOutcome {
        ExtensionHookOutcome::pass()
    }

    /// Plugin 부트스트랩이 끝나고 worker가 첫 dispatch에 들어가기 직전 1회 호출.
    /// plugin이 자체 background thread를 spawn해 polling 등 능동 작업을 시작할
    /// 때 사용한다. 전달된 [`HostHandle`]은 `Clone`이므로 spawn한 thread로 옮길
    /// 수 있다. 기본 구현은 no-op.
    ///
    /// `bus`는 plugin이 Event Bus에 publish/subscribe할 때 사용. 매니페스트의
    /// `event_subscribe`/`event_publish` 패턴이 비어 있으면 호스트가 등록을 거부하므로
    /// 핸들은 받아도 의미 없는 호출만 가능하다.
    fn on_start(&mut self, _host: HostHandle, _bus: BusHandle) {}
}

/// `event.dispatch` 콜백 컨텍스트.
#[derive(Debug, Clone)]
pub struct EventDispatchCtx {
    /// plugin이 [`BusHandle::subscribe`] 시 받은 sub_id. 한 plugin이 여러 구독을
    /// 유지할 때 어떤 구독이 fire 됐는지 식별.
    pub sub_id: u64,
    pub envelope: tasty_plugin_protocol::EventEnvelope,
}

use crate::bus::BusHandle;
