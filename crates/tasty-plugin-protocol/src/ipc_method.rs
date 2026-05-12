//! Plugin이 contributes한 namespace IPC 메서드를 호출하기 위한 protocol.
//!
//! 호스트가 `<prefix>.<method>` 호출을 받으면 그 prefix를 점유한 plugin에
//! [`METHOD_IPC_INVOKE`] 메서드로 forward한다. plugin은 응답을 표준
//! `PluginResponse`로 돌려준다.

use serde::{Deserialize, Serialize};

/// 호스트 → plugin: namespace 메서드 forward용 메서드 이름.
pub const METHOD_IPC_INVOKE: &str = "ipc.invoke";

/// 호스트 → plugin: namespace 메서드 forward params.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IpcInvokeParams {
    /// 원본 메서드 이름 (예: "codex.spawn").
    pub method: String,
    /// 원본 params.
    pub params: serde_json::Value,
    /// caller가 plugin이면 그 plugin의 id, CLI/사용자면 None.
    #[serde(default)]
    pub caller_plugin_id: Option<String>,
}
