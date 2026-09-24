//! 훅 실행과 수동 dispatch에서 자식 프로세스에 TASTY_HOOK_* 환경변수가 전달되는지 확인한다.
//! 훅 명령은 셸 문법으로 실행되고 수동 dispatch는 직접 exec하므로 시험 핸들러가 셸을 명시한다.
//! 한 인스턴스에서 두 경로를 순서대로 검사한다.

mod marker_wait;
mod webhook_common;

use std::time::Duration;

use marker_wait::wait_file_content;
use serde_json::json;
use webhook_common::{WebhookInstance, free_port};

#[test]
fn shell_handlers_receive_tasty_hook_env() {
    // 시계 해상도 대신 PID와 프로세스 내 카운터로 임시 경로를 구별한다.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let hook_marker = std::env::temp_dir().join(format!("tasty-hookenv-{unique}.txt"));
    let dispatch_marker = std::env::temp_dir().join(format!("tasty-dispenv-{unique}.txt"));

    // cmd는 > 앞 숫자를 파일 디스크립터로 해석할 수 있어 공백을 넣고 결과의 끝 공백은 trim한다.
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

    let ack = inst.call(
        "hook_handler.dispatch",
        json!({ "id": "user/envdispatch", "body": { "repo": "tasty" } }),
    );
    assert_eq!(ack["accepted"].as_bool(), Some(true));
    let content = wait_file_content(&dispatch_marker, Duration::from_secs(10));
    assert_eq!(content, "user/envdispatch/dispatch/tasty");

    std::fs::remove_file(&hook_marker).ok();
    std::fs::remove_file(&dispatch_marker).ok();
}
