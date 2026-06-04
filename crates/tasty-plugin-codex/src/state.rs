//! Codex plugin 자체 상태 (child registry + idle/needs_input).
//!
//! 호스트의 `ClaudeState`와 동등한 정보를 plugin이 직접 관리한다. `TASTY_PLUGIN_DATA_DIR/state.json`
//! 에 영속화하여 재시작 후 자식 surface 매핑이 보존된다.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildEntry {
    pub child_surface_id: u32,
    pub index: u32,
    pub cwd: Option<String>,
    pub role: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CodexState {
    /// parent_surface → 자식 목록
    children: HashMap<u32, Vec<ChildEntry>>,
    /// child_surface → parent_surface
    parent_of: HashMap<u32, u32>,
    /// parent_surface별 다음 child index
    next_index: HashMap<u32, u32>,
    /// child_surface → idle 상태
    idle: HashMap<u32, bool>,
    /// child_surface → needs_input 상태
    needs_input: HashMap<u32, bool>,
    /// 영속화 경로 (load 시 결정)
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl CodexState {
    pub fn load() -> Self {
        let dir = std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(PathBuf::from);
        let path = dir.map(|d| d.join("state.json"));
        let mut s = match &path {
            Some(p) if p.exists() => match std::fs::read_to_string(p) {
                Ok(text) => serde_json::from_str::<CodexState>(&text).unwrap_or_default(),
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
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("codex state mkdir {} failed: {e}", parent.display());
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::warn!("codex state save failed: {e}");
                }
            }
            Err(e) => tracing::warn!("codex state encode failed: {e}"),
        }
    }

    pub fn next_index_for(&mut self, parent: u32) -> u32 {
        let entry = self.next_index.entry(parent).or_insert(0);
        let idx = *entry;
        *entry += 1;
        idx
    }

    pub fn register_child(&mut self, parent: u32, child: ChildEntry) {
        self.parent_of.insert(child.child_surface_id, parent);
        self.children.entry(parent).or_default().push(child);
    }

    pub fn list_children(&self, parent: u32) -> &[ChildEntry] {
        self.children.get(&parent).map(Vec::as_slice).unwrap_or(&[])
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

    pub fn remove_child(&mut self, parent: u32, index: u32) -> Option<ChildEntry> {
        let list = self.children.get_mut(&parent)?;
        let pos = list.iter().position(|c| c.index == index)?;
        let removed = list.remove(pos);
        self.parent_of.remove(&removed.child_surface_id);
        self.idle.remove(&removed.child_surface_id);
        self.needs_input.remove(&removed.child_surface_id);
        Some(removed)
    }

    /// surface_id로 child를 찾아 제거한다. `surface.closed` 이벤트 처리용 —
    /// 사용자가 codex child 탭을 닫았을 때 stale registry 정리. 미존재 시 false.
    pub fn unregister_child_by_surface(&mut self, surface_id: u32) -> bool {
        let Some(parent) = self.parent_of.get(&surface_id).copied() else {
            return false;
        };
        let Some(list) = self.children.get_mut(&parent) else {
            return false;
        };
        let Some(pos) = list.iter().position(|c| c.child_surface_id == surface_id) else {
            return false;
        };
        list.remove(pos);
        self.parent_of.remove(&surface_id);
        self.idle.remove(&surface_id);
        self.needs_input.remove(&surface_id);
        true
    }

    pub fn set_idle(&mut self, child_surface: u32, idle: bool) {
        self.idle.insert(child_surface, idle);
        if !idle {
            self.needs_input.insert(child_surface, false);
        }
    }

    pub fn set_needs_input(&mut self, child_surface: u32, val: bool) {
        self.needs_input.insert(child_surface, val);
    }

    pub fn state_of(&self, child_surface: u32) -> &'static str {
        if self
            .needs_input
            .get(&child_surface)
            .copied()
            .unwrap_or(false)
        {
            "needs_input"
        } else if self.idle.get(&child_surface).copied().unwrap_or(false) {
            "idle"
        } else {
            "active"
        }
    }

    /// `--surface`가 주어지지 않았을 때 사용. children 보유 parent가 정확히 한 개일 때만
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

    #[test]
    fn register_and_list() {
        let mut s = CodexState::default();
        let idx = s.next_index_for(10);
        s.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: idx,
                cwd: Some("/tmp".into()),
                role: None,
                nickname: None,
            },
        );
        assert_eq!(s.list_children(10).len(), 1);
        assert_eq!(s.find_child(10, 0).unwrap().child_surface_id, 100);
    }

    #[test]
    fn next_index_monotonic_per_parent() {
        let mut s = CodexState::default();
        assert_eq!(s.next_index_for(10), 0);
        assert_eq!(s.next_index_for(10), 1);
        assert_eq!(s.next_index_for(20), 0);
        assert_eq!(s.next_index_for(10), 2);
    }

    #[test]
    fn state_priority_needs_input_over_idle() {
        let mut s = CodexState::default();
        s.set_idle(50, true);
        assert_eq!(s.state_of(50), "idle");
        s.set_needs_input(50, true);
        assert_eq!(s.state_of(50), "needs_input");
    }

    #[test]
    fn set_idle_false_clears_needs_input() {
        let mut s = CodexState::default();
        s.set_idle(50, true);
        s.set_needs_input(50, true);
        s.set_idle(50, false);
        assert_eq!(s.state_of(50), "active");
    }

    #[test]
    fn remove_child_clears_indexes() {
        let mut s = CodexState::default();
        let idx = s.next_index_for(10);
        s.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: idx,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        s.set_idle(100, true);
        let removed = s.remove_child(10, idx).unwrap();
        assert_eq!(removed.child_surface_id, 100);
        assert!(s.find_child(10, idx).is_none());
        assert_eq!(s.state_of(100), "active"); // idle 데이터도 함께 제거
    }

    #[test]
    fn unregister_child_by_surface_removes_entry_and_indexes() {
        let mut s = CodexState::default();
        let idx = s.next_index_for(10);
        s.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: idx,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        s.set_idle(100, true);
        assert!(s.unregister_child_by_surface(100));
        assert!(s.find_child(10, idx).is_none());
        assert_eq!(s.parent_of_child(100), None);
        assert_eq!(s.state_of(100), "active");
        // 두 번째 호출은 false.
        assert!(!s.unregister_child_by_surface(100));
    }

    #[test]
    fn single_parent_returns_some_when_one() {
        let mut s = CodexState::default();
        assert_eq!(s.single_parent(), None);
        s.register_child(
            10,
            ChildEntry {
                child_surface_id: 100,
                index: 0,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        assert_eq!(s.single_parent(), Some(10));
        s.register_child(
            20,
            ChildEntry {
                child_surface_id: 200,
                index: 0,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        assert_eq!(s.single_parent(), None);
    }
}
