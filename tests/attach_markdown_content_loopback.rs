//! markdown mirror(ADR-0254) — 원격 attach 채널의 `markdown` role 직렬화와
//! `markdown_content_request`/`markdown_content_result` 왕복을 loopback `TcpStream` 으로
//! 실제 실행 중인 서버 인스턴스에 대해 검증한다.
//!
//! frame/handshake 헬퍼는 `tests/attach_common/mod.rs` 를 공유한다 — attach client 는
//! 실제 `tasty` GUI 앱이 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다.
//!
//! **GUI 두 인스턴스를 실제로 attach 해 mirror 문서를 눈으로 확인하는 e2e** 는 이
//! 헤드리스 환경(GPU 디스플레이 없음)에서 실행할 수 없다 — 이 test 는 그 대체로 서버가
//! 실제로 띄운 markdown surface 에 대해 (1) attach 점유 획득 → (2) 핸드셰이크
//! 디스크립터에 `role:"markdown"` 이 실리는지 → (3) `markdown_content_request` 전송 →
//! (4) 서버가 실제 디스크의 파일을 읽어 `markdown_content_result` 로 회신하는지를
//! 프로토콜 레벨에서 전부 실행한다. 과거 `attach_markdown_mesh_mirror_loopback.rs` 가
//! 같은 자리를 mesh 채널로 검증했고, markdown 이 그 채널을 벗어나며(ADR-0065) 삭제됐다.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`crates/tasty-doc-guards/tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use attach_common::{
    AttachStream, open_stream_without_attach, try_open_workspace_attach_stream,
    wait_for_control_event, write_control_frame,
};
use common::TastyInstance;
use serde_json::{Value, json};

const DOC_BODY: &str = "# remote doc\n\n원격에서만 존재하는 문서다.\n";

/// 테스트용 markdown 파일을 만들고 그 경로를 돌려준다. 호출자가 지운다.
///
/// 키에 **단조 카운터**를 넣는다 — `std::process::id()` 는 프로세스 *간*만 가르므로,
/// 같은 자리를 두 번 부르면 뒤 호출이 앞 호출의 트리를 조용히 지운다.
fn write_doc(tag: &str) -> std::path::PathBuf {
    write_doc_with_body(tag, DOC_BODY)
}

/// [`write_doc`] 과 같되 본문을 호출자가 정한다(예산 초과 문서용).
fn write_doc_with_body(tag: &str, body: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tasty_md_content_loopback_{tag}_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // 이유: 이전 회차의 잔재를 지우는 best-effort — 없으면 실패하는데 그게 원하던 상태다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("README.md");
    std::fs::write(&file, body).unwrap();
    file
}

/// workspace 에 markdown surface 를 하나 만들고 그 surface_id 를 돌려준다.
fn open_markdown_surface(server: &TastyInstance, workspace_id: u64, file: &std::path::Path) -> u64 {
    let pane_id = server.first_pane_id_in_workspace(workspace_id);
    let r = server.call(
        "tab.create",
        json!({ "pane_id": pane_id, "type": "markdown", "file": file.to_string_lossy() }),
    );
    r["surface_id"]
        .as_u64()
        .expect("tab.create returned surface_id")
}

/// 핸드셰이크 디스크립터에서 그 surface 의 항목을 꺼낸다.
fn surface_descriptor(descriptor: &Value, surface_id: u64) -> Option<Value> {
    descriptor["surfaces"]
        .as_array()?
        .iter()
        .find(|s| s["remote_id"].as_u64() == Some(surface_id))
        .cloned()
}

/// `role:"markdown"` 디스크립터에 **경로까지** 실릴 때까지 붙었다 떼며 기다린다.
///
/// plugin 이 `surface.create` 응답으로 올리는 snapshot(`{"file": ...}`)은 `tab.create`
/// IPC 가 돌아온 뒤 비동기로 도착한다 — 그 전에 붙으면 role 은 맞지만 `file` 이 비어
/// 있다. 디스크립터는 핸드셰이크에 **한 번만** 실리므로, 기다리는 방법은 실제 client 가
/// 재접속할 때와 똑같이 붙었다 떼는 것뿐이다(끊으면 서버가 EOF 로 점유를 회수한다).
fn attach_when_descriptor_has_file(
    server: &TastyInstance,
    workspace_id: u64,
    surface_id: u64,
) -> (AttachStream, Value) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = None;
    while Instant::now() < deadline {
        if let Some((stream, descriptor)) =
            try_open_workspace_attach_stream(server.port(), workspace_id)
        {
            let entry = surface_descriptor(&descriptor, surface_id);
            if entry
                .as_ref()
                .and_then(|e| e["file"].as_str())
                .is_some_and(|f| !f.is_empty())
            {
                return (stream, entry.expect("entry checked above"));
            }
            last = entry;
            drop(stream); // 점유를 놓고 다시 붙는다 — snapshot 이 아직 안 왔다.
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("markdown 디스크립터에 file 이 끝내 실리지 않았다: {last:?}");
}

#[test]
fn markdown_surface_is_sent_as_its_own_role_with_the_remote_path() {
    let server = common::shared();
    let ws = server.create_workspace("md-content-role");
    let file = write_doc("role");
    let surface_id = open_markdown_surface(server, ws.id, &file);

    let (_stream, entry) = attach_when_descriptor_has_file(server, ws.id, surface_id);

    assert_eq!(
        entry["role"], "markdown",
        "markdown 은 placeholder 가 아니라 전용 role 이어야 한다: {entry:?}"
    );
    assert_eq!(entry["file"], file.to_string_lossy().as_ref());
    assert!(
        entry["display_name"]
            .as_str()
            .is_some_and(|n| !n.is_empty()),
        "탭 제목용 display_name 이 함께 실려야 한다: {entry:?}"
    );

    // 이유: 뒷정리 best-effort — 실패해도 temp 디렉토리가 남을 뿐 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn markdown_content_request_round_trips_over_attach_channel() {
    let server = common::shared();
    let ws = server.create_workspace("md-content-round-trip");
    let file = write_doc("roundtrip");
    let surface_id = open_markdown_surface(server, ws.id, &file);

    let (mut stream, _entry) = attach_when_descriptor_has_file(server, ws.id, surface_id);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 1,
            "surface_id": surface_id,
        }),
    );
    let result = wait_for_control_event(&mut stream, "markdown_content_result");

    assert_eq!(result["request_id"], 1);
    assert_eq!(result["surface_id"], surface_id);
    assert_eq!(result["ok"], true, "expected ok reply: {result:?}");
    assert_eq!(
        result["source"], DOC_BODY,
        "서버가 실제 디스크의 원문을 그대로 실어야 한다: {result:?}"
    );
    assert_eq!(result["file"], file.to_string_lossy().as_ref());
    assert_eq!(result["truncated"], false);

    // 이유: 뒷정리 best-effort — 실패해도 temp 디렉토리가 남을 뿐 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

/// 파일이 사라진 뒤의 요청은 **연결이 끊기거나 무응답이 아니라** `ok:false` + 사유다.
#[test]
fn markdown_content_request_reports_a_reason_for_a_missing_file() {
    let server = common::shared();
    let ws = server.create_workspace("md-content-missing");
    let file = write_doc("missing");
    let surface_id = open_markdown_surface(server, ws.id, &file);

    let (mut stream, _entry) = attach_when_descriptor_has_file(server, ws.id, surface_id);
    // 디스크립터가 경로를 실은 **뒤에** 지운다 — 그래야 "경로는 아는데 읽을 수 없다" 를 잰다.
    std::fs::remove_file(&file).unwrap();

    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 2,
            "surface_id": surface_id,
        }),
    );
    let result = wait_for_control_event(&mut stream, "markdown_content_result");

    assert_eq!(result["request_id"], 2);
    assert_eq!(result["ok"], false, "expected error reply: {result:?}");
    assert!(result["reason"].as_str().is_some_and(|r| !r.is_empty()));
    assert!(
        result["source"].is_null(),
        "실패 회신에 원문이 실리면 안 된다"
    );

    // 이유: 뒷정리 best-effort — 실패해도 temp 디렉토리가 남을 뿐 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

/// 하이브리드 신뢰 모델(ADR-0053 결정 3 · ADR-0254 항목 2): attach 점유가 유일한 인가
/// 조건이다. 점유 없는 client 는 파일을 한 바이트도 읽히지 못한 채 거절돼야 한다.
#[test]
fn markdown_content_request_rejected_without_workspace_occupancy() {
    let server = common::shared();
    let mut stream = open_stream_without_attach(server.port());

    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 3,
            "surface_id": 1,
        }),
    );
    let result = wait_for_control_event(&mut stream, "markdown_content_result");

    assert_eq!(result["request_id"], 3);
    assert_eq!(
        result["ok"], false,
        "unattached client must be rejected: {result:?}"
    );
    assert!(result["source"].is_null(), "no source on rejection");
}

/// 예산(`MARKDOWN_CONTENT_BYTE_BUDGET`, 700 KiB)을 넘는 문서는 **연결이 끊기지 않고**
/// 잘린 채 `truncated: true` 와 함께 도착한다.
///
/// 본문을 따옴표로만 채운 것이 이 test 의 핵심이다 — 원문 400 KiB 는 예산 안이지만
/// JSON 이스케이프(`\"`)로 2 배가 돼 800 KiB 가 된다. 예산을 **원문 바이트**로 재면
/// 이 문서는 그대로 통과한 뒤 프레임 하드 상한(`MAX_FRAME_LEN`, 1 MiB)에 걸려 세션의
/// write thread 가 죽는다 — 예산이 막으려던 바로 그 연결 끊김이다. 그래서 여기서
/// 재는 것은 "잘렸다" 뿐 아니라 **무엇을 세어 잘랐는가** 다.
#[test]
fn markdown_content_over_budget_arrives_truncated_instead_of_killing_the_session() {
    const BUDGET: usize = 700 * 1024;
    const RAW_QUOTES: usize = 400 * 1024;

    let server = common::shared();
    let ws = server.create_workspace("md-content-truncated");
    let body = "\"".repeat(RAW_QUOTES);
    let file = write_doc_with_body("truncated", &body);
    let surface_id = open_markdown_surface(server, ws.id, &file);

    let (mut stream, _entry) = attach_when_descriptor_has_file(server, ws.id, surface_id);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 4,
            "surface_id": surface_id,
        }),
    );
    let result = wait_for_control_event(&mut stream, "markdown_content_result");

    assert_eq!(result["request_id"], 4);
    assert_eq!(result["ok"], true, "예산 초과는 실패가 아니다: {result:?}");
    assert_eq!(
        result["truncated"], true,
        "예산을 넘는 문서는 잘렸다고 알려야 한다"
    );

    let source = result["source"].as_str().expect("source on ok reply");
    assert!(
        source.len() < RAW_QUOTES,
        "잘렸다면 원문보다 짧아야 한다: {} vs {RAW_QUOTES}",
        source.len()
    );
    assert!(
        source.chars().all(|c| c == '"'),
        "실린 부분은 원문의 접두사 그대로여야 한다"
    );
    // ★ 직렬화된 길이가 예산 안이다 — 원문 바이트로 쟀다면 여기서 800 KiB 가 나온다.
    let serialized = serde_json::to_vec(&Value::String(source.to_string()))
        .expect("string always serializes")
        .len();
    assert!(
        serialized <= BUDGET,
        "직렬화 길이가 예산을 넘었다: {serialized} > {BUDGET}"
    );
    assert_eq!(
        serialized, BUDGET,
        "따옴표만 있는 문서는 예산에 정확히 차야 한다(2 + n*2)"
    );

    // 세션이 살아 있다 — 같은 연결로 한 번 더 왕복한다(write thread 가 죽었다면 여기서 멈춘다).
    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 5,
            "surface_id": surface_id,
        }),
    );
    let again = wait_for_control_event(&mut stream, "markdown_content_result");
    assert_eq!(again["request_id"], 5);
    assert_eq!(
        again["ok"], true,
        "예산 초과 회신 뒤에도 세션이 살아 있어야 한다"
    );

    // 이유: 뒷정리 best-effort — 실패해도 temp 디렉토리가 남을 뿐 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

/// 인가 집합은 **engine 전체**다 — 그 surface 를 담은 워크스페이스의 holder 로 좁혀 있지
/// 않다(ADR-0254 항목 2).
///
/// 술어는 `client_holds_workspace`("이 engine 의 워크스페이스를 **하나라도** 점유했는가")
/// 이고, 대상 조회는 `find_surface_by_id`(전 워크스페이스 순회)다. 그래서 W2 만 점유한
/// client 도 W1 의 markdown 원문을 받는다 — list_dir(`dir` 문자열만 실어 워크스페이스
/// 바인딩 필드가 없다)·git_query(engine 전역 `TerminalStore` 조회)와 같은 갈래이며,
/// `markdown_changed` 의 수신자도 이 집합이어야 한다(요청할 수 있는 client 와 신호를 받는
/// client 가 갈리면 안 된다).
///
/// 인가를 "그 surface 의 워크스페이스 holder" 로 좁히면 이 test 가 빨개진다.
#[test]
fn markdown_content_request_is_authorized_engine_wide_not_per_workspace() {
    let server = common::shared();
    let doc_ws = server.create_workspace("md-content-engine-wide-doc");
    let held_ws = server.create_workspace("md-content-engine-wide-held");
    let file = write_doc("engine-wide");
    let surface_id = open_markdown_surface(server, doc_ws.id, &file);

    // 문서 워크스페이스에 잠깐 붙는 것은 snapshot(`file`) 도착을 기다리기 위해서다.
    // 요청은 그 점유를 **놓은 뒤** 다른 워크스페이스 점유로만 보낸다.
    let (probe, _entry) = attach_when_descriptor_has_file(server, doc_ws.id, surface_id);
    drop(probe);

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut stream = loop {
        if let Some((s, _)) = try_open_workspace_attach_stream(server.port(), held_ws.id) {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "다른 워크스페이스 점유를 끝내 얻지 못했다"
        );
        std::thread::sleep(Duration::from_millis(250));
    };

    write_control_frame(
        &mut stream,
        &json!({
            "event": "markdown_content_request",
            "request_id": 6,
            "surface_id": surface_id,
        }),
    );
    let result = wait_for_control_event(&mut stream, "markdown_content_result");

    assert_eq!(result["request_id"], 6);
    assert_eq!(
        result["ok"], true,
        "다른 워크스페이스만 점유한 client 도 인가된다(engine 전체): {result:?}"
    );
    assert_eq!(
        result["source"], DOC_BODY,
        "원문이 그대로 실려야 한다: {result:?}"
    );

    // 이유: 뒷정리 best-effort — 실패해도 temp 디렉토리가 남을 뿐 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}
