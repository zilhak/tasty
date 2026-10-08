//! 자식 터미널 surface의 부모·번호·보고된 상태를 보관하고 child-terminals.json에 저장한다.
//! SessionToken의 권한 위임이나 task runner의 자식 프로세스 관리는 이 레지스트리의 역할이 아니다.
//! 호출자가 실제 surface 목록과 대조해 없어진 항목을 정리한다.
//!
//! 윈도우(engine)마다 이 레지스트리를 따로 읽는다. surface 는 한 engine 에만 있으므로 각 engine 은
//! 자기 레이아웃 슬롯의 항목과, 자기가 등록·변경했거나 살아 있는 것을 본 surface 의 항목만 소유한다.
//! 저장할 때 파일을 다시 읽어 소유한 항목은 메모리 값으로, 나머지는 파일 값 그대로 합친다. 정리도
//! 소유한 항목만 지운다. 그래서 한 윈도우의 저장·정리가 다른 윈도우나 아직 열지 않은 윈도우의 관계를
//! 지우지 않는다. 어느 창에도 돌아오지 않을 항목은 시작할 때([`prune_on_boot`])와 engine 이 사라질 때
//! ([`ChildTerminalRegistry::release_owned`]) 지운다.

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
    /// 항목 surface 가 속한 레이아웃 슬롯. 슬롯 engine 의 소유 판정과 시작 시 정리에 쓴다.
    #[serde(default)]
    slot_of: HashMap<u32, u32>,
    /// 이 engine 이 소유한 surface. 저장 병합과 정리의 범위를 정한다.
    #[serde(skip)]
    owned: HashSet<u32>,
    /// 이 engine 의 레이아웃 슬롯. [`Self::bind_slot`] 로 정한다.
    #[serde(skip)]
    slot: Option<u32>,
}

impl ChildTerminalRegistry {
    /// TASTY_HOME 아래에서 읽는다. 파일 읽기·파싱 실패는 빈 상태로 처리한다.
    pub fn load() -> Self {
        let path = tasty_utils::path::tasty_home().map(|d| d.join("child-terminals.json"));
        let mut s = path.as_deref().map(read_file).unwrap_or_default();
        s.path = path;
        s
    }

    /// 파일을 다시 읽어 다른 engine 의 항목을 받아 합친 뒤 쓴다. 메모리도 합친 결과로 바꾼다.
    pub fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.merge_from(read_file(&path));
        ensure_parent_dir(&path);
        self.write_json_to(&path);
    }

    /// 이 engine 의 슬롯을 정하고 그 슬롯의 항목을 소유한다. 슬롯이 없는 engine(headless)은 한
    /// 프로세스에 하나뿐이므로 읽은 항목을 모두 소유한다.
    pub(crate) fn bind_slot(&mut self, slot: Option<u32>) {
        self.slot = slot;
        let mine: Vec<u32> = self
            .surfaces()
            .into_iter()
            .filter(|s| slot.is_none() || self.slot_of.get(s) == slot.as_ref())
            .collect();
        self.owned.extend(mine);
    }

    /// 항목 어디에든 나오는 surface.
    fn surfaces(&self) -> HashSet<u32> {
        let mut all: HashSet<u32> = self.children.keys().copied().collect();
        all.extend(self.parent_of.keys().copied());
        all.extend(self.parent_of.values().copied());
        all.extend(self.next_index.keys().copied());
        all.extend(self.idle.keys().copied());
        all.extend(self.needs_input.keys().copied());
        all.extend(self.last_state_report_at.keys().copied());
        all
    }

    /// 소유한 surface 의 항목은 그대로 두고, 나머지는 `disk` 의 값으로 바꾼다.
    fn merge_from(&mut self, mut disk: Self) {
        let present = self.surfaces();
        self.slot_of.retain(|surface, _| present.contains(surface));
        if let Some(slot) = self.slot {
            for surface in present.intersection(&self.owned) {
                self.slot_of.insert(*surface, slot);
            }
        }
        let owned = &self.owned;
        let others = |key: &u32| !owned.contains(key);
        disk.parent_of.retain(|child, _| others(child));
        for (child, parent) in std::mem::take(&mut self.parent_of) {
            if owned.contains(&child) {
                disk.parent_of.insert(child, parent);
            }
        }
        for list in disk.children.values_mut() {
            list.retain(|c| others(&c.child_surface_id));
        }
        let my_parents: HashSet<u32> = self.children.keys().copied().collect();
        for (parent, list) in std::mem::take(&mut self.children) {
            let mine = list
                .into_iter()
                .filter(|c| owned.contains(&c.child_surface_id));
            disk.children.entry(parent).or_default().extend(mine);
        }
        // 이 engine 이 지운 부모 목록은 파일에서도 지운다. 다른 engine 의 빈 목록은 그대로 둔다.
        disk.children.retain(|parent, list| {
            !list.is_empty() || my_parents.contains(parent) || others(parent)
        });
        for list in disk.children.values_mut() {
            list.sort_by_key(|c| c.index);
        }
        merge_owned(&mut disk.next_index, &mut self.next_index, owned);
        merge_owned(&mut disk.idle, &mut self.idle, owned);
        merge_owned(&mut disk.needs_input, &mut self.needs_input, owned);
        merge_owned(
            &mut disk.last_state_report_at,
            &mut self.last_state_report_at,
            owned,
        );
        merge_owned(&mut disk.slot_of, &mut self.slot_of, owned);
        disk.path = self.path.take();
        disk.owned = std::mem::take(&mut self.owned);
        disk.slot = self.slot;
        *self = disk;
    }

    /// engine 이 사라질 때 그 engine 이 소유한 항목(빈 부모 목록·다음 번호 포함)을 지우고 저장한다.
    #[cfg(feature = "gui")]
    pub(crate) fn release_owned(&mut self) {
        let gone = self.owned.clone();
        self.drop_surfaces(&gone);
        self.save();
    }

    /// `gone` surface 의 자식 항목·상태와, 그래서 빈 `gone` 부모의 목록·다음 번호를 지운다.
    #[cfg(feature = "gui")]
    fn drop_surfaces(&mut self, gone: &HashSet<u32>) {
        for list in self.children.values_mut() {
            list.retain(|c| !gone.contains(&c.child_surface_id));
        }
        self.children
            .retain(|parent, list| !(gone.contains(parent) && list.is_empty()));
        let children = &self.children;
        self.next_index
            .retain(|parent, _| !gone.contains(parent) || children.contains_key(parent));
        for map in [&mut self.idle, &mut self.needs_input] {
            map.retain(|surface, _| !gone.contains(surface));
        }
        self.parent_of.retain(|child, _| !gone.contains(child));
        self.last_state_report_at
            .retain(|surface, _| !gone.contains(surface));
        self.slot_of.retain(|surface, _| !gone.contains(surface));
    }

    fn own(&mut self, surface: u32) {
        self.owned.insert(surface);
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

    pub(crate) fn reserve_index(&mut self, parent: u32) -> Result<u32, String> {
        self.own(parent);
        let entry = self.next_index.entry(parent).or_insert(0);
        let index = *entry;
        *entry = entry.checked_add(1).ok_or("child index space exhausted")?;
        self.save();
        Ok(index)
    }

    pub fn next_index_for(&mut self, parent: u32) -> u32 {
        self.own(parent);
        let entry = self.next_index.entry(parent).or_insert(0);
        let idx = *entry;
        *entry += 1;
        idx
    }

    pub fn register_child(&mut self, parent: u32, child: ChildEntry) {
        self.own(parent);
        self.own(child.child_surface_id);
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
        self.owned.insert(entry.child_surface_id);
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
        self.owned.insert(removed.child_surface_id);
        self.parent_of.remove(&removed.child_surface_id);
        self.idle.remove(&removed.child_surface_id);
        self.needs_input.remove(&removed.child_surface_id);
        self.last_state_report_at.remove(&removed.child_surface_id);
        Some(removed)
    }

    /// surface ID로 자식을 제거한다. 항목이 없으면 false다.
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
        self.owned.insert(surface_id);
        self.parent_of.remove(&surface_id);
        self.idle.remove(&surface_id);
        self.needs_input.remove(&surface_id);
        self.last_state_report_at.remove(&surface_id);
        true
    }

    /// idle 값과 무관하게 needs_input도 해제한다. 대기 플래그가 남아 이후 idle·active를 가리지 않게 한다.
    pub fn set_idle(&mut self, child_surface: u32, idle: bool) {
        self.own(child_surface);
        self.idle.insert(child_surface, idle);
        self.needs_input.insert(child_surface, false);
        self.stamp_state_report(child_surface);
    }

    pub fn set_needs_input(&mut self, child_surface: u32, val: bool) {
        self.own(child_surface);
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

    /// 이 창의 부모 가운데 자식이 있는 것이 정확히 하나일 때만 ID를 반환한다.
    /// 이 창의 부모는 이 engine 이 소유했거나 `live`(이 engine 의 살아 있는 surface)에 있는 부모다.
    /// 저장 병합으로 받은 다른 창의 항목은 세지 않는다.
    pub fn single_parent(&self, live: &HashSet<u32>) -> Option<u32> {
        let parents: Vec<u32> = self
            .children
            .iter()
            .filter(|(parent, list)| {
                !list.is_empty() && (self.owned.contains(parent) || live.contains(parent))
            })
            .map(|(k, _)| *k)
            .collect();
        if parents.len() == 1 {
            Some(parents[0])
        } else {
            None
        }
    }

    /// `live` 는 이 engine 의 살아 있는 surface 다. 이 engine 이 소유했는데 `live` 에 없는 자식과 상태를
    /// 지우고, 그래서 비거나 원래 빈 소유 부모의 목록과 다음 번호도 지운다. 다른 engine 이나 아직 열지
    /// 않은 윈도우의 surface 는 `live` 에 없어도 지우지 않는다.
    /// 부모 자체의 생존 여부만으로 살아 있는 자식을 지우지는 않는다.
    pub fn reconcile_with_live_surfaces(&mut self, live: &HashSet<u32>) -> ReconcileSummary {
        self.owned.extend(live.iter().copied());
        let owned = &self.owned;
        let dead = |sid: &u32| owned.contains(sid) && !live.contains(sid);
        let mut summary = ReconcileSummary::default();

        let mut dead_parents: Vec<u32> = Vec::new();
        for (parent, list) in self.children.iter_mut() {
            let before = list.len();
            list.retain(|c| !dead(&c.child_surface_id));
            let removed = before - list.len();
            summary.removed_children += removed as u32;
            if list.is_empty() && owned.contains(parent) {
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

        self.parent_of.retain(|sid, _| !dead(sid));
        self.idle.retain(|sid, _| !dead(sid));
        self.needs_input.retain(|sid, _| !dead(sid));
        self.last_state_report_at.retain(|sid, _| !dead(sid));
        self.slot_of.retain(|sid, _| !dead(sid));

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

/// 첫 engine 을 만들기 전에 한 번 부른다. 레이아웃을 복원하지 않으면(`restore_layout = false`)
/// 이전 실행의 surface 는 어느 창에도 돌아오지 않으므로 관계를 모두 버린다. 복원하면 `known_slots`
/// (복원할 수 있는 슬롯) 밖의 슬롯에 속한 항목을 지운다. 슬롯 표시가 없는 이전 형식의 항목은 첫 창의
/// 슬롯 `boot_slot` 에 속한 것으로 본다(그 창의 첫 정리가 살아 있지 않은 것을 지운다).
#[cfg(feature = "gui")]
pub(crate) fn prune_on_boot(restore_layout: bool, known_slots: &HashSet<u32>, boot_slot: u32) {
    let Some(path) = tasty_utils::path::tasty_home().map(|d| d.join("child-terminals.json")) else {
        return;
    };
    if !path.exists() {
        return;
    }
    let registry = if restore_layout {
        let mut registry = read_file(&path);
        for surface in registry.surfaces() {
            registry.slot_of.entry(surface).or_insert(boot_slot);
        }
        let gone: HashSet<u32> = registry
            .slot_of
            .iter()
            .filter(|(_, slot)| **slot != boot_slot && !known_slots.contains(slot))
            .map(|(surface, _)| *surface)
            .collect();
        registry.drop_surfaces(&gone);
        registry
    } else {
        ChildTerminalRegistry::default()
    };
    ensure_parent_dir(&path);
    registry.write_json_to(&path);
}

/// 파일 읽기·파싱 실패는 빈 상태로 처리한다.
fn read_file(path: &std::path::Path) -> ChildTerminalRegistry {
    if !path.exists() {
        return ChildTerminalRegistry::default();
    }
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("child-terminal registry parse failed: {e}");
            ChildTerminalRegistry::default()
        }),
        Err(e) => {
            tracing::warn!("child-terminal registry read failed: {e}");
            ChildTerminalRegistry::default()
        }
    }
}

/// `owned` 키는 `mine` 의 값(없으면 삭제)으로, 나머지는 `disk` 의 값으로 둔다.
fn merge_owned<V>(disk: &mut HashMap<u32, V>, mine: &mut HashMap<u32, V>, owned: &HashSet<u32>) {
    disk.retain(|key, _| !owned.contains(key));
    for (key, value) in mine.drain() {
        if owned.contains(&key) {
            disk.insert(key, value);
        }
    }
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
        assert_eq!(s.single_parent(&HashSet::new()), None);
        s.register_child(10, entry(100, 0));
        assert_eq!(s.single_parent(&HashSet::new()), Some(10));
        s.register_child(20, entry(200, 0));
        assert_eq!(s.single_parent(&HashSet::new()), None);
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

    fn live(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    fn children_on_disk() -> Vec<(u32, u32)> {
        let disk = ChildTerminalRegistry::load();
        let mut pairs: Vec<(u32, u32)> = disk.parent_of.iter().map(|(c, p)| (*p, *c)).collect();
        pairs.sort();
        pairs
    }

    /// 윈도우 두 개가 각자 읽은 레지스트리로 자식을 등록해도 저장이 서로를 지우지 않는다.
    #[test]
    fn children_registered_in_two_windows_both_survive_a_restart() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut first = ChildTerminalRegistry::load();
        let mut second = ChildTerminalRegistry::load();
        let index = first.next_index_for(513);
        first.register_child(513, entry(1027, index));
        first.save();
        let index = second.next_index_for(1026);
        second.register_child(1026, entry(1028, index));
        second.save();
        assert_eq!(children_on_disk(), [(513, 1027), (1026, 1028)]);
        assert_eq!(
            second.list_children(513).len(),
            1,
            "save also refreshes the copy"
        );

        // 지운 것도 자기 항목만 파일에 반영한다.
        assert!(first.unregister_child_by_surface(1027));
        first.save();
        assert_eq!(children_on_disk(), [(1026, 1028)]);

        // 재시작 뒤 아직 정리를 거치지 않은 윈도우가 surface 를 닫아도 그 해제가 파일에 남는다.
        let mut restarted = ChildTerminalRegistry::load();
        assert!(restarted.unregister_child_by_surface(1028));
        restarted.save();
        assert_eq!(children_on_disk(), []);
    }

    /// 재시작 뒤 먼저 연 윈도우의 정리는 아직 열지 않은 윈도우의 관계를 지우지 않는다.
    #[test]
    fn reconcile_keeps_children_of_windows_it_does_not_own() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut before = ChildTerminalRegistry::load();
        before.register_child(513, entry(1027, 0));
        before.register_child(1026, entry(1028, 0));
        before.save();

        let mut first = ChildTerminalRegistry::load();
        let summary = first.reconcile_with_live_surfaces(&live(&[513, 1027]));
        assert!(!summary.changed());
        first.save();
        assert_eq!(children_on_disk(), [(513, 1027), (1026, 1028)]);

        // 소유한 surface 가 사라지면 그것만 지운다.
        let summary = first.reconcile_with_live_surfaces(&live(&[513]));
        assert_eq!(summary.removed_children, 1);
        first.save();
        assert_eq!(children_on_disk(), [(1026, 1028)]);
    }

    /// 슬롯 `slot` 의 창이 읽은 레지스트리.
    #[cfg(feature = "gui")]
    fn window(slot: u32) -> ChildTerminalRegistry {
        let mut registry = ChildTerminalRegistry::load();
        registry.bind_slot(Some(slot));
        registry
    }

    /// 다른 창의 항목을 저장 병합으로 받아도 부모 생략 폴백은 이 창의 부모만 센다. 결과가 마지막 저장
    /// 시점에 따라 달라지지 않는다.
    #[cfg(feature = "gui")]
    #[test]
    fn single_parent_counts_only_this_windows_parents_after_a_merge() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut first = window(1);
        let mut second = window(2);
        first.register_child(513, entry(1027, 0));
        first.save();
        second.register_child(1026, entry(1028, 0));
        second.save();
        assert_eq!(first.single_parent(&live(&[513, 1027])), Some(513));

        // 첫 창이 다시 저장해 둘째 창의 항목을 받은 뒤에도 같다.
        first.set_idle(1027, true);
        first.save();
        assert_eq!(
            first.list_children(1026).len(),
            1,
            "merged the other window"
        );
        assert_eq!(first.single_parent(&live(&[513, 1027])), Some(513));
        assert_eq!(second.single_parent(&live(&[1026, 1028])), Some(1026));

        // 재시작 뒤 아직 정리하지 않은 창도 자기 슬롯의 부모만 센다.
        assert_eq!(window(1).single_parent(&live(&[])), Some(513));
        assert_eq!(window(2).single_parent(&live(&[])), Some(1026));
    }

    /// 레이아웃을 복원하지 않으면 재시작마다 이전 실행의 관계를 버린다. 다시 열리지 않는 둘째 창의
    /// 관계도 남지 않으므로 유령이 쌓여 부모 생략 폴백을 막지 않는다.
    #[cfg(feature = "gui")]
    #[test]
    fn relations_do_not_pile_up_across_restarts_without_layout_restore() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut second = window(2);
        second.register_child(1026, entry(1029, 0));
        second.save();
        for (parent, child) in [(1027, 1028), (1541, 1542), (2055, 2056)] {
            prune_on_boot(false, &HashSet::new(), 1);
            let mut registry = window(1);
            registry.reconcile_with_live_surfaces(&live(&[parent, child]));
            registry.register_child(parent, entry(child, 0));
            registry.save();
            assert_eq!(
                registry.single_parent(&live(&[parent, child])),
                Some(parent)
            );
            assert_eq!(children_on_disk(), [(parent, child)]);
        }
    }

    /// 레이아웃을 복원하면 다시 열 수 있는 슬롯의 관계만 남긴다. 슬롯 표시가 없는 이전 형식의 항목은
    /// 첫 창의 슬롯에 속한다.
    #[cfg(feature = "gui")]
    #[test]
    fn boot_prune_keeps_relations_of_slots_that_can_reopen() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut legacy = ChildTerminalRegistry::load();
        legacy.register_child(7, entry(8, 0));
        legacy.save();
        let mut second = window(2);
        second.register_child(1026, entry(1028, 0));
        second.save();
        let mut third = window(3);
        third.register_child(1540, entry(1542, 0));
        third.save();

        prune_on_boot(true, &[1, 2].into_iter().collect(), 1);
        assert_eq!(children_on_disk(), [(7, 8), (1026, 1028)]);
        assert_eq!(window(1).single_parent(&live(&[])), Some(7));
    }

    /// 다시 열지 않는 창이 닫히면 그 창의 관계와 빈 부모 목록·다음 번호를 지우고 다른 창 것은 둔다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_released_window_removes_its_parent_keys() {
        let _home = tasty_test_support::IsolatedHome::new();
        let mut first = window(1);
        let index = first.next_index_for(513);
        first.register_child(513, entry(1027, index));
        first.save();
        let mut second = window(2);
        let index = second.next_index_for(1026);
        second.register_child(1026, entry(1028, index));
        assert!(second.unregister_child_by_surface(1028));
        second.save();
        let on_disk = ChildTerminalRegistry::load();
        assert!(
            on_disk.next_index.contains_key(&1026),
            "keys stay while open"
        );

        second.release_owned();
        let on_disk = ChildTerminalRegistry::load();
        assert_eq!(children_on_disk(), [(513, 1027)]);
        assert!(!on_disk.children.contains_key(&1026));
        assert!(!on_disk.next_index.contains_key(&1026));
        assert!(on_disk.next_index.contains_key(&513));
    }
}
