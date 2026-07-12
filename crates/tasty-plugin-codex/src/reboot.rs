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
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

/// 명령 접수 → kill 시작까지 기본 대기 (초). `--delay` 로 오버라이드.
const DEFAULT_DELAY_SECS: u64 = 5;
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
/// 문구 확인 후 추가 Enter 전까지 대기 — claude reboot 와 동일한 63자+ paste
/// 흡수 대비(제출 CR 이 paste 로 먹히면 입력창 잔류 → 별도 Enter 가 제출).
const NOTICE_SUBMIT_DELAY: Duration = Duration::from_millis(500);

/// codex 종료 시 출력되는 힌트 라인의 식별 조각 (v0.142 실측:
/// "To continue this session, run codex resume <id>").
const EXIT_MARKER: &str = "run codex resume";
/// codex 기동 배너의 식별 조각 (v0.142 실측: "│ >_ OpenAI Codex (v0.142.2)").
const BANNER_MARKER: &str = ">_ OpenAI Codex";
/// 화면 검증에 쓰는 안내문 선두 조각.
const NOTICE_SNIPPET: &str = "tasty codex reboot";

/// 재시작된 codex 에게 자동 제출되는 안내 프롬프트.
const REBOOT_NOTICE: &str = "tasty codex reboot : 이 세션은 tasty 의 reboot 기능으로 재시작되었습니다 (codex resume 으로 동일 세션 resume). 직전 턴이 잘렸을 수 있으니 마지막 작업 상태를 확인하고 이어서 진행하세요.";

/// `codex.reboot` 진입점. 검증·캡처를 동기로 끝내고 시퀀스는 background thread
/// 로 넘긴 뒤 즉시 응답한다 — 호출한 codex 가 턴을 마무리할 시간을 준다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface(params)?;
    let (delay_secs, extra_prompt) = parse_reboot_options(params);

    // 요청 시점 캡처.
    let session_id = fetch_session_id(host, surface_id)?;
    if !is_safe_session_id(&session_id) {
        return Err(IpcMethodError::new(format!(
            "surface {surface_id} has malformed codex-session-id meta: {session_id:?}"
        )));
    }

    // 마커 기준 카운트도 요청 시점에 스냅샷 — 과거 exit/기동 잔상에 속지 않기 위함.
    let Some(screen) = screen_text(host, surface_id) else {
        return Err(IpcMethodError::new(format!(
            "cannot read screen of surface {surface_id}"
        )));
    };
    let exit_c0 = count_occurrences(&screen, EXIT_MARKER);
    let banner_c0 = count_occurrences(&screen, BANNER_MARKER);

    {
        let mut set = inflight
            .lock()
            .map_err(|e| IpcMethodError::new(format!("reboot in-flight lock poisoned: {e}")))?;
        if !set.insert(surface_id) {
            return Err(IpcMethodError::new(format!(
                "reboot already in progress for surface {surface_id}"
            )));
        }
    }

    let thread_host = host.clone();
    let thread_inflight = inflight.clone();
    let thread_session = session_id.clone();
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
                extra_prompt.as_deref(),
            );
            if let Ok(mut set) = thread_inflight.lock() {
                set.remove(&surface_id);
            }
        });
    if let Err(e) = spawned {
        if let Ok(mut set) = inflight.lock() {
            set.remove(&surface_id);
        }
        return Err(IpcMethodError::new(format!(
            "failed to spawn reboot thread: {e}"
        )));
    }

    Ok(json!({
        "surface_id": surface_id,
        "session_id": session_id,
        "reboot_in_secs": delay_secs,
    }))
}

fn require_surface(params: &Value) -> Result<u32, IpcMethodError> {
    params
        .get("surface")
        .or_else(|| params.get("surface_id"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'surface' parameter"))
}

/// `--delay`(기본 5초) / `--prompt`(안내문 뒤에 덧붙일 추가 텍스트) 파싱.
pub(crate) fn parse_reboot_options(params: &Value) -> (u64, Option<String>) {
    let delay = params
        .get("delay")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_DELAY_SECS);
    let extra = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    (delay, extra)
}

/// surface meta 에서 codex session id 를 읽는다. 없으면 에러 — hook 미설치/미trust
/// 이거나 그 surface 에서 codex session-start hook 이 아직 발화하지 않은 것.
fn fetch_session_id(host: &HostHandle, surface_id: u32) -> Result<String, IpcMethodError> {
    let resp = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface_id, "key": "codex-session-id" }),
        )
        .map_err(|e| IpcMethodError::new(format!("host call 'surface.meta.get' failed: {e}")))?;
    let session_id = resp
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return Err(IpcMethodError::new(format!(
            "no active codex session on surface {surface_id} (codex-session-id meta not set — are tasty hooks installed and trusted? run `tasty codex install`, then approve via /hooks in codex)"
        )));
    }
    Ok(session_id)
}

/// session id 가 셸에 평문으로 들어가므로 uuid 계열 문자만 허용한다.
pub(crate) fn is_safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 셸에 전송할 resume 명령 (제출 `\r` 포함). 모든 셸(cmd/pwsh/bash)에서 동일하게
/// 동작하는 평문. `check_for_update_on_startup=false` 로 업데이트 프롬프트를 끈다
/// — 켜져 있으면 기동이 메뉴 다이얼로그에 가로채여 안내 프롬프트의 Enter 가
/// "Update now" 를 확정해 버린다.
pub(crate) fn resume_command(session_id: &str) -> String {
    format!("codex resume -c check_for_update_on_startup=false {session_id}\r")
}

/// 안내 프롬프트 본문. `--prompt` 추가 텍스트가 있으면 빈 줄 뒤에 덧붙인다.
pub(crate) fn build_notice(extra: Option<&str>) -> String {
    match extra {
        Some(t) => format!("{REBOOT_NOTICE}\n\n{t}"),
        None => REBOOT_NOTICE.to_string(),
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
    extra_prompt: Option<&str>,
) {
    thread::sleep(Duration::from_secs(delay_secs));

    if screen_text(host, surface_id).is_none() {
        tracing::warn!("codex reboot s{surface_id}: surface gone before kill — aborting");
        return;
    }

    // codex 는 Ctrl+C 1회로 종료된다(실측). 여분은 셸 프롬프트에서 no-op.
    for _ in 0..CTRL_C_COUNT {
        if let Err(e) = host.call(
            "surface.send_combo",
            json!({ "surface_id": surface_id, "key": "c", "modifiers": ["ctrl"] }),
        ) {
            tracing::warn!("codex reboot s{surface_id}: send_combo failed: {e} — aborting");
            return;
        }
        thread::sleep(CTRL_C_INTERVAL);
    }

    // 종료 확인: exit 마커("run codex resume" 힌트)가 요청 시점보다 늘어날 때까지.
    // 실패 시 절대 진행 금지 — 살아있는 codex TUI 입력창에 resume 명령이
    // 타이핑되는 사고 방지.
    if !poll_screen(host, surface_id, EXIT_WAIT, |s| {
        count_occurrences(s, EXIT_MARKER) > exit_c0
    }) {
        tracing::warn!(
            "codex reboot s{surface_id}: exit marker did not appear after {CTRL_C_COUNT}x Ctrl+C — aborting (nothing sent)"
        );
        return;
    }

    if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": resume_command(session_id) }),
    ) {
        tracing::warn!("codex reboot s{surface_id}: resume send failed: {e}");
        return;
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
        return;
    }
    thread::sleep(TUI_READY_GRACE);

    if !deliver_notice(host, surface_id, &build_notice(extra_prompt)) {
        tracing::warn!(
            "codex reboot s{surface_id}: notice not confirmed on screen after {NOTICE_ATTEMPTS} attempts"
        );
    }
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
            ensure_submitted(host, surface_id);
            return true;
        }
        tracing::info!(
            "codex reboot s{surface_id}: notice attempt {attempt}/{NOTICE_ATTEMPTS} not visible yet — retrying"
        );
        thread::sleep(NOTICE_RETRY_INTERVAL);
    }
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id);
        return true;
    }
    false
}

/// 문구가 화면에 있어도 제출(`\r`)이 paste 로 흡수돼 입력창에 잔류할 수 있으므로
/// 별도 Enter 를 한 번 더 보낸다. 이미 제출된 상태면 빈 입력창 Enter 라 no-op.
fn ensure_submitted(host: &HostHandle, surface_id: u32) {
    thread::sleep(NOTICE_SUBMIT_DELAY);
    if let Err(e) = host.call(
        "surface.send_key",
        json!({ "surface_id": surface_id, "key": "enter" }),
    ) {
        tracing::warn!("codex reboot s{surface_id}: extra submit enter failed: {e}");
    }
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
    fn parse_defaults_delay_5_and_no_prompt() {
        let (delay, extra) = parse_reboot_options(&json!({ "surface": 1 }));
        assert_eq!(delay, 5);
        assert_eq!(extra, None);
    }

    #[test]
    fn parse_explicit_delay_and_prompt() {
        let (delay, extra) = parse_reboot_options(&json!({ "delay": 2, "prompt": "이어서 계속" }));
        assert_eq!(delay, 2);
        assert_eq!(extra.as_deref(), Some("이어서 계속"));
    }

    #[test]
    fn safe_session_id_accepts_uuid() {
        assert!(is_safe_session_id("019f55e7-3dfa-7292-a8a9-9cf73a8b000b"));
    }

    #[test]
    fn safe_session_id_rejects_shell_metachars() {
        assert!(!is_safe_session_id(""));
        assert!(!is_safe_session_id("abc; rm -rf /"));
        assert!(!is_safe_session_id("a$(x)"));
    }

    #[test]
    fn resume_command_disables_update_prompt_and_submits() {
        assert_eq!(
            resume_command("019f55e7-3dfa"),
            "codex resume -c check_for_update_on_startup=false 019f55e7-3dfa\r"
        );
    }

    #[test]
    fn notice_without_extra_is_fixed_text() {
        assert_eq!(build_notice(None), REBOOT_NOTICE);
    }

    #[test]
    fn notice_with_extra_appends_after_blank_line() {
        let n = build_notice(Some("soak 이어서"));
        assert!(n.starts_with(REBOOT_NOTICE));
        assert!(n.ends_with("\n\nsoak 이어서"));
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
    fn require_surface_accepts_both_keys() {
        assert_eq!(require_surface(&json!({ "surface": 7 })).unwrap(), 7);
        assert_eq!(require_surface(&json!({ "surface_id": 9 })).unwrap(), 9);
        assert!(require_surface(&json!({})).is_err());
    }
}
