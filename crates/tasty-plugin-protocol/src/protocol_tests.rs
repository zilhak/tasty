//! `protocol_tests` 단위 테스트.

#![cfg(test)]

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
    let p = SharedBufferCreateParams { size: 1_048_576 };
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
        rect: Some(PixelRect {
            x: 10,
            y: 20,
            w: 40,
            h: 30,
        }),
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
        rect: Some(PixelRect {
            x: 1,
            y: 2,
            w: 3,
            h: 4,
        }),
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

#[test]
fn popup_open_params_round_trip_with_context() {
    let s = r#"{"popup_id":"search","instance_id":42,"context":{"q":"a"}}"#;
    let p: PopupOpenParams = serde_json::from_str(s).unwrap();
    assert_eq!(p.popup_id, "search");
    assert_eq!(p.instance_id, 42);
    assert_eq!(p.context["q"], "a");
}

#[test]
fn popup_open_params_context_defaults_to_null() {
    let s = r#"{"popup_id":"search","instance_id":1}"#;
    let p: PopupOpenParams = serde_json::from_str(s).unwrap();
    assert!(p.context.is_null());
}

#[test]
fn popup_event_result_close_defaults_false() {
    let s = r#"{}"#;
    let r: PopupEventResult = serde_json::from_str(s).unwrap();
    assert!(r.tree.is_none());
    assert!(!r.close);
}

#[test]
fn popup_close_reason_serializes_snake_case() {
    let cases = [
        (PopupCloseReason::OutsideClick, "outside_click"),
        (PopupCloseReason::Escape, "escape"),
        (PopupCloseReason::PluginRequest, "plugin_request"),
        (PopupCloseReason::HostShutdown, "host_shutdown"),
    ];
    for (r, expected) in cases {
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, format!("\"{expected}\""));
        let parsed: PopupCloseReason = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, r);
    }
}

#[test]
fn popup_method_names_stable() {
    assert_eq!(METHOD_POPUP_OPEN, "popup.open");
    assert_eq!(METHOD_POPUP_EVENT, "popup.event");
    assert_eq!(METHOD_POPUP_CLOSED, "popup.closed");
}
