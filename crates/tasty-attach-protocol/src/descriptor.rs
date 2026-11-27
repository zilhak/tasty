//! workspace attach가 성공하면 서버가 처음 보내는 Control(descriptor)의 형식.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::PTY_INPUT_VT;

/// workspace attach descriptor의 `event` 값.
const EVENT: &str = "attached_workspace";

/// workspace attach descriptor. `event`는 `attached_workspace`이고, 읽을 때 다른 값이나 빠진 `event`는 거절한다.
/// `tree`·`surfaces`는 [`crate::StreamControl::StructuralDelta`]와 같은 형식이라 JSON 값으로 둔다.
/// 키 이름 `stream_capabilities`·`pty_input`은 [`crate::DESCRIPTOR_CAPABILITIES`]·[`crate::DESCRIPTOR_PTY_INPUT`]이다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", tag = "event", rename = "attached_workspace")]
pub struct AttachedWorkspaceDescriptor {
    /// 서버의 workspace ID.
    pub workspace_id: u32,
    /// 서버 workspace의 이름.
    pub name: String,
    /// 서버 workspace의 전체 트리.
    pub tree: Value,
    /// surface마다 하나씩, role별 모양의 항목.
    pub surfaces: Vec<Value>,
    /// 이 연결의 서버 기능 이름 목록. 구 서버는 보내지 않으므로 없으면 빈 목록이다.
    #[serde(default)]
    pub stream_capabilities: Vec<String>,
    /// 서버 PTY가 읽는 키 입력 형식. 구 서버는 보내지 않으므로 없으면 [`PTY_INPUT_VT`]다.
    #[serde(default = "vt")]
    pub pty_input: String,
}

// 파생 Deserialize는 struct의 tag를 확인하지 않으므로 `event`를 먼저 본 뒤 파생 본문으로 읽는다.
impl Serialize for AttachedWorkspaceDescriptor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AttachedWorkspaceDescriptor::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for AttachedWorkspaceDescriptor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let event = value.get("event").and_then(Value::as_str);
        if event != Some(EVENT) {
            return Err(serde::de::Error::custom(format!(
                "not a workspace attach descriptor: event {event:?}"
            )));
        }
        AttachedWorkspaceDescriptor::deserialize(value).map_err(serde::de::Error::custom)
    }
}

fn vt() -> String {
    PTY_INPUT_VT.to_string()
}

#[cfg(test)]
#[path = "descriptor_tests.rs"]
mod tests;
