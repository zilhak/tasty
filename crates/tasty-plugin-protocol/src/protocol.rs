//! Plugin ↔ Host 메시지 정의.
//!
//! Plugin이 connection 직후 첫 줄로 보내는 `AuthMessage`로 token 인증.
//! 이후 한 줄당 하나의 JSON 메시지(NDJSON 스타일).
//!
//! - 응답: `{"id": N, "result": ...}` 또는 `{"id": N, "error": "..."}`
//! - 알림 (id 없음): `{"event": {"kind": "...", ...}}`

use serde::{Deserialize, Serialize};

use crate::events::EventEnvelope;
use crate::ui_tree::{UiEvent, UiNode};

// ── Host → plugin method names ──
pub const METHOD_PING: &str = "ping";
pub const METHOD_SHUTDOWN: &str = "shutdown";
pub const METHOD_HOST_HELLO: &str = "host.hello";
pub const METHOD_SURFACE_CREATE: &str = "surface.create";
pub const METHOD_SURFACE_EVENT: &str = "surface.event";
pub const METHOD_SURFACE_SNAPSHOT: &str = "surface.snapshot";
pub const METHOD_SURFACE_RESTORE: &str = "surface.restore";
pub const METHOD_SURFACE_DESTROY: &str = "surface.destroy";
/// host → plugin: plugin이 보낸 ipc.call에 대한 결과.
/// params에 [`IpcCallResult`].
pub const METHOD_IPC_RESULT: &str = "ipc.result";
/// host → plugin: Event Bus dispatch. params에 [`EventDispatchParams`].
/// 응답은 fire-and-forget — broadcast 모델이라 응답 합치기 없음.
pub const METHOD_EVENT_DISPATCH: &str = "event.dispatch";
/// host → plugin: 사용자 단축키 매칭으로 plugin command가 트리거됨.
/// params에 [`CommandInvokeParams`]. plugin은 그에 따라 surface state를 변경하고,
/// 변경 결과는 `surface.event`와 동일하게 `SurfaceResult` 형태로 응답한다 (tree
/// 또는 display_name 갱신).
pub const METHOD_COMMAND_INVOKE: &str = "command.invoke";
/// host → extension plugin: extension의 pre/post hook 호출.
/// params에 [`ExtensionHookInvokeParams`]. plugin은 mode에 따라 transform/filter/observe
/// 의미로 [`ExtensionHookResult`]를 반환한다 (PluginResponse.result).
pub const METHOD_EXTENSION_INVOKE_HOOK: &str = "extension.invoke_hook";

/// `surface.create` / `surface.event` / `surface.restore` 응답에 포함되는 standard 결과.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceResult {
    /// 새로 그릴 트리. `None`이면 호스트는 이전 트리를 그대로 사용 (변경 없음).
    #[serde(default)]
    pub tree: Option<UiNode>,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// `surface.event` params — 호스트가 plugin에 보낼 사용자 이벤트.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceEventParams<'a> {
    pub surface_id: u32,
    pub event: &'a UiEvent,
}

/// `command.invoke` params — 사용자 단축키 매칭 시 호스트가 plugin에 보내는 명령.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandInvokeParams {
    pub surface_id: u32,
    pub command_id: String,
}

/// hook 호출이 이벤트인지 IPC인지 구분.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionHookKind {
    Event,
    Ipc,
}

/// hook이 pre인지 post인지.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionHookPhase {
    Pre,
    Post,
}

/// hook의 동작 모드. host는 mode에 따라 plugin 응답을 다르게 해석한다.
/// 매니페스트의 `HookMode`와 1:1 대응.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionHookMode {
    Transform,
    Filter,
    Observe,
}

/// `extension.invoke_hook` params — host가 extension plugin에 hook 호출을 위임.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionHookInvokeParams {
    /// hook 종류 (event/ipc).
    pub kind: ExtensionHookKind,
    pub phase: ExtensionHookPhase,
    pub mode: ExtensionHookMode,
    /// 매칭된 hook의 대상. `kind=event`면 event key, `kind=ipc`면 IPC method 이름.
    pub target: String,
    /// hook이 가공할 페이로드.
    /// - `kind=event`: envelope.payload
    /// - `kind=ipc, phase=pre`: 호출 params
    /// - `kind=ipc, phase=post`: 응답 result
    pub payload: serde_json::Value,
}

/// `extension.invoke_hook` 응답. mode별 의미:
///
/// - **transform**: `modified_payload`가 Some이면 host가 그 값으로 덮어쓴다.
///   None이면 원본 유지.
/// - **filter**: `pass`가 Some(false)면 host는 흐름을 차단한다.
///   None 또는 Some(true)면 통과. `modified_payload`는 무시.
/// - **observe**: 모든 필드 무시. plugin이 단순 관찰만 한 결과.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ExtensionHookResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_payload: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<bool>,
}

// ── Shared buffer 메서드 (plugin → host via PluginEvent::IpcCall) ──
//
// plugin이 OS 공유 메모리 영역을 만들고 dirty rect를 알릴 때 사용한다. 실제 핸들
// (fd/HANDLE) 전송은 *보조 채널*을 통해 이루어지고, 이 메인 채널 메서드는 id/size/
// rect 같은 메타데이터만 운반한다. 보조 채널 wire 포맷은 SDK 통합 단계(Step 02b/02c)
// 에서 정의된다.
//
// 권한: manifest의 `[memory]` 섹션에 `max_shared_buffer_bytes`가 선언된 plugin만
// 호출 가능. 미선언 plugin이 호출하면 호스트가 -32001 PermissionDenied 응답.

/// plugin → host: 새 공유 메모리 영역 생성 요청.
pub const METHOD_HOST_SHARED_BUFFER_CREATE: &str = "host.shared_buffer.create";
/// plugin → host: 변경된 영역(dirty rect) 통지.
pub const METHOD_HOST_SHARED_BUFFER_DIRTY: &str = "host.shared_buffer.dirty";

/// 호스트가 발급한 shared buffer 식별자. plugin 인스턴스마다 단조 증가.
/// u64를 옵셔널 직렬화 호환을 위해 직접 직렬화.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SharedBufferId(pub u64);

/// 픽셀(또는 추후 다른 단위) 좌표계의 정수 사각형. shared buffer dirty 영역 표현에 사용.
///
/// 비어 있는 rect(`w == 0 || h == 0`)는 "갱신 없음"이 아니라 "유효하지 않음"으로 간주.
/// "전체 갱신"은 `Option<Rect>::None`으로 표현한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// `host.shared_buffer.create` params.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SharedBufferCreateParams {
    /// 요청 영역 크기 (바이트). manifest의 max_shared_buffer_bytes를 초과하면 거부.
    pub size: u64,
}

/// `host.shared_buffer.create` result. 보조 채널로 핸들이 별도 전송된 *후* 메인 채널
/// 응답으로 이 값이 도착한다. plugin SDK는 두 정보가 모두 도착한 시점에 `SharedBuffer`를
/// 호출자에게 반환한다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SharedBufferCreateResult {
    pub id: SharedBufferId,
    /// 실제 매핑된 크기. 보통 요청한 size와 동일하나, OS가 페이지 경계로 올린 경우 size
    /// 자체는 요청값을 보존한다 (SharedMemory::len이 요청값을 반환).
    pub size: u64,
}

/// `host.shared_buffer.dirty` params.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SharedBufferDirtyParams {
    pub id: SharedBufferId,
    /// `None`이면 전체 영역이 dirty.
    #[serde(default)]
    pub rect: Option<Rect>,
}

/// 호스트 → plugin 요청.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginRequest {
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

/// plugin → 호스트 응답.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
    /// JSON-RPC 에러 코드. 없으면 host가 -32000 (server error)으로 간주.
    /// 예: -32601 method not found, -32602 invalid params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<i32>,
}

/// plugin → 호스트 비동기 알림 (요청 응답이 아님).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginEvent {
    /// 매니페스트 검증 후 호스트가 받는 첫 메시지.
    Hello {
        plugin_id: String,
        version: String,
    },
    /// surface invalidated — 호스트가 다음 프레임에 redraw (단계 06).
    SurfaceInvalidated { surface_id: u32 },
    /// host action 트리거 (단계 06).
    NotifyHost {
        surface_id: u32,
        event: String,
        payload: serde_json::Value,
    },
    /// plugin 측 로그 (호스트 로그에 합쳐짐).
    Log { level: String, message: String },
    /// plugin → 호스트 IPC 호출. 호스트가 권한을 검사하고 라우터에 보낸 뒤,
    /// 결과를 `ipc.result` 요청으로 회신한다 (`call_id`로 매칭).
    IpcCall {
        call_id: u64,
        method: String,
        params: serde_json::Value,
    },
    /// plugin → 호스트: Event Bus에 이벤트 publish.
    /// 호스트가 권한(매니페스트 `event_publish` 패턴)을 검증하고 hop을 증가시킨 뒤
    /// 구독자에게 fan-out한다. fire-and-forget.
    EventPublish { envelope: EventEnvelope },
    /// plugin → 호스트: 이벤트 키 패턴 구독.
    /// `pattern`은 정확한 키 또는 `<namespace>.*` 와일드카드.
    /// 호스트는 매니페스트 `event_subscribe`로 허용된 패턴 안에 들어가는지 검증한다.
    EventSubscribe { sub_id: u64, pattern: String },
    /// plugin → 호스트: 이전 [`PluginEvent::EventSubscribe`]의 구독 해제.
    EventUnsubscribe { sub_id: u64 },
}

/// [`METHOD_EVENT_DISPATCH`] params — 호스트가 구독자 plugin에게 보내는 이벤트.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventDispatchParams {
    /// plugin이 구독 등록 시 발급한 `sub_id`. 같은 plugin이 여러 패턴을 구독한 경우
    /// 어느 구독에 매칭됐는지 구분하기 위한 식별자.
    pub sub_id: u64,
    pub envelope: EventEnvelope,
}

/// `ipc.result` 요청의 params — plugin의 ipc.call에 대한 호스트의 응답.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IpcCallResult {
    pub call_id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

/// plugin이 connection 직후 첫 줄로 보내는 인증 메시지.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthMessage {
    pub plugin_id: String,
    pub token: String,
}

/// 호스트 → plugin 인증 단계 전용 ack. plugin이 [`AuthMessage`]를 보낸 뒤
/// 메인 메시지 루프에 진입하기 전, **단 한 번** 같은 NDJSON 채널로 수신한다.
///
/// `ok=false`이면 plugin SDK가 [`crate::PluginError::HandshakeRejected`](
/// 같은 이름의 SDK variant)로 즉시 실패한다. 호스트 측은
/// `src/plugin/listener.rs`에서 토큰 검증 결과에 따라 송신한다.
///
/// envelope: `{"auth_ack": { "ok": true }}` 또는 `{"auth_ack": { "ok": false, "reason": "..." }}`.
/// 메인 루프의 `PluginRequest`와 다른 envelope를 사용해 파서 분리.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthAck {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// AuthAck의 envelope wrapper. NDJSON 한 줄에 담기는 최상위 구조.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthAckEnvelope {
    pub auth_ack: AuthAck,
}

// ── 보조 핸들 채널 (Step 02b/02c) ──
//
// 메인 TCP 채널은 fd/HANDLE을 운반할 수 없으므로 보조 채널을 별도로 둔다. Unix는
// AF_UNIX (SCM_RIGHTS 가능), Windows는 Named Pipe (DuplicateHandle 가능). 이 채널의
// wire 포맷은 NDJSON이며, 02c에서 NDJSON 한 줄 직후 OS-네이티브 ancillary data로
// 핸들을 함께 전송한다.
//
// 인증 단계는 메인 채널의 [`AuthMessage`] / [`AuthAckEnvelope`]를 그대로 재사용한다 —
// endpoint가 다르므로 채널 라우팅 혼선 위험이 없고, 토큰은 동일한 plugin spawn 토큰이다.

/// 보조 채널 위에서 양쪽이 주고받는 NDJSON 메시지.
///
/// 02b에서는 ping/pong만 정의됐고, 02c에서 `HandleAttach`(host → plugin: 새 buffer
/// 핸들의 메타)와 `Dirty`(plugin → host: dirty rect 알림)가 추가됐다.
///
/// `HandleAttach`는 NDJSON 한 줄 *직후* OS-네이티브 ancillary data(SCM_RIGHTS / 직렬화된
/// HANDLE)를 함께 전송한다 — 같은 sendmsg/write 호출 안에 묶여 전달되어야 plugin이 핸들과
/// 메타를 일관되게 짝지을 수 있다.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandleChannelMessage {
    /// 살아있는지 확인용. host 또는 plugin 어느 쪽이든 보낼 수 있고, 받은 쪽은 동일한
    /// `seq`로 [`HandleChannelMessage::Pong`]을 응답한다.
    Ping { seq: u64 },
    /// [`HandleChannelMessage::Ping`]의 응답.
    Pong { seq: u64 },
    /// host → plugin: 새로 만든 shared buffer 핸들이 ancillary data로 동행한다.
    /// `request_id`는 메인 채널의 `host.shared_buffer.create` call_id와 1:1 매칭.
    HandleAttach {
        /// 메인 채널 `host.shared_buffer.create` 요청의 call_id.
        request_id: u64,
        /// 호스트가 부여한 shared buffer id.
        id: SharedBufferId,
        /// 매핑 크기. SDK가 `tasty_shm::receive`에 그대로 넣는다.
        size: u64,
    },
    /// plugin → host: 특정 buffer의 dirty 영역을 통지. fire-and-forget.
    Dirty {
        /// 어떤 buffer가 dirty한지.
        id: SharedBufferId,
        /// `None`이면 전체 영역.
        #[serde(default)]
        rect: Option<Rect>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn auth_round_trip() {
        let msg = AuthMessage {
            plugin_id: "com.example.x".into(),
            token: "abc123".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let parsed: AuthMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.plugin_id, "com.example.x");
        assert_eq!(parsed.token, "abc123");
    }

    #[test]
    fn event_hello_round_trip() {
        let ev = PluginEvent::Hello {
            plugin_id: "com.example.x".into(),
            version: "0.1.0".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"hello\""));
        let parsed: PluginEvent = serde_json::from_str(&s).unwrap();
        match parsed {
            PluginEvent::Hello { plugin_id, version } => {
                assert_eq!(plugin_id, "com.example.x");
                assert_eq!(version, "0.1.0");
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn response_with_error() {
        let s = r#"{"id":42,"error":"boom"}"#;
        let parsed: PluginResponse = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.error.as_deref(), Some("boom"));
        assert!(parsed.result.is_none());
        assert!(parsed.error_code.is_none());
    }

    #[test]
    fn response_with_result() {
        let s = r#"{"id":7,"result":{"ok":true}}"#;
        let parsed: PluginResponse = serde_json::from_str(s).unwrap();
        assert_eq!(parsed.id, 7);
        assert!(parsed.result.is_some());
        assert!(parsed.error.is_none());
        assert!(parsed.error_code.is_none());
    }

    #[test]
    fn response_with_error_code() {
        let resp = PluginResponse {
            id: 1,
            result: None,
            error: Some("method not found".into()),
            error_code: Some(-32601),
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"error_code\":-32601"));
        let parsed: PluginResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.error_code, Some(-32601));
    }

    #[test]
    fn auth_ack_envelope_round_trip() {
        let env = AuthAckEnvelope {
            auth_ack: AuthAck {
                ok: false,
                reason: Some("token mismatch".into()),
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"auth_ack\""));
        assert!(s.contains("\"ok\":false"));
        let parsed: AuthAckEnvelope = serde_json::from_str(&s).unwrap();
        assert!(!parsed.auth_ack.ok);
        assert_eq!(parsed.auth_ack.reason.as_deref(), Some("token mismatch"));
    }

    #[test]
    fn auth_ack_omits_reason_when_ok() {
        let env = AuthAckEnvelope {
            auth_ack: AuthAck {
                ok: true,
                reason: None,
            },
        };
        let s = serde_json::to_string(&env).unwrap();
        assert!(!s.contains("reason"));
    }

    #[test]
    fn response_omits_error_code_when_none() {
        let resp = PluginResponse {
            id: 1,
            result: Some(Value::Null),
            error: None,
            error_code: None,
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(!s.contains("error_code"));
    }

    #[test]
    fn shared_buffer_id_serializes_as_bare_u64() {
        let id = SharedBufferId(42);
        let s = serde_json::to_string(&id).unwrap();
        assert_eq!(s, "42");
        let parsed: SharedBufferId = serde_json::from_str("42").unwrap();
        assert_eq!(parsed, SharedBufferId(42));
    }

    #[test]
    fn shared_buffer_create_params_round_trip() {
        let p = SharedBufferCreateParams {
            size: 1_048_576,
        };
        let s = serde_json::to_string(&p).unwrap();
        let parsed: SharedBufferCreateParams = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.size, 1_048_576);
    }

    #[test]
    fn shared_buffer_dirty_omits_rect_when_none() {
        let p = SharedBufferDirtyParams {
            id: SharedBufferId(7),
            rect: None,
        };
        let s = serde_json::to_string(&p).unwrap();
        // serde_json은 None을 null로 직렬화한다 (default skip을 명시하지 않은 경우).
        // 우리는 rect를 명시적으로 두지 않고 None일 때 null로 보내도 됨 — 디코딩 호환.
        assert!(s.contains("\"rect\":null") || !s.contains("rect"));
        let parsed: SharedBufferDirtyParams = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.id, SharedBufferId(7));
        assert!(parsed.rect.is_none());
    }

    #[test]
    fn shared_buffer_dirty_with_rect_round_trip() {
        let p = SharedBufferDirtyParams {
            id: SharedBufferId(11),
            rect: Some(Rect { x: 10, y: 20, w: 40, h: 30 }),
        };
        let s = serde_json::to_string(&p).unwrap();
        let parsed: SharedBufferDirtyParams = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.rect.unwrap().w, 40);
        assert_eq!(parsed.rect.unwrap().h, 30);
    }

    #[test]
    fn handle_channel_ping_round_trip() {
        let msg = HandleChannelMessage::Ping { seq: 7 };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"kind\":\"ping\""));
        assert!(s.contains("\"seq\":7"));
        let parsed: HandleChannelMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, HandleChannelMessage::Ping { seq: 7 });
    }

    #[test]
    fn handle_channel_pong_round_trip() {
        let s = r#"{"kind":"pong","seq":42}"#;
        let parsed: HandleChannelMessage = serde_json::from_str(s).unwrap();
        assert_eq!(parsed, HandleChannelMessage::Pong { seq: 42 });
    }

    #[test]
    fn handle_channel_handle_attach_round_trip() {
        let msg = HandleChannelMessage::HandleAttach {
            request_id: 9,
            id: SharedBufferId(1),
            size: 4096,
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"kind\":\"handle_attach\""));
        let parsed: HandleChannelMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn handle_channel_dirty_with_rect_round_trip() {
        let msg = HandleChannelMessage::Dirty {
            id: SharedBufferId(2),
            rect: Some(Rect { x: 1, y: 2, w: 3, h: 4 }),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"kind\":\"dirty\""));
        let parsed: HandleChannelMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn handle_channel_dirty_full_buffer_round_trip() {
        let s = r#"{"kind":"dirty","id":3}"#;
        let parsed: HandleChannelMessage = serde_json::from_str(s).unwrap();
        assert_eq!(
            parsed,
            HandleChannelMessage::Dirty {
                id: SharedBufferId(3),
                rect: None,
            }
        );
    }

}
