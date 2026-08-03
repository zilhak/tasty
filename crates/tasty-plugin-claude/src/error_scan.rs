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
//! `enable`은 `handlers.rs::handle_launch`가 호출한다. `disable`은
//! `ef57061d`(2026-07-07)가 걷어낸 `on_surface_lifecycle`/`surface.closed`
//! 구독을 되살리는 대신, `main.rs::error_scan_loop`의 800ms 폴링 주기에 편승해
//! `surface.locate`로 생존을 주기적으로 대조하는 방식으로 대체했다 — 추가
//! 구독 배선 없이 기존 poll 인프라만으로 "surface 종료 시 dedupe 상태 정리"를
//! 만족한다. `reset_dedupe`는 `hook.rs`가 새 턴 시작(prompt-submit 등) hook
//! 이벤트에서 호출해, 이전 턴의 에러 텍스트로 눌린 dedupe 가 새 턴의 에러까지
//! 억제하지 않게 한다.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::json;
use tasty_plugin_sdk::HostHandle;

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

#[derive(Default)]
pub struct ErrorScanner {
    /// 스캔 대상 child surface 집합. spawn 시 enable, kill/unregister 시 disable.
    enabled: HashSet<u32>,
    /// 마지막으로 `claude-error`를 발사한 surface (dedupe용).
    /// 같은 surface에서 연속으로 매치되어도 다음 surface state 변경 전까지는
    /// 다시 발사하지 않는다.
    last_fired: HashMap<u32, String>,
}

impl ErrorScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enable(&mut self, surface_id: u32) {
        self.enabled.insert(surface_id);
    }

    pub fn disable(&mut self, surface_id: u32) {
        self.enabled.remove(&surface_id);
        self.last_fired.remove(&surface_id);
    }

    pub fn is_enabled(&self, surface_id: u32) -> bool {
        self.enabled.contains(&surface_id)
    }

    /// dedupe state를 초기화한다. user가 에러 상태를 해소했음을 알리기 위해
    /// 호출 — 예: idle 상태 해제, 새 prompt 시작 시.
    pub fn reset_dedupe(&mut self, surface_id: u32) {
        self.last_fired.remove(&surface_id);
    }

    /// enabled set의 snapshot. polling thread가 lock을 짧게 잡고 빠져나오도록.
    pub fn enabled_snapshot(&self) -> Vec<u32> {
        self.enabled.iter().copied().collect()
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
        s.enable(7);
        assert!(s.is_enabled(7));
        assert_eq!(s.enabled_snapshot(), vec![7]);
        s.disable(7);
        assert!(!s.is_enabled(7));
    }

    #[test]
    fn disable_clears_dedupe_state() {
        // 같은 surface가 disable 후 다시 enable 되면 이전 fire 기록은
        // 사라져야 한다 (새 child 인스턴스로 가정).
        let mut s = ErrorScanner::new();
        s.enable(3);
        s.last_fired
            .insert(3, "API Error: foo".chars().take(200).collect());
        assert!(s.last_fired.contains_key(&3));
        s.disable(3);
        assert!(!s.last_fired.contains_key(&3));
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
}
