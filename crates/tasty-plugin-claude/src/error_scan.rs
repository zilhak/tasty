//! Claude child PTY 에러 패턴 스캐너.
//!
//! 호스트에 있던 카탈로그 정규식을 그대로 옮겨 왔고, cutover 로 호스트 쪽은
//! 제거됐다 — 지금은 본 모듈이 단일 출처다.
//! 호스트는 main loop tick마다 in-memory terminal buffer를 직접 스캔했지만,
//! plugin은 호스트 메모리에 접근할 수 없으므로 IPC `surface.read_since_mark`로
//! 텍스트를 받아 매칭한다. 매치 시 `surface.fire_hook`으로 `claude-error`를
//! 발사한다.
//!
//! 호출자는 plugin의 background thread에서 [`ErrorScanner::scan_one`]을 일정
//! 간격으로 호출. 짧은 polling 간격(800ms 권장)으로 호스트 메모리 스캔과의
//! 정확도 차이를 좁힌다.
//!
//! `enable`은 `handlers.rs`의 `handle_launch`(top-level)·`handle_spawn`/
//! `handle_respawn`(자식)이 호출한다. `disable`은 `ef57061d`(2026-07-07)가 걷어낸
//! `on_surface_lifecycle`/`surface.closed` 구독을 되살리는 대신,
//! `main.rs::error_scan_loop`의 800ms 폴링 주기에 편승해 생존을 주기적으로
//! 대조하는 방식으로 대체했다 — 추가 구독 배선 없이 기존 poll 인프라만으로
//! "추적 대상이 사라지면 dedupe 상태 정리"를 만족한다. 생존 판정 기준은
//! [`ScanTarget`]에 따라 다르다(자식은 surface 존재가 아니라 **부모-자식 관계**
//! 존재로 판정 — `terminal.release`처럼 surface 를 남긴 채 관계만 끊는 경로가
//! 있기 때문). `reset_dedupe`는 `hook.rs`가 새 턴 시작(prompt-submit 등) hook
//! 이벤트에서 호출해, 이전 턴의 에러 텍스트로 눌린 dedupe 가 새 턴의 에러까지
//! 억제하지 않게 한다.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::json;

use tasty_plugin_agent_common::host_call::HostCall;

/// 호스트에 있던 `CLAUDE_ERROR_PATTERN` 을 직접 옮긴 것. cutover 로 호스트
/// 측은 제거됐고 이 상수가 단일 출처다.
const CLAUDE_ERROR_PATTERN: &str = r"(?i)(\bAPI Error\b|Output blocked by content filtering policy|\boverloaded_error\b|\brate_limit_error\b|\bInternal Server Error\b|\bnetwork error\b|\bBad Request\b)";

static CLAUDE_ERROR_REGEX: OnceLock<Regex> = OnceLock::new();

fn claude_error_regex() -> &'static Regex {
    CLAUDE_ERROR_REGEX.get_or_init(|| {
        Regex::new(CLAUDE_ERROR_PATTERN).expect("ClaudeError catalog regex must compile")
    })
}

/// 에러 매치 이후 "정지" 로 판정하기까지 요구하는 **무출력** 시간.
///
/// `claude-error` 자체는 재시도로 복구되는 일시적 에러(`overloaded_error` /
/// `rate_limit_error`)에도 뜨므로, 그대로 부모에게 알리면 노이즈가 된다. 재시도
/// 중에는 Claude Code 가 시도 횟수·백오프 카운트다운을 계속 그려 PTY 출력이
/// 흐르지만, 응답이 오지 않고 매달리면(TCP 블랙홀 등) 출력이 완전히 멈춘다 —
/// 그 정적이 "재시도 중" 과 "멈춤" 을 가르는 신호다. 값이 짧으면 긴 백오프를
/// 정지로 오인하고, 길면 진짜 정지 통보가 늦어진다.
const STALL_QUIET: Duration = Duration::from_secs(30);

/// 같은 surface 에 정지 알림을 다시 보내기까지의 최소 간격 — 에러가 반복되는
/// 세션에서 알림이 무한히 쌓이지 않게 하는 상한.
const STALL_NOTIFY_COOLDOWN: Duration = Duration::from_secs(300);

/// 정지 판정 시 발사하는 hook 이벤트 키. `claude-error`(패턴 매치 즉시)와 **다른**
/// 이벤트다 — 부모 알림은 노이즈를 걸러낸 이 쪽만 구독한다. 매니페스트
/// `[[contributes.hook_events]]` 에 같은 문자열이 선언돼 있어야 host 가 등록·발사를
/// 받아준다.
pub(crate) const STALLED_EVENT: &str = "claude-error-stalled";

/// `text`(ANSI-stripped 권장)에 알려진 Claude 에러 패턴이 포함됐는지.
pub fn detect_claude_error(text: &str) -> bool {
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

/// 스캔 대상이 어떤 경로로 등록됐는지 — **생존 판정 기준이 갈리기 때문에** 구분한다.
///
/// - [`ScanTarget::TopLevel`] (`claude.launch`): 호스트 child registry 에 등록되지
///   않는 독립 surface 다. 관계로 판정할 수 없으므로 `surface.locate` 로 surface
///   자체의 존재만 본다.
/// - [`ScanTarget::Child`] (`claude.spawn`/`claude.respawn`): `terminal.parent` 로
///   부모-자식 **관계**가 살아있는지 본다. surface 존재만 보면
///   `terminal.release`(관계·soft 점유만 해제하고 surface 는 남긴다 —
///   `docs/features/child-terminal/index.md`) 이후 영원히 정리되지 않고, 더 이상
///   자식이 아닌 사용자 터미널에 `claude-error` 를 계속 발화한다. 관계 조회는
///   호스트에서 `reconcile_child_terminals()` 를 먼저 돌리므로 죽은 surface 는
///   관계도 함께 사라져, 이 한 번의 호출이 close/kill 실패로 surface 가 살아남은
///   케이스까지 같이 정리한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTarget {
    TopLevel,
    Child,
}

/// `ErrorScanner` 락을 잡는 유일한 통로. 이 값을 잡는 자리가 세 모듈(`main` 폴링 루프 ·
/// `handlers` 등록/해제 · `hook` dedupe 초기화)에 흩어져 있어, 관측을 한 곳에 모은다.
///
/// **복구가 답인 이유**: 임계구역이 enabled 집합과 dedupe 맵 조작뿐이라 패닉이 지나가도
/// 남는 값이 성립한다. 그리고 이 락을 잡는 자리 중 하나는 plugin 의 IPC 핸들러 스레드다.
///
/// **조용히 건너뛰면 안 되는 이유는 자리마다 다르다** — 그래서 전부 이 통로를 지난다:
/// - 등록(`enable`)을 건너뛰면 그 surface 만 에러 스캔에서 통째로 빠진다.
/// - 해제(`disable`)를 건너뛰면 죽은 surface 가 영원히 스캔 대상으로 남는다.
/// - dedupe 초기화를 건너뛰면 지난 턴의 에러 텍스트가 이번 턴의 새 에러까지 억제한다.
/// - 스캔 자체(`scan_one`)를 건너뛰면 기능이 조용히 아무것도 안 한다.
///
/// poison 은 sticky 라 넷 다 일회성이 아니라 영구적이다.
pub fn lock_scanner(
    scanner: &std::sync::Mutex<ErrorScanner>,
) -> std::sync::MutexGuard<'_, ErrorScanner> {
    const WHAT: &str = "the claude error scanner";
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    tasty_utils::poison::recover_mutex(scanner.lock(), WHAT, &REPORTED)
}

#[derive(Default)]
pub struct ErrorScanner {
    /// 스캔 대상 surface → 등록 경로. launch/spawn/respawn 시 enable,
    /// kill 및 폴링 생존 대조 실패 시 disable.
    enabled: HashMap<u32, ScanTarget>,
    /// 마지막으로 `claude-error`를 발사한 surface (dedupe용).
    /// 같은 surface에서 연속으로 매치되어도 다음 surface state 변경 전까지는
    /// 다시 발사하지 않는다.
    last_fired: HashMap<u32, String>,
    /// 에러 이후의 출력 흐름 추적 — "재시도 중" 과 "멈춤" 을 가른다.
    watch: HashMap<u32, OutputWatch>,
    /// surface 별 마지막 정지 알림 시각 (쿨다운 상한).
    last_stall_notify: HashMap<u32, Instant>,
}

/// PTY 출력이 계속 흐르는지를 보는 관측치. 지문이 바뀌면 무엇이든 새로 그려졌다는
/// 뜻이라 "아직 움직이는 중" 으로 본다.
struct OutputWatch {
    /// 마지막으로 본 텍스트의 해시.
    fingerprint: u64,
    /// 지문이 마지막으로 바뀐 시각.
    last_change: Instant,
    /// 이번 정적 구간에서 정지 알림을 이미 보냈는지 — 출력이 다시 흐르면 해제된다.
    stall_notified: bool,
}

fn output_fingerprint(text: &str) -> u64 {
    // dedupe 스니펫(앞 200자)과 달리 **전체 텍스트**를 해싱해야 한다 — 뒤에 새
    // 출력이 붙어도 앞 200자는 그대로라, 스니펫으로는 "출력이 흐르는 중" 을 볼 수
    // 없다(재시도를 정지로 오판).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// 상태 조회(IPC) 전에 값싸게 거를 수 있는 조건 — 무출력 지속시간 · 중복 · 쿨다운.
fn stall_pre_gate(
    quiet: Duration,
    already_notified: bool,
    since_last_notify: Option<Duration>,
) -> bool {
    if already_notified || quiet < STALL_QUIET {
        return false;
    }
    !matches!(since_last_notify, Some(d) if d < STALL_NOTIFY_COOLDOWN)
}

/// 정지 알림을 보낼지의 최종 판정(순수 함수).
///
/// `child_state` 는 호스트가 보는 자식 상태(`terminal.state`)다. **`active` 일 때만**
/// 알린다:
///
/// - `idle`/`needs_input`/`exited` = 턴이 이미 끝났다 → 기존 완료 알림 경로
///   (`claude-idle`/`needs-input`/`process-exit` 형제 hook)가 부모에게 이미 알렸다.
///   여기서 또 알리면 같은 사건에 알림이 두 번 간다.
/// - `active` + 무출력 = 호스트는 작업 중이라고 보는데 실제로는 아무것도 진행되지
///   않는 상태. 부모가 무한정 기다리게 되는 유일한 조합이고, 턴이 끝나지 않으므로
///   Claude Code 의 `Stop` 훅도 구조적으로 오지 않는다.
fn should_notify_stall(
    quiet: Duration,
    child_state: &str,
    already_notified: bool,
    since_last_notify: Option<Duration>,
) -> bool {
    stall_pre_gate(quiet, already_notified, since_last_notify) && child_state == "active"
}

impl ErrorScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// 스캔 대상으로 등록한다. 이미 등록된 surface 를 다시 `enable` 해도 안전하다
    /// (`ScanTarget` 만 갱신, dedupe 상태는 보존) — respawn 처럼 같은 surface_id 를
    /// 유지한 채 재기동하는 경로가 중복 호출한다.
    pub fn enable(&mut self, surface_id: u32, target: ScanTarget) {
        self.enabled.insert(surface_id, target);
    }

    pub fn disable(&mut self, surface_id: u32) {
        self.enabled.remove(&surface_id);
        self.last_fired.remove(&surface_id);
        self.watch.remove(&surface_id);
        self.last_stall_notify.remove(&surface_id);
    }

    pub fn is_enabled(&self, surface_id: u32) -> bool {
        self.enabled.contains_key(&surface_id)
    }

    /// 테스트 전용 — 등록 경로 확인.
    #[cfg(test)]
    pub(crate) fn target_of(&self, surface_id: u32) -> Option<ScanTarget> {
        self.enabled.get(&surface_id).copied()
    }

    /// dedupe state를 초기화한다. user가 에러 상태를 해소했음을 알리기 위해
    /// 호출 — 예: idle 상태 해제, 새 prompt 시작 시. 새 턴이 시작됐다는 뜻이므로
    /// 정지 관측(무출력 구간)도 함께 리셋한다 — 이전 턴의 정적이 새 턴의 판정에
    /// 이월되면 안 된다. 쿨다운(`last_stall_notify`)은 **유지**한다: 턴을 넘나들며
    /// 에러가 반복될 때의 알림 빈도 상한이라 턴 경계에서 풀리면 상한이 무의미해진다.
    pub fn reset_dedupe(&mut self, surface_id: u32) {
        self.last_fired.remove(&surface_id);
        self.watch.remove(&surface_id);
    }

    /// enabled set의 snapshot. polling thread가 lock을 짧게 잡고 빠져나오도록.
    pub fn enabled_snapshot(&self) -> Vec<(u32, ScanTarget)> {
        self.enabled.iter().map(|(&sid, &t)| (sid, t)).collect()
    }

    /// 테스트 전용 — dedupe 상태를 직접 시딩한다. `scan_one`은 host 호출이 필요해
    /// `hook.rs`의 `reset_dedupe` 배선 테스트에서 직접 쓰기 어렵다.
    #[cfg(test)]
    pub(crate) fn seed_dedupe_for_test(&mut self, surface_id: u32, snippet: &str) {
        self.last_fired.insert(surface_id, snippet.to_string());
    }

    /// 테스트 전용 — dedupe 상태 존재 여부.
    #[cfg(test)]
    pub(crate) fn has_dedupe_state(&self, surface_id: u32) -> bool {
        self.last_fired.contains_key(&surface_id)
    }

    /// 한 surface에 대해 1회 스캔 + 매치 시 hook 발사. 호스트 IPC 두 번 호출
    /// (`read_since_mark` → `fire_hook`). 매치 안 하면 fire_hook 안 호출.
    ///
    /// 매치 상태가 이어지는 동안에는 무출력 지속시간을 함께 재서, 정지로 판정되면
    /// [`STALLED_EVENT`] 를 추가로 발사한다([`Self::maybe_notify_stall`]).
    ///
    /// 반환: 발사 직전 매치된 라벨/스니펫. 단위 테스트가 dedupe 동작을 검증할 때
    /// 사용한다. 실제로 hook이 발사됐는지 여부는 호스트의 `surface.fire_hook`
    /// 응답에 달려 있다 (예: surface가 이미 사라지면 0).
    pub fn scan_one<H: HostCall>(&mut self, host: &H, surface_id: u32) -> Option<String> {
        self.scan_one_at(host, surface_id, Instant::now())
    }

    /// [`Self::scan_one`] 의 시간 주입 버전 — 테스트가 30 초를 실제로 기다리지 않고
    /// 정적 구간을 재현할 수 있게 `now` 를 받는다.
    fn scan_one_at<H: HostCall>(
        &mut self,
        host: &H,
        surface_id: u32,
        now: Instant,
    ) -> Option<String> {
        let resp = host
            .call(
                "surface.read_since_mark",
                json!({
                    "surface_id": surface_id,
                    "strip_ansi": true,
                }),
            )
            .ok()?;
        let text = resp.get("text").and_then(|v| v.as_str()).unwrap_or("");
        // 출력 흐름 추적은 에러 매치 여부와 무관하게 매 tick 갱신한다 — 에러가 뜨기
        // 전부터 흐름을 보고 있어야 "에러 직후부터의 정적" 을 잴 수 있다.
        self.track_output(surface_id, text, now);
        if !detect_claude_error(text) {
            return None;
        }

        // 같은 텍스트가 연속 polling에서 다시 잡히면 무시 (dedupe). prompt가
        // 새로 그려지지 않는 한 mark는 그대로 유지되므로 같은 chunk가 반복
        // 노출될 수 있다.
        let snippet: String = text.chars().take(200).collect();
        let already_fired = self.last_fired.get(&surface_id) == Some(&snippet);

        let mut fired = None;
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
                    tracing::warn!("claude error fire_hook failed for surface {surface_id}: {e}");
                }
            }
        }

        // dedupe 로 `claude-error` 재발사가 눌린 tick 에서도 정지 판정은 계속 돈다 —
        // "같은 에러 텍스트가 그대로 멈춰 있다" 가 바로 정지의 모습이라, 여기서
        // 빠져나가면 정작 알려야 할 케이스를 영영 못 잡는다.
        self.maybe_notify_stall(host, surface_id, now);
        fired
    }

    /// 이번 tick 의 텍스트로 출력 흐름 관측치를 갱신한다. 지문이 바뀌면 정적 구간을
    /// 처음부터 다시 재고, 이미 보낸 정지 알림도 해제한다(출력이 재개됐으므로 다음
    /// 정적 구간은 새 사건이다).
    fn track_output(&mut self, surface_id: u32, text: &str, now: Instant) {
        let fingerprint = output_fingerprint(text);
        match self.watch.get_mut(&surface_id) {
            Some(w) if w.fingerprint == fingerprint => {}
            Some(w) => {
                w.fingerprint = fingerprint;
                w.last_change = now;
                w.stall_notified = false;
            }
            None => {
                self.watch.insert(
                    surface_id,
                    OutputWatch {
                        fingerprint,
                        last_change: now,
                        stall_notified: false,
                    },
                );
            }
        }
    }

    /// 에러가 매치된 상태에서 무출력이 [`STALL_QUIET`] 이상 이어졌고 호스트가 그
    /// 자식을 여전히 `active` 로 보면 [`STALLED_EVENT`] 를 발사한다.
    ///
    /// 상태 축은 건드리지 않는다 — `terminal.set_state` 를 호출하지 않으므로
    /// `claude children` 의 `state` 는 이 경로로 변하지 않는다(파생 상태 출력 전용
    /// 계약, `docs/adr/0072-child-state-hook-observation-fusion.md`).
    fn maybe_notify_stall<H: HostCall>(&mut self, host: &H, surface_id: u32, now: Instant) {
        let Some(w) = self.watch.get(&surface_id) else {
            return;
        };
        let quiet = now.saturating_duration_since(w.last_change);
        let already_notified = w.stall_notified;
        let since_last_notify = self
            .last_stall_notify
            .get(&surface_id)
            .map(|t| now.saturating_duration_since(*t));
        if !stall_pre_gate(quiet, already_notified, since_last_notify) {
            return;
        }

        // 상태 조회는 값싼 게이트를 모두 통과한 tick 에서만 — 매 tick IPC 를 늘리지
        // 않는다.
        let child_state = host
            .call("terminal.state", json!({ "surface": surface_id }))
            .ok()
            .and_then(|r| {
                r.get("state")
                    .and_then(|v| v.as_str())
                    .map(str::to_ascii_lowercase)
            })
            .unwrap_or_default();
        if !should_notify_stall(quiet, &child_state, already_notified, since_last_notify) {
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

/// 폴링 루프가 매 주기 호출하는 생존 대조 — 등록 경로([`ScanTarget`])에 따라 판정
/// 기준이 다르다. 죽었다고 판정되면 호출자가 `disable` 한다.
///
/// 조회 실패(IPC 오류 등)는 어느 쪽이든 "죽었다"로 단정하지 않고 "살아있다"로
/// 폴백한다 — enable 은 launch/spawn/respawn 시점에만 일어나므로, 일시적 오류로
/// 오인 disable 되면 그 surface 의 에러 감시가 재활성화 경로 없이 영구히 멈춘다
/// (오탐 유지가 오탐 정리보다 안전).
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
        // 자식은 surface 존재가 아니라 부모-자식 관계로 판정한다 — `terminal.release`
        // 는 surface 를 남긴 채 관계만 끊으므로 `surface.locate` 로는 영원히 정리되지
        // 않는다. 호스트가 조회 전 `reconcile_child_terminals()` 를 돌리므로, surface
        // 가 실제로 죽은 경우에도 관계가 함께 사라져 같은 판정에 걸린다(kill 의
        // `surface.close` 가 실패해 surface 가 살아남은 경우 포함).
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
        // respawn 은 같은 surface_id 를 유지한 채 PTY 만 갈아끼우므로 enable 이
        // 중복 호출된다 — 중복 항목/에러 없이 통과해야 한다.
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

    // ── 정지 판정(재시도 중 vs 멈춤) ──

    #[test]
    fn stall_needs_sustained_silence() {
        // 정적이 짧으면 아직 판정하지 않는다 — 재시도 백오프 중일 수 있다.
        assert!(!should_notify_stall(
            STALL_QUIET - Duration::from_secs(1),
            "active",
            false,
            None
        ));
        assert!(should_notify_stall(STALL_QUIET, "active", false, None));
    }

    #[test]
    fn stall_only_when_host_still_thinks_child_is_active() {
        // 턴이 끝난 상태(idle/needs_input/exited)면 기존 완료 알림 경로가 이미
        // 부모에게 알렸다 — 같은 사건으로 두 번 알리지 않는다.
        for state in ["idle", "needs_input", "exited", ""] {
            assert!(
                !should_notify_stall(STALL_QUIET * 2, state, false, None),
                "state={state} 에서는 정지 알림이 나가면 안 된다"
            );
        }
        assert!(should_notify_stall(STALL_QUIET * 2, "active", false, None));
    }

    #[test]
    fn stall_notifies_once_per_silence_and_respects_cooldown() {
        // 같은 정적 구간에서 반복 알림 금지.
        assert!(!should_notify_stall(STALL_QUIET * 3, "active", true, None));
        // 쿨다운 안이면 새 정적 구간이라도 억제 — 에러 반복 세션의 빈도 상한.
        assert!(!should_notify_stall(
            STALL_QUIET * 3,
            "active",
            false,
            Some(STALL_NOTIFY_COOLDOWN - Duration::from_secs(1))
        ));
        assert!(should_notify_stall(
            STALL_QUIET * 3,
            "active",
            false,
            Some(STALL_NOTIFY_COOLDOWN)
        ));
    }

    #[test]
    fn output_fingerprint_tracks_appended_text() {
        // dedupe 스니펫(앞 200 자)은 뒤에 출력이 붙어도 그대로지만, 지문은 달라져야
        // 한다 — 이게 같아지면 "재시도 중" 을 "정지" 로 오판한다.
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

    // ── 정지 판정 + 발사 배선 (`scan_one_at`) ──

    /// `read_since_mark` 텍스트와 `terminal.state` 를 바꿔 끼우고 `fire_hook` 을
    /// 기록하는 스텁.
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
                "surface.read_since_mark" => Ok(json!({ "text": *self.text.borrow() })),
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
        // 재시도 중에는 시도 횟수·백오프 카운트다운이 계속 그려진다 → 출력이 흐르므로
        // 정적 구간이 리셋되고, 아무리 시간이 지나도 정지 알림이 나가지 않는다.
        let host = ScanHost::new(ERR, "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        for i in 1..=6u32 {
            host.set_text(&format!("{ERR}Retrying in {i}s… (attempt {i}/10)"));
            s.scan_one_at(&host, 1, t0 + Duration::from_secs(10 * u64::from(i)));
        }
        assert_eq!(host.stalled_count(), 0, "재시도 중에는 알림이 없어야 한다");
        // 패턴 매치 자체(`claude-error`)는 텍스트가 바뀔 때마다 그대로 발화한다 —
        // 이 작업이 거르는 것은 **부모 알림**이지 감지 이벤트가 아니다.
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

        // 같은 텍스트가 그대로 멈춰 있다 → dedupe 로 claude-error 는 안 뜨지만
        // 정지 판정은 계속 돈다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET);
        assert_eq!(host.stalled_count(), 1, "정적이 임계를 넘으면 1회 발사");

        // 계속 조용해도 같은 정적 구간에서는 다시 알리지 않는다.
        s.scan_one_at(&host, 1, t0 + STALL_QUIET * 3);
        assert_eq!(host.stalled_count(), 1, "에피소드당 1회");
    }

    #[test]
    fn silence_after_turn_ended_does_not_fire_stalled() {
        // 재시도 소진 후 턴이 에러로 끝난 경우 — 호스트는 idle 로 본다. 그 사건은
        // `claude-idle` 형제 hook 이 이미 부모에게 알렸다.
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
    fn clean_output_without_error_never_fires_anything() {
        // 노이즈 회귀 가드 — 정상 세션은 아무 hook 도 발사하지 않는다.
        let host = ScanHost::new("running tests…\nall good\n", "active");
        let mut s = ErrorScanner::new();
        let t0 = Instant::now();
        s.scan_one_at(&host, 1, t0);
        s.scan_one_at(&host, 1, t0 + STALL_QUIET * 5);
        assert!(host.events().is_empty(), "발사 없음: {:?}", host.events());
    }

    /// surface **둘**을 켠다. 하나만 켜면 `disable(1)` 이 네 맵을 통째로 비워도 이
    /// 시험은 초록이라, "1 을 껐다" 와 "전부 껐다" 가 안 갈린다.
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
        // 끄라고 하지 않은 surface 의 감시는 그대로다.
        assert!(s.watch.contains_key(&2));
        assert!(s.is_enabled(2));
    }

    // ── 생존 대조(`scan_target_is_alive`) ──

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
        // `terminal.release` 는 surface 를 남긴 채 관계만 끊는다 — surface 존재만
        // 봤다면 영원히 폴링되며 비-child surface 에 `claude-error` 를 오발화한다.
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

    /// poison 된 스캐너 락에서도 등록·조회가 계속 선다.
    ///
    /// 조준점이 **`enabled_snapshot`** 인 것이 요점이다. 이 락을 잡는 네 자리 중 셋은
    /// 조용히 건너뛰고 있었지만, 폴링 루프의 스냅샷 자리만은 이미 로그를 남기고 있었다 —
    /// 대신 `return` 으로 **루프를 접었다**. poison 은 sticky 라 그 한 번이 프로세스가
    /// 끝날 때까지 에러 스캔 전체를 죽인다. 즉 여기 결함은 "무음" 이 아니라 "영구 정지"
    /// 였고, 무음 세 곳만 고치면 이 자리는 그대로 남는다.
    ///
    /// 되돌리는 변이(`match … Err(_) => return`)는 스냅샷이 비어 아래 단언이 깨진다.
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
