//! Claude plugin 자체 상태 (child registry + idle/needs_input).
//!
//! 호스트 `src/state/claude.rs::AppState` 위에 박혀 있던 `ClaudeState` 로직을 plugin이
//! 직접 관리한다. `TASTY_PLUGIN_DATA_DIR/state.json` 에 영속화하여 재시작 후 자식
//! surface 매핑이 보존된다. 호스트 동작 의미를 그대로 보존한다 — 특히
//! `next_child_index`는 1-based로 증가한다 (codex plugin의 0-based와 다름).
//!
//! step 02(PTY error scanner) / step 04(IPC handler migration)에서 `error_scan_enabled`,
//! `spawn_panes` 같은 추가 필드와 핸들러 메서드가 합류한다. 본 단계는 child registry만
//! 옮긴다. 따라서 일부 헬퍼는 아직 호출되지 않는다 — `#[allow(dead_code)]`로 임시 허용.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 호스트의 `ClaudeChildEntry`와 wire-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildEntry {
    pub child_surface_id: u32,
    pub index: u32,
    pub cwd: Option<String>,
    pub role: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClaudeState {
    /// parent_surface → 자식 목록
    children: HashMap<u32, Vec<ChildEntry>>,
    /// child_surface → parent_surface
    parent_of: HashMap<u32, u32>,
    /// parent_surface별 마지막 발급된 child index (1-based; 호스트 동작 보존)
    last_index: HashMap<u32, u32>,
    /// 사용자가 닫았지만 자식이 남아 있는 부모 surface. 모든 자식이 빠지면 정리된다.
    closed_parents: HashSet<u32>,
    /// child/parent surface → idle 상태
    idle: HashMap<u32, bool>,
    /// surface → needs_input 상태
    needs_input: HashMap<u32, bool>,
    /// 영속화 경로 (load 시 결정; serde 직렬화 제외)
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl ClaudeState {
    pub fn load() -> Self {
        let dir = std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(PathBuf::from);
        let path = dir.map(|d| d.join("state.json"));
        let mut s = match &path {
            Some(p) if p.exists() => match std::fs::read_to_string(p) {
                Ok(text) => serde_json::from_str::<ClaudeState>(&text).unwrap_or_default(),
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        };
        s.path = path;
        s
    }

    pub fn save(&self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::warn!("claude state save failed: {e}");
                }
            }
            Err(e) => tracing::warn!("claude state encode failed: {e}"),
        }
    }

    /// 호스트의 `AppState::next_child_index` 의미 그대로. 첫 호출은 1을 반환하고,
    /// 이후 호출은 직전 반환값 + 1을 반환한다.
    pub fn next_child_index(&mut self, parent: u32) -> u32 {
        let entry = self.last_index.entry(parent).or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn register_child(&mut self, parent: u32, child: ChildEntry) {
        self.parent_of.insert(child.child_surface_id, parent);
        self.children.entry(parent).or_default().push(child);
    }

    pub fn list_children(&self, parent: u32) -> &[ChildEntry] {
        self.children
            .get(&parent)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn find_child(&self, parent: u32, index: u32) -> Option<&ChildEntry> {
        self.children
            .get(&parent)?
            .iter()
            .find(|c| c.index == index)
    }

    /// 자식 항목을 mutable하게 업데이트한다. 자식이 없으면 false 반환.
    pub fn update_child<F>(&mut self, parent: u32, index: u32, f: F) -> bool
    where
        F: FnOnce(&mut ChildEntry),
    {
        let Some(list) = self.children.get_mut(&parent) else {
            return false;
        };
        let Some(entry) = list.iter_mut().find(|c| c.index == index) else {
            return false;
        };
        f(entry);
        true
    }

    /// child surface로 parent surface를 역인덱싱한다.
    pub fn parent_of_child(&self, child_surface: u32) -> Option<u32> {
        self.parent_of.get(&child_surface).copied()
    }

    /// 주어진 surface_id가 자식을 가진 부모로 registry에 있는지.
    /// surface lifecycle observer에서 parent close 판정에 사용.
    pub fn is_known_parent(&self, parent_surface: u32) -> bool {
        self.children.contains_key(&parent_surface)
    }

    /// 주어진 부모 surface가 "닫힘" 상태(`mark_parent_closed`로 마킹됐고 자식이
    /// 아직 남아 있는 상태)에 있는지. `claude.parent` 응답의 status 결정에 사용.
    pub fn is_parent_closed(&self, parent_surface: u32) -> bool {
        self.closed_parents.contains(&parent_surface)
    }

    /// 자식을 한 명 제거하고, 그 자식의 idle/needs_input 데이터도 함께 삭제한다.
    /// 호스트 `unregister_child`와 동일하게, 자식 제거 후 부모가 closed_parents에 있고
    /// 자식이 0명이면 부모 관련 데이터도 함께 정리한다.
    pub fn unregister_child(&mut self, child_surface: u32) {
        self.idle.remove(&child_surface);
        self.needs_input.remove(&child_surface);
        let parent = match self.parent_of.remove(&child_surface) {
            Some(p) => p,
            None => return,
        };
        if let Some(list) = self.children.get_mut(&parent) {
            list.retain(|c| c.child_surface_id != child_surface);
            if list.is_empty() && self.closed_parents.contains(&parent) {
                self.children.remove(&parent);
                self.closed_parents.remove(&parent);
                self.last_index.remove(&parent);
            }
        }
    }

    /// 부모 surface가 닫혔음을 마킹한다. 자식이 비어 있으면 즉시 정리. 호스트
    /// `mark_parent_closed`와 동일 의미.
    pub fn mark_parent_closed(&mut self, parent_surface: u32) {
        self.idle.remove(&parent_surface);
        self.needs_input.remove(&parent_surface);
        let Some(list) = self.children.get(&parent_surface) else {
            return;
        };
        if list.is_empty() {
            self.children.remove(&parent_surface);
            self.last_index.remove(&parent_surface);
        } else {
            self.closed_parents.insert(parent_surface);
        }
    }

    /// 호스트 `set_claude_idle`. idle=false면 needs_input도 함께 clear.
    pub fn set_idle(&mut self, surface: u32, idle: bool) {
        self.idle.insert(surface, idle);
        if !idle {
            self.needs_input.remove(&surface);
        }
    }

    /// 호스트 `set_claude_needs_input`.
    pub fn set_needs_input(&mut self, surface: u32, val: bool) {
        self.needs_input.insert(surface, val);
    }

    /// 호스트 `claude_state_of`. needs_input > idle > active 우선순위.
    pub fn state_of(&self, surface: u32) -> &'static str {
        if self.needs_input.get(&surface).copied().unwrap_or(false) {
            "needs_input"
        } else if self.idle.get(&surface).copied().unwrap_or(false) {
            "idle"
        } else {
            "active"
        }
    }

    /// `--surface`가 주어지지 않았을 때 사용. 자식 보유 부모가 정확히 한 개일 때만
    /// 그 id를 반환. 그 외에는 None (호출자가 명시 요구).
    pub fn single_parent(&self) -> Option<u32> {
        let parents: Vec<u32> = self
            .children
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, _)| *k)
            .collect();
        if parents.len() == 1 {
            Some(parents[0])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(child_surface_id: u32, index: u32) -> ChildEntry {
        ChildEntry {
            child_surface_id,
            index,
            cwd: None,
            role: None,
            nickname: None,
        }
    }

    #[test]
    fn next_child_index_is_one_based_per_parent() {
        // 호스트 동작 보존: 첫 호출이 1을 반환해야 회귀가 없다 (codex는 0-based).
        let mut s = ClaudeState::default();
        assert_eq!(s.next_child_index(10), 1);
        assert_eq!(s.next_child_index(10), 2);
        assert_eq!(s.next_child_index(20), 1);
        assert_eq!(s.next_child_index(10), 3);
    }

    #[test]
    fn register_and_list() {
        let mut s = ClaudeState::default();
        let idx = s.next_child_index(10);
        s.register_child(10, entry(100, idx));
        assert_eq!(s.list_children(10).len(), 1);
        assert_eq!(s.find_child(10, idx).unwrap().child_surface_id, 100);
        assert_eq!(s.parent_of_child(100), Some(10));
        assert!(s.is_known_parent(10));
    }

    #[test]
    fn unregister_child_clears_idle_and_needs_input() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        s.set_idle(100, true);
        s.set_needs_input(100, true);
        s.unregister_child(100);
        assert!(s.find_child(10, 1).is_none());
        assert_eq!(s.parent_of_child(100), None);
        // idle/needs_input 데이터도 함께 제거되었으므로 상태는 default("active")
        assert_eq!(s.state_of(100), "active");
    }

    #[test]
    fn mark_parent_closed_with_no_children_cleans_up_immediately() {
        let mut s = ClaudeState::default();
        // 자식을 register했다가 제거하면 children entry는 비어 있지만 last_index는 남아 있다.
        s.register_child(10, entry(100, 1));
        s.unregister_child(100);
        // closed_parents에 들어가지 않고 last_index만 잔재 — mark가 그대로 정리해야 함.
        s.mark_parent_closed(10);
        assert!(!s.is_known_parent(10));
    }

    #[test]
    fn mark_parent_closed_with_children_marks_and_defers_cleanup() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        s.register_child(10, entry(101, 2));
        s.mark_parent_closed(10);
        // 자식들이 남아 있으니 children 엔트리는 유지된다.
        assert_eq!(s.list_children(10).len(), 2);
        // 마지막 자식까지 unregister되면 closed_parents 마킹과 함께 부모도 정리된다.
        s.unregister_child(100);
        assert!(s.is_known_parent(10)); // 아직 자식 1명 남음
        s.unregister_child(101);
        assert!(!s.is_known_parent(10));
    }

    #[test]
    fn state_priority_needs_input_over_idle() {
        let mut s = ClaudeState::default();
        s.set_idle(50, true);
        assert_eq!(s.state_of(50), "idle");
        s.set_needs_input(50, true);
        assert_eq!(s.state_of(50), "needs_input");
    }

    #[test]
    fn set_idle_false_clears_needs_input() {
        let mut s = ClaudeState::default();
        s.set_idle(50, true);
        s.set_needs_input(50, true);
        s.set_idle(50, false);
        assert_eq!(s.state_of(50), "active");
    }

    #[test]
    fn is_parent_closed_reflects_mark_parent_closed_state() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        assert!(!s.is_parent_closed(10));
        s.mark_parent_closed(10);
        // 자식이 남아 있으므로 closed_parents 마킹은 유지된다.
        assert!(s.is_parent_closed(10));
        s.unregister_child(100);
        // 마지막 자식 unregister 시 closed_parents에서도 제거된다.
        assert!(!s.is_parent_closed(10));
    }

    #[test]
    fn single_parent_returns_some_when_exactly_one_has_children() {
        let mut s = ClaudeState::default();
        assert_eq!(s.single_parent(), None);
        s.register_child(10, entry(100, 1));
        assert_eq!(s.single_parent(), Some(10));
        s.register_child(20, entry(200, 1));
        assert_eq!(s.single_parent(), None);
    }
}
