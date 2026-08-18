//! Claude child PTY 에러 패턴 스캐너.
//!
//! 호스트 `src/state/claude_error.rs`의 카탈로그 정규식을 그대로 옮긴다.
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
use std::sync::OnceLock;

use regex::Regex;
use serde_json::json;
use tasty_plugin_sdk::HostHandle;

use crate::handlers::HostCall;

/// 호스트와 1:1 동일한 정규식. `src/state/claude_error.rs`의
/// `CLAUDE_ERROR_PATTERN`을 직접 옮긴 것. 어느 한 쪽을 수정하면 cutover 후
/// 양쪽이 어긋나므로 step 04 cutover 시 호스트 측은 제거된다.
const CLAUDE_ERROR_PATTERN: &str = r"(?i)(\bAPI Error\b|Output blocked by content filtering policy|\boverloaded_error\b|\brate_limit_error\b|\bInternal Server Error\b|\bnetwork error\b|\bBad Request\b)";

static CLAUDE_ERROR_REGEX: OnceLock<Regex> = OnceLock::new();

fn claude_error_regex() -> &'static Regex {
    CLAUDE_ERROR_REGEX.get_or_init(|| {
        Regex::new(CLAUDE_ERROR_PATTERN).expect("ClaudeError catalog regex must compile")
    })
}

/// `text`(ANSI-stripped 권장)에 알려진 Claude 에러 패턴이 포함됐는지.
pub fn detect_claude_error(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    claude_error_regex().is_match(text)
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

#[derive(Default)]
pub struct ErrorScanner {
    /// 스캔 대상 surface → 등록 경로. launch/spawn/respawn 시 enable,
    /// kill 및 폴링 생존 대조 실패 시 disable.
    enabled: HashMap<u32, ScanTarget>,
    /// 마지막으로 `claude-error`를 발사한 surface (dedupe용).
    /// 같은 surface에서 연속으로 매치되어도 다음 surface state 변경 전까지는
    /// 다시 발사하지 않는다.
    last_fired: HashMap<u32, String>,
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
    /// 호출 — 예: idle 상태 해제, 새 prompt 시작 시.
    pub fn reset_dedupe(&mut self, surface_id: u32) {
        self.last_fired.remove(&surface_id);
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
    /// 반환: 발사 직전 매치된 라벨/스니펫. 단위 테스트가 dedupe 동작을 검증할 때
    /// 사용한다. 실제로 hook이 발사됐는지 여부는 호스트의 `surface.fire_hook`
    /// 응답에 달려 있다 (예: surface가 이미 사라지면 0).
    pub fn scan_one(&mut self, host: &HostHandle, surface_id: u32) -> Option<String> {
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
        if !detect_claude_error(text) {
            return None;
        }

        // 같은 텍스트가 연속 polling에서 다시 잡히면 무시 (dedupe). prompt가
        // 새로 그려지지 않는 한 mark는 그대로 유지되므로 같은 chunk가 반복
        // 노출될 수 있다.
        let snippet: String = text.chars().take(200).collect();
        if self.last_fired.get(&surface_id) == Some(&snippet) {
            return None;
        }

        let fire_result = host.call(
            "surface.fire_hook",
            json!({
                "surface_id": surface_id,
                "event": "claude-error",
            }),
        );
        if let Err(e) = fire_result {
            tracing::warn!("claude error fire_hook failed for surface {surface_id}: {e}");
            return None;
        }
        self.last_fired.insert(surface_id, snippet.clone());
        Some(snippet)
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
                    }),
                },
                "terminal.parent" => match self.parent_status {
                    Some("none") => Ok(json!({ "parent_surface_id": null, "status": "none" })),
                    Some(st) => Ok(json!({ "parent_surface_id": 1, "status": st })),
                    None => Err(tasty_plugin_sdk::PluginError::HostCall {
                        method: method.to_string(),
                        message: "host down".into(),
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
