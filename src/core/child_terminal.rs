//! 자식 터미널 surface의 부모·번호·보고된 상태를 보관하고 child-terminals.json에 저장한다.
//! SessionToken의 권한 위임이나 task runner의 자식 프로세스 관리는 이 레지스트리의 역할이 아니다.
//! 호출자가 실제 surface 목록과 대조해 없어진 항목을 정리한다.

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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub removed_children: u32,
    pub removed_parents: u32,
}

impl ReconcileSummary {
    pub fn changed(&self) -> bool {
        self.removed_children > 0 || self.removed_parents > 0
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChildTerminalRegistry {
    children: HashMap<u32, Vec<ChildEntry>>,
    parent_of: HashMap<u32, u32>,
    next_index: HashMap<u32, u32>,
    idle: HashMap<u32, bool>,
    needs_input: HashMap<u32, bool>,
    /// 마지막 상태 보고 시각(Unix epoch ms). 등록 시각으로 시작하고 상태 보고마다 갱신한다.
    /// 재시작 뒤에도 보고가 끊긴 기간을 잴 수 있다. 유효성 판정은 child_liveness에서 맡는다.
    #[serde(default)]
    last_state_report_at: HashMap<u32, u64>,
    /// load로 정하며 default는 저장 경로가 없다.
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl ChildTerminalRegistry {
    /// TASTY_HOME 아래에서 읽는다. 파일 읽기·파싱 실패는 빈 상태로 처리한다.
    pub fn load() -> Self {
        let path = tasty_utils::path::tasty_home().map(|d| d.join("child-terminals.json"));
        let mut s = match &path {
            Some(p) if p.exists() => match std::fs::read_to_string(p) {
                Ok(text) => {
                    serde_json::from_str::<ChildTerminalRegistry>(&text).unwrap_or_default()
                }
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
        ensure_parent_dir(path);
        self.write_json_to(path);
    }

    fn write_json_to(&self, path: &std::path::Path) {
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::warn!("child-terminal registry save failed: {e}");
                }
            }
            Err(e) => tracing::warn!("child-terminal registry encode failed: {e}"),
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
        self.last_state_report_at
            .insert(child.child_surface_id, now_epoch_ms());
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

    /// 해당 자식이 없으면 false다. 콜백이 바꾼 필드의 별도 인덱스는 여기서 다시 만들지 않는다.
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
        self.last_state_report_at.remove(&removed.child_surface_id);
        Some(removed)
    }

    /// surface ID로 자식을 제거한다. 항목이 없으면 false다.
    #[allow(dead_code)]
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
        self.last_state_report_at.remove(&surface_id);
        true
    }

    /// idle 값과 무관하게 needs_input도 해제한다. 대기 플래그가 남아 이후 idle·active를 가리지 않게 한다.
    pub fn set_idle(&mut self, child_surface: u32, idle: bool) {
        self.idle.insert(child_surface, idle);
        self.needs_input.insert(child_surface, false);
        self.stamp_state_report(child_surface);
    }

    pub fn set_needs_input(&mut self, child_surface: u32, val: bool) {
        self.needs_input.insert(child_surface, val);
        self.stamp_state_report(child_surface);
    }

    fn stamp_state_report(&mut self, child_surface: u32) {
        self.last_state_report_at
            .insert(child_surface, now_epoch_ms());
    }

    /// 등록 또는 마지막 상태 보고 시각. 해당 필드가 없는 오래된 저장 항목은 None일 수 있다.
    pub fn last_state_report_at(&self, child_surface: u32) -> Option<u64> {
        self.last_state_report_at.get(&child_surface).copied()
    }

    /// 마지막 보고 이후 시간. 시각 정보가 없으면 None, 시계가 뒤로 가면 0이다.
    /// 0으로 처리하는 동안에는 실제 보고 중단 시간을 짧게 판단할 수 있다.
    pub fn hook_silence(&self, child_surface: u32, now_ms: u64) -> Option<std::time::Duration> {
        let at = self.last_state_report_at(child_surface)?;
        Some(std::time::Duration::from_millis(now_ms.saturating_sub(at)))
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

    /// 자식이 있는 부모가 정확히 하나일 때만 ID를 반환한다.
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

    /// live에 없는 자식과 상태를 지우고, 빈 부모 목록과 다음 번호도 지운다.
    /// 부모 자체의 생존 여부만으로 살아 있는 자식을 지우지는 않는다.
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
            self.next_index.remove(parent);
            if !live.contains(parent) {
                summary.removed_parents += 1;
            }
        }

        self.parent_of.retain(|sid, _| live.contains(sid));
        self.idle.retain(|sid, _| live.contains(sid));
        self.needs_input.retain(|sid, _| live.contains(sid));
        self.last_state_report_at
            .retain(|sid, _| live.contains(sid));

        summary
    }
}

/// 재시작 뒤에도 비교할 수 있도록 상태 보고 시각을 Unix epoch ms로 기록한다.
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn ensure_parent_dir(path: &std::path::Path) {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(
            "child-terminal registry mkdir {} failed: {e}",
            parent.display()
        );
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
    fn register_and_list() {
        let mut s = ChildTerminalRegistry::default();
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
        let mut s = ChildTerminalRegistry::default();
        assert_eq!(s.next_index_for(10), 0);
        assert_eq!(s.next_index_for(10), 1);
        assert_eq!(s.next_index_for(20), 0);
        assert_eq!(s.next_index_for(10), 2);
    }

    #[test]
    fn state_priority_needs_input_over_idle() {
        let mut s = ChildTerminalRegistry::default();
        s.set_idle(50, true);
        assert_eq!(s.state_of(50), "idle");
        s.set_needs_input(50, true);
        assert_eq!(s.state_of(50), "needs_input");
    }

    #[test]
    fn set_idle_false_clears_needs_input() {
        let mut s = ChildTerminalRegistry::default();
        s.set_idle(50, true);
        s.set_needs_input(50, true);
        s.set_idle(50, false);
        assert_eq!(s.state_of(50), "active");
    }

    #[test]
    fn remove_child_clears_indexes() {
        let mut s = ChildTerminalRegistry::default();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));
        s.set_idle(100, true);
        let removed = s.remove_child(10, idx).unwrap();
        assert_eq!(removed.child_surface_id, 100);
        assert!(s.find_child(10, idx).is_none());
        assert_eq!(s.state_of(100), "active");
    }

    #[test]
    fn unregister_child_by_surface_removes_entry_and_indexes() {
        let mut s = ChildTerminalRegistry::default();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));
        s.set_idle(100, true);
        assert!(s.unregister_child_by_surface(100));
        assert!(s.find_child(10, idx).is_none());
        assert_eq!(s.parent_of_child(100), None);
        assert_eq!(s.state_of(100), "active");
        assert!(!s.unregister_child_by_surface(100));
    }

    #[test]
    fn register_seeds_state_report_baseline() {
        let mut s = ChildTerminalRegistry::default();
        let before = now_epoch_ms();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));
        let at = s
            .last_state_report_at(100)
            .expect("등록 시각이 기록돼야 한다");
        assert!(at >= before, "{at} >= {before}");
        assert_eq!(
            s.last_state_report_at(999),
            None,
            "미등록 surface 는 기준점 없음"
        );
    }

    #[test]
    fn hook_push_refreshes_state_report() {
        let mut s = ChildTerminalRegistry::default();
        s.set_idle(50, true);
        let first = s.last_state_report_at(50).unwrap();
        s.set_needs_input(50, true);
        let second = s.last_state_report_at(50).unwrap();
        assert!(second >= first);
        s.set_idle(50, false);
        assert!(s.last_state_report_at(50).unwrap() >= second);
    }

    #[test]
    fn hook_silence_measures_from_last_report() {
        let mut s = ChildTerminalRegistry::default();
        s.set_idle(50, true);
        let at = s.last_state_report_at(50).unwrap();
        assert_eq!(
            s.hook_silence(50, at + 7_000),
            Some(std::time::Duration::from_secs(7))
        );
        assert_eq!(s.hook_silence(999, at), None, "기준점 없으면 판정 불가");
    }

    #[test]
    fn hook_silence_clamps_on_clock_rewind() {
        let mut s = ChildTerminalRegistry::default();
        s.set_idle(50, true);
        let at = s.last_state_report_at(50).unwrap();
        assert_eq!(
            s.hook_silence(50, at.saturating_sub(60_000)),
            Some(std::time::Duration::ZERO),
            "시계가 뒤로 가면 경과 시간을 0으로 처리해야 한다"
        );
    }

    #[test]
    fn state_report_axis_is_cleared_with_the_child() {
        let mut s = ChildTerminalRegistry::default();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));
        s.remove_child(10, idx);
        assert_eq!(s.last_state_report_at(100), None);

        let idx = s.next_index_for(20);
        s.register_child(20, entry(200, idx));
        s.reconcile_with_live_surfaces(&HashSet::new());
        assert_eq!(s.last_state_report_at(200), None);
    }

    #[test]
    fn legacy_persisted_registry_loads_without_state_report_field() {
        // last_state_report_at 필드가 없는 저장 파일도 읽을 수 있어야 한다.
        let legacy = r#"{
            "children": { "10": [{ "child_surface_id": 100, "index": 0,
                                   "cwd": null, "role": null, "nickname": null }] },
            "parent_of": { "100": 10 },
            "next_index": { "10": 1 },
            "idle": {},
            "needs_input": {}
        }"#;
        let s: ChildTerminalRegistry = serde_json::from_str(legacy).expect("하위호환 로드");
        assert_eq!(s.list_children(10).len(), 1);
        assert_eq!(s.state_of(100), "active");
        assert_eq!(
            s.last_state_report_at(100),
            None,
            "보고 시각이 없으면 경과 시간도 알 수 없다"
        );
    }

    #[test]
    fn single_parent_returns_some_when_one() {
        let mut s = ChildTerminalRegistry::default();
        assert_eq!(s.single_parent(), None);
        s.register_child(10, entry(100, 0));
        assert_eq!(s.single_parent(), Some(10));
        s.register_child(20, entry(200, 0));
        assert_eq!(s.single_parent(), None);
    }

    #[test]
    fn reconcile_removes_dead_child_surface() {
        let mut s = ChildTerminalRegistry::default();
        let idx0 = s.next_index_for(10);
        s.register_child(10, entry(100, idx0));
        let idx1 = s.next_index_for(10);
        s.register_child(10, entry(101, idx1));
        s.set_idle(101, true);
        s.set_needs_input(101, true);

        let live: HashSet<u32> = [10u32, 100].into_iter().collect();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 0);
        assert!(s.find_child(10, idx0).is_some());
        assert!(s.find_child(10, idx1).is_none());
        assert_eq!(s.parent_of_child(101), None);
        assert_eq!(s.state_of(101), "active");
        assert_eq!(s.next_index_for(10), 2);
    }

    #[test]
    fn reconcile_removes_orphan_parent_key() {
        let mut s = ChildTerminalRegistry::default();
        let idx = s.next_index_for(10);
        s.register_child(10, entry(100, idx));

        let live: HashSet<u32> = HashSet::new();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 1);
        assert_eq!(s.list_children(10).len(), 0);
        assert_eq!(s.parent_of_child(100), None);
        assert_eq!(s.next_index_for(10), 0);
    }

    #[test]
    fn reconcile_preserves_live_entries() {
        let mut s = ChildTerminalRegistry::default();
        let idx0 = s.next_index_for(10);
        s.register_child(10, entry(100, idx0));
        let idx1 = s.next_index_for(20);
        s.register_child(20, entry(200, idx1));
        s.set_idle(100, true);

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
        let mut s = ChildTerminalRegistry::default();
        let i0 = s.next_index_for(10);
        s.register_child(10, entry(100, i0));
        let i1 = s.next_index_for(10);
        s.register_child(10, entry(101, i1));
        let j0 = s.next_index_for(20);
        s.register_child(20, entry(200, j0));
        let j1 = s.next_index_for(20);
        s.register_child(20, entry(201, j1));
        let k0 = s.next_index_for(30);
        s.register_child(30, entry(300, k0));

        let live: HashSet<u32> = [20u32, 30, 200].into_iter().collect();
        let summary = s.reconcile_with_live_surfaces(&live);

        assert_eq!(summary.removed_children, 4);
        assert_eq!(summary.removed_parents, 1);
        assert_eq!(s.list_children(20).len(), 1);
        assert_eq!(s.list_children(10).len(), 0);
        assert_eq!(s.list_children(30).len(), 0);
    }
}
