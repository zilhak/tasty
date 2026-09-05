//! `codex.reboot` — surface 안의 codex 를 종료하고 같은 세션으로 재기동한다.
//!
//! claude plugin 의 `reboot`(crates/tasty-plugin-claude/src/reboot.rs)와 목적 동형.
//! 단, **진행 판정 방식이 다르다**: claude 는 `surface.foreground_process` 이름
//! (baseline) 비교로 종료/복귀를 판정하지만, Windows 의 codex 는 npm shim 체인
//! (`sh.exe → node.exe → codex.exe`)에서 sh.exe 가 먼저 죽어 부모 체인이 끊기고,
//! 고아가 된 node/codex 는 surface 셸의 자손으로 걸리지 않아 전경 이름이 항상
//! 셸로 나온다(실측 2026-07-12). 그래서 codex 는 **화면 마커 카운트 증가**로
//! 판정한다:
//! - 종료: codex 가 exit 시 항상 출력하는 `run codex resume` 힌트 라인
//! - 복귀: 기동 배너 `>_ OpenAI Codex`
//!
//! 요청 시점 카운트 대비 **증가**를 요구하므로 화면에 남아있는 과거 마커에
//! 속지 않는다. 마커 문자열은 codex CLI 출력에 결합돼 있다(v0.142 실측) —
//! codex 가 문구를 바꾸면 폴링이 timeout 으로 안전 중단되고 아무것도 타이핑하지
//! 않는다(보수적 실패).
//!
//! codex 특화 나머지:
//! - session id 는 surface meta `codex-session-id`(session-start hook 이 stdin
//!   JSON payload 의 `session_id` 로 기록)에서 캡처.
//! - resume 명령은 `codex resume <id>` + `-c check_for_update_on_startup=false`.
//!   업데이트 프롬프트("Update now / Skip")가 기동을 가로채면 안내 프롬프트의
//!   제출 Enter 가 "Update now" 를 확정해 버리는 사고가 나므로 반드시 끈다(실측).
//! - codex TUI 는 Ctrl+C 1회로 즉시 종료된다(실측). 여분의 Ctrl+C 는 셸 프롬프트
//!   에서 no-op 이므로 claude 와 같은 4회 시퀀스를 그대로 쓴다. 이미 스스로
//!   종료돼 있던 경우도 여분 Ctrl+C 는 무해하나, exit 마커가 증가하지 않으므로
//!   보수적으로 중단된다(이미 죽은 codex 의 reboot 는 지원하지 않는다).
//! - codex 에는 SessionEnd hook 이 없어 meta unset 경로가 없다 — 다음
//!   session-start 가 덮어쓴다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_plugin_agent_common::reboot::{ensure_submitted, is_safe_session_id, parse_options};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

use crate::handlers::resolve_policy_args;

/// in-flight 집합의 락. 임계구역이 `HashSet<u32>` 의 insert/remove 뿐이라 패닉이
/// 지나가도 남는 값이 성립한다 — 복구가 답이다.
///
/// **획득과 해제의 답이 다르다는 점이 이 락의 요점이다.** 획득(`insert`)은 실패를
/// 호출자에게 에러로 돌려주면 그만이지만, 해제(`remove`)를 조용히 건너뛰면 그
/// `surface_id` 가 집합에 **영구히 남아** 이후 모든 reboot 이 "already in progress" 로
/// 거절된다. poison 은 sticky 라 한 번 걸리면 모든 surface 의 해제가 같이 막혀
/// 기능 전체가 잠긴다. 그래서 해제 쪽은 복구하고 이유를 남긴다.
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
/// 복귀 감지 후 TUI 입력 준비 grace.
const TUI_READY_GRACE: Duration = Duration::from_secs(3);
/// 안내 프롬프트 제출 시도 횟수 / 재시도 간격 / 제출→화면 검증 대기.
const NOTICE_ATTEMPTS: u32 = 4;
const NOTICE_RETRY_INTERVAL: Duration = Duration::from_secs(3);
const NOTICE_VERIFY_DELAY: Duration = Duration::from_millis(1500);
/// codex 종료 시 출력되는 힌트 라인의 식별 조각 (v0.142 실측:
/// "To continue this session, run codex resume <id>").
const EXIT_MARKER: &str = "run codex resume";
/// codex 기동 배너의 식별 조각 (v0.142 실측: "│ >_ OpenAI Codex (v0.142.2)").
const BANNER_MARKER: &str = ">_ OpenAI Codex";
/// 화면 검증에 쓰는 안내문 선두 조각.
const NOTICE_SNIPPET: &str = "tasty codex reboot";

/// 안내 프롬프트의 번역 키. 값은 `lang/{en,ko,ja}.toml` 에 있다.
///
/// 문구는 번역되지만 **선두 조각 [`NOTICE_SNIPPET`] 은 로케일과 무관하게 고정**이다 —
/// 화면 검증이 그 조각으로 "안내가 실제로 떴는가" 를 판정하기 때문이다. 세 언어 값이
/// 모두 그 조각으로 시작하는 것은 `notice_starts_with_the_snippet_in_every_locale` 가
/// 못 박는다. 형제 plugin(claude)의 `claude.reboot.notice` 와 같은 구조다.
const REBOOT_NOTICE_KEY: &str = "codex.reboot.notice";

/// `codex.reboot` 진입점. 검증·캡처를 동기로 끝내고 시퀀스는 background thread
/// 로 넘긴 뒤 즉시 응답한다 — 호출한 codex 가 턴을 마무리할 시간을 준다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    tr: &Translator,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = crate::handlers::require_target_surface(params, tr)?;
    let (delay_secs, extra_prompt) = parse_options(params);
    // resume 명령에 붙일 승인/샌드박스 정책(docs/plugins/codex/index.md 의 승인/샌드박스
    // 정책 플래그 절 참조) — spawn/launch/respawn 과 동일한 우선순위(호출별 override >
    // 전역 기본값 > codex 자체 기본값)로 해석한다.
    let policy_args = resolve_policy_args(host, params, tr)?;

    // 요청 시점 캡처.
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

    // 마커 기준 카운트도 요청 시점에 스냅샷 — 과거 exit/기동 잔상에 속지 않기 위함.
    let Some(screen) = screen_text(host, surface_id) else {
        return Err(IpcMethodError::new(tr.t_replace(
            "codex.reboot.screen_unreadable",
            "{surface}",
            &surface_id.to_string(),
        )));
    };
    let exit_c0 = count_occurrences(&screen, EXIT_MARKER);
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
    // 안내문은 **스레드에 넘기기 전에** 조립한다 — `Translator` 를 워커로 옮기지 않으려고
    // 완성된 문자열만 보낸다. 내용이 실행 시점 상태에 의존하지 않아 시점 차이가 없다.
    let thread_notice = build_notice(tr, extra_prompt.as_deref());
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

/// surface meta 에서 codex session id 를 읽는다. 없으면 에러 — hook 미설치/미trust
/// 이거나 그 surface 에서 codex session-start hook 이 아직 발화하지 않은 것.
fn fetch_session_id(
    host: &HostHandle,
    surface_id: u32,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    // 호스트 에러는 `PluginError::HostCall` 의 Display 가 이미
    // `host call '<method>' failed: <message>` 라 다시 감싸지 않는다 — 감싸면 그
    // 접두가 사용자에게 두 번 나간다(`handlers::host_call` 의 같은 주석 참조).
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

/// 셸에 전송할 resume 명령 (제출 `\r` 포함). 모든 셸(cmd/pwsh/bash)에서 동일하게
/// 동작하는 평문. `check_for_update_on_startup=false` 로 업데이트 프롬프트를 끈다
/// — 켜져 있으면 기동이 메뉴 다이얼로그에 가로채여 안내 프롬프트의 Enter 가
/// "Update now" 를 확정해 버린다. `--dangerously-bypass-hook-trust` 로 재시작된
/// codex 도 hook 이 항상 fire 되게 한다(`handlers::make_codex_command` 와 동일 이유).
///
/// `policy_args` 는 `handlers::resolve_policy_args` 가 만든 `-a ...`/`-s ...`/
/// `--dangerously-bypass-approvals-and-sandbox` 조각(또는 빈 문자열) — resume 된
/// codex 도 원래 기동과 같은 승인/샌드박스 정책 해석 규칙을 따른다
/// (docs/plugins/codex/index.md 의 승인/샌드박스 정책 플래그 절 참조).
pub(crate) fn resume_command(session_id: &str, policy_args: &str) -> String {
    let policy_suffix = if policy_args.is_empty() {
        String::new()
    } else {
        format!(" {policy_args}")
    };
    format!(
        "codex resume --dangerously-bypass-hook-trust{policy_suffix} -c check_for_update_on_startup=false {session_id}\r"
    )
}

/// 안내 프롬프트 본문. `--prompt` 추가 텍스트가 있으면 빈 줄 뒤에 덧붙인다.
pub(crate) fn build_notice(tr: &Translator, extra: Option<&str>) -> String {
    let base = tr.t(REBOOT_NOTICE_KEY);
    match extra {
        Some(t) => format!("{base}\n\n{t}"),
        None => base.to_string(),
    }
}

/// 겹치지 않는 부분 문자열 등장 횟수. 순수 함수 — 단위 테스트 대상.
pub(crate) fn count_occurrences(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    hay.matches(needle).count()
}

/// 전체 시퀀스 (background thread). 각 단계 실패는 warn 로그 후 중단 —
/// 살아있는 TUI/셸에 잘못된 텍스트를 흘리지 않는 것이 최우선.
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
        tracing::warn!("codex reboot s{surface_id}: surface gone before kill — aborting");
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

/// Ctrl+C ×N 전송 후 exit 마커("run codex resume" 힌트)가 요청 시점(`exit_c0`)보다
/// 늘어날 때까지 확인. 실패 시 `false` — 살아있는 codex TUI 입력창에 resume 명령이
/// 타이핑되는 사고 방지.
fn kill_codex_via_ctrlc(host: &HostHandle, surface_id: u32, exit_c0: usize) -> bool {
    // codex 는 Ctrl+C 1회로 종료된다(실측). 여분은 셸 프롬프트에서 no-op.
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

    // 종료 확인: exit 마커가 요청 시점보다 늘어날 때까지. 실패 시 절대 진행 금지 —
    // 살아있는 codex TUI 입력창에 resume 명령이 타이핑되는 사고 방지.
    if !poll_screen(host, surface_id, EXIT_WAIT, |s| {
        count_occurrences(s, EXIT_MARKER) > exit_c0
    }) {
        tracing::warn!(
            "codex reboot s{surface_id}: exit marker did not appear after {CTRL_C_COUNT}x Ctrl+C — aborting (nothing sent)"
        );
        return false;
    }
    true
}

/// resume 명령 전송 + 기동 배너 마커가 요청 시점(`banner_c0`)보다 늘어날 때까지
/// 확인. 미복귀면 `false` — 안내 프롬프트도 보내지 않는다(셸 프롬프트에 평문이
/// 명령으로 실행되는 사고 방지).
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

    // 복귀 확인: 기동 배너가 요청 시점보다 늘어날 때까지. 미복귀면 안내 프롬프트도
    // 보내지 않는다 — 셸 프롬프트에 평문이 명령으로 실행되는 사고 방지.
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

/// 안내 프롬프트를 제출하고 화면에 실제로 나타났는지 검증한다. TUI 초기화 중
/// PTY 입력이 유실될 수 있어 확인될 때까지 재시도한다. (배너 증가를 확인한 뒤에만
/// 도달하므로 셸에 타이핑될 위험은 배너 확인이 차단한다.)
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

/// `surface.screen_text` 1회 조회. 실패 → None (surface 소멸 등).
fn screen_text(host: &HostHandle, surface_id: u32) -> Option<String> {
    host.call("surface.screen_text", json!({ "surface_id": surface_id }))
        .ok()
        .and_then(|r| r.get("text").and_then(|t| t.as_str()).map(String::from))
}

fn screen_contains(host: &HostHandle, surface_id: u32, needle: &str) -> bool {
    screen_text(host, surface_id)
        .map(|t| t.contains(needle))
        .unwrap_or(false)
}

/// 화면 텍스트가 조건을 만족할 때까지 폴링. 조회 실패(surface 소멸)는 즉시 false.
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
            "codex resume --dangerously-bypass-hook-trust -c check_for_update_on_startup=false 019f55e7-3dfa\r"
        );
    }

    #[test]
    fn resume_command_includes_policy_args_when_present() {
        assert_eq!(
            resume_command("019f55e7-3dfa", "-a never -s read-only"),
            "codex resume --dangerously-bypass-hook-trust -a never -s read-only -c check_for_update_on_startup=false 019f55e7-3dfa\r"
        );
    }

    fn test_translator_for(code: &str) -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, code)
    }

    #[test]
    fn notice_without_extra_is_the_translated_text() {
        let tr = test_translator_for("ko");
        assert_eq!(build_notice(&tr, None), tr.t(REBOOT_NOTICE_KEY));
    }

    #[test]
    fn notice_with_extra_appends_after_blank_line() {
        let tr = test_translator_for("ko");
        let n = build_notice(&tr, Some("soak 이어서"));
        assert!(n.starts_with(tr.t(REBOOT_NOTICE_KEY)));
        assert!(n.ends_with("\n\nsoak 이어서"));
    }

    /// **세 로케일 모두** 안내문이 화면 검증 조각으로 시작한다.
    ///
    /// 전달 성공 판정(`deliver_notice`)이 [`NOTICE_SNIPPET`] 을 화면에서 찾는 것으로
    /// 이뤄진다 — 번역문이 그 조각을 잃으면 안내는 떴는데 **못 떴다고 판정**해
    /// 재시도를 반복하다 경고를 남긴다. 그 회귀는 그 언어를 쓰는 사용자에게만
    /// 나타나므로 한 언어만 보는 테스트로는 안 잡힌다.
    #[test]
    fn notice_starts_with_the_snippet_in_every_locale() {
        for code in ["en", "ko", "ja"] {
            let tr = test_translator_for(code);
            let notice = build_notice(&tr, None);
            assert!(
                notice.starts_with(NOTICE_SNIPPET),
                "[{code}] 안내문이 화면 검증 조각(`{NOTICE_SNIPPET}`)으로 시작하지 않는다: {notice}"
            );
        }
    }

    /// 문구가 실제로 `t()` 를 거친다 — 로케일을 바꾸면 완성 문구가 달라진다.
    #[test]
    fn notice_changes_with_the_locale() {
        let en = build_notice(&test_translator_for("en"), None);
        let ko = build_notice(&test_translator_for("ko"), None);
        assert_ne!(en, ko, "로케일이 달라도 같은 문구다 — t() 를 안 거친다");
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

        // 자르지 않는다 — `u32::MAX + 2` 를 자르면 1 이 되고, 그것은 실재할 수 있는
        // 다른 surface 의 id 다. `handlers::require_u32` 와 같은 갈래.
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
