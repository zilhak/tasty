//! workspace attach가 성공하면 서버가 처음 보내는 Control(descriptor)의 형식.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::PTY_INPUT_VT;

/// workspace attach descriptor의 `event` 값.
const EVENT: &str = "attached_workspace";

/// workspace attach descriptor. `event`는 `attached_workspace`이고, 읽을 때 다른 값이나 빠진 `event`는 거절한다.
/// `tree`·`surfaces`는 [`crate::StreamControl::StructuralDelta`]와 같은 형식이라 JSON 값으로 둔다.
/// 키 이름 `stream_capabilities`·`pty_input`은 [`crate::DESCRIPTOR_CAPABILITIES`]·[`crate::DESCRIPTOR_PTY_INPUT`]이다.
///
/// 배포된 서버가 만든 descriptor가 계약이다. 서버는 늘 `event`·`workspace_id`·`name`·`tree`·`surfaces`를
/// 보내므로 이 중 하나라도 빠진 입력은 거부한다. 기본값은 구 서버가 보내지 않던 `stream_capabilities`·`pty_input`에만 있다.
///
/// 공개 역직렬화 경로는 [`Deserialize`] impl 하나이고, 그 impl이 `event`를 본다.
/// 태그를 건너뛰는 고유 `deserialize`는 없다.
///
/// ```compile_fail
/// let value = serde_json::json!({"workspace_id": 1, "name": "w", "tree": null, "surfaces": []});
/// let _ = tasty_attach_protocol::AttachedWorkspaceDescriptor::deserialize(value);
/// ```
///
/// `event`를 보지 않는 읽기 본문(`Wire`)도 크레이트 밖에서 이름으로 부를 수 없다.
///
/// ```compile_fail
/// let value = serde_json::json!({"workspace_id": 1, "name": "w", "tree": null, "surfaces": []});
/// let _ = serde_json::from_value::<tasty_attach_protocol::Wire>(value);
/// ```
#[derive(Debug, Clone, PartialEq)]
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
    pub stream_capabilities: Vec<String>,
    /// 서버 PTY가 읽는 키 입력 형식. 구 서버는 보내지 않으므로 없으면 [`PTY_INPUT_VT`]다.
    pub pty_input: String,
}

impl Serialize for AttachedWorkspaceDescriptor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("AttachedWorkspaceDescriptor", 7)?;
        out.serialize_field("event", EVENT)?;
        out.serialize_field("workspace_id", &self.workspace_id)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("tree", &self.tree)?;
        out.serialize_field("surfaces", &self.surfaces)?;
        out.serialize_field("stream_capabilities", &self.stream_capabilities)?;
        out.serialize_field("pty_input", &self.pty_input)?;
        out.end()
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
        let wire = Wire::deserialize(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            workspace_id: wire.workspace_id,
            name: wire.name,
            tree: wire.tree,
            surfaces: wire.surfaces,
            stream_capabilities: wire.stream_capabilities,
            pty_input: wire.pty_input,
        })
    }
}

/// 읽기 본문. 파생 Deserialize는 `event`를 보지 않으므로 공개 impl이 먼저 본 뒤에만 쓴다.
#[derive(Deserialize)]
struct Wire {
    workspace_id: u32,
    name: String,
    tree: Value,
    surfaces: Vec<Value>,
    #[serde(default)]
    stream_capabilities: Vec<String>,
    #[serde(default = "vt")]
    pty_input: String,
}

fn vt() -> String {
    PTY_INPUT_VT.to_string()
}

#[cfg(test)]
#[path = "descriptor_tests.rs"]
mod tests;
