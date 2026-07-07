//! Claude plugin 특화 상태 — 세션 wall-time 타이밍만.
//!
//! child registry(children/parent_of/last_index/closed_parents/idle/needs_input)와
//! 그 영속화·reconcile 은 호스트가 내재화한 `terminal.*` registry(ADR-0040 /
//! occupancy-04)로 이관됐다(occupancy-05). 이 plugin 은 더 이상 자식 매핑을 보유하지
//! 않는다 — 호스트 registry 가 단일 SoT.
//!
//! 여기 남는 것은 claude hook 텔레메트리 전용 상태뿐: `session-start` 시각을 기록해
//! `stop`/`session-end` 시 `wall_time_ms` 를 계산한다. 재시작 시 휘발되므로(비영속),
//! span 이 끊겨도 누락만 발생하고 잘못된 값은 나오지 않는다.

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

    /// 기록된 session-start 시각을 꺼내고 (있으면) elapsed 를 반환.
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
