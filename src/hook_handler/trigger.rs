//! 내부 이벤트(hook) 트리거 → 바인딩 실행 (S9).
//!
//! `tasty-hooks` 는 leaf 크레이트라 공유 훅 핸들러 레지스트리를 볼 수 없어
//! (surface, event) 매칭만 하고 [`HookBinding`] 을 돌려준다. 실제 실행 —
//! 레지스트리 조회 + `source` 게이트(hook 수용 여부) + ShellCommand/IpcSequence
//! 분기 — 는 여기서 한다.
//!
//! ## source 게이트
//! 트리거 출처는 항상 [`TriggerSource::Hook`]. [`validate_binding`] 이 핸들러의
//! `source` 가 `Hook | Any` 인지 확인한다 — `Webhook` 전용 핸들러는 hook 트리거로
//! 실행되지 않는다(§3.1 셸/흐름 분리의 역방향 게이트).
//!
//! ## 하위호환
//! [`HookBinding::InlineShell`] 은 옛 `hook.set --command` 의 인라인 셸을 그대로
//! 실행한다(익명 hook 핸들러). 옛 `tasty-hooks::check_and_fire` 가 하던 셸 spawn 을
//! 여기로 옮긴 것이라 동작이 동일하다.

use tasty_hooks::{HookBinding, HookEvent};

use super::env::{HookShellEnv, build_env};
use super::exec::{SubstitutionContext, execute_sequence};
use super::registry::global;
use super::types::{HookHandlerAction, HookHandlerId, TriggerSource, validate_binding};
use crate::adapters::ipc::host_call::HostIpcInjector;

/// 발사된 훅의 바인딩을 실행한다 (fire-and-forget).
///
/// `injector` 는 IpcSequence 핸들러 실행에만 필요하다 — 없으면 IpcSequence 는
/// 건너뛰고 warn 을 남긴다(셸은 injector 불요). `event`(등록 이벤트) + `surface_id`
/// 는 셸 핸들러 자식 프로세스에 `TASTY_HOOK_*` env 로 노출되는 트리거 컨텍스트다
/// ([`super::env`]).
pub fn execute_binding(
    binding: &HookBinding,
    injector: Option<&HostIpcInjector>,
    event: &HookEvent,
    surface_id: u32,
) {
    let shell_env = || {
        build_env(&HookShellEnv {
            event: event.to_display_string(),
            source: "hook",
            surface_id: Some(surface_id),
            // hook 트리거엔 HTTP 페이로드가 없다(IpcSequence 의 빈 치환 컨텍스트와
            // 대칭). payload env 는 hook_handler.dispatch 경로에서만 채워진다.
            payload: serde_json::Value::Null,
        })
    };
    match binding {
        // 하위호환: 익명 셸 핸들러. 옛 check_and_fire 의 셸 spawn 과 동일 동작.
        HookBinding::InlineShell(command) => spawn_shell(command.clone(), Vec::new(), shell_env()),
        // 레지스트리 핸들러 참조 — 조회 + source 게이트 후 action 분기.
        HookBinding::Handler(id) => {
            let handler = match global().get(&HookHandlerId(id.clone())) {
                Some(h) => h,
                None => {
                    tracing::warn!("hook references unknown handler '{id}' — skipped");
                    return;
                }
            };
            if let Err(e) = validate_binding(&handler, TriggerSource::Hook) {
                tracing::warn!("hook handler '{id}' cannot bind to hook trigger: {e} — skipped");
                return;
            }
            match handler.action {
                HookHandlerAction::ShellCommand { command, args } => {
                    spawn_shell(command, args, shell_env())
                }
                HookHandlerAction::IpcSequence { calls } => match injector {
                    Some(inj) => {
                        // hook 트리거엔 HTTP 페이로드가 없으므로 빈 치환 컨텍스트.
                        execute_sequence(inj, &calls, &SubstitutionContext::default());
                    }
                    None => tracing::warn!(
                        "hook handler '{id}' is an IpcSequence but no IPC injector is available — skipped"
                    ),
                },
            }
        }
    }
}

/// 셸 명령을 백그라운드 스레드에서 fire-and-forget 실행. spawn 실패는 warn.
///
/// `args` 가 있으면 명령 문자열 뒤에 공백 join 해 붙인다(레지스트리 ShellCommand
/// 용 — 인라인 셸은 항상 args 없음). `env` 는 트리거 컨텍스트(`TASTY_HOOK_*`) —
/// 값 전달 전용이며 실행 대상은 바꾸지 못한다.
fn spawn_shell(command: String, args: Vec<String>, env: Vec<(String, String)>) {
    std::thread::spawn(move || {
        let full = if args.is_empty() {
            command
        } else {
            format!("{command} {}", args.join(" "))
        };
        let mut process = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", &full]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", &full]);
            c
        };
        process.envs(env);
        if let Err(e) = tasty_utils::process::hide_console(&mut process).output() {
            tracing::warn!("hook shell command spawn failed: {e}; cmd: {full}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 하위호환(익명 셸) 경로가 실제로 셸을 spawn 해 명령을 실행함을 증명한다 —
    /// 옛 `tasty-hooks::check_and_fire` 가 하던 셸 실행이 본체로 옮겨온 뒤에도
    /// 동일하게 동작하는지(ProcessExit 훅 등 셸 훅 회귀 방어) 검증한다.
    #[test]
    fn inline_shell_binding_spawns_and_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("hook-ran.txt");
        // 리다이렉트(`>`)는 cmd/sh 양쪽에서 동일 문법. 경로에 인용부호를 넣지
        // 않는다 — cmd 는 std 의 백슬래시-인용 이스케이프를 이해하지 못한다(셸
        // 호출은 옛 tasty-hooks 와 동일하며, 여기선 공백 없는 temp 경로를 쓴다).
        let cmd = format!("echo ok > {}", marker.display());

        execute_binding(&HookBinding::InlineShell(cmd), None, &HookEvent::Bell, 1);

        // fire-and-forget 백그라운드 스레드 — 파일 생성까지 폴링.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !marker.exists() {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            marker.exists(),
            "inline shell hook did not create marker file"
        );
    }

    /// 셸 훅 자식 프로세스가 `TASTY_HOOK_*` env 를 실제로 받는지 실 spawn 으로
    /// 증명한다 — cmd(`%VAR%`)/sh(`$VAR`) 각자의 확장 문법으로 값을 파일에 남긴다.
    #[test]
    fn shell_binding_receives_hook_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("hook-env.txt");
        // cmd 함정: `>` 직전 문자가 숫자면 fd 리다이렉트로 파싱된다(`42> f` = stderr).
        // 리다이렉트 앞에 공백을 두고, 결과의 trailing space 는 trim 으로 흡수한다.
        let cmd = if cfg!(windows) {
            format!(
                "echo %TASTY_HOOK_EVENT%/%TASTY_HOOK_SOURCE%/%TASTY_HOOK_SURFACE_ID% > {}",
                marker.display()
            )
        } else {
            format!(
                "echo \"$TASTY_HOOK_EVENT/$TASTY_HOOK_SOURCE/$TASTY_HOOK_SURFACE_ID\" > {}",
                marker.display()
            )
        };

        execute_binding(&HookBinding::InlineShell(cmd), None, &HookEvent::Bell, 42);

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !marker.exists() {
            std::thread::sleep(Duration::from_millis(25));
        }
        let content = std::fs::read_to_string(&marker).expect("marker written");
        assert_eq!(content.trim(), "bell/hook/42");
    }

    /// 알 수 없는 핸들러 id 참조는 조용히 무시(패닉 없음).
    #[test]
    fn unknown_handler_reference_is_noop() {
        execute_binding(
            &HookBinding::Handler("user/does-not-exist".into()),
            None,
            &HookEvent::Bell,
            1,
        );
    }
}
