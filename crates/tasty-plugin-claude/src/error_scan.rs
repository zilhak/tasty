//! Claude 터미널 출력의 오류 패턴과 오래 변하지 않는 출력을 감시한다.
//! surface.read_since_scan_mark의 전용 커서로 새 텍스트를 받아 매칭 창에 누적한다.
//! 에이전트가 쓰는 mark와 별개이므로 에이전트의 조회가 이 커서를 움직이지 않는다.
//!
//! launch·spawn·respawn에서 등록하고 폴링 중 대상 상태를 확인한다.
//! 자식은 surface 존재와 별도로 부모 관계를 확인해 release된 대상도 정리한다.
//! TopLevel 조회 오류는 대상 부재 오류까지 추적 유지로 처리하는 한계가 있다.
//! 새 턴에서는 중복 알림 기록과 정지 관측을 초기화하되 알림 간격 제한은 유지한다.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::json;

use tasty_plugin_agent_common::host_call::HostCall;

/// 출력에서 찾는 오류 패턴.
const CLAUDE_ERROR_PATTERN: &str = r"(?i)(\bAPI Error\b|Output blocked by content filtering policy|\boverloaded_error\b|\brate_limit_error\b|\bInternal Server Error\b|\bnetwork error\b|\bBad Request\b)";

/// 누적 매칭 창의 바이트 상한. 호스트 OUTPUT_RETENTION_MAX_BYTES와 같은 값을 따로 선언한다.
/// 두 값이 달라지면 보관하는 출력 범위도 달라진다
/// (docs/features/terminal-output/index.md#출력-스캐너-전용-커서).
const SCAN_WINDOW_MAX: usize = 1_048_576;

/// 중복 알림 비교에 사용할 매칭 창 앞부분의 문자 수.
const DEDUPE_SNIPPET_CHARS: usize = 200;

static CLAUDE_ERROR_REGEX: OnceLock<Regex> = OnceLock::new();

fn claude_error_regex() -> &'static Regex {
    CLAUDE_ERROR_REGEX.get_or_init(|| {
        Regex::new(CLAUDE_ERROR_PATTERN).expect("ClaudeError catalog regex must compile")
    })
}

/// 오류를 본 뒤 출력 변화가 없을 때 정지 알림까지 기다리는 시간.
/// 재시도의 일시적인 대기를 정지로 오인하지 않도록 지연을 둔다.
const STALL_QUIET: Duration = Duration::from_secs(30);

/// 오류 없이 조용한 경우의 대기 시간. 긴 추론과 정지를 구분할 증거가 적어 더 오래 기다린다.
/// 호스트 CHILD_OUTPUT_SILENCE와 같은 값을 별도로 선언한다.
const STALL_QUIET_NO_ERROR: Duration = Duration::from_secs(120);

/// 같은 surface에 정지 알림을 다시 보낼 때 지킬 최소 간격.
const STALL_NOTIFY_COOLDOWN: Duration = Duration::from_secs(300);

/// 정지 의심 알림용 이벤트. 매니페스트의 hook_events와 이름이 같아야 한다.
/// 오류 없는 정지도 알리지만 기존 구독과 호환되도록 이름은 유지한다.
pub(crate) const STALLED_EVENT: &str = "claude-error-stalled";

/// `text`(ANSI-stripped 권장)에 알려진 Claude 에러 패턴이 포함됐는지.
pub(crate) fn detect_claude_error(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    claude_error_regex().is_match(text)
}

/// `text` 에서 에러 패턴이 매치된 **첫 줄** 전체를 돌려준다 — 알림에 붙일 힌트용.
/// 매칭 규칙 자체는 [`detect_claude_error`] 와 같은 정규식을 그대로 쓴다.
pub(crate) fn first_error_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && claude_error_regex().is_match(line))
}

/// 독립 surface는 surface.locate, 자식은 terminal.parent로 추적 유지 여부를 판단한다.
/// release는 surface를 남기고 부모 관계만 해제하므로 자식은 존재 여부만 보면 안 된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTarget {
    TopLevel,
    Child,
}

/// 스캐너의 모든 잠금에서 poison을 알리고 기존 상태를 재사용한다.
/// 복구가 패닉 전의 부분 변경을 되돌리는 것은 아니다.
pub(crate) fn lock_scanner(
    scanner: &std::sync::Mutex<ErrorScanner>,
) -> std::sync::MutexGuard<'_, ErrorScanner> {
    const WHAT: &str = "the claude error scanner";
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    tasty_utils::poison::recover_mutex(scanner.lock(), WHAT, &REPORTED)
}

#[derive(Default)]
pub struct ErrorScanner {
    /// 추적할 surface와 등록 경로. launch·spawn·respawn에서 넣고 kill·폴링 정리에서 뺀다.
    enabled: HashMap<u32, ScanTarget>,
    /// surface별로 마지막 알림에 사용한 창의 앞부분. 같은 값이면 중복 알림을 생략한다.
    last_fired: HashMap<u32, String>,
    /// 출력 변화가 멈춘 기간을 추정할 관측값.
    watch: HashMap<u32, OutputWatch>,
    /// 전용 커서로 받은 텍스트를 누적한 매칭 창. SCAN_WINDOW_MAX를 넘으면 앞에서 자른다.
    window: HashMap<u32, String>,
    /// surface별 마지막 정지 알림 시각.
    last_stall_notify: HashMap<u32, Instant>,
}

/// 누적 창의 해시 변화로 출력 변화를 추정한다. 실제 작업 진행 여부를 확정하지는 않는다.
struct OutputWatch {
    /// 마지막으로 본 텍스트의 해시.
    fingerprint: u64,
    /// 지문이 마지막으로 바뀐 시각.
    last_change: Instant,
    /// 현재 조용한 구간에서 알림을 보냈는지. 창의 해시가 바뀌면 초기화한다.
    stall_notified: bool,
    /// 마지막 창에 오류가 있었는지. 정지 알림까지 기다릴 시간을 고른다.
    saw_error: bool,
}

fn output_fingerprint(text: &str) -> u64 {
    // 앞부분이 같아도 뒤에 붙은 출력은 감지해야 하므로 창 전체의 해시를 쓴다.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// UTF-8 문자 경계를 지키며 앞에서 잘라 바이트 상한 이하로 줄인다.
fn trim_window(window: &mut String) {
    if window.len() <= SCAN_WINDOW_MAX {
        return;
    }
    let want = window.len() - SCAN_WINDOW_MAX;
    let cut = (want..=window.len())
        .find(|i| window.is_char_boundary(*i))
        .unwrap_or(window.len());
    window.drain(..cut);
}

/// 오류 유무에 따라 정지 알림까지 기다릴 시간을 고른다.
fn stall_threshold(saw_error: bool) -> Duration {
    if saw_error {
        STALL_QUIET
    } else {
        STALL_QUIET_NO_ERROR
    }
}

/// 상태 조회(IPC) 전에 값싸게 거를 수 있는 조건 — 무출력 지속시간 · 중복 · 쿨다운.
fn stall_pre_gate(
    quiet: Duration,
    already_notified: bool,
    since_last_notify: Option<Duration>,
    threshold: Duration,
) -> bool {
    if already_notified || quiet < threshold {
        return false;
    }
    !matches!(since_last_notify, Some(d) if d < STALL_NOTIFY_COOLDOWN)
}

/// active·stale에서만 정지 의심 알림을 허용한다. 두 상태가 실제 정지를 확정하는 것은 아니다.
/// idle·needs_input·exited는 기존 완료 알림 대상이라 여기서 다시 알리지 않는다.
fn state_allows_stall_notice(child_state: &str) -> bool {
    matches!(child_state, "active" | "stale")
}

/// 정지 알림을 보낼지의 최종 판정(순수 함수).
fn should_notify_stall(
    quiet: Duration,
    child_state: &str,
    already_notified: bool,
    since_last_notify: Option<Duration>,
    threshold: Duration,
) -> bool {
    stall_pre_gate(quiet, already_notified, since_last_notify, threshold)
        && state_allows_stall_notice(child_state)
}

impl ErrorScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// 추적 대상을 등록한다. 같은 surface는 등록 경로만 바꾸고 중복 알림 기록은 유지한다.
    pub fn enable(&mut self, surface_id: u32, target: ScanTarget) {
        self.enabled.insert(surface_id, target);
    }

    pub fn disable(&mut self, surface_id: u32) {
        self.enabled.remove(&surface_id);
        self.last_fired.remove(&surface_id);
        self.watch.remove(&surface_id);
        self.last_stall_notify.remove(&surface_id);
        self.window.remove(&surface_id);
    }

    pub fn is_enabled(&self, surface_id: u32) -> bool {
        self.enabled.contains_key(&surface_id)
    }

    /// 테스트 전용 — 등록 경로 확인.
    #[cfg(test)]
    pub(crate) fn target_of(&self, surface_id: u32) -> Option<ScanTarget> {
        self.enabled.get(&surface_id).copied()
    }

    /// 새 턴을 위해 중복 알림 기록과 정지 관측을 초기화한다.
    /// 턴마다 오류가 반복돼도 알림이 너무 잦아지지 않도록 마지막 알림 시각은 유지한다.
    pub fn reset_dedupe(&mut self, surface_id: u32) {
        self.last_fired.remove(&surface_id);
        self.watch.remove(&surface_id);
        // 호스트 출력 버퍼가 턴마다 비워지지 않으므로 누적 매칭 창도 유지한다.
    }

    /// enabled set의 snapshot. polling thread가 lock을 짧게 잡고 빠져나오도록.
    pub fn enabled_snapshot(&self) -> Vec<(u32, ScanTarget)> {
        self.enabled.iter().map(|(&sid, &t)| (sid, t)).collect()
    }

    /// 호스트 IPC 없이 중복 알림 상태를 준비하는 시험용 함수.
    #[cfg(test)]
    pub(crate) fn seed_dedupe_for_test(&mut self, surface_id: u32, snippet: &str) {
        self.last_fired.insert(surface_id, snippet.to_string());
    }

    /// 테스트 전용 — dedupe 상태 존재 여부.
    #[cfg(test)]
    pub(crate) fn has_dedupe_state(&self, surface_id: u32) -> bool {
        self.last_fired.contains_key(&surface_id)
    }

    /// 새 출력을 누적해 오류를 찾고 정지 의심 여부도 확인한다.
    /// 오류 훅 요청이 성공하면 스니펫을 반환한다. 수신 훅이 실제 실행됐다는 보장은 아니다.
    pub fn scan_one<H: HostCall>(&mut self, host: &H, surface_id: u32) -> Option<String> {
        self.scan_one_at(host, surface_id, Instant::now())
    }

    /// 실제로 기다리지 않고 출력 정지 기간을 시험하도록 시각을 전달받는다.
    fn scan_one_at<H: HostCall>(
        &mut self,
        host: &H,
        surface_id: u32,
        now: Instant,
    ) -> Option<String> {
        let resp = host
            .call(
                "surface.read_since_scan_mark",
                json!({
                    "surface_id": surface_id,
                    "strip_ansi": true,
                }),
            )
            .ok()?;
        // 새로 받은 텍스트만으로는 분할된 패턴을 놓칠 수 있어 창에 누적한다.
        let delta = resp.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let window = self.window.entry(surface_id).or_default();
        window.push_str(delta);
        trim_window(window);
        let has_error = detect_claude_error(window);
        let fingerprint = output_fingerprint(window);
        let snippet: String = window.chars().take(DEDUPE_SNIPPET_CHARS).collect();
        // 오류를 발견하기 전부터 창의 변화를 추적한다.
        self.track_output(surface_id, fingerprint, now, has_error);

        let mut fired = None;
        if has_error {
            // 창의 앞부분이 마지막 알림과 같으면 오류 알림을 반복하지 않는다.
            let already_fired = self.last_fired.get(&surface_id) == Some(&snippet);
            if !already_fired {
                let fire_result = host.call(
                    "surface.fire_hook",
                    json!({
                        "surface_id": surface_id,
                        "event": "claude-error",
                    }),
                );
                match fire_result {
                    Ok(_) => {
                        self.last_fired.insert(surface_id, snippet.clone());
                        fired = Some(snippet);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "claude error fire_hook failed for surface {surface_id}: {e}"
                        );
                    }
                }
            }
        }

        // 오류가 없거나 중복 알림을 생략한 경우에도 정지 의심 여부는 확인한다.
        self.maybe_notify_stall(host, surface_id, now);
        fired
    }

    /// 창의 해시가 바뀌면 조용한 구간을 다시 센다. 같은 창을 다시 해시하지 않도록 값으로 받는다.
    fn track_output(&mut self, surface_id: u32, fingerprint: u64, now: Instant, has_error: bool) {
        match self.watch.get_mut(&surface_id) {
            Some(w) if w.fingerprint == fingerprint => {}
            Some(w) => {
                w.fingerprint = fingerprint;
                w.last_change = now;
                w.stall_notified = false;
                w.saw_error = has_error;
            }
            None => {
                self.watch.insert(
                    surface_id,
                    OutputWatch {
                        fingerprint,
                        last_change: now,
                        stall_notified: false,
                        saw_error: has_error,
                    },
                );
            }
        }
    }

    /// 출력 변화가 오래 없고 자식이 active·stale이면 정지 의심 훅을 요청한다.
    /// terminal.set_state는 호출하지 않아 자식 상태를 바꾸지는 않는다.
    fn maybe_notify_stall<H: HostCall>(&mut self, host: &H, surface_id: u32, now: Instant) {
        let Some(w) = self.watch.get(&surface_id) else {
            return;
        };
        let quiet = now.saturating_duration_since(w.last_change);
        let already_notified = w.stall_notified;
        let threshold = stall_threshold(w.saw_error);
        let since_last_notify = self
            .last_stall_notify
            .get(&surface_id)
            .map(|t| now.saturating_duration_since(*t));
        if !stall_pre_gate(quiet, already_notified, since_last_notify, threshold) {
            return;
        }

        // 시간·중복·알림 간격 조건을 통과했을 때만 호스트 상태를 조회한다.
        let child_state = host
            .call("terminal.state", json!({ "surface": surface_id }))
            .ok()
            .and_then(|r| {
                r.get("state")
                    .and_then(|v| v.as_str())
                    .map(str::to_ascii_lowercase)
            })
            .unwrap_or_default();
        if !should_notify_stall(
            quiet,
            &child_state,
            already_notified,
            since_last_notify,
            threshold,
        ) {
            return;
        }

        if let Err(e) = host.call(
            "surface.fire_hook",
            json!({
                "surface_id": surface_id,
                "event": STALLED_EVENT,
            }),
        ) {
            tracing::warn!("claude stall fire_hook failed for surface {surface_id}: {e}");
            return;
        }
        if let Some(w) = self.watch.get_mut(&surface_id) {
            w.stall_notified = true;
        }
        self.last_stall_notify.insert(surface_id, now);
    }
}

/// 폴링 중 추적 유지 여부를 확인한다. 조회 오류는 추적을 유지하도록 true로 처리한다.
/// TopLevel은 대상 부재를 오류로 돌려받아도 유지한다. Child는 정상 응답의 status=none이면 제거한다.
pub(crate) fn scan_target_is_alive<H: HostCall>(
    host: &H,
    surface_id: u32,
    target: ScanTarget,
) -> bool {
    match target {
        ScanTarget::TopLevel => host
            .call("surface.locate", json!({ "surface_id": surface_id }))
            .ok()
            .and_then(|r| r.get("exists").and_then(|v| v.as_bool()))
            .unwrap_or(true),
        // surface를 남기는 release도 감지하도록 부모 관계를 확인한다.
        ScanTarget::Child => host
            .call("terminal.parent", json!({ "surface": surface_id }))
            .ok()
            .map(|r| r.get("status").and_then(|v| v.as_str()) != Some("none"))
            .unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_api_error() {
        assert!(detect_claude_error("…\nAPI Error: Connection lost\n"));
    }

    #[test]
    fn detects_content_filter_block() {
        assert!(detect_claude_error(
            "Output blocked by content filtering policy"
        ));
    }

    #[test]
    fn detects_overloaded_and_rate_limit() {
        assert!(detect_claude_error("{\"type\":\"overloaded_error\"}"));
        assert!(detect_claude_error("got rate_limit_error from upstream"));
    }

    #[test]
    fn detects_case_insensitive() {
        assert!(detect_claude_error("api error: foo"));
    }

    #[test]
    fn detects_internal_server_error_and_network() {
        assert!(detect_claude_error("HTTP 500 Internal Server Error\n"));
        assert!(detect_claude_error("encountered network error: ECONNRESET"));
    }

    #[test]
    fn detects_bad_request() {
        assert!(detect_claude_error("400 Bad Request — model not allowed"));
    }

    #[test]
    fn ignores_unrelated_text() {
        assert!(!detect_claude_error("compile error fixed"));
        assert!(!detect_claude_error("no errors here"));
        assert!(!detect_claude_error(""));
    }

    #[test]
    fn enable_disable_tracking() {
        let mut s = ErrorScanner::new();
        assert!(!s.is_enabled(7));
        s.enable(7, ScanTarget::TopLevel);
        assert!(s.is_enabled(7));
        assert_eq!(s.enabled_snapshot(), vec![(7, ScanTarget::TopLevel)]);
        s.disable(7);
        assert!(!s.is_enabled(7));
    }

    #[test]
    fn enable_records_target_kind() {
        let mut s = ErrorScanner::new();
        s.enable(1, ScanTarget::TopLevel);
        s.enable(2, ScanTarget::Child);
        assert_eq!(s.target_of(1), Some(ScanTarget::TopLevel));
        assert_eq!(s.target_of(2), Some(ScanTarget::Child));
        assert_eq!(s.target_of(3), None);
    }

    #[test]
    fn re_enabling_same_surface_is_idempotent_and_keeps_dedupe() {
        // 같은 surface를 재등록해도 중복 항목을 만들지 않는다.
        let mut s = ErrorScanner::new();
        s.enable(5, ScanTarget::Child);
        s.seed_dedupe_for_test(5, "API Error: foo");
        s.enable(5, ScanTarget::Child);
        assert_eq!(s.enabled_snapshot(), vec![(5, ScanTarget::Child)]);
        assert!(s.has_dedupe_state(5));
    }

    #[test]
    fn disable_clears_dedupe_state() {
        // 같은 surface가 disable 후 다시 enable 되면 이전 fire 기록은
        // 사라져야 한다 (새 child 인스턴스로 가정).
        let mut s = ErrorScanner::new();
        s.enable(3, ScanTarget::Child);
        s.last_fired
            .insert(3, "API Error: foo".chars().take(200).collect());
        assert!(s.last_fired.contains_key(&3));
        s.disable(3);
        assert!(!s.last_fired.contains_key(&3));
        assert!(!s.has_dedupe_state(3));
    }

    #[test]
    fn reset_dedupe_clears_only_target_surface() {
        let mut s = ErrorScanner::new();
        s.last_fired.insert(1, "a".into());
        s.last_fired.insert(2, "b".into());
        s.reset_dedupe(1);
        assert!(!s.last_fired.contains_key(&1));
        assert!(s.last_fired.contains_key(&2));
    }

    #[test]
    fn stall_needs_sustained_silence() {
        // 정적이 짧으면 아직 판정하지 않는다 — 재시도 백오프 중일 수 있다.
        assert!(!should_notify_stall(
            STALL_QUIET - Duration::from_secs(1),
            "active",
            false,
            None,
            STALL_QUIET
        ));
        assert!(should_notify_stall(
            STALL_QUIET,
            "active",
            false,
            None,
            STALL_QUIET
        ));
    }

    #[test]
    fn stall_only_when_the_turn_has_not_ended() {
        // 기존 완료 알림 대상인 상태에서는 정지 알림을 추가하지 않는다.
        for state in ["idle", "needs_input", "exited", ""] {
            assert!(
                !should_notify_stall(STALL_QUIET * 2, state, false, None, STALL_QUIET),
                "state={state} 에서는 정지 알림이 나가면 안 된다"
            );
        }
        assert!(should_notify_stall(
            STALL_QUIET * 2,
            "active",
            false,
            None,
            STALL_QUIET
        ));
        // stale도 정지 의심 알림 대상이다.
        assert!(should_notify_stall(
            STALL_QUIET * 2,
            "stale",
            false,
            None,
            STALL_QUIET
        ));
    }

    #[test]
    fn stall_notifies_once_per_silence_and_respects_cooldown() {
        // 같은 정적 구간에서 반복 알림 금지.
        assert!(!should_notify_stall(
            STALL_QUIET * 3,
            "active",
            true,
            None,
            STALL_QUIET
        ));
        // 쿨다운 안이면 새 정적 구간이라도 억제 — 에러 반복 세션의 빈도 상한.
        assert!(!should_notify_stall(
            STALL_QUIET * 3,
            "active",
            false,
            Some(STALL_NOTIFY_COOLDOWN - Duration::from_secs(1)),
            STALL_QUIET
        ));
        assert!(should_notify_stall(
            STALL_QUIET * 3,
            "active",
            false,
            Some(STALL_NOTIFY_COOLDOWN),
            STALL_QUIET
        ));
    }

    #[test]
    fn the_threshold_depends_on_whether_an_error_is_on_screen() {
        assert_eq!(stall_threshold(true), STALL_QUIET);
        assert_eq!(stall_threshold(false), STALL_QUIET_NO_ERROR);
        assert!(
            STALL_QUIET_NO_ERROR > STALL_QUIET,
            "보강 증거가 없으면 더 길게 본다"
        );
        // 에러 없는 정적은 짧은 문턱으로는 정지가 아니다.
        assert!(!should_notify_stall(
            STALL_QUIET,
            "active",
            false,
            None,
            STALL_QUIET_NO_ERROR
        ));
        assert!(should_notify_stall(
            STALL_QUIET_NO_ERROR,
            "active",
            false,
            None,
            STALL_QUIET_NO_ERROR
        ));
    }

    #[test]
    fn output_fingerprint_tracks_appended_text() {
        // 창 앞부분이 같아도 뒤의 출력이 늘면 해시가 달라져야 한다.
        let head = "x".repeat(300);
        let grown = format!("{head}retrying…");
        assert_eq!(
            head.chars().take(200).collect::<String>(),
            grown.chars().take(200).collect::<String>(),
            "스니펫은 같다(전제)"
        );
        assert_ne!(output_fingerprint(&head), output_fingerprint(&grown));
    }

    #[test]
    fn first_error_line_extracts_matching_line() {
        let text = "building…\n  API Error: connection reset by peer\nnext line\n";
        assert_eq!(
            first_error_line(text),
            Some("API Error: connection reset by peer")
        );
        assert_eq!(first_error_line("all good\n"), None);
    }

    /// 새 출력·자식 상태를 제공하고 훅 요청을 기록하는 stub.
    /// set_text로 지정한 텍스트는 한 번만 읽히며 이후에는 빈 문자열을 반환한다.
    struct ScanHost {
        text: std::cell::RefCell<String>,
        state: std::cell::RefCell<&'static str>,
        fired: std::cell::RefCell<Vec<String>>,
    }

    impl ScanHost {
        fn new(text: &str, state: &'static str) -> Self {
            Self {
                text: std::cell::RefCell::new(text.to_string()),
                state: std::cell::RefCell::new(state),
                fired: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn set_text(&self, text: &str) {
            *self.text.borrow_mut() = text.to_string();
        }
        fn events(&self) -> Vec<String> {
            self.fired.borrow().clone()
        }
        fn stalled_count(&self) -> usize {
            self.events().iter().filter(|e| *e == STALLED_EVENT).count()
        }
    }

    impl HostCall for ScanHost {
        fn call(
            &self,
            method: &str,
            params: serde_json::Value,
        ) -> Result<serde_json::Value, tasty_plugin_sdk::PluginError> {
            match method {
                "surface.read_since_scan_mark" => {
                    let delta = std::mem::take(&mut *self.text.borrow_mut());
                    Ok(json!({ "text": delta }))
                }
                "terminal.state" => Ok(json!({ "state": *self.state.borrow() })),
                "surface.fire_hook" => {
                    let event = params["event"].as_str().unwrap_or_default().to_string();
                    self.fired.borrow_mut().push(event);
                    Ok(json!({ "fired": 1 }))
                }
                other => panic!("unexpected host call: {other}"),
            }
        }
    }

    const ERR: &str = "⎿ API Error: Connection error\n";

    #[test]
    fn transient_error_that_keeps_producing_output_never_stalls() {
        // 반복해서 바뀌는 출력은 조용한 구간으로 판정하지 않는다.
        let host = ScanHost::new(ERR, "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        for i in 1..=6u32 {
            host.set_text(&format!("{ERR}Retrying in {i}s… (attempt {i}/10)"));
            s.scan_one_at(&host, 1, t0 + Duration::from_secs(10 * u64::from(i)));
        }
        assert_eq!(host.stalled_count(), 0, "재시도 중에는 알림이 없어야 한다");
        // 오류 감지 이벤트와 부모에게 전달할 정지 알림은 별개다.
        assert!(
            host.events().iter().all(|e| e == "claude-error"),
            "정지 이벤트가 섞이면 안 된다: {:?}",
            host.events()
        );
    }

    #[test]
    fn error_followed_by_silence_fires_stalled_once() {
        let host = ScanHost::new(ERR, "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        assert_eq!(host.stalled_count(), 0, "에러 직후엔 아직 정지가 아니다");

        // 오류 중복 알림을 생략해도 정지 판정은 계속한다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET);
        assert_eq!(
            host.stalled_count(),
            1,
            "출력 변화가 없는 시간이 기준에 도달하면 한 번 알린다"
        );

        // 계속 조용해도 같은 정적 구간에서는 다시 알리지 않는다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET * 3);
        assert_eq!(
            host.stalled_count(),
            1,
            "같은 조용한 구간에서는 한 번만 알린다"
        );
    }

    #[test]
    fn silence_after_turn_ended_does_not_fire_stalled() {
        // idle은 정지 의심 알림에서 제외한다.
        let host = ScanHost::new(ERR, "idle");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET * 2);
        assert_eq!(host.stalled_count(), 0);
    }

    #[test]
    fn new_turn_resets_stall_watch() {
        let host = ScanHost::new(ERR, "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        // 새 턴 신호(prompt-submit 등)로 dedupe + 정지 관측이 초기화된다.
        s.reset_dedupe(1);
        assert!(!s.has_dedupe_state(1));
        // 이전 턴의 정적이 이월되지 않으므로 바로 다음 tick 에 정지로 판정되지 않는다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET);
        assert_eq!(host.stalled_count(), 0, "정적 구간은 새 턴부터 다시 잰다");
        s.scan_one_at(&host, 1, t0 + STALL_QUIET * 2);
        assert_eq!(host.stalled_count(), 1);
    }

    #[test]
    fn clean_output_never_fires_an_error_event() {
        // 오류 패턴이 없으면 claude-error는 보내지 않는다. 정지 의심 알림은 별개다.
        let host = ScanHost::new("running tests…\nall good\n", "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        // 문턱 이전까지는 아무것도 안 나간다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR - Duration::from_secs(1));
        assert!(
            host.events().is_empty(),
            "이벤트가 없어야 한다: {:?}",
            host.events()
        );
        // 문턱을 넘겨도 에러 이벤트는 없다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR * 5);
        assert!(
            !host.events().iter().any(|e| e == "claude-error"),
            "에러가 없는데 claude-error 가 나갔다: {:?}",
            host.events()
        );
    }

    /// 오류 없이 오래 조용한 경우에도 정지 의심 알림을 보낸다.
    #[test]
    fn silence_without_error_fires_stall() {
        let host = ScanHost::new("waiting for your approval…\n", "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        assert_eq!(host.stalled_count(), 0);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR);
        assert_eq!(host.stalled_count(), 1, "에러가 없어도 정적이 길면 알린다");
        assert!(
            host.events().iter().all(|e| e == STALLED_EVENT),
            "에러 이벤트가 섞이면 안 된다: {:?}",
            host.events()
        );
    }

    /// 호스트가 stale로 판정한 경우도 알린다.
    #[test]
    fn stale_state_fires_stall() {
        let host = ScanHost::new("…\n", "stale");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR);
        assert_eq!(host.stalled_count(), 1);
    }

    /// 기존 완료 알림 대상 상태에서는 정지 의심 알림을 보내지 않는다.
    #[test]
    fn idle_and_needs_input_still_suppressed() {
        for state in ["idle", "needs_input", "exited"] {
            let host = ScanHost::new("…\n", state);
            let mut s = ErrorScanner::new();
            let t0 = Instant::now();
            s.scan_one_at(&host, 1, t0);
            s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR * 2);
            assert_eq!(host.stalled_count(), 0, "state={state}");
        }
    }

    /// 출력이 재개되면 dedupe 가 풀리고 다음 정적 구간은 새 사건으로 센다.
    #[test]
    fn output_resumption_rearms_and_does_not_double_fire() {
        let host = ScanHost::new("thinking…\n", "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET_NO_ERROR * 2);
        assert_eq!(host.stalled_count(), 1, "같은 정적 구간에서는 1 회");

        // 출력 재개 → 다시 정적. 쿨다운을 넘긴 시점이라 두 번째 사건이 나간다.
        let t1 = t0 + STALL_NOTIFY_COOLDOWN + STALL_QUIET_NO_ERROR;
        host.set_text("thinking…\nstill here\n");
        s.scan_one_at(&host, 1, t1);
        assert_eq!(host.stalled_count(), 1, "재개 직후는 정적이 아니다");
        s.scan_one_at(&host, 1, t1 + STALL_QUIET_NO_ERROR);
        assert_eq!(host.stalled_count(), 2);
    }

    /// 두 번의 읽기에 나뉜 오류 문자열도 누적해서 찾아야 한다.
    #[test]
    fn an_error_split_across_two_polls_is_still_detected() {
        let host = ScanHost::new("\u{23bf} API Er", "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        assert!(
            host.events().is_empty(),
            "반쪽으로는 아직 매치가 아니다: {:?}",
            host.events()
        );

        host.set_text("ror: Connection error\n");
        s.scan_one_at(&host, 1, t0 + Duration::from_millis(800));
        assert_eq!(
            host.events(),
            vec!["claude-error".to_string()],
            "두 델타를 이어 붙이면 매치여야 한다"
        );
    }

    /// 창 상한과 자르는 자리. 바이트로 자르면 UTF-8 중간을 갈라 패닉한다.
    #[test]
    fn the_window_is_capped_and_cut_on_a_char_boundary() {
        // 세 바이트짜리 문자로만 채워 상한을 조금 넘긴다 — 잘라야 할 바이트 수가
        // char 경계에 안 떨어지는 배치다.
        let mut w = "\u{ac00}".repeat(SCAN_WINDOW_MAX / 3 + 10);
        assert!(
            w.len() > SCAN_WINDOW_MAX,
            "시험 입력이 바이트 상한을 넘지 않았다"
        );
        trim_window(&mut w);
        assert!(w.len() <= SCAN_WINDOW_MAX, "상한을 안 지켰다: {}", w.len());
        assert!(
            w.starts_with('\u{ac00}'),
            "char 경계가 아닌 자리에서 잘렸다"
        );
        assert!(
            w.len() + 3 > SCAN_WINDOW_MAX,
            "필요 이상으로 잘랐다: {}",
            w.len()
        );
    }

    /// 한 surface를 해제해도 다른 surface의 상태는 유지돼야 한다.
    #[test]
    fn disable_clears_stall_state() {
        let host = ScanHost::new(ERR, "active");
        let mut s = ErrorScanner::new();
        s.enable(1, ScanTarget::Child);
        s.enable(2, ScanTarget::Child);
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        s.scan_one_at(&host, 2, t0);
        s.disable(1);
        assert!(!s.watch.contains_key(&1));
        assert!(!s.last_stall_notify.contains_key(&1));
        assert!(!s.window.contains_key(&1));
        // 끄라고 하지 않은 surface 의 감시는 그대로다.
        assert!(s.watch.contains_key(&2));
        assert!(s.window.contains_key(&2));
        assert!(s.is_enabled(2));
    }

    struct StubHost {
        locate_exists: Option<bool>,
        parent_status: Option<&'static str>,
    }

    impl HostCall for StubHost {
        fn call(
            &self,
            method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, tasty_plugin_sdk::PluginError> {
            match method {
                "surface.locate" => match self.locate_exists {
                    Some(e) => Ok(json!({ "exists": e })),
                    None => Err(tasty_plugin_sdk::PluginError::HostCall {
                        method: method.to_string(),
                        message: "host down".into(),
                        code: None,
                    }),
                },
                "terminal.parent" => match self.parent_status {
                    Some("none") => Ok(json!({ "parent_surface_id": null, "status": "none" })),
                    Some(st) => Ok(json!({ "parent_surface_id": 1, "status": st })),
                    None => Err(tasty_plugin_sdk::PluginError::HostCall {
                        method: method.to_string(),
                        message: "host down".into(),
                        code: None,
                    }),
                },
                other => panic!("unexpected host call: {other}"),
            }
        }
    }

    #[test]
    fn top_level_liveness_uses_surface_locate() {
        let alive = StubHost {
            locate_exists: Some(true),
            parent_status: None,
        };
        let dead = StubHost {
            locate_exists: Some(false),
            parent_status: None,
        };
        assert!(scan_target_is_alive(&alive, 4, ScanTarget::TopLevel));
        assert!(!scan_target_is_alive(&dead, 4, ScanTarget::TopLevel));
    }

    #[test]
    fn child_liveness_uses_parent_relation_not_surface_existence() {
        // release 재현: surface 는 살아있지만(locate 는 아예 호출되지 않아야 한다)
        // 부모-자식 관계가 끊겼으므로 스캔 대상에서 빠져야 한다.
        let released = StubHost {
            locate_exists: None,
            parent_status: Some("none"),
        };
        assert!(!scan_target_is_alive(&released, 4, ScanTarget::Child));

        let attached = StubHost {
            locate_exists: None,
            parent_status: Some("active"),
        };
        assert!(scan_target_is_alive(&attached, 4, ScanTarget::Child));
    }

    #[test]
    fn liveness_lookup_failure_keeps_target_enabled() {
        // 일시적 IPC 오류로 감시를 영구히 끄지 않는다 (재활성화 경로가 없다).
        let broken = StubHost {
            locate_exists: None,
            parent_status: None,
        };
        assert!(scan_target_is_alive(&broken, 4, ScanTarget::TopLevel));
        assert!(scan_target_is_alive(&broken, 4, ScanTarget::Child));
    }

    /// 폴링 루프(`main.rs::error_scan_loop`)의 생존 대조 tick 을 재현한다.
    fn liveness_tick<H: HostCall>(scanner: &mut ErrorScanner, host: &H) {
        for (sid, target) in scanner.enabled_snapshot() {
            if !scan_target_is_alive(host, sid, target) {
                scanner.disable(sid);
            }
        }
    }

    #[test]
    fn spawned_child_survives_ticks_while_relation_holds() {
        let mut s = ErrorScanner::new();
        s.enable(42, ScanTarget::Child); // handle_spawn
        let attached = StubHost {
            locate_exists: None,
            parent_status: Some("active"),
        };
        liveness_tick(&mut s, &attached);
        assert!(s.is_enabled(42));
    }

    #[test]
    fn released_child_is_dropped_with_its_dedupe_state() {
        // release는 surface를 남기므로 부모 관계 해제를 기준으로 정리해야 한다.
        let mut s = ErrorScanner::new();
        s.enable(42, ScanTarget::Child);
        s.seed_dedupe_for_test(42, "API Error: boom");
        let released = StubHost {
            locate_exists: Some(true),
            parent_status: Some("none"),
        };
        liveness_tick(&mut s, &released);
        assert!(!s.is_enabled(42), "release 후에는 스캔 대상에서 빠져야 함");
        assert!(!s.has_dedupe_state(42), "dedupe 상태도 함께 정리돼야 함");
    }

    #[test]
    fn top_level_launch_surface_is_not_judged_by_child_relation() {
        // launch surface 는 child registry 에 없다 — 관계로 판정하면 즉시 꺼진다.
        let mut s = ErrorScanner::new();
        s.enable(9, ScanTarget::TopLevel);
        let host = StubHost {
            locate_exists: Some(true),
            parent_status: Some("none"),
        };
        liveness_tick(&mut s, &host);
        assert!(s.is_enabled(9));
    }
}

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::sync::Mutex;

    /// poison 상태에서도 등록·조회·해제가 가능하고 폴링 대상 목록을 유지해야 한다.
    #[test]
    fn a_poisoned_scanner_keeps_the_scan_loop_alive() {
        let scanner = Mutex::new(ErrorScanner::default());
        lock_scanner(&scanner).enable(7, ScanTarget::TopLevel);

        let panicked = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = scanner.lock().expect("not poisoned yet");
                    panic!("poison the error scanner");
                })
                .join()
        });
        assert!(panicked.is_err(), "the helper thread must have panicked");
        assert!(
            scanner.lock().is_err(),
            "the scanner lock must actually be poisoned now"
        );

        assert_eq!(
            lock_scanner(&scanner).enabled_snapshot().len(),
            1,
            "a poisoned scanner must still hand the polling loop its work list — giving up \
             here kills error scanning for the rest of the process"
        );
        assert!(
            lock_scanner(&scanner).is_enabled(7),
            "reads must survive the poison"
        );

        lock_scanner(&scanner).enable(9, ScanTarget::Child);
        assert!(
            lock_scanner(&scanner).is_enabled(9),
            "registration must land on a poisoned scanner, or that surface is never scanned"
        );
        lock_scanner(&scanner).disable(7);
        assert!(
            !lock_scanner(&scanner).is_enabled(7),
            "deregistration must land too, or a dead surface is scanned forever"
        );
    }
}
