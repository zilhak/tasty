//! 실행한 서버에 loopback attach를 연결해 git_query_request/result 왕복을 확인한다.
//! 각 시험이 workspace와 임시 Git 저장소를 만들고 커밋·상태·diff 결과를 비교한다.
//! 실제 두 GUI의 attach 화면을 확인하는 시험은 아니다.

// 이유: 시험의 정리용 결과 무시는 제품 코드의 오류 처리 목록과 구분한다.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::net::TcpStream;
use std::process::Command;

use attach_common::{
    TAG_CONTROL, open_stream_without_attach, open_workspace_attach, read_frame, write_control_frame,
};
use serde_json::{Value, json};

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
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    // Git stderr를 다른 병렬 시험 출력과 섞지 않고 실패 진단에 담는다.
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tasty Test")
        .env("GIT_AUTHOR_EMAIL", "test@tasty.invalid")
        .env("GIT_COMMITTER_NAME", "Tasty Test")
        .env("GIT_COMMITTER_EMAIL", "test@tasty.invalid")
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed in {dir:?}: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
}

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
    let server = common::shared();
    let ws = server.create_workspace("git-query-snapshot");
    let repo = make_test_repo("snapshot");

    let mut stream = open_workspace_attach(server.port(), ws.id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 1,
            "surface_id": ws.surface_id,
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

    let log_entries = result["log_entries"].as_array().expect("log_entries array");
    assert_eq!(log_entries.len(), 1, "expected 1 commit: {log_entries:?}");
    assert_eq!(log_entries[0]["summary"], "initial commit");

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
    let server = common::shared();
    let ws = server.create_workspace("git-query-refresh");
    let repo = make_test_repo("refresh");

    let mut stream = open_workspace_attach(server.port(), ws.id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 10,
            "surface_id": ws.surface_id,
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
            "surface_id": ws.surface_id,
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
    let server = common::shared();
    let ws = server.create_workspace("git-query-diff");
    let repo = make_test_repo("diff");
    std::fs::write(repo.join("README.md"), b"hello\nworld\n").unwrap();

    let mut stream = open_workspace_attach(server.port(), ws.id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 20,
            "surface_id": ws.surface_id,
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
    let server = common::shared();
    let ws = server.create_workspace("git-query-nonrepo");

    let non_repo = std::env::temp_dir().join(format!(
        "tasty_git_query_loopback_nonrepo_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&non_repo);
    std::fs::create_dir_all(&non_repo).unwrap();

    let mut stream = open_workspace_attach(server.port(), ws.id);
    write_control_frame(
        &mut stream,
        &json!({
            "event": "git_query_request",
            "request_id": 30,
            "surface_id": ws.surface_id,
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
    // 점유 없는 클라이언트의 거절을 확인하므로 workspace를 만들거나 attach하지 않는다(ADR-0022).
    let server = common::shared();
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
