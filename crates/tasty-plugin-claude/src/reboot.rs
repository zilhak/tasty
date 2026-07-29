//! `claude.reboot` — surface 안의 claude 를 종료하고 같은 세션으로 재기동한다.
//!
//! claude code 는 스스로 자기 TUI 를 껐다 켤 수 없으므로, 에이전트가
//! `tasty claude reboot` 를 호출하면 plugin 이 대신 수행한다:
//! 지연(기본 5s) → Ctrl+C ×4(0.5s 간격) → 셸에 `claude -r <session_id>` 전송 →
//! TUI 복귀 확인 후 재시작 안내 프롬프트를 `terminal.tell` 로 제출.
//!
//! session id 는 **요청 시점에** surface meta(`claude-session-id`, session-start
//! hook 이 기록)에서 캡처한다 — Ctrl+C 종료가 session-end hook 을 발화시키면
//! meta 가 지워지기 때문. 안내 프롬프트는 셸 인자가 아니라 `terminal.tell` 로
//! 보낸다 — Windows 기본 셸(cmd.exe)에는 `"$(cat …)"` 전달 패턴이 없기 때문.
//!
//! 안전 가드: 전경 프로세스 이름을 요청 시점에 baseline 으로 캡처해 두고,
//! Ctrl+C 후에도 전경이 baseline(=claude)이면 **아무 텍스트도 보내지 않고 중단**
//! 한다(살아있는 TUI 입력창 오염 방지). resume 후 전경이 baseline 으로 복귀하지
//! 않으면 안내 프롬프트도 보내지 않는다(셸에 평문이 명령으로 실행되는 사고 방지).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};

use crate::handlers::require_surface_id;

/// 명령 접수 → kill 시작까지 기본 대기 (초). `--delay` 로 오버라이드.
const DEFAULT_DELAY_SECS: u64 = 5;
/// Ctrl+C 전송 횟수 / 간격.
const CTRL_C_COUNT: u32 = 4;
const CTRL_C_INTERVAL: Duration = Duration::from_millis(500);
/// 전경 프로세스 폴링 간격.
const FG_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Ctrl+C 후 claude 종료(전경 이탈) 대기 한도.
const EXIT_WAIT: Duration = Duration::from_secs(5);
/// resume 명령 후 claude 복귀(전경 재진입) 대기 한도.
const RETURN_WAIT: Duration = Duration::from_secs(15);
/// 복귀 감지 후 TUI 입력 준비 grace. 프로세스는 전경에 즉시 잡히지만 TUI 가
/// 입력을 받기까지는 초기화(MCP 로딩 등)가 더 걸린다.
const TUI_READY_GRACE: Duration = Duration::from_secs(3);
/// 안내 프롬프트 제출 시도 횟수 / 재시도 간격 / 제출→화면 검증 대기.
/// TUI 초기화 중 입력은 소리 없이 유실되므로(실측), 제출 후 화면에 문구가
/// 나타났는지 `surface.screen_text` 로 확인하고 없으면 재시도한다.
const NOTICE_ATTEMPTS: u32 = 4;
const NOTICE_RETRY_INTERVAL: Duration = Duration::from_secs(3);
const NOTICE_VERIFY_DELAY: Duration = Duration::from_millis(1500);
/// 문구 확인 후 추가 Enter 전까지 대기. tell 의 본문/`\r` 분리 write 도 TUI 부팅
/// 직후엔 한 read burst 로 합쳐져 `\r` 이 paste 로 흡수될 수 있다(실측: 문구가
/// 입력창에 미제출로 잔류). 이미 제출된 경우 빈 입력창 Enter 는 no-op 이므로
/// 확인 후 별도 Enter 1회는 항상 안전하다.
const NOTICE_SUBMIT_DELAY: Duration = Duration::from_millis(500);
/// 화면 검증에 쓰는 문구 조각 — 안내문 선두라 112col 화면에서도 줄바꿈 없이
/// 붙어서 렌더된다.
const NOTICE_SNIPPET: &str = "tasty claude reboot";

/// 재시작된 claude 에게 자동 제출되는 안내 프롬프트.
const REBOOT_NOTICE: &str = "tasty claude reboot : 이 세션은 tasty 의 reboot 기능으로 재시작되었습니다 (claude -r 로 동일 세션 resume). 직전 턴이 잘렸을 수 있으니 마지막 작업 상태를 확인하고 이어서 진행하세요.";

/// `claude.reboot` 진입점. 검증·캡처를 동기로 끝내고 시퀀스는 background thread
/// 로 넘긴 뒤 즉시 응답한다 — 호출한 claude 가 턴을 마무리할 시간을 준다.
pub(crate) fn handle_reboot(
    inflight: &Arc<Mutex<HashSet<u32>>>,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_surface_id(params)?;
    let (delay_secs, extra_prompt) = parse_reboot_options(params);

    // 요청 시점 캡처 (session-end 가 meta 를 지우기 전).
    let session_id = fetch_session_id(host, surface_id)?;
    if !is_safe_session_id(&session_id) {
        return Err(IpcMethodError::new(format!(
            "surface {surface_id} has malformed claude-session-id meta: {session_id:?}"
        )));
    }

    let Some(baseline) = query_foreground(host, surface_id) else {
        return Err(IpcMethodError::new(format!(
            "cannot determine foreground process of surface {surface_id}"
        )));
    };

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
        .name(format!("claude-reboot-s{surface_id}"))
        .spawn(move || {
            run_reboot_sequence(
                &thread_host,
                surface_id,
                delay_secs,
                &baseline,
                &thread_session,
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

/// surface meta 에서 claude session id 를 읽는다. 없으면 에러 — hook 미설치이거나
/// 그 surface 에 살아있는 claude 세션이 없다는 뜻.
fn fetch_session_id(host: &HostHandle, surface_id: u32) -> Result<String, IpcMethodError> {
    let resp = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface_id, "key": "claude-session-id" }),
        )
        .map_err(IpcMethodError::from)?;
    let session_id = resp
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return Err(IpcMethodError::new(format!(
            "no active claude session on surface {surface_id} (claude-session-id meta not set — are tasty hooks installed? run `tasty claude install`)"
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
/// 동작하는 평문 — inline env prefix 는 붙이지 않는다(PTY env 에 `TASTY_SURFACE_ID`
/// 가 이미 주입돼 있고, `VAR=x cmd` 문법은 POSIX 전용이라 cmd.exe 에서 깨진다).
pub(crate) fn resume_command(session_id: &str) -> String {
    format!("claude -r {session_id}\r")
}

/// 안내 프롬프트 본문. `--prompt` 추가 텍스트가 있으면 빈 줄 뒤에 덧붙인다.
pub(crate) fn build_notice(extra: Option<&str>) -> String {
    match extra {
        Some(t) => format!("{REBOOT_NOTICE}\n\n{t}"),
        None => REBOOT_NOTICE.to_string(),
    }
}

/// delay 후 전경 상태에 따른 다음 행동. 순수 함수 — 단위 테스트 대상.
#[derive(Debug, PartialEq)]
pub(crate) enum AfterDelay {
    /// 여전히 claude(baseline) — Ctrl+C 시퀀스로 종료시킨다.
    SendCtrlC,
    /// 이미 전경이 바뀜(스스로 종료 등) — 바로 resume 으로 간다.
    SkipToResume,
}

pub(crate) fn after_delay_action(current: &str, baseline: &str) -> AfterDelay {
    if current == baseline {
        AfterDelay::SendCtrlC
    } else {
        AfterDelay::SkipToResume
    }
}

/// 전체 시퀀스 (background thread). 각 단계 실패는 warn 로그 후 중단 —
/// 살아있는 TUI/셸에 잘못된 텍스트를 흘리지 않는 것이 최우선.
fn run_reboot_sequence(
    host: &HostHandle,
    surface_id: u32,
    delay_secs: u64,
    baseline: &str,
    session_id: &str,
    extra_prompt: Option<&str>,
) {
    thread::sleep(Duration::from_secs(delay_secs));

    let Some(current) = query_foreground(host, surface_id) else {
        tracing::warn!("claude reboot s{surface_id}: surface gone before kill — aborting");
        return;
    };

    if !kill_or_skip(host, surface_id, baseline, &current) {
        return;
    }

    if !resume_and_wait(host, surface_id, baseline, session_id) {
        return;
    }
    thread::sleep(TUI_READY_GRACE);

    if !deliver_notice(host, surface_id, baseline, &build_notice(extra_prompt)) {
        tracing::warn!(
            "claude reboot s{surface_id}: notice not confirmed on screen after {NOTICE_ATTEMPTS} attempts"
        );
    }
}

/// delay 후 판정(`AfterDelay`)에 따라 Ctrl+C 로 종료시키거나(SendCtrlC) 이미 바뀐
/// 전경을 그대로 인정하고 넘어간다(SkipToResume). kill 실패 시 `false`.
fn kill_or_skip(host: &HostHandle, surface_id: u32, baseline: &str, current: &str) -> bool {
    match after_delay_action(current, baseline) {
        AfterDelay::SendCtrlC => kill_claude_via_ctrlc(host, surface_id, baseline),
        AfterDelay::SkipToResume => {
            tracing::info!(
                "claude reboot s{surface_id}: foreground already '{current}' (was '{baseline}') — skipping Ctrl+C"
            );
            true
        }
    }
}

/// Ctrl+C ×N 전송 후 전경이 baseline 에서 이탈할 때까지 확인. 실패 시 `false` —
/// 살아있는 claude TUI 입력창에 resume 명령이 타이핑되는 사고 방지.
fn kill_claude_via_ctrlc(host: &HostHandle, surface_id: u32, baseline: &str) -> bool {
    for _ in 0..CTRL_C_COUNT {
        if let Err(e) = host.call(
            "surface.send_combo",
            json!({ "surface_id": surface_id, "key": "c", "modifiers": ["ctrl"] }),
        ) {
            tracing::warn!("claude reboot s{surface_id}: send_combo failed: {e} — aborting");
            return false;
        }
        thread::sleep(CTRL_C_INTERVAL);
    }
    // 종료 확인: 전경이 baseline 에서 이탈할 때까지. 실패 시 절대 진행 금지 —
    // 살아있는 claude TUI 입력창에 resume 명령이 타이핑되는 사고 방지.
    if !poll_foreground(host, surface_id, EXIT_WAIT, |name| name != baseline) {
        tracing::warn!(
            "claude reboot s{surface_id}: claude still in foreground after {CTRL_C_COUNT}x Ctrl+C — aborting (nothing sent)"
        );
        return false;
    }
    true
}

/// resume 명령 전송 + 전경이 baseline(claude 계열 이름)으로 돌아올 때까지 확인.
/// 미복귀면 `false` — 안내 프롬프트도 보내지 않는다(셸 프롬프트에 평문이 명령으로
/// 실행되는 사고 방지).
fn resume_and_wait(host: &HostHandle, surface_id: u32, baseline: &str, session_id: &str) -> bool {
    if let Err(e) = host.call(
        "surface.send",
        json!({ "surface_id": surface_id, "text": resume_command(session_id) }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: resume send failed: {e}");
        return false;
    }

    if !poll_foreground(host, surface_id, RETURN_WAIT, |name| name == baseline) {
        tracing::warn!(
            "claude reboot s{surface_id}: claude did not return to foreground within {}s — resume sent but notice skipped",
            RETURN_WAIT.as_secs()
        );
        return false;
    }
    true
}

/// 안내 프롬프트를 제출하고 화면에 실제로 나타났는지 검증한다. TUI 초기화 중
/// PTY 입력이 유실될 수 있어(실측: 복귀 직후 tell 이 소리 없이 사라짐) 확인될
/// 때까지 재시도한다. 매 시도 전 전경이 여전히 claude(baseline)인지 재확인 —
/// resume 이 실패해 셸로 떨어진 경우 평문이 셸 명령으로 실행되는 사고 방지.
fn deliver_notice(host: &HostHandle, surface_id: u32, baseline: &str, notice: &str) -> bool {
    for attempt in 1..=NOTICE_ATTEMPTS {
        match try_deliver_notice_once(host, surface_id, baseline, notice) {
            NoticeAttempt::Confirmed => return true,
            NoticeAttempt::Aborted => return false,
            NoticeAttempt::NotYetVisible => {
                tracing::info!(
                    "claude reboot s{surface_id}: notice attempt {attempt}/{NOTICE_ATTEMPTS} not visible yet — retrying"
                );
                thread::sleep(NOTICE_RETRY_INTERVAL);
            }
        }
    }
    // 마지막 시도 직후 verify 가 아슬하게 놓쳤을 수 있으니 한 번 더 확인.
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id);
        return true;
    }
    false
}

/// 안내 프롬프트 제출 1회 시도 결과.
enum NoticeAttempt {
    /// 화면에서 확인, 제출까지 완료.
    Confirmed,
    /// 전경 변경/tell 실패 — 시퀀스 전체를 중단해야 함.
    Aborted,
    /// 제출은 했으나 아직 화면에 안 보임 — 재시도 대상.
    NotYetVisible,
}

/// 전경이 여전히 baseline(claude) 인지 확인 후 안내 프롬프트를 `terminal.tell` 로
/// 제출하고 화면에 나타났는지 검사한다.
fn try_deliver_notice_once(
    host: &HostHandle,
    surface_id: u32,
    baseline: &str,
    notice: &str,
) -> NoticeAttempt {
    match query_foreground(host, surface_id) {
        Some(name) if name == baseline => {}
        other => {
            tracing::warn!(
                "claude reboot s{surface_id}: foreground changed to {other:?} before notice — aborting"
            );
            return NoticeAttempt::Aborted;
        }
    }
    if let Err(e) = host.call(
        "terminal.tell",
        json!({ "surface": surface_id, "text": notice }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: notice tell failed: {e}");
        return NoticeAttempt::Aborted;
    }
    thread::sleep(NOTICE_VERIFY_DELAY);
    if screen_contains(host, surface_id, NOTICE_SNIPPET) {
        ensure_submitted(host, surface_id);
        NoticeAttempt::Confirmed
    } else {
        NoticeAttempt::NotYetVisible
    }
}

/// 문구가 화면에 있어도 제출(`\r`)이 paste 로 흡수돼 입력창에 잔류할 수 있으므로
/// 별도 Enter 를 한 번 더 보낸다. 이미 제출된 상태면 빈 입력창 Enter 라 no-op.
fn ensure_submitted(host: &HostHandle, surface_id: u32) {
    thread::sleep(NOTICE_SUBMIT_DELAY);
    if let Err(e) = host.call(
        "surface.send_key",
        json!({ "surface_id": surface_id, "key": "enter" }),
    ) {
        tracing::warn!("claude reboot s{surface_id}: extra submit enter failed: {e}");
    }
}

/// `surface.screen_text` 로 현재 화면에 문구가 보이는지 확인. 실패 → false.
fn screen_contains(host: &HostHandle, surface_id: u32, needle: &str) -> bool {
    host.call("surface.screen_text", json!({ "surface_id": surface_id }))
        .ok()
        .and_then(|r| {
            r.get("text")
                .and_then(|t| t.as_str())
                .map(|t| t.contains(needle))
        })
        .unwrap_or(false)
}

/// `surface.foreground_process` 1회 조회. 실패/이름 없음 → None.
fn query_foreground(host: &HostHandle, surface_id: u32) -> Option<String> {
    let resp = host
        .call(
            "surface.foreground_process",
            json!({ "surface_id": surface_id }),
        )
        .ok()?;
    resp.get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 전경 이름이 조건을 만족할 때까지 폴링. 조회 실패(surface 소멸)는 즉시 false.
fn poll_foreground(
    host: &HostHandle,
    surface_id: u32,
    timeout: Duration,
    pred: impl Fn(&str) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match query_foreground(host, surface_id) {
            Some(name) if pred(&name) => return true,
            Some(_) => {}
            None => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(FG_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_delay_5_and_no_prompt() {
        let (delay, extra) = parse_reboot_options(&json!({ "surface_id": 1 }));
        assert_eq!(delay, 5);
        assert_eq!(extra, None);
    }

    #[test]
    fn parse_explicit_delay_and_prompt() {
        let (delay, extra) =
            parse_reboot_options(&json!({ "delay": 2, "prompt": "빌드부터 다시 확인" }));
        assert_eq!(delay, 2);
        assert_eq!(extra.as_deref(), Some("빌드부터 다시 확인"));
    }

    #[test]
    fn parse_empty_prompt_treated_as_none() {
        let (_, extra) = parse_reboot_options(&json!({ "prompt": "" }));
        assert_eq!(extra, None);
    }

    #[test]
    fn safe_session_id_accepts_uuid() {
        assert!(is_safe_session_id("0e5cbdf4-32a1-4a5c-9c1d-8f2b3a4c5d6e"));
        assert!(is_safe_session_id("abc_DEF-123"));
    }

    #[test]
    fn safe_session_id_rejects_shell_metachars() {
        assert!(!is_safe_session_id(""));
        assert!(!is_safe_session_id("abc; rm -rf /"));
        assert!(!is_safe_session_id("a b"));
        assert!(!is_safe_session_id("a$(x)"));
        assert!(!is_safe_session_id("a&b"));
    }

    #[test]
    fn resume_command_is_plain_and_submits() {
        assert_eq!(resume_command("0e5cbdf4-32a1"), "claude -r 0e5cbdf4-32a1\r");
    }

    #[test]
    fn notice_without_extra_is_fixed_text() {
        assert_eq!(build_notice(None), REBOOT_NOTICE);
    }

    #[test]
    fn notice_with_extra_appends_after_blank_line() {
        let n = build_notice(Some("이어서 soak 돌려"));
        assert!(n.starts_with(REBOOT_NOTICE));
        assert!(n.ends_with("\n\n이어서 soak 돌려"));
    }

    #[test]
    fn after_delay_same_foreground_sends_ctrl_c() {
        assert_eq!(after_delay_action("node", "node"), AfterDelay::SendCtrlC);
    }

    #[test]
    fn after_delay_changed_foreground_skips_to_resume() {
        assert_eq!(
            after_delay_action("cmd.exe", "node"),
            AfterDelay::SkipToResume
        );
    }
}
