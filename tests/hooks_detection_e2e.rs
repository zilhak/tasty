//! 실제 PTY 출력과 유휴 시간으로 OutputMatch·IdleTimeout 훅이 실행되는지 확인한다.
//! 다른 시험의 출력이 매칭·유휴 타이머에 섞이지 않도록 각자 전용 워크스페이스를 사용한다.

mod common;
mod marker_wait;

use marker_wait::wait_file_content;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

fn marker_write_command(marker: &Path, content: &str) -> String {
    format!("echo {} > {}", content, marker.display())
}

#[test]
fn output_match_hook_fires_on_real_pty_output() {
    let tasty = common::shared();
    let sid = tasty.create_workspace("hook-output-match").surface_id;
    tasty.wait_for_shell(sid);

    // PID와 프로세스 내 카운터로 임시 경로를 구별한다.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let marker = std::env::temp_dir().join(format!("tasty-outputmatch-{unique}.txt"));
    std::fs::remove_file(&marker).ok();

    tasty.call(
        "hook.set",
        json!({
            "surface_id": sid,
            "event": "output-match:TASTY_OUTPUT_MATCH_MARKER",
            "command": marker_write_command(&marker, "fired"),
        }),
    );

    tasty.send_text(sid, "echo TASTY_OUTPUT_MATCH_MARKER\r");

    let content = wait_file_content(&marker, Duration::from_secs(15));
    assert_eq!(content, "fired");

    std::fs::remove_file(&marker).ok();
}

#[test]
fn idle_timeout_hook_fires_after_no_output() {
    let tasty = common::shared();
    // 대상에 출력이 생기면 idle 카운트가 초기화되므로 전용 서피스를 사용하고 셸 준비를 기다린다.
    let sid = tasty.create_workspace("hook-idle-timeout").surface_id;
    tasty.wait_for_shell(sid);

    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let marker = std::env::temp_dir().join(format!("tasty-idletimeout-{unique}.txt"));
    std::fs::remove_file(&marker).ok();

    tasty.call(
        "hook.set",
        json!({
            "surface_id": sid,
            "event": "idle-timeout:2",
            "command": marker_write_command(&marker, "idle-fired"),
        }),
    );

    let content = wait_file_content(&marker, Duration::from_secs(15));
    assert_eq!(content, "idle-fired");

    std::fs::remove_file(&marker).ok();
}
