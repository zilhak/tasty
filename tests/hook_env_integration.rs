//! 셸 훅 핸들러 `TASTY_HOOK_*` env 노출 E2E (HOOK-ENV).
//!
//! 실 tasty 인스턴스를 띄워 두 발화 경로 모두에서 자식 프로세스가 env 를 실제로
//! 받는지 검증한다:
//! 1. **hook 트리거** — `hook.set --handler` 로 등록 후 `surface.fire_hook` 발화.
//!    trigger 경로는 명령을 셸(cmd/sh) 래핑하므로 핸들러 command 가 그대로 셸 문법.
//! 2. **수동 발화** — `hook_handler.dispatch` + `body` payload. dispatch 경로는
//!    셸 경유 없는 직접 exec 이므로(인젝션 표면 축소) 핸들러가 셸 자체를 command 로
//!    가진다. payload 최상위 key 가 `TASTY_HOOK_<KEY>` 로 노출되는지 함께 본다.
//!
//! 윈도우 spawn(포커스 도난) 최소화를 위해 단일 공유 인스턴스에서 순차 실행한다
//! (webhook_integration.rs 와 동일 설계).

mod marker_wait;
mod webhook_common;

use std::time::Duration;

use marker_wait::wait_file_content;
use serde_json::json;
use webhook_common::{WebhookInstance, free_port};

#[test]
fn shell_handlers_receive_tasty_hook_env() {
    // 유일화 키에 **시각을 안 쓴다.** 시계의 해상도는 플랫폼의 성질이라 같은 코드가
    // 어떤 OS 에서는 유일하고 어떤 OS 에서는 겹친다 — 겹치면 두 완주가 같은 경로를
    // 쓰고 먼저 끝난 쪽이 다른 쪽의 파일을 지운다. 단조 카운터는 해상도가 없어
    // 플랫폼을 안 읽고, 프로세스 전역이라 같은 스레드의 재호출도 가른다.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let hook_marker = std::env::temp_dir().join(format!("tasty-hookenv-{unique}.txt"));
    let dispatch_marker = std::env::temp_dir().join(format!("tasty-dispenv-{unique}.txt"));

    // cmd 함정: `>` 직전 문자가 숫자면 fd 리다이렉트로 파싱되므로(`42> f` = stderr)
    // 리다이렉트 앞에 공백을 두고 결과 trailing space 는 trim 으로 흡수한다.
    let (hook_cmd, dispatch_shell, dispatch_flag, dispatch_line) = if cfg!(windows) {
        (
            format!(
                "echo %TASTY_HOOK_EVENT%/%TASTY_HOOK_SOURCE%/%TASTY_HOOK_SURFACE_ID% > {}",
                hook_marker.display()
            ),
            "cmd",
            "/C",
            format!(
                "echo %TASTY_HOOK_EVENT%/%TASTY_HOOK_SOURCE%/%TASTY_HOOK_REPO% > {}",
                dispatch_marker.display()
            ),
        )
    } else {
        (
            format!(
                "echo \"$TASTY_HOOK_EVENT/$TASTY_HOOK_SOURCE/$TASTY_HOOK_SURFACE_ID\" > {}",
                hook_marker.display()
            ),
            "sh",
            "-c",
            format!(
                "echo \"$TASTY_HOOK_EVENT/$TASTY_HOOK_SOURCE/$TASTY_HOOK_REPO\" > {}",
                dispatch_marker.display()
            ),
        )
    };

    let handlers_toml = format!(
        r#"
[[handler]]
id = "user/envhook"
source = "hook"
priority = 50
[handler.action]
kind = "shell_command"
command = '{hook_cmd}'

[[handler]]
id = "user/envdispatch"
source = "hook"
priority = 50
[handler.action]
kind = "shell_command"
command = "{dispatch_shell}"
args = ["{dispatch_flag}", '{dispatch_line}']
"#
    );

    let inst = WebhookInstance::builder(free_port())
        .file("hook-handlers.toml", &handlers_toml)
        .spawn();
    let sid = inst.first_surface_id();

    // ── 1. hook 트리거 경로 ──────────────────────────────────────────────
    inst.call(
        "hook.set",
        json!({ "surface_id": sid, "event": "bell", "handler": "user/envhook" }),
    );
    let fired = inst.call(
        "surface.fire_hook",
        json!({ "surface_id": sid, "event": "bell" }),
    );
    assert_eq!(fired["fired"].as_u64(), Some(1), "hook should fire once");
    let content = wait_file_content(&hook_marker, Duration::from_secs(10));
    assert_eq!(content, format!("bell/hook/{sid}"));

    // ── 2. hook_handler.dispatch 수동 발화 (payload env) ────────────────
    let ack = inst.call(
        "hook_handler.dispatch",
        json!({ "id": "user/envdispatch", "body": { "repo": "tasty" } }),
    );
    assert_eq!(ack["accepted"].as_bool(), Some(true));
    let content = wait_file_content(&dispatch_marker, Duration::from_secs(10));
    assert_eq!(content, "user/envdispatch/dispatch/tasty");

    // best-effort 정리 — temp 마커 잔류는 무해.
    std::fs::remove_file(&hook_marker).ok();
    std::fs::remove_file(&dispatch_marker).ok();
}
