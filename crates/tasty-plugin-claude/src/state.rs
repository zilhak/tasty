//! session-start 시각을 저장하고 stop/session-end에서 경과 시간을 계산한다.
//! 메모리에서만 보관하므로 플러그인을 재시작하면 기록이 사라진다.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ClaudeState {
    /// surface → session-start 시각 (unix ms).
    wall_time_starts: HashMap<u32, u64>,
}

impl ClaudeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// `session-start` 시각을 기록.
    pub fn mark_session_start(&mut self, surface: u32, ts_ms: u64) {
        self.wall_time_starts.insert(surface, ts_ms);
    }

    /// 시작 시각을 꺼내 경과 시간을 반환한다. 기록이 없으면 None이다.
    pub fn take_wall_time(&mut self, surface: u32, now_ms: u64) -> Option<u64> {
        let start = self.wall_time_starts.remove(&surface)?;
        Some(now_ms.saturating_sub(start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_time_start_then_elapsed() {
        let mut s = ClaudeState::default();
        s.mark_session_start(7, 1_000);
        assert_eq!(s.take_wall_time(7, 5_000), Some(4_000));
        // 두 번째 호출은 이미 소비돼 None.
        assert_eq!(s.take_wall_time(7, 9_000), None);
    }

    #[test]
    fn take_wall_time_without_start_is_none() {
        let mut s = ClaudeState::default();
        assert_eq!(s.take_wall_time(1, 100), None);
    }

    #[test]
    fn saturating_when_now_before_start() {
        let mut s = ClaudeState::default();
        s.mark_session_start(1, 5_000);
        // now < start → saturating_sub → 0 (음수 방지).
        assert_eq!(s.take_wall_time(1, 1_000), Some(0));
    }
}
