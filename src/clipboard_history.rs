//! 시스템 클립보드 히스토리 저장소. 메모리 전용 (재시작 시 휘발).
//!
//! 동작:
//! - `EngineState`에 `ClipboardHistory` 소유.
//! - 메인 스레드의 `AppEvent::ClipboardTick` 수신 시 `record`로 시스템 값 기록.
//! - Tasty 내부 복사(텍스트 선택 복사 등)는 `record`에 `ClipboardSource::Internal`.
//! - 연속 중복은 자동 제거(`last_seen`).
//!
//! 주의: 비밀번호 관리자 등 민감 정보도 폴링된다. OS 레벨에서 이를 구분할 수단이
//! 제한적이라 1차는 필터 없음. 사용자에게 보이도록 docs에 경고.

use std::collections::VecDeque;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardSource {
    /// Tasty가 아닌 외부에서 복사된 값(폴링으로 감지).
    System,
    /// Tasty 내부에서 복사된 값(선택 영역 copy 등).
    Internal,
}

#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub text: String,
    pub captured_at: Instant,
    pub source: ClipboardSource,
}

pub struct ClipboardHistory {
    entries: VecDeque<ClipboardEntry>,
    max_entries: usize,
    /// 마지막으로 관찰한 값. 같은 값이 연속 들어오면 무시.
    last_seen: Option<String>,
}

impl ClipboardHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(256)),
            max_entries: max_entries.max(1),
            last_seen: None,
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &ClipboardEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 새 값을 앞에 push. `last_seen`과 같으면 추가하지 않고 false 반환.
    /// 빈 문자열도 무시.
    pub fn record(&mut self, text: String, source: ClipboardSource) -> bool {
        if text.is_empty() {
            return false;
        }
        if self.last_seen.as_deref() == Some(&text) {
            return false;
        }
        self.last_seen = Some(text.clone());
        self.entries.push_front(ClipboardEntry {
            text,
            captured_at: Instant::now(),
            source,
        });
        self.truncate();
        true
    }

    pub fn get(&self, index: usize) -> Option<&ClipboardEntry> {
        self.entries.get(index)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.last_seen = None;
    }

    /// Remove the entry at the given index. Out-of-range는 no-op.
    pub fn remove_at(&mut self, index: usize) -> Option<ClipboardEntry> {
        let removed = self.entries.remove(index);
        // 0번 항목을 지우면 last_seen과 불일치할 수 있음 — 다음 같은 값 re-record 허용.
        if index == 0 {
            self.last_seen = self.entries.front().map(|e| e.text.clone());
        }
        removed
    }

    pub fn set_max(&mut self, max: usize) {
        self.max_entries = max.max(1);
        self.truncate();
    }

    fn truncate(&mut self) {
        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
    }
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_adds_new_entry() {
        let mut h = ClipboardHistory::new(10);
        assert!(h.record("hello".to_string(), ClipboardSource::System));
        assert_eq!(h.len(), 1);
        assert_eq!(h.get(0).unwrap().text, "hello");
        assert_eq!(h.get(0).unwrap().source, ClipboardSource::System);
    }

    #[test]
    fn record_skips_consecutive_duplicates() {
        let mut h = ClipboardHistory::new(10);
        assert!(h.record("hello".to_string(), ClipboardSource::System));
        assert!(!h.record("hello".to_string(), ClipboardSource::System));
        assert!(!h.record("hello".to_string(), ClipboardSource::Internal));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn record_accepts_same_value_after_different() {
        let mut h = ClipboardHistory::new(10);
        h.record("a".to_string(), ClipboardSource::System);
        h.record("b".to_string(), ClipboardSource::System);
        h.record("a".to_string(), ClipboardSource::System);
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(0).unwrap().text, "a");
        assert_eq!(h.get(1).unwrap().text, "b");
        assert_eq!(h.get(2).unwrap().text, "a");
    }

    #[test]
    fn record_ignores_empty_string() {
        let mut h = ClipboardHistory::new(10);
        assert!(!h.record("".to_string(), ClipboardSource::System));
        assert_eq!(h.len(), 0);
    }

    #[test]
    fn truncate_limits_size() {
        let mut h = ClipboardHistory::new(3);
        for i in 0..5 {
            h.record(format!("entry{i}"), ClipboardSource::System);
        }
        assert_eq!(h.len(), 3);
        // Newest first
        assert_eq!(h.get(0).unwrap().text, "entry4");
        assert_eq!(h.get(2).unwrap().text, "entry2");
    }

    #[test]
    fn clear_resets_state() {
        let mut h = ClipboardHistory::new(10);
        h.record("x".to_string(), ClipboardSource::System);
        h.clear();
        assert_eq!(h.len(), 0);
        // After clear, same value can be recorded again
        assert!(h.record("x".to_string(), ClipboardSource::System));
    }

    #[test]
    fn set_max_shrinks_when_smaller() {
        let mut h = ClipboardHistory::new(10);
        for i in 0..5 {
            h.record(format!("e{i}"), ClipboardSource::System);
        }
        h.set_max(2);
        assert_eq!(h.len(), 2);
        assert_eq!(h.get(0).unwrap().text, "e4");
        assert_eq!(h.get(1).unwrap().text, "e3");
    }

    #[test]
    fn set_max_clamps_to_one() {
        let mut h = ClipboardHistory::new(10);
        h.set_max(0);
        h.record("a".to_string(), ClipboardSource::System);
        h.record("b".to_string(), ClipboardSource::System);
        assert_eq!(h.len(), 1);
        assert_eq!(h.get(0).unwrap().text, "b");
    }
}
