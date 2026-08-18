//! 호스트 child-terminal registry (ADR-0040 / occupancy-04).
//!
//! 에이전트가 spawn 한 **자식 터미널 surface** 의 부모/인덱스/상태 매핑을 호스트가
//! 단일 SoT 로 보관한다. 지금까지 이 기계는 `tasty-plugin-codex`(`CodexState`) 와
//! `tasty-plugin-claude`(`ClaudeState`) 에 각각 중복 구현돼 있었다 — 04 가 그 범용
//! 부분(registry + spawn 조합 + self-heal)을 호스트로 내재화한다. 영속화 경로는
//! 호스트 데이터 디렉토리(`~/.tasty/child-terminals.json`).
//!
//! **다른 서브시스템과의 경계 (레지스트리 파편화 방지)**:
//! - `adapters/ipc/handler/session.rs` 의 `SessionStore` 는 자식 agent 프로세스에
//!   발급하는 **SessionToken**(권한 위임·검증)을 추적한다 — surface 매핑이 아니다.
//! - `core/agent/runner_host.rs` 의 `shell_children` 은 `agent.task` DAG 러너가
//!   spawn 한 **shell 서브프로세스(PID)** 의 종료코드를 감시한다 — PTY surface 가
//!   아니다.
//!   둘 다 child-**terminal**(surface) registry 와 역할이 갈리며, 본 registry 로
//!   통합 대상이 **아니다**.
//!
//! host-IPC-free — 단위 테스트 가능. self-heal(reconcile)은 호스트가 라이브 surface
//! 트리를 직접 소유하므로 이벤트 구독 없이 접근 시점마다 동기 reconcile 로 처리한다.

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

/// `ChildTerminalRegistry::reconcile_with_live_surfaces` 결과 요약. 부팅/접근 시
/// reconcile 로그 및 단위 테스트 검증에 사용. parent/child 분리 카운트로 정리 폭을
/// 한눈에 본다.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub removed_children: u32,
    pub removed_parents: u32,
}

impl ReconcileSummary {
    /// 실제로 제거된 항목이 있었는가 — caller 가 이때만 save 하도록 게이트.
    pub fn changed(&self) -> bool {
        self.removed_children > 0 || self.removed_parents > 0
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChildTerminalRegistry {
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
    /// child_surface → 이 자식의 상태를 마지막으로 **보고받은** 시각 (unix epoch ms).
    ///
    /// `idle`/`needs_input` 두 bool 맵이 "무엇을 보고받았나" 라면 이 맵은 "언제
    /// 보고받았나" 다 — 별개의 축이다. hook push(`set_idle`/`set_needs_input`)마다
    /// 갱신되고, `register_child` 가 등록 시각으로 시딩한다(등록 자체가 "이 시점엔
    /// 이 자식이 막 태어났다" 는 보고이므로, hook 이 한 번도 오지 않은 자식도
    /// 침묵 경과시간을 잴 기준점을 갖는다).
    ///
    /// **이 축이 필요한 이유**: `state_of` 는 hook push 캐시를 되읽을 뿐이라,
    /// hook 이 유실되면 마지막으로 찍힌 `active` 가 영구히 남는다. "hook 이 N 시간째
    /// 안 온다" 는 사실은 bool 맵 두 개로는 표현할 수 없다. PTY 무출력 경과시간과
    /// 달리 **epoch 기반이라 호스트 재시작을 건너 살아남는다**(`Instant` 는 소멸).
    ///
    /// 판정 자체는 여기서 하지 않는다 — `state_of` 계약은 불변이고, 이 값을 관측
    /// 축과 합성하는 것은 `core/state/child_liveness.rs` 의 상위 계층이다.
    #[serde(default)]
    last_state_report_at: HashMap<u32, u64>,
    /// 영속화 경로 (load 시 결정, `default()` 는 None → 비영속·테스트용)
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl ChildTerminalRegistry {
    /// 호스트 데이터 디렉토리(`~/.tasty/child-terminals.json`)에서 로드. 부팅 시
    /// `CoreState::new_with_ids` 가 1회 호출한다. `TASTY_HOME` override 를 따르므로
    /// 테스트/샌드박스 격리도 자동으로 반영된다.
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
        self.last_state_report_at.remove(&removed.child_surface_id);
        Some(removed)
    }

    /// surface_id로 child를 찾아 제거한다. 미존재 시 false.
    ///
    /// 호스트의 능동 self-heal 은 접근 시점 `reconcile_with_live_surfaces` 로 처리하므로
    /// 현재 lib 경로에서 직접 호출되진 않지만(테스트에선 사용), 향후 surface-close 훅이
    /// 단건 정밀 정리를 원할 때를 위해 plugin `CodexState` 와 동형으로 보존한다.
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

    pub fn set_idle(&mut self, child_surface: u32, idle: bool) {
        self.idle.insert(child_surface, idle);
        if !idle {
            self.needs_input.insert(child_surface, false);
        }
        self.stamp_state_report(child_surface);
    }

    pub fn set_needs_input(&mut self, child_surface: u32, val: bool) {
        self.needs_input.insert(child_surface, val);
        self.stamp_state_report(child_surface);
    }

    /// hook push 를 받은 시각을 기록한다 — `set_idle`/`set_needs_input` 공통.
    fn stamp_state_report(&mut self, child_surface: u32) {
        self.last_state_report_at
            .insert(child_surface, now_epoch_ms());
    }

    /// 이 자식의 상태를 마지막으로 보고받은 시각(unix epoch ms). 등록 이력이 없거나
    /// 업그레이드 전에 영속된 registry 에서 로드된 항목은 `None`.
    pub fn last_state_report_at(&self, child_surface: u32) -> Option<u64> {
        self.last_state_report_at.get(&child_surface).copied()
    }

    /// 마지막 상태 보고 이후 경과 시간 — **hook 침묵 축**. `None` 이면 잴 기준점이
    /// 없다는 뜻이므로(판정 불가) 호출자는 임의 기본값을 만들지 않는다.
    ///
    /// 시계 되감김(NTP 보정·수동 변경)으로 `now_ms < 보고시각` 이 되면 `ZERO` 로
    /// 클램프한다 — 음수 경과시간이 침묵 판정을 뒤집는 것보다 "방금 보고받았다" 쪽
    /// 오탐이 안전하다(stale 오탐이 아니라 stale 미탐).
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

    /// 라이브 surface 집합과 registry 를 cross-check 한다. `live` 에 없는
    /// child_surface_id 를 모두 제거하고, 자식이 0명이 된 parent 의 `next_index` 와
    /// 빈 `children[parent]` vec 도 함께 정리한다. host-IPC-free — 단위 테스트 가능.
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

/// 현재 시각(unix epoch ms). registry 는 호스트 재시작을 건너 영속되므로 상태 보고
/// 시각은 프로세스 로컬한 `Instant` 가 아니라 벽시계여야 한다.
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
            .expect("등록이 기준점을 시딩한다");
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
        // 등록 없이 hook 만 와도 기준점이 생긴다(adopt 이전 push 등).
        s.set_idle(50, true);
        let first = s.last_state_report_at(50).unwrap();
        s.set_needs_input(50, true);
        let second = s.last_state_report_at(50).unwrap();
        assert!(second >= first);
        // active 로 되돌리는 push 도 축을 갱신한다 — hook 침묵 판정의 반증이다.
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
            "시계 되감김은 stale 오탐이 아니라 미탐 쪽으로 클램프"
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
        // 이 축 도입 이전에 영속된 파일에는 `last_state_report_at` 키가 없다.
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
            "기준점 부재는 판정 불가로 표현된다 — 임의값을 지어내지 않는다"
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
