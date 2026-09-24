//! 같은 터미널의 Codex를 재시작하고 저장된 세션 ID로 재개한다.
//!
//! Windows의 npm 실행 체인에서는 전경 이름을 읽기 어려워 화면 문구로 진행을 추정한다.
//! Ctrl+C 뒤에는 run codex resume, 재기동 뒤에는 OpenAI Codex의 등장 횟수가
//! 요청 시점보다 늘어나야 한다. 임의 출력도 일치할 수 있고 화면에서 지워질 수도 있어
//! 프로세스 종료·복귀를 확정하거나 입력의 안전을 보장하는 검사는 아니다.
//!
//! 세션 ID와 화면의 기준 횟수는 종료 전에 읽는다.
//! 재시작 안내의 Enter가 업데이트 메뉴를 선택하지 않도록 업데이트 확인을 끈다.
//! 각 단계에서 조회나 시간 제한 검사가 실패하면 이후 명령 전송을 중단한다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_plugin_agent_common::reboot::{
    build_notice, ensure_submitted, is_safe_session_id, parse_options, screen_contains, screen_text,
};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::handlers::resolve_policy_args;

/// 진행 중 집합에 넣을 때 poison이면 요청을 거부한다.
/// 제거할 때는 내부 값을 사용해 ID가 남아 이후 요청을 계속 막는 일을 피한다.
const INFLIGHT_WHAT: &str = "the codex reboot in-flight set";
static INFLIGHT_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Ctrl+C 전송 횟수 / 간격.
const CTRL_C_COUNT: u32 = 4;
const CTRL_C_INTERVAL: Duration = Duration::from_millis(500);
/// 화면 마커 폴링 간격.
const SCREEN_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Ctrl+C 후 codex 종료(exit 마커 증가) 대기 한도.
const EXIT_WAIT: Duration = Duration::from_secs(8);
/// resume 명령 후 codex 복귀(배너 마커 증가) 대기 한도.
const RETURN_WAIT: Duration = Duration::from_secs(20);
/// 복귀 문구를 확인한 뒤 TUI 초기화를 기다리는 시간.
const TUI_READY_GRACE: Duration = Duration::from_secs(3);
/// 안내 프롬프트 제출 시도 횟수 / 재시도 간격 / 제출→화면 검증 대기.
const NOTICE_ATTEMPTS: u32 = 4;
const NOTICE_RETRY_INTERVAL: Duration = Duration::from_secs(3);
const NOTICE_VERIFY_DELAY: Duration = Duration::from_millis(1500);
/// 종료 안내에서 찾는 부분 문자열.
const EXIT_MARKER: &str = "run codex resume";
/// 시작 배너에서 찾는 부분 문자열.
const BANNER_MARKER: &str = "OpenAI Codex";
/// 화면 검증에 쓰는 안내문 선두 조각.
const NOTICE_SNIPPET: &str = "tasty codex reboot";

/// 안내문의 번역 키. 화면 확인에 쓰므로 모든 언어에서 NOTICE_SNIPPET으로 시작해야 한다.
const REBOOT_NOTICE_KEY: &str = "codex.reboot.notice";

/// 검증과 상태 조회를 마친 뒤 재시작 절차를 별도 스레드에 맡기고 응답한다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = crate::handlers::require_target_surface(params, tr)?;
    let (delay_secs, extra_prompt) = parse_options(params);
    // 기동과 같은 우선순위로 정책을 정한다. 최종 승인 기본값은 never다.
    let policy_args = resolve_policy_args(host, params, tr)?;

    let session_id = fetch_session_id(host, surface_id, tr)?;
    if !is_safe_session_id(&session_id) {
        return Err(IpcMethodError::new(crate::handlers::t_args(
            tr,
            "codex.reboot.malformed_session_id",
            &[
                ("{surface}", &surface_id.to_string()),
                ("{value}", &format!("{session_id:?}")),
            ],
        )));
    }

    // 이미 화면에 남은 문구와 구분하도록 현재 횟수를 기록한다.
    let Some(screen) = screen_text(host, surface_id) else {
        return Err(IpcMethodError::new(tr.t_replace(
            "codex.reboot.screen_unreadable",
            "{surface}",
            &surface_id.to_string(),
        )));
    };
    let exit_c0 = exit_marker_count(&screen);
    let banner_c0 = count_occurrences(&screen, BANNER_MARKER);

    {
        let mut set = inflight.lock().map_err(|e| {
            IpcMethodError::new(tr.t_replace(
                "codex.reboot.lock_poisoned",
                "{detail}",
                &e.to_string(),
            ))
        })?;
        if !set.insert(surface_id) {
            return Err(IpcMethodError::new(tr.t_replace(
                "codex.reboot.already_in_progress",
                "{surface}",
                &surface_id.to_string(),
            )));
        }
    }

    let thread_host = host.clone();
    let thread_inflight = inflight.clone();
    let thread_session = session_id.clone();
    let thread_policy_args = policy_args.clone();
    // 스레드에는 번역기를 빌려주지 않고 완성된 안내문을 넘긴다.
    let thread_notice = build_notice(&tr.t(REBOOT_NOTICE_KEY), extra_prompt.as_deref());
    let spawned = thread::Builder::new()
        .name(format!("codex-reboot-s{surface_id}"))
        .spawn(move || {
            run_reboot_sequence(
                &thread_host,
                surface_id,
                delay_secs,
                &thread_session,
                exit_c0,
                banner_c0,
                &thread_notice,
                &thread_policy_args,
            );
            tasty_utils::poison::recover_mutex(
                thread_inflight.lock(),
                INFLIGHT_WHAT,
                &INFLIGHT_POISON_REPORTED,
            )
            .remove(&surface_id);
        });
    if let Err(e) = spawned {
        tasty_utils::poison::recover_mutex(
            inflight.lock(),
            INFLIGHT_WHAT,
            &INFLIGHT_POISON_REPORTED,
        )
        .remove(&surface_id);
        return Err(IpcMethodError::new(tr.t_replace(
            "codex.reboot.spawn_thread_failed",
            "{detail}",
            &e.to_string(),
        )));
    }

    Ok(json!({
        "surface_id": surface_id,
        "session_id": session_id,
        "reboot_in_secs": delay_secs,
    }))
}

/// 세션 ID 메타데이터를 읽는다. 조회 실패나 값 부재는 오류로 반환한다.
fn fetch_session_id(
    host: &HostHandle,
    surface_id: u32,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    // 호스트 오류에 이미 있는 접두어를 다시 붙이지 않는다.
    let resp = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface_id, "key": "codex-session-id" }),
        )
        .map_err(IpcMethodError::from)?;
    let session_id = resp
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return Err(IpcMethodError::new(tr.t_replace(
            "codex.reboot.no_active_session",
            "{surface}",
            &surface_id.to_string(),
        )));
    }
    Ok(session_id)
}

/// 제출 문자(\r)를 포함한 resume 명령을 만든다.
/// POSIX에서는 alias/function을 우회하고 Windows에서는 기존 codex 명령을 유지한다.
/// 업데이트 확인을 끄고 호출자가 정한 정책과 훅 신뢰 우회 플래그를 전달한다.
/// 이 플래그가 원격 resume의 훅 검토 화면까지 없애지는 않을 수 있다.
pub(crate) fn resume_command(session_id: &str, policy_args: &str) -> String {
    // Windows surfaces may use Git Bash, cmd, or PowerShell. Preserve the existing
    // token until the receiving shell family is known; cmd switches break in MSYS.
    let command = if cfg!(windows) {
        "codex"
    } else {
        crate::POSIX_CODEX_COMMAND
    };
    let policy_suffix = if policy_args.is_empty() {
        String::new()
    } else {
        format!(" {policy_args}")
    };
    format!(
        "{} resume --dangerously-bypass-hook-trust{policy_suffix} -c check_for_update_on_startup=false {session_id}\r",
        command
    )
}

fn exit_marker_count(screen: &str) -> usize {
    count_occurrences(screen, EXIT_MARKER)
}

/// 겹치지 않는 부분 문자열의 등장 횟수를 센다.
pub(crate) fn count_occurrences(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    hay.matches(needle).count()
}

/// 별도 스레드에서 각 단계를 실행한다. 실패하면 경고 후 중단한다.
#[allow(clippy::too_many_arguments)]
fn run_reboot_sequence(
    host: &HostHandle,
    surface_id: u32,
    delay_secs: u64,
    session_id: &str,
    exit_c0: usize,
    banner_c0: usize,
    notice: &str,
    policy_args: &str,
) {
    thread::sleep(Duration::from_secs(delay_secs));

    if screen_text(host, surface_id).is_none() {
        tracing::warn!("codex reboot s{surface_id}: screen unavailable before Ctrl+C; aborting");
        return;
    }

    if !kill_codex_via_ctrlc(host, surface_id, exit_c0) {
        return;
    }

    if !resume_and_wait(host, surface_id, session_id, policy_args, banner_c0) {
        return;
    }
    thread::sleep(TUI_READY_GRACE);

    if !deliver_notice(host, surface_id, notice) {
        tracing::warn!(
            "codex reboot s{surface_id}: notice not confirmed on screen after {NOTICE_ATTEMPTS} attempts"
        );
    }
}

/// Ctrl+C를 보낸 뒤 종료 안내 문구가 요청 때보다 늘었는지 확인한다.
fn kill_codex_via_ctrlc(host: &HostHandle, surface_id: u32, exit_c0: usize) -> bool {
    for _ in 0..CTRL_C_COUNT {
        if let Err(e) = host.call(
            "surface.send_combo",
            json!({ "surface_id": surface_id, "key": "c", "modifiers": ["ctrl"] }),
        ) {
            tracing::warn!("codex reboot s{surface_id}: send_combo failed: {e} — aborting");
            return false;
        }
        thread::sleep(CTRL_C_INTERVAL);
    }

    if !poll_screen(host, surface_id, EXIT_WAIT, |s| {
        exit_marker_count(s) > exit_c0
    }) {
        tracing::warn!(
            "codex reboot s{surface_id}: exit marker count did not increase after {CTRL_C_COUNT}x Ctrl+C; resume command not sent"
        );
        return false;
    }
    true
}

/// resume 명령을 보내고 시작 배너가 요청 때보다 늘었는지 확인한다.
fn resume_and_wait(
    host: &HostHandle,
    surface_id: u32,
    session_id: &str,
    policy_args: &str,
    banner_c0: usize,
) -> bool {
    if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": resume_command(session_id, policy_args) }),
    ) {
        tracing::warn!("codex reboot s{surface_id}: resume send failed: {e}");
        return false;
    }

    if !poll_screen(host, surface_id, RETURN_WAIT, |s| {
        count_occurrences(s, BANNER_MARKER) > banner_c0
    }) {
        tracing::warn!(
            "codex reboot s{surface_id}: codex banner did not reappear within {}s — resume sent but notice skipped",
            RETURN_WAIT.as_secs()
        );
        return false;
    }
    true
}

/// 안내문을 보내고 화면에서 선두 문구를 찾는다. 제한된 횟수만큼 재시도한다.
fn deliver_notice(host: &HostHandle, surface_id: u32, notice: &str) -> bool {
    for attempt in 1..=NOTICE_ATTEMPTS {
        if let Err(e) = host.call(
            "terminal.tell",
            json!({ "surface": surface_id, "text": notice }),
        ) {
            tracing::warn!("codex reboot s{surface_id}: notice tell failed: {e}");
            return false;
        }
        thread::sleep(NOTICE_VERIFY_DELAY);
        if screen_contains(host, surface_id, NOTICE_SNIPPET) {
            ensure_submitted(host, surface_id, "codex");
            return true;
        }
        tracing::info!(
            "codex reboot s{surface_id}: notice attempt {attempt}/{NOTICE_ATTEMPTS} not visible yet — retrying"
        );
        thread::sleep(NOTICE_RETRY_INTERVAL);
    }
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id, "codex");
        return true;
    }
    false
}

/// 화면을 폴링한다. 조회 실패 시 중단하고, 조건 불일치 시 경과 시간을 확인한다.
fn poll_screen(
    host: &HostHandle,
    surface_id: u32,
    timeout: Duration,
    pred: impl Fn(&str) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match screen_text(host, surface_id) {
            Some(text) if pred(&text) => return true,
            Some(_) => {}
            None => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SCREEN_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_command_disables_update_prompt_and_submits() {
        assert_eq!(
            resume_command("019f55e7-3dfa", ""),
            format!(
                "{} resume --dangerously-bypass-hook-trust -c check_for_update_on_startup=false 019f55e7-3dfa\r",
                if cfg!(windows) {
                    "codex"
                } else {
                    "command codex"
                }
            )
        );
    }

    #[test]
    fn resume_command_includes_policy_args_when_present() {
        assert_eq!(
            resume_command("019f55e7-3dfa", "-a never -s read-only"),
            format!(
                "{} resume --dangerously-bypass-hook-trust -a never -s read-only -c check_for_update_on_startup=false 019f55e7-3dfa\r",
                if cfg!(windows) {
                    "codex"
                } else {
                    "command codex"
                }
            )
        );
    }

    fn test_translator_for(code: &str) -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, code)
    }

    #[test]
    fn notice_without_extra_is_the_translated_text() {
        let tr = test_translator_for("ko");
        assert_eq!(
            build_notice(&tr.t(REBOOT_NOTICE_KEY), None),
            tr.t(REBOOT_NOTICE_KEY)
        );
    }

    #[test]
    fn notice_with_extra_appends_after_blank_line() {
        let tr = test_translator_for("ko");
        let n = build_notice(&tr.t(REBOOT_NOTICE_KEY), Some("soak 이어서"));
        assert!(n.starts_with(tr.t(REBOOT_NOTICE_KEY)));
        assert!(n.ends_with("\n\nsoak 이어서"));
    }

    /// 모든 언어에서 화면 확인에 사용할 선두 문구를 유지해야 한다.
    #[test]
    fn notice_starts_with_the_snippet_in_every_locale() {
        for code in ["en", "ko", "ja"] {
            let tr = test_translator_for(code);
            let notice = build_notice(&tr.t(REBOOT_NOTICE_KEY), None);
            assert!(
                notice.starts_with(NOTICE_SNIPPET),
                "[{code}] 안내문이 화면 검증 조각(`{NOTICE_SNIPPET}`)으로 시작하지 않는다: {notice}"
            );
        }
    }

    /// 언어별 카탈로그를 사용하는지 확인한다.
    #[test]
    fn notice_changes_with_the_locale() {
        let en = build_notice(&test_translator_for("en").t(REBOOT_NOTICE_KEY), None);
        let ko = build_notice(&test_translator_for("ko").t(REBOOT_NOTICE_KEY), None);
        assert_ne!(en, ko, "언어가 달라도 같은 문구를 반환했다");
    }

    #[test]
    fn count_occurrences_counts_and_handles_empty() {
        assert_eq!(count_occurrences("a b a b a", "a"), 3);
        assert_eq!(
            count_occurrences("run codex resume x\nrun codex resume y", "run codex resume"),
            2
        );
        assert_eq!(count_occurrences("anything", ""), 0);
        assert_eq!(count_occurrences("", "x"), 0);
    }

    /// 종료 안내와 시작 배너를 구분해 세는지 확인한다.
    #[test]
    fn exit_marker_count_counts_only_the_quit_hint() {
        assert_eq!(
            exit_marker_count("To continue this session, run codex resume thread"),
            1
        );
        assert_eq!(exit_marker_count("\u{2502} >_ OpenAI Codex (v0.142.2)"), 0);
    }

    #[test]
    fn exit_marker_matches_observed_quit_hint() {
        // v0.142 실측 종료 출력에서 마커가 잡히는지 회귀 가드.
        let observed = "Token usage: total=509 input=504\nTo continue this session, run codex resume 019f55f1-cd32-7790-9459-ee7f5488051e";
        assert_eq!(count_occurrences(observed, EXIT_MARKER), 1);
    }

    #[test]
    fn banner_marker_matches_observed_banner() {
        let observed =
            "\u{256d}\u{2500}\u{2500}\u{256e}\n\u{2502} >_ OpenAI Codex (v0.142.2) \u{2502}";
        assert_eq!(count_occurrences(observed, BANNER_MARKER), 1);
    }

    #[test]
    fn require_target_surface_accepts_both_keys() {
        assert_eq!(
            crate::handlers::require_target_surface(
                &json!({ "surface": 7 }),
                &test_translator_for("en")
            )
            .unwrap(),
            7
        );
        assert_eq!(
            crate::handlers::require_target_surface(
                &json!({ "surface_id": 9 }),
                &test_translator_for("en")
            )
            .unwrap(),
            9
        );
        assert!(
            crate::handlers::require_target_surface(&json!({}), &test_translator_for("en"))
                .is_err()
        );

        // 범위를 넘는 ID를 잘라 다른 대상을 선택하지 않아야 한다.
        assert!(
            crate::handlers::require_target_surface(
                &json!({ "surface": u64::from(u32::MAX) + 2 }),
                &test_translator_for("en")
            )
            .is_err()
        );
        assert!(
            crate::handlers::require_target_surface(
                &json!({ "surface": "conductor" }),
                &test_translator_for("en")
            )
            .is_err()
        );
        assert_eq!(
            crate::handlers::require_target_surface(
                &json!({ "surface": u32::MAX }),
                &test_translator_for("en")
            )
            .unwrap(),
            u32::MAX
        );
    }
}
