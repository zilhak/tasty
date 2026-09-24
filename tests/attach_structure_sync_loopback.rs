//! 셸 종료로 바뀐 구조가 attach holder에 전달되고, 이미 사라진 surface의 요청은 IPC와 같은 이유로 거절되는지 확인한다.
//! 각 실행은 빌드한 GUI 또는 헤드리스 서버 경로 하나를 검증한다(ADR-0023).

// 이유: 시험의 정리용 결과 무시는 제품 코드의 오류 처리 목록과 구분한다.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::net::TcpStream;
use std::time::{Duration, Instant};

use attach_common::{
    TAG_CONTROL, open_workspace_attach, read_frame, write_control_frame, write_workspace_input,
};
use serde_json::{Value, json};

fn split_and_read_new_surface(stream: &mut TcpStream, anchor: u64) -> u64 {
    write_control_frame(
        stream,
        &json!({ "event": "structural_op", "op_id": 1, "op": {
            "kind": "split_surface", "surface_id": anchor, "direction": "vertical",
        }}),
    );
    let delta = next_structural(stream, "structural_delta");
    surface_ids(&delta)
        .into_iter()
        .find(|&sid| sid != anchor)
        .unwrap_or_else(|| panic!("split delta has no new surface: {delta:?}"))
}

/// 다른 출력 프레임이 계속 오면 소켓 타임아웃이 발생하지 않을 수 있어 별도 경과 시간 제한을 둔다.
const STRUCTURAL_WAIT: Duration = Duration::from_secs(20);

/// 예상한 구조 프레임을 기다린다. 성공한 structural_result는 delta 앞에서 건너뛰고 다른 순서는 실패시킨다.
fn next_structural(stream: &mut TcpStream, event: &str) -> Value {
    let deadline = Instant::now() + STRUCTURAL_WAIT;
    loop {
        assert!(
            Instant::now() < deadline,
            "no {event} within {STRUCTURAL_WAIT:?}"
        );
        let (tag, payload) = read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        match v.get("event").and_then(|e| e.as_str()) {
            Some(e) if e == event => return v,
            Some("structural_result") if event != "structural_result" => {
                assert_eq!(v["ok"], true, "forward rejected: {v:?}");
            }
            Some(e @ ("structural_delta" | "structural_result")) => {
                panic!("expected {event}, got {e}: {v:?}")
            }
            _ => continue,
        }
    }
}

fn structural_delta_until(stream: &mut TcpStream, until: Instant) -> Option<Value> {
    while Instant::now() < until {
        let (tag, payload) = read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        match v.get("event").and_then(|e| e.as_str()) {
            Some("structural_delta") => return Some(v),
            Some("structural_result") => panic!("unexpected structural_result: {v:?}"),
            _ => continue,
        }
    }
    None
}

fn surface_ids(delta: &Value) -> Vec<u64> {
    delta["surfaces"]
        .as_array()
        .expect("surfaces array")
        .iter()
        .filter_map(|s| s["remote_id"].as_u64())
        .collect()
}

#[test]
fn a_shell_exit_on_the_server_reaches_the_holder_as_a_delta() {
    let server = common::shared();
    let ws = server.create_workspace("structure-sync-exit");
    let mut stream = open_workspace_attach(server.port(), ws.id);
    let b = split_and_read_new_surface(&mut stream, ws.surface_id);

    // 셸이 아직 입력을 못 받을 수 있어 delta가 올 때까지 exit를 반복한다. 종료된 surface의 입력은 서버가 버린다.
    let deadline = Instant::now() + STRUCTURAL_WAIT;
    let delta = loop {
        assert!(
            Instant::now() < deadline,
            "no structural_delta within {STRUCTURAL_WAIT:?}"
        );
        write_workspace_input(&mut stream, b as u32, b"exit\r");
        let resend_at = Instant::now() + Duration::from_secs(2);
        if let Some(d) = structural_delta_until(&mut stream, resend_at) {
            break d;
        }
    };
    assert_eq!(delta["workspace_id"], ws.id);
    assert_eq!(
        surface_ids(&delta),
        vec![ws.surface_id],
        "셸이 끝난 surface 가 빠진 트리여야 한다: {delta:?}"
    );
}

/// workspace는 남아 있고 surface만 사라진 경우의 거절을 IPC와 비교한다. 요청 이름은 structural op에 맞춰 다르다.
#[test]
fn a_forward_naming_a_gone_surface_is_answered_like_ipc() {
    let server = common::shared();
    let ws = server.create_workspace("structure-sync-gone");
    let mut stream = open_workspace_attach(server.port(), ws.id);
    let b = split_and_read_new_surface(&mut stream, ws.surface_id);

    let close = json!({ "event": "structural_op", "op_id": 2, "op": {
        "kind": "close_surface", "surface_id": b,
    }});
    write_control_frame(&mut stream, &close);
    let first = next_structural(&mut stream, "structural_result");
    assert_eq!(first["ok"], true, "{first:?}");
    next_structural(&mut stream, "structural_delta");

    write_control_frame(&mut stream, &close);
    let again = next_structural(&mut stream, "structural_result");
    assert_eq!(again["ok"], false, "{again:?}");
    let reason = again["reason"].as_str().expect("reason");
    assert!(
        reason.starts_with(&format!(
            "no live surface {b} (named by 'structural_op.close_surface'); "
        )),
        "IPC 와 같은 문구여야 한다: {reason}"
    );

    let resp = server.call_raw("surface.close", json!({ "surface_id": b }));
    let ipc = resp["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("IPC close of a gone surface must fail: {resp:?}"))
        .to_string();
    assert!(
        ipc.contains(&format!("no live surface {b} (named by 'surface.close'); ")),
        "대조군 IPC 문구: {ipc}"
    );
    assert_eq!(
        reason.split_once("); ").map(|(_, tail)| tail),
        ipc.split_once("); ").map(|(_, tail)| tail),
        "꼬리 문장까지 같아야 한다"
    );
}
