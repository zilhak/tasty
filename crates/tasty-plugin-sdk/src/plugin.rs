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

use serde_json::Value;
use tasty_plugin_protocol::ui_tree::UiEvent;
pub use tasty_plugin_protocol::SurfaceResult;

use crate::host::HostHandle;

#[derive(Debug, Clone)]
pub struct SurfaceCreateCtx {
    pub surface_id: u32,
    pub kind: String,
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
    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        let _ = ctx;
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    /// `surface.snapshot` — 영속화할 데이터 반환. 기본 구현은 null.
    fn snapshot_surface(&mut self, ctx: SurfaceSnapshotCtx) -> Value {
        let _ = ctx;
        Value::Null
    }

    /// `surface.destroy` — 호스트가 surface를 닫을 때 호출. 자원 해제용.
    fn destroy_surface(&mut self, surface_id: u32) {
        let _ = surface_id;
    }

    /// `command.invoke` — 매니페스트로 등록한 command가 사용자 단축키 매칭으로
    /// 호출됨. tree가 None이면 호스트는 이전 tree 유지. 기본 구현은 no-op.
    fn handle_command(&mut self, ctx: CommandInvokeCtx) -> SurfaceResult {
        let _ = ctx;
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    /// `ipc.invoke` — 매니페스트 `[[contributes.ipc_namespace]]`로 점유한 prefix에
    /// 해당하는 IPC 메서드 호출이 호스트로부터 forward됨. plugin은 method 이름으로
    /// 자체 dispatch하고 JSON 결과 또는 [`IpcMethodError`]를 반환한다.
    /// 기본 구현은 `not_implemented` 에러.
    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        let _ = ctx;
        Err(IpcMethodError::not_implemented())
    }
}
