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

/// `ClaudeState::reconcile_with_live_surfaces` 결과 요약. 부팅 시 reconcile 로그
/// 및 단위 테스트 검증에 사용. codex plugin 의 동명 struct 와 시그니처/의미 동일.
/// 향후 SDK 공통화 시 한 곳으로 합치기 위해 두 plugin 이 같은 형태 유지.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub removed_children: u32,
    pub removed_parents: u32,
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
    /// `claude.spawn`의 (parent_surface_id, workspace_id) → spawn_pane_id 매핑.
    /// 호스트 `engine_state.rs::spawn_panes`와 동일하게 in-memory only — 재시작 후
    /// 새 spawn pane이 만들어진다. pane이 사용자에 의해 닫혔는지는 매 사용 시점에
    /// IPC로 검증한다 (`pane.list`).
    #[serde(skip)]
    spawn_panes: HashMap<(u32, u32), u32>,
    /// surface → session-start 시각 (unix ms). stop/session-end 시 `wall_time_ms`
    /// 텔레메트리 기록에 사용. 재시작 시 휘발되므로 spans 가 끊겨도 누락만
    /// 발생하고 잘못된 값은 나오지 않는다.
    #[serde(skip)]
    wall_time_starts: HashMap<u32, u64>,
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
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("claude state dir {} create failed: {e}", parent.display());
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
        // surface_id 가 재활용되거나 호스트 재시작 사이에 옛 idle/needs_input
        // 잔재가 남아 있을 수 있다. 새 spawn 의 초기 상태는 *active* 가 invariant
        // 이므로 두 map 에서 명시적으로 제거한다 — 그렇지 않으면 `wait` polling
        // 첫 호출이 옛 잔재를 보고 즉시 "idle" 응답하는 false positive 발생.
        self.idle.remove(&child.child_surface_id);
        self.needs_input.remove(&child.child_surface_id);
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

    /// `session-start` 시각을 기록 — 호스트 재시작 시 휘발.
    pub fn mark_session_start(&mut self, surface: u32, ts_ms: u64) {
        self.wall_time_starts.insert(surface, ts_ms);
    }

    /// 기록된 session-start 시각을 꺼내고 (있으면) elapsed 를 반환.
    pub fn take_wall_time(&mut self, surface: u32, now_ms: u64) -> Option<u64> {
        let start = self.wall_time_starts.remove(&surface)?;
        Some(now_ms.saturating_sub(start))
    }

    /// `claude.spawn`의 (parent, workspace) → spawn_pane 조회.
    pub fn spawn_pane_for(&self, parent_surface_id: u32, workspace_id: u32) -> Option<u32> {
        self.spawn_panes
            .get(&(parent_surface_id, workspace_id))
            .copied()
    }

    /// 새 spawn pane id를 등록한다.
    pub fn set_spawn_pane(&mut self, parent_surface_id: u32, workspace_id: u32, pane_id: u32) {
        self.spawn_panes
            .insert((parent_surface_id, workspace_id), pane_id);
    }

    /// 사용자가 spawn pane을 닫았을 때 호출. 매핑을 제거해 다음 spawn에서
    /// 새 pane을 만들도록 한다.
    pub fn clear_spawn_pane(&mut self, parent_surface_id: u32, workspace_id: u32) {
        self.spawn_panes.remove(&(parent_surface_id, workspace_id));
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

    /// reconcile 전에 dead 가 될 child_surface_id 목록을 미리 모은다.
    /// `reconcile_with_live_surfaces` 가 이들을 제거한 후, 호출자는 ErrorScanner
    /// 등 외부 부수효과 정리에 본 목록을 사용한다 (codex 와 달리 claude 는
    /// error_scan thread 가 surface 별 enable/disable 을 갖기 때문).
    pub fn collect_stale_child_surface_ids(&self, live: &HashSet<u32>) -> Vec<u32> {
        self.children
            .values()
            .flat_map(|v| v.iter())
            .map(|c| c.child_surface_id)
            .filter(|sid| !live.contains(sid))
            .collect()
    }

    /// 호스트의 살아있는 surface 집합과 child registry 를 cross-check 한다.
    /// `live` 에 없는 child_surface_id 를 모두 제거하고, 그 결과 자식이 0명이 된
    /// parent 의 `last_index` / `closed_parents` 키도 함께 정리한다. parent key
    /// 자체가 dead 인데 살아있는 자식이 남은 경우 → `closed_parents` 마킹
    /// (`mark_parent_closed` 와 동일 의미 — 마지막 자식이 빠지면 deferred GC).
    ///
    /// host-IPC-free — 단위 테스트 가능. 부팅 시 `on_start` 가 호스트에서 받은
    /// surface 목록으로 `HashSet` 을 구성해 본 메서드에 위임한다.
    ///
    /// codex plugin 의 동명 메서드와 시그니처/의미가 동일해야 두 plugin 의 동작
    /// 일관성이 유지된다 (향후 SDK 공통화 후보). claude 특유 정리:
    /// `closed_parents` cleanup + dead parent 마킹.
    pub fn reconcile_with_live_surfaces(&mut self, live: &HashSet<u32>) -> ReconcileSummary {
        let mut summary = ReconcileSummary::default();

        // ── Step 1: stale child 제거 + 빈 parent 수집.
        let mut dead_parents: Vec<u32> = Vec::new();
        for (parent, list) in self.children.iter_mut() {
            let before = list.len();
            list.retain(|c| live.contains(&c.child_surface_id));
            summary.removed_children += (before - list.len()) as u32;
            if list.is_empty() {
                dead_parents.push(*parent);
            }
        }

        // ── Step 2: 자식 0명이 된 parent 정리 (children, last_index, closed_parents).
        for parent in &dead_parents {
            self.children.remove(parent);
            // 자식이 0명이 된 parent 의 last_index 키도 정리. 살아있는 자식이 1명
            // 이상 남은 parent 의 last_index 는 단조 증가 invariant 보존을 위해
            // 건드리지 않는다 (사용자/agent 가 외운 child index 가 reconcile 직후
            // 다른 child 에 재할당되는 surprise 방지).
            self.last_index.remove(parent);
            self.closed_parents.remove(parent);
            // parent surface 자체도 live 가 아니면 — host 가 surface.closed 발화
            // 누락한 케이스이므로 — removed_parents 로 카운트.
            if !live.contains(parent) {
                summary.removed_parents += 1;
            }
        }

        // ── Step 3: dead parent + 살아있는 자식이 남은 케이스 → closed_parents 마킹.
        // `mark_parent_closed` 의 의미 보존 — 마지막 자식이 빠지면 deferred GC.
        let dead_parents_with_live_children: Vec<u32> = self
            .children
            .iter()
            .filter(|(p, l)| !live.contains(p) && !l.is_empty())
            .map(|(p, _)| *p)
            .collect();
        for parent in dead_parents_with_live_children {
            self.closed_parents.insert(parent);
        }

        // ── Step 4: 보조 map 동기화. parent_of/idle/needs_input 은 surface_id 가
        // live 에 없으면 잔재이므로 제거. closed_parents 는 children map 에 여전히
        // key 가 있는 경우만 유효.
        self.parent_of.retain(|sid, _| live.contains(sid));
        self.idle.retain(|sid, _| live.contains(sid));
        self.needs_input.retain(|sid, _| live.contains(sid));
        let valid_parents: HashSet<u32> = self.children.keys().copied().collect();
        self.closed_parents.retain(|p| valid_parents.contains(p));

        summary
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

    #[test]
    fn spawn_pane_round_trip() {
        let mut s = ClaudeState::default();
        // 등록 전 None.
        assert_eq!(s.spawn_pane_for(10, 1), None);
        // 등록 → 조회.
        s.set_spawn_pane(10, 1, 42);
        assert_eq!(s.spawn_pane_for(10, 1), Some(42));
        // 다른 (parent, ws) 키는 영향 없음.
        assert_eq!(s.spawn_pane_for(11, 1), None);
        assert_eq!(s.spawn_pane_for(10, 2), None);
        // 같은 키 재등록은 덮어쓰기.
        s.set_spawn_pane(10, 1, 43);
        assert_eq!(s.spawn_pane_for(10, 1), Some(43));
        // 삭제.
        s.clear_spawn_pane(10, 1);
        assert_eq!(s.spawn_pane_for(10, 1), None);
    }

    #[test]
    fn spawn_panes_skipped_from_serialization() {
        // 호스트도 in-memory only이므로 plugin 재시작 후엔 새로 만들어진다.
        let mut s = ClaudeState::default();
        s.set_spawn_pane(10, 1, 42);
        let text = serde_json::to_string(&s).unwrap();
        assert!(
            !text.contains("spawn_panes"),
            "spawn_panes should be #[serde(skip)], got: {text}"
        );
        let restored: ClaudeState = serde_json::from_str(&text).unwrap();
        assert_eq!(restored.spawn_pane_for(10, 1), None);
    }

    fn live(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn reconcile_removes_stale_children_and_keeps_live() {
        let mut s = ClaudeState::default();
        // 프로덕션 spawn 과 동일 흐름: next_child_index 로 last_index 도 함께 bump.
        let i1 = s.next_child_index(10);
        s.register_child(10, entry(100, i1));
        let i2 = s.next_child_index(10);
        s.register_child(10, entry(101, i2)); // stale
        let i3 = s.next_child_index(10);
        s.register_child(10, entry(102, i3));
        s.set_idle(101, true);
        s.set_needs_input(101, true);

        let summary = s.reconcile_with_live_surfaces(&live(&[10, 100, 102]));

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 0);
        assert!(s.find_child(10, 2).is_none());
        assert!(s.find_child(10, 1).is_some());
        assert!(s.find_child(10, 3).is_some());
        assert_eq!(s.parent_of_child(101), None);
        // idle/needs_input 모두 stale sid 에 대해 정리됨.
        assert_eq!(s.state_of(101), "active");
        // last_index 보존 (살아있는 자식 잔존 parent) → 다음 spawn 은 4.
        assert_eq!(s.next_child_index(10), 4);
    }

    #[test]
    fn reconcile_gc_dead_parent_with_no_live_children() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        s.register_child(10, entry(101, 2));
        // live = empty → parent 10 도, 자식 100/101 도 모두 dead.

        let summary = s.reconcile_with_live_surfaces(&live(&[]));

        assert_eq!(summary.removed_children, 2);
        assert_eq!(summary.removed_parents, 1);
        assert!(!s.is_known_parent(10));
        // last_index 도 통째 GC → next_child_index 가 1 부터 재시작 (1-based).
        assert_eq!(s.next_child_index(10), 1);
    }

    #[test]
    fn reconcile_dead_parent_with_live_children_marks_closed() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        s.register_child(10, entry(101, 2));
        // parent 10 은 dead, child 100 만 살아 있음.

        let summary = s.reconcile_with_live_surfaces(&live(&[100]));

        assert_eq!(summary.removed_children, 1);
        assert_eq!(summary.removed_parents, 0);
        assert!(s.is_known_parent(10));
        // dead parent + live child → closed_parents 마킹 (mark_parent_closed 의미 보존).
        assert!(s.is_parent_closed(10));
        assert_eq!(s.list_children(10).len(), 1);
        // 마지막 자식이 unregister 되면 parent 도 자동 정리되는 의미 보존 확인.
        s.unregister_child(100);
        assert!(!s.is_known_parent(10));
    }

    #[test]
    fn reconcile_cleans_orphan_closed_parents_and_idle() {
        let mut s = ClaudeState::default();
        // 비어 있는 parent 10 이 closed_parents 에만 남아 있는 상태 (state.json
        // 로딩 후 부분적으로 inconsistent 한 상황을 모사).
        s.register_child(10, entry(100, 1));
        s.mark_parent_closed(10);
        s.unregister_child(100);
        // unregister 가 list 가 비고 closed_parents 에 있으면 정리하지만, 만약
        // 외부에서 직접 closed_parents 만 남겨놓은 경우 reconcile 가 정리해야 함.
        s.closed_parents.insert(99); // 어떤 children 에도 없는 잔재
        s.set_idle(999, true); // 어떤 surface 에도 없는 잔재
        s.set_needs_input(888, true);
        s.register_child(20, entry(200, 1));

        let _ = s.reconcile_with_live_surfaces(&live(&[20, 200])); // 본 테스트는 summary 가 아닌 부수효과(orphan cleanup) 검증

        // 잔재 closed_parents 정리됨.
        assert!(!s.is_parent_closed(99));
        // 잔재 idle/needs_input 정리됨.
        assert_eq!(s.state_of(999), "active");
        assert_eq!(s.state_of(888), "active");
        // 살아있는 entry 는 보존.
        assert!(s.find_child(20, 1).is_some());
    }

    #[test]
    fn reconcile_summary_counts_multiple_parents() {
        let mut s = ClaudeState::default();
        // parent 10: child 100, 101 → 둘 다 dead, parent 도 dead.
        s.register_child(10, entry(100, 1));
        s.register_child(10, entry(101, 2));
        // parent 20: child 200 live, 201 dead. parent 도 live.
        s.register_child(20, entry(200, 1));
        s.register_child(20, entry(201, 2));
        // parent 30: live, 자식 모두 dead → list 비고 parent 는 live 라서
        // removed_parents 는 카운트하지 않지만 children 키와 last_index 는 정리.
        s.register_child(30, entry(300, 1));

        let summary = s.reconcile_with_live_surfaces(&live(&[20, 30, 200]));

        assert_eq!(summary.removed_children, 4); // 100, 101, 201, 300
        assert_eq!(summary.removed_parents, 1); // parent 10 만 dead-and-empty
        assert_eq!(s.list_children(20).len(), 1);
        assert_eq!(s.list_children(10).len(), 0);
        assert_eq!(s.list_children(30).len(), 0);
    }

    #[test]
    fn collect_stale_child_surface_ids_returns_dead_only() {
        let mut s = ClaudeState::default();
        s.register_child(10, entry(100, 1));
        s.register_child(10, entry(101, 2));
        s.register_child(20, entry(200, 1));

        let stale = s.collect_stale_child_surface_ids(&live(&[10, 100, 20]));
        let mut sorted = stale.clone();
        sorted.sort();
        assert_eq!(sorted, vec![101, 200]);
    }
}
