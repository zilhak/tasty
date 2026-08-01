//! git-viewer 원격(attach mirror) 조회(ADR-0056) — attach 채널의 `git_query_request`/
//! `git_query_result` 왕복을 loopback `TcpStream` 으로 실제 실행 중인 서버 인스턴스에
//! 대해 검증한다.
//!
//! `tests/attach_list_dir_loopback.rs` 와 동일한 접근(공유 handshake/frame 헬퍼는
//! 이 파일에도 그대로 복제 — `mod common` 은 여러 test binary 가 공유하지만 개별
//! `#[test]` 파일끼리는 서로 `mod` 할 수 없다): attach client 는 실제 `tasty` GUI 앱이
//! 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다.
//!
//! **GUI 두 인스턴스를 실제로 attach 하는 e2e**(`tasty tool attach --ssh
//! 127.0.0.1:<port>` 로 mirror workspace 를 만들고 git-viewer popup 을 열어 눈으로
//! 확인하는 것)는 이 headless 작업 환경(GPU 디스플레이 없음)에서 실행할 수 없다 — 이
//! test 는 그 대체로, 서버가 실제로 만든 git 저장소에 대해 (1) attach 점유 획득 →
//! (2) `git_query_request` 전송 → (3) 서버측 `handle_git_query_request` 가 실제
//! 디스크의 임시 git 저장소를 `tasty-git-core` 로 조회 → (4) `git_query_result` 로
//! 정확히 회신하는 전체 왕복을 프로토콜 레벨에서 실행한다.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use common::TastyInstance;
use serde_json::{Value, json};

const TAG_CONTROL: u8 = 1;

fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).expect("read frame header");
    let tag = hdr[0];
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload).expect("read frame payload");
    }
    (tag, payload)
}

fn write_control_frame(stream: &mut TcpStream, payload: &Value) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let mut hdr = [0u8; 5];
    hdr[0] = TAG_CONTROL;
    hdr[1..5].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    stream.write_all(&hdr).expect("write frame header");
    stream.write_all(&bytes).expect("write frame payload");
}

fn open_workspace_attach(port: u16, workspace_id: u64) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target_workspace": workspace_id},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if let Some(ok) = v.get("ok") {
            assert_eq!(ok, true, "handshake rejected: {v:?}");
            continue;
        }
        match v.get("event").and_then(|e| e.as_str()) {
            Some("attached_workspace") => return stream,
            Some("attach_error") => panic!("workspace attach rejected: {v:?}"),
            _ => continue, // 터미널 스냅샷 등 무관한 control 프레임 — 계속 대기.
        }
    }
}

fn open_stream_without_attach(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    let (tag, payload) = read_frame(&mut stream);
    assert_eq!(tag, TAG_CONTROL, "expected control ack frame");
    let ack: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(ack["ok"], true, "handshake rejected: {ack:?}");
    stream
}

fn first_workspace_id(instance: &TastyInstance) -> u64 {
    let workspaces = instance.call("workspace.list", json!({}));
    workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
}

fn wait_for_git_query_result(stream: &mut TcpStream, request_id: u64) -> Value {
    loop {
        let (tag, payload) = read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if v.get("event").and_then(|e| e.as_str()) == Some("git_query_result")
            && v.get("request_id").and_then(|r| r.as_u64()) == Some(request_id)
        {
            return v;
        }
        // 터미널 스냅샷/구조 델타 등 무관한 control 프레임 — 계속 대기.
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tasty Test")
        .env("GIT_AUTHOR_EMAIL", "test@tasty.invalid")
        .env("GIT_COMMITTER_NAME", "Tasty Test")
        .env("GIT_COMMITTER_EMAIL", "test@tasty.invalid")
        .status()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

/// 실제 디스크에 커밋 1개 + 추적되지 않은 파일 1개가 있는 git 저장소를 만든다.
fn make_test_repo(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tasty_git_query_loopback_{tag}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README.md"), b"hello\n").unwrap();
    git(&dir, &["add", "README.md"]);
    git(&dir, &["commit", "-q", "-m", "initial commit"]);
    std::fs::write(dir.join("untracked.txt"), b"scratch\n").unwrap();
    dir
}

#[test]
fn git_query_snapshot_matches_real_repo_over_attach_channel() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let sid = server.first_surface_id();
    let repo = make_test_repo("snapshot");

    let mut stream = open_workspace_attach(server.port(), ws_id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 1,
            "surface_id": sid,
            "kind": "snapshot",
            "worktree_path": repo.to_string_lossy(),
        }),
    );

    let result = wait_for_git_query_result(&mut stream, 1);
    assert_eq!(result["ok"], true, "expected ok reply: {result:?}");
    assert_eq!(result["kind"], "snapshot");

    let worktrees = result["worktrees"].as_array().expect("worktrees array");
    assert_eq!(
        worktrees.len(),
        1,
        "expected single main worktree: {worktrees:?}"
    );
    assert_eq!(worktrees[0]["is_main"], true);
    assert_eq!(worktrees[0]["is_current"], true);
    assert_eq!(worktrees[0]["branch"], "main");

    // git log 와 대조 — COMMITS 목록이 실제 `git log` 와 일치해야 한다.
    let log_entries = result["log_entries"].as_array().expect("log_entries array");
    assert_eq!(log_entries.len(), 1, "expected 1 commit: {log_entries:?}");
    assert_eq!(log_entries[0]["summary"], "initial commit");

    // status — untracked 파일이 status_entries 에 반영돼야 한다.
    let status_entries = result["status_entries"]
        .as_array()
        .expect("status_entries array");
    assert_eq!(
        status_entries.len(),
        1,
        "expected 1 status entry: {status_entries:?}"
    );
    assert_eq!(status_entries[0]["status"], "untracked");
    assert_eq!(status_entries[0]["path"], "untracked.txt");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn git_query_snapshot_reflects_new_commit_after_refresh() {
    // 새 커밋이 추가된 뒤 다시 로드하면 반영되는가 를 재현 — 첫 조회 이후 서버
    // 디스크에 커밋을 하나 더 만들고 재조회(refresh 와 동형의 두 번째 요청)하면
    // COMMITS 목록에 반영돼야 한다.
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let sid = server.first_surface_id();
    let repo = make_test_repo("refresh");

    let mut stream = open_workspace_attach(server.port(), ws_id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 10,
            "surface_id": sid,
            "kind": "snapshot",
            "worktree_path": repo.to_string_lossy(),
        }),
    );
    let first = wait_for_git_query_result(&mut stream, 10);
    assert_eq!(first["ok"], true);
    assert_eq!(first["log_entries"].as_array().unwrap().len(), 1);

    std::fs::write(repo.join("second.txt"), b"more\n").unwrap();
    git(&repo, &["add", "second.txt"]);
    git(&repo, &["commit", "-q", "-m", "second commit"]);

    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 11,
            "surface_id": sid,
            "kind": "snapshot",
            "worktree_path": repo.to_string_lossy(),
        }),
    );
    let second = wait_for_git_query_result(&mut stream, 11);
    assert_eq!(second["ok"], true);
    let log_entries = second["log_entries"].as_array().expect("log_entries array");
    assert_eq!(
        log_entries.len(),
        2,
        "expected 2 commits after refresh: {log_entries:?}"
    );
    assert_eq!(log_entries[0]["summary"], "second commit");
    assert_eq!(log_entries[1]["summary"], "initial commit");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn git_query_diff_returns_hunks_for_modified_file() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let sid = server.first_surface_id();
    let repo = make_test_repo("diff");
    std::fs::write(repo.join("README.md"), b"hello\nworld\n").unwrap();

    let mut stream = open_workspace_attach(server.port(), ws_id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 20,
            "surface_id": sid,
            "kind": "diff",
            "worktree_path": repo.to_string_lossy(),
            "diff_path": "README.md",
        }),
    );

    let result = wait_for_git_query_result(&mut stream, 20);
    assert_eq!(result["ok"], true, "expected ok reply: {result:?}");
    assert_eq!(result["kind"], "diff");
    assert_eq!(result["file_path"], "README.md");
    let hunks = result["hunks"].as_array().expect("hunks array");
    assert!(!hunks.is_empty(), "expected at least 1 hunk: {result:?}");
    let lines = hunks[0]["lines"].as_array().expect("lines array");
    assert!(
        lines
            .iter()
            .any(|l| l["kind"] == "addition" && l["content"] == "world"),
        "expected an addition line for 'world': {lines:?}"
    );

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn git_query_reports_error_for_non_repo_path() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let sid = server.first_surface_id();

    let non_repo = std::env::temp_dir().join(format!(
        "tasty_git_query_loopback_nonrepo_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&non_repo);
    std::fs::create_dir_all(&non_repo).unwrap();

    let mut stream = open_workspace_attach(server.port(), ws_id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 30,
            "surface_id": sid,
            "kind": "snapshot",
            "worktree_path": non_repo.to_string_lossy(),
        }),
    );

    let result = wait_for_git_query_result(&mut stream, 30);
    assert_eq!(result["ok"], false, "expected error reply: {result:?}");
    assert!(result["reason"].as_str().is_some_and(|r| !r.is_empty()));

    let _ = std::fs::remove_dir_all(&non_repo);
}

#[test]
fn git_query_rejected_without_workspace_occupancy() {
    // 하이브리드 신뢰 모델(ADR-0053 결정 3 과 동일 원칙, ADR-0056): attach 점유가
    // 유일한 인가 조건이다. 이 client 는 stream 을 upgrade 했을 뿐 어떤 workspace 도
    // 점유하지 않았으므로 서버는 실제 git 조회를 하지 않은 채 즉시 거부해야 한다.
    let server = TastyInstance::spawn();
    let mut stream = open_stream_without_attach(server.port());

    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 40,
            "surface_id": 1,
            "kind": "snapshot",
            "worktree_path": "/",
        }),
    );

    let result = wait_for_git_query_result(&mut stream, 40);
    assert_eq!(
        result["ok"], false,
        "unattached client must be rejected: {result:?}"
    );
    assert!(result["worktrees"].is_null(), "no worktrees on rejection");
}
