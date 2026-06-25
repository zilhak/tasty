//! Codex plugin 자체 상태 (child registry + idle/needs_input).
//!
//! 호스트의 `ClaudeState`와 동등한 정보를 plugin이 직접 관리한다. `TASTY_PLUGIN_DATA_DIR/state.json`
//! 에 영속화하여 재시작 후 자식 surface 매핑이 보존된다.

use std::collections::{HashMap, HashSet};
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

/// `CodexState::reconcile_with_live_surfaces` 결과 요약. 부팅 시 reconcile 로그
/// 및 단위 테스트 검증에 사용. parent/child 분리 카운트로 정리 폭을 한눈에 본다.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub removed_children: u32,
    pub removed_parents: u32,
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

    /// codex 0.130 의 hook 시스템은 Notification event 가 없어 현재 fire 되지
    /// 않지만, codex 가 향후 추가하거나 외부에서 manual invoke 할 경우를 위해
    /// 보존. `state_of` 의 needs_input 분기와 짝.
    // 이유: codex hook 미지원으로 현재 미호출 — 향후 codex Notification event/manual invoke 대비 보존.
    #[allow(dead_code)]
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

    /// 호스트의 살아있는 surface 집합과 child registry 를 cross-check 한다.
    /// `live` 에 없는 child_surface_id 를 모두 제거하고, 자식이 0명이 된 parent 의
    /// `next_index` 와 빈 `children[parent]` vec 도 함께 정리한다.
    ///
    /// host-IPC-free — 단위 테스트 가능. 부팅 시 `on_start` 가 호스트에서 받은
    /// surface 목록으로 `HashSet` 을 구성해 본 메서드에 위임한다. parent key 자체가
    /// `live` 에 없는 경우, 그 parent 의 모든 자식 child_surface_id 도 `live` 에 없을
    /// 것이므로 동일 루프에서 자연히 모두 제거된다 — 그 결과 parent key 도 정리된다.
    ///
    /// claude plugin 의 동명 메서드와 시그니처/의미가 동일해야 두 plugin 의 동작
    /// 일관성이 유지된다 (향후 SDK 공통화 후보).
    pub fn reconcile_with_live_surfaces(&mut self, live: &HashSet<u32>) -> ReconcileSummary {
        let mut summary = ReconcileSummary::default();

        let mut dead_parents: Vec<u32> = Vec::new();
        for (parent, list) in self.children.iter_mut() {
            let before = list.len();
            list.retain(|c| live.contains(&c.child_surface_id));
            let removed = before - list.len();
            summary.removed_children += removed as u32;
            if list.is_empty() {
                dead_parents.push(*parent);
            }
        }

        for parent in &dead_parents {
            self.children.remove(parent);
            // 자식이 0명이 된 parent 의 next_index 키도 정리. 살아있는 자식이 1명
            // 이상 남은 parent 의 next_index 는 단조 증가 invariant 보존을 위해
            // 건드리지 않는다.
            self.next_index.remove(parent);
            // parent surface 자체도 live 가 아니면 — 어차피 host 가 surface.closed
            // 발화 누락한 케이스이므로 — removed_parents 로 카운트.
            if !live.contains(parent) {
                summary.removed_parents += 1;
            }
        }

        // parent_of / idle / needs_input 보조 map 동기화: child_surface_id 가 live 에
        // 없는 항목은 모두 제거.
        self.parent_of.retain(|sid, _| live.contains(sid));
        self.idle.retain(|sid, _| live.contains(sid));
        self.needs_input.retain(|sid, _| live.contains(sid));

        summary
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
    fn reconcile_removes_dead_child_surface() {
        let mut s = CodexState::default();
        let idx0 = s.next_index_for(10);
        s.register_child(10, entry(100, idx0));
        let idx1 = s.next_index_for(10);
        s.register_child(10, entry(101, idx1));
        s.set_idle(101, true);
        s.set_needs_input(101, true);

        // live = {10, 100}: child 101 은 stale → 제거 대상. parent 10 은 살아있음.
        let live: HashSet<u32> = [10u32, 100].into_iter().collect();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 0);
        // 살아있는 child 는 보존.
        assert!(s.find_child(10, idx0).is_some());
        // 죽은 child 는 자취 없이 제거 (parent_of / idle / needs_input).
        assert!(s.find_child(10, idx1).is_none());
        assert_eq!(s.parent_of_child(101), None);
        assert_eq!(s.state_of(101), "active");
        // 살아있는 자식 1명 남았으므로 parent 10 의 next_index 는 보존.
        assert_eq!(s.next_index_for(10), 2);
    }

    #[test]
    fn reconcile_removes_orphan_parent_key() {
        let mut s = CodexState::default();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));

        // live 가 비어있음 → parent 10 과 child 100 모두 dead.
        let live: HashSet<u32> = HashSet::new();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 1);
        // children[10] 키와 next_index[10] 키 모두 정리 → list 는 빈 슬라이스,
        // parent_of 도 0건. dead parent 의 next_index 키도 비워졌으므로 새로 0부터
        // 발급된다.
        assert_eq!(s.list_children(10).len(), 0);
        assert_eq!(s.parent_of_child(100), None);
        assert_eq!(s.next_index_for(10), 0);
    }

    #[test]
    fn reconcile_preserves_live_entries() {
        let mut s = CodexState::default();
        let idx0 = s.next_index_for(10);
        s.register_child(10, entry(100, idx0));
        let idx1 = s.next_index_for(20);
        s.register_child(20, entry(200, idx1));
        s.set_idle(100, true);

        // 모든 surface 가 live → 변경 없음.
        let live: HashSet<u32> = [10u32, 20, 100, 200].into_iter().collect();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 0);
        assert_eq!(summary.removed_parents, 0);
        assert!(s.find_child(10, idx0).is_some());
        assert!(s.find_child(20, idx1).is_some());
        assert_eq!(s.parent_of_child(100), Some(10));
        assert_eq!(s.state_of(100), "idle");
    }

    #[test]
    fn reconcile_summary_counts_multiple_parents() {
        let mut s = CodexState::default();
        // parent 10: child 100, 101 → 둘 다 dead.
        let i0 = s.next_index_for(10);
        s.register_child(10, entry(100, i0));
        let i1 = s.next_index_for(10);
        s.register_child(10, entry(101, i1));
        // parent 20: child 200 살아있음, 201 dead.
        let j0 = s.next_index_for(20);
        s.register_child(20, entry(200, j0));
        let j1 = s.next_index_for(20);
        s.register_child(20, entry(201, j1));
        // parent 30: parent 자체가 live 이지만 자식 모두 dead.
        let k0 = s.next_index_for(30);
        s.register_child(30, entry(300, k0));

        let live: HashSet<u32> = [20u32, 30, 200].into_iter().collect();
        let summary = s.reconcile_with_live_surfaces(&live);

        // children 제거: 100, 101, 201, 300 = 4 명.
        assert_eq!(summary.removed_children, 4);
        // parent 가 dead 인 케이스: 10 (live 에 없음). 30 은 live 에 있지만 자식이
        // 0명이 되어 children 키는 정리되되 removed_parents 로 카운트되지 않음.
        assert_eq!(summary.removed_parents, 1);
        // 살아있는 child 만 남음.
        assert_eq!(s.list_children(20).len(), 1);
        assert_eq!(s.list_children(10).len(), 0);
        assert_eq!(s.list_children(30).len(), 0);
    }
}
