//! `OutputMatch`/`IdleTimeout` 훅이 실제로 fire 되는지 회귀 방지 E2E.
//!
//! `tasty-hooks` 의 `matches()` 단위 테스트는 이미 만들어진 `HookEvent` 값끼리
//! 비교만 한다 — 등록된 훅이 실 PTY 청크/유휴시간으로부터 자동으로 fire 되는
//! 감지 루프(`handle_output_appended` → `cascade_terminal_output_match` /
//! `poll_idle_timeout_hooks`)는 이 파일에서만 검증한다. `hook_env_integration.rs`
//! 는 `surface.fire_hook` 로 **수동** 발화만 확인하므로 이 감지 루프 자체는
//! 커버하지 않는다.
//!
//! 두 테스트가 `common::shared()` 인스턴스 하나를 공유하되 각자 `create_workspace()`
//! 로 **자기 전용 surface** 를 확보한다. `hook.set` 은 surface 단위라 그것만으로
//! 격리되고, 특히 idle-timeout 테스트는 대상 surface 에 다른 출력이 생기면 idle
//! 카운트가 리셋되므로 전용 surface 가 정확성 요건이다.

mod common;
mod marker_wait;

use marker_wait::wait_file_content;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

/// 마커 파일에 고정 문자열을 쓰는 셸 커맨드 문자열. cmd/sh 양쪽에서 동일하게
/// 동작하는 단순 리다이렉트라 OS 분기가 필요 없다.
fn marker_write_command(marker: &Path, content: &str) -> String {
    format!("echo {} > {}", content, marker.display())
}

#[test]
fn output_match_hook_fires_on_real_pty_output() {
    let tasty = common::shared();
    // 자기 전용 surface — 다른 테스트의 출력이 이 훅의 매칭 대상에 섞이지 않게 한다.
    let sid = tasty.create_workspace("hook-output-match").surface_id;
    tasty.wait_for_shell(sid);

    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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

    // 등록만으론 훅이 안 돈다 — 실제 셸에 매칭 텍스트를 출력시켜야
    // `handle_output_appended` → `cascade_terminal_output_match` 감지 루프가 돈다.
    tasty.send_text(sid, "echo TASTY_OUTPUT_MATCH_MARKER\r");

    let content = wait_file_content(&marker, Duration::from_secs(15));
    assert_eq!(content, "fired");

    std::fs::remove_file(&marker).ok();
}

#[test]
fn idle_timeout_hook_fires_after_no_output() {
    let tasty = common::shared();
    // **전용 surface 가 정확성 요건이다** — 이 surface 에 출력이 생기면 idle 카운트가
    // 리셋되므로, 다른 테스트가 쓰는 surface 를 재사용하면 훅이 영영 안 fire 될 수
    // 있다. 프롬프트가 나온 뒤부터 idle 을 세도록 shell 준비까지 기다린다.
    let sid = tasty.create_workspace("hook-idle-timeout").surface_id;
    tasty.wait_for_shell(sid);

    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let marker = std::env::temp_dir().join(format!("tasty-idletimeout-{unique}.txt"));
    std::fs::remove_file(&marker).ok();

    // 아무 입력도 없이 이 시점부터 idle 카운트가 시작된다(마지막 셸 프롬프트 출력
    // 이후). 임계값은 1Hz BusyPoll tick 해상도를 감안해 2초로 짧게 잡는다.
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
