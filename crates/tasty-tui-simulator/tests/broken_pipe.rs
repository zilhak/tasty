//! Verifies that `tasty-tui-sim` exits cleanly (no panic) when the peer
//! reading its stdout closes the pipe mid-REPL, instead of panicking on a
//! `BrokenPipe` write error.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[test]
fn broken_pipe_during_repl_exits_quietly_without_panic() {
    let exe = env!("CARGO_BIN_EXE_tasty-tui-sim");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn tasty-tui-sim");

    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = child.stdout.take().expect("child stdout");

    // Wait for the REPL to signal it's actually running before cutting it off.
    let mut ready = [0u8; 7]; // "READY\r\n"
    stdout
        .read_exact(&mut ready)
        .expect("failed to read READY signal");
    assert_eq!(&ready, b"READY\r\n");

    // Close our read end mid-session: the process's next stdout write(s)
    // will fail with BrokenPipe.
    drop(stdout);

    // Keep sending commands; some write on the other side is guaranteed to
    // observe the closed pipe and the loop should exit instead of panicking.
    for _ in 0..20 {
        if stdin.write_all(b"cursor 1 1\n").is_err() {
            break;
        }
    }
    let _ = stdin.flush(); // 정리: 이미 끊긴 파이프면 실패해도 무시
    drop(stdin);

    let mut stderr = String::new();
    if let Some(mut se) = child.stderr.take() {
        let _ = se.read_to_string(&mut stderr); // 정리: 진단용 best-effort 수집, 실패해도 무시
    }
    let status = child.wait().expect("failed to wait on child");

    assert!(
        status.success(),
        "expected clean exit on broken pipe, got {status:?}; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "expected no panic on broken pipe; stderr:\n{stderr}"
    );
}
