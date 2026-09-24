//! 실행한 서버에 loopback attach를 연결해 list_dir_request/result와 실제 디렉터리 내용을 비교한다.
//! 공유 서버에서 각 시험이 workspace를 만들며 클라이언트 GUI의 파일 선택기 렌더링은 검사하지 않는다.

// 이유: 시험의 정리용 결과 무시는 제품 코드의 오류 처리 목록과 구분한다.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use attach_common::{
    open_stream_without_attach, open_workspace_attach, wait_for_control_event, write_control_frame,
};
use serde_json::json;

#[test]
fn list_dir_request_round_trips_over_attach_channel() {
    let server = common::shared();
    let ws = server.create_workspace("list-dir-round-trip");

    let dir = std::env::temp_dir().join(format!(
        "tasty_list_dir_loopback_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub_folder")).unwrap();
    std::fs::write(dir.join("hello.txt"), b"hello world").unwrap();

    let mut stream = open_workspace_attach(server.port(), ws.id);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 1,
            "dir": dir.to_string_lossy(),
        }),
    );

    let result = wait_for_control_event(&mut stream, "list_dir_result");

    assert_eq!(result["request_id"], 1);
    assert_eq!(result["ok"], true, "expected ok reply: {result:?}");
    assert_eq!(result["dir"], dir.to_string_lossy().as_ref());
    let entries = result["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "expected 2 entries: {entries:?}");
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"sub_folder"));
    assert!(names.contains(&"hello.txt"));
    let sub = entries.iter().find(|e| e["name"] == "sub_folder").unwrap();
    assert_eq!(sub["is_dir"], true);
    let file = entries.iter().find(|e| e["name"] == "hello.txt").unwrap();
    assert_eq!(file["is_dir"], false);
    assert_eq!(file["size"], 11);
    assert!(
        file["modified_unix"].as_u64().unwrap() > 0,
        "modified_unix should be a positive epoch: {file:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_dir_request_reports_permission_error_for_missing_dir() {
    let server = common::shared();
    let ws = server.create_workspace("list-dir-missing");
    let mut stream = open_workspace_attach(server.port(), ws.id);

    let missing = std::env::temp_dir().join(format!(
        "tasty_list_dir_loopback_missing_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&missing);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 2,
            "dir": missing.to_string_lossy(),
        }),
    );

    let result = wait_for_control_event(&mut stream, "list_dir_result");

    assert_eq!(result["request_id"], 2);
    assert_eq!(result["ok"], false, "expected error reply: {result:?}");
    assert!(result["reason"].as_str().is_some_and(|r| !r.is_empty()));
}

#[test]
fn list_dir_request_rejected_without_workspace_occupancy() {
    // 점유 없는 연결의 거절을 검증하므로 workspace를 만들거나 attach하지 않는다(ADR-0022).
    let server = common::shared();
    let mut stream = open_stream_without_attach(server.port());

    write_control_frame(
        &mut stream,
        &json!({
            "event": "list_dir_request",
            "request_id": 3,
            "dir": "/",
        }),
    );

    let result = wait_for_control_event(&mut stream, "list_dir_result");

    assert_eq!(result["request_id"], 3);
    assert_eq!(
        result["ok"], false,
        "unattached client must be rejected: {result:?}"
    );
    assert!(result["entries"].is_null(), "no entries on rejection");
}
