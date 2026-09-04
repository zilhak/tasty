//! (04) Native file picker — 원격 attach 채널의 `list_dir_request`/`list_dir_result`
//! 왕복을 loopback `TcpStream` 으로 실제 실행 중인 서버 인스턴스에 대해 검증한다.
//!
//! frame/handshake 헬퍼는 `tests/attach_common/mod.rs` 를 공유한다 — attach client 는
//! 실제 `tasty` GUI 앱이 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다. 서버
//! 인스턴스는 `common::shared()` 하나를 이 test binary 전체가 함께 쓰고, 점유가 필요한
//! 테스트는 `create_workspace()` 로 자기 workspace 를 만든다.
//!
//! **GUI 두 인스턴스를 실제로 attach 하는 e2e**(`tasty tool attach --ssh
//! 127.0.0.1:<port>` 로 mirror workspace 를 만들고 popup 을 열어 눈으로 확인하는 것)는
//! 이 headless 작업 환경(GPU 디스플레이 없음)에서 실행할 수 없다 — 이 test 는 그
//! 대체로, 서버가 실제로 띄운 워크스페이스에 대해 (1) attach 점유 획득 →
//! (2) `list_dir_request` 전송 → (3) 서버측 `handle_list_dir_request` 가 실제
//! 디스크의 임시 디렉토리를 읽어 → (4) `list_dir_result` 로 정확히 회신하는 전체
//! 왕복을 프로토콜 레벨에서 실행한다. `docs/features/native-file-picker/index.md` 의
//! "검증 한계" 절 참고.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
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

    // 실제 디스크에 검증용 디렉토리를 만든다 — read_dir_entries 가 실제 파일시스템을
    // 읽는지(mock 아님) 확인하기 위함.
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
    // 하이브리드 신뢰 모델(ADR-0053 결정 3): attach 점유가 유일한 인가 조건이다.
    // 이 client 는 stream 을 upgrade 했을 뿐 어떤 workspace 도 점유하지 않았으므로
    // `client_holds_workspace` 가 false 여야 하고, 서버는 실제 파일시스템을 읽지
    // 않은 채 즉시 거부해야 한다. 점유가 없다는 것 자체가 조건이므로 workspace 를
    // 만들지 않는다.
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
