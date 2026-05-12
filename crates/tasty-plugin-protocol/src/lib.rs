//! Tasty 호스트와 외부 plugin이 공유하는 wire 타입 정의.
//!
//! Plugin이 surface를 그리기 위한 UI tree DSL ([`ui_tree`])과
//! 호스트 ↔ plugin 양방향 JSON 메시지 envelope ([`protocol`])을 제공한다.
//!
//! 이 크레이트는 `serde`/`serde_json` 외 의존성이 없도록 유지한다 — plugin은
//! 무거운 host 의존(예: egui/wgpu) 없이 컴파일 가능해야 한다.

pub mod ipc_method;
pub mod protocol;
pub mod ui_tree;

pub use ipc_method::{IpcInvokeParams, METHOD_IPC_INVOKE};

pub use protocol::{
    AuthAck, AuthAckEnvelope, AuthMessage, CommandInvokeParams, IpcCallResult, PluginEvent,
    PluginRequest, PluginResponse, SurfaceCloseReason, SurfaceEventParams, SurfaceLifecycleEvent,
    SurfaceLifecycleParams, SurfaceResult,
};
pub use protocol::{
    METHOD_COMMAND_INVOKE, METHOD_HOST_HELLO, METHOD_IPC_RESULT, METHOD_PING, METHOD_SHUTDOWN,
    METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT, METHOD_SURFACE_LIFECYCLE,
    METHOD_SURFACE_RESTORE, METHOD_SURFACE_SNAPSHOT,
};
pub use ui_tree::{
    ButtonStyle, LabelStyle, SelectionMode, SplitDir, TreeNode, UiEvent, UiNode,
};
