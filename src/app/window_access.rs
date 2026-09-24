//! 모달을 제외한 MainView와 engine의 대상 자원을 찾는다.

use std::collections::HashSet;

use winit::window::WindowId;

use crate::app::App;
use crate::core::layout_persistence::LayoutSlotId;
use crate::view;

impl App {
    pub(crate) fn focused_window(&self) -> Option<&view::main::MainView> {
        self.view
            .focused_view_id
            .and_then(|id| self.view.views.get(&id))
            .and_then(|w| w.as_main())
    }

    pub(crate) fn focused_window_mut(&mut self) -> Option<&mut view::main::MainView> {
        self.view
            .focused_view_id
            .and_then(|id| self.view.views.get_mut(&id))
            .and_then(|w| w.as_main_mut())
    }

    /// 안내는 포커스가 모달에 있어도 다른 MainView에 표시할 수 있다.
    pub(crate) fn notice_window_mut(&mut self) -> Option<&mut view::main::MainView> {
        let id = match self.focused_window() {
            Some(_) => self.view.focused_view_id,
            None => self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id),
        }?;
        self.view.views.get_mut(&id).and_then(|w| w.as_main_mut())
    }

    pub(crate) fn main_windows_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut view::main::MainView> {
        self.view.views.values_mut().filter_map(|w| w.as_main_mut())
    }

    pub(crate) fn main_window_count(&self) -> usize {
        self.view
            .views
            .values()
            .filter(|w| w.as_main().is_some())
            .count()
    }

    /// 새 engine이 공용 레지스트리·ID 발급기를 공유할 기존 engine을 찾는다. 창을 먼저, parked를 나중에 본다.
    pub(crate) fn any_main_engine(&self) -> Option<&crate::core::CoreState> {
        for w in self.view.views.values() {
            if let Some(m) = w.as_main() {
                return Some(&m.core_state);
            }
        }
        self.parked_states.first().map(|(_, e)| e)
    }

    /// 점유를 별도로 관리하지 않고 살아 있는 engine의 슬롯을 모은다.
    /// 창에 등록되기 전 임시 App.core_state도 포함해야 중복 배정을 피할 수 있다.
    pub(crate) fn occupied_layout_slots(&self) -> HashSet<LayoutSlotId> {
        let mut occupied = HashSet::new();
        for w in self.view.views.values() {
            if let Some(m) = w.as_main()
                && let Some(slot) = m.core_state.layout_slot
            {
                occupied.insert(slot);
            }
        }
        for (_, engine) in &self.parked_states {
            if let Some(slot) = engine.layout_slot {
                occupied.insert(slot);
            }
        }
        if let Some(engine) = self.core_state.as_ref()
            && let Some(slot) = engine.layout_slot
        {
            occupied.insert(slot);
        }
        occupied
    }

    /// 빈 슬롯을 고르기만 한다. engine이 만들어져야 실제 점유로 센다.
    pub(crate) fn claim_free_layout_slot(&self) -> LayoutSlotId {
        pick_free_slot(
            &crate::core::layout_persistence::list_slots(),
            &self.occupied_layout_slots(),
        )
    }

    pub(crate) fn find_main_with_surface(&self, surface_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.has_surface(surface_id)
            {
                return Some(*wid);
            }
        }
        None
    }

    pub(crate) fn find_main_with_workspace(&self, workspace_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.has_workspace(workspace_id)
            {
                return Some(*wid);
            }
        }
        None
    }

    /// mirror workspace의 engine이 창 또는 parked 상태에 남아 있으면 세션을 유지한다.
    /// cleanup_mirror_workspace·mirror_output_host와 같은 범위를 찾아야 한다.
    /// parked 조회는 find_parked_with_workspace를 공유하지만 창 있는 engine의 순회는 별도다.
    /// 세 경로의 집합이 같은지 자동 비교하는 검사는 없어 함께 검토해야 한다.
    /// 임시 App.core_state는 제외한다. start_gui_attach는 실제 창에만 mirror를 만들고
    /// 임시 engine에는 정리 후 active_workspace를 보정할 AppState도 없다. 이 조건이 바뀌면 세 경로를 함께 고친다.
    pub(crate) fn mirror_workspace_engine_alive(&self, workspace_id: u32) -> bool {
        any_engine_has_workspace(
            self.view
                .views
                .values()
                .filter_map(|w| w.as_main())
                .map(|m| &m.core_state),
            &self.parked_states,
            workspace_id,
        )
    }

    pub(crate) fn find_main_with_pane(&self, pane_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.has_pane(pane_id)
            {
                return Some(*wid);
            }
        }
        None
    }

    pub(crate) fn find_main_with_tab(&self, tab_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.find_pane_for_tab(tab_id).is_some()
            {
                return Some(*wid);
            }
        }
        None
    }

    /// PTY 레지스트리는 engine별로 있으므로 ID의 소유 창을 전체 MainView에서 찾는다.
    pub(crate) fn find_main_with_headless_pty(&self, pty_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.pty_registry.contains(pty_id)
            {
                return Some(*wid);
            }
        }
        None
    }

    pub(crate) fn find_main_with_resource(
        &self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<WindowId> {
        use crate::core::request_target::Kind;
        // 범위를 넘는 ID를 잘라 다른 자원으로 바꾸지 않는다.
        let narrow = u32::try_from(rid.id).ok();
        match rid.kind {
            Kind::Surface => narrow.and_then(|id| self.find_main_with_surface(id)),
            Kind::Workspace => narrow.and_then(|id| self.find_main_with_workspace(id)),
            Kind::Pane => narrow.and_then(|id| self.find_main_with_pane(id)),
            Kind::Tab => narrow.and_then(|id| self.find_main_with_tab(id)),
            Kind::HeadlessPty => narrow.and_then(|id| self.find_main_with_headless_pty(id)),
            Kind::Hook | Kind::GlobalHook | Kind::Observer | Kind::Category => {
                self.find_main_with_engine_resource(rid)
            }
        }
    }

    /// hook·observer 등은 parked 라우팅과 같은 engine_has_resource 판정을 사용한다.
    fn find_main_with_engine_resource(
        &self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && crate::core::request_target::engine_has_resource(&m.core_state, rid)
            {
                return Some(*wid);
            }
        }
        None
    }

    /// 전체 창에서 숫자 ID를 먼저 찾고 없으면 이름을 찾는다.
    /// 같은 이름이 여러 창에 있으면 HashMap 순회 순서로 고르지 않고 거절한다.
    pub(crate) fn find_main_with_workspace_target(
        &self,
        target: &str,
    ) -> Result<Option<WindowId>, String> {
        if let Ok(id) = target.parse::<u32>()
            && let Some(wid) = self.find_main_with_workspace(id)
        {
            return Ok(Some(wid));
        }
        find_workspace_by_name(
            self.view
                .views
                .iter()
                .filter_map(|(wid, w)| w.as_main().map(|m| (*wid, &m.core_state))),
            target,
        )
    }
}

/// 이름이 일치하는 창이 없으면 Ok(None), 하나면 해당 창, 여럿이면 Err다.
fn find_workspace_by_name<'a>(
    windows: impl Iterator<Item = (WindowId, &'a crate::core::CoreState)>,
    target: &str,
) -> Result<Option<WindowId>, String> {
    let mut matches: Vec<WindowId> = Vec::new();
    for (wid, engine) in windows {
        if engine.workspaces.iter().any(|ws| ws.name == target) {
            matches.push(wid);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches[0])),
        n => Err(format!(
            "workspace name '{target}' matches {n} windows, use --surface/--workspace-id instead"
        )),
    }
}

/// 창과 parked engine 양쪽을 확인한다. parked 조회는 mirror 적용·정리 경로와 공유한다.
fn any_engine_has_workspace<'a>(
    mut main: impl Iterator<Item = &'a crate::core::CoreState>,
    parked: &'a [(crate::state::AppState, crate::core::CoreState)],
    workspace_id: u32,
) -> bool {
    main.any(|e| e.has_workspace(workspace_id))
        || super::attach_client::find_parked_with_workspace(parked, workspace_id).is_some()
}

/// 저장된 레이아웃을 우선 복원하도록 오름차순 files의 첫 미점유 슬롯을 고른다.
/// 모두 점유 중이면 files와 occupied의 최댓값 다음 번호를 쓴다.
/// 아직 파일이 없는 engine의 슬롯도 중복 배정을 피하려고 포함하며, 둘 다 비었으면 1이다.
fn pick_free_slot(files: &[LayoutSlotId], occupied: &HashSet<LayoutSlotId>) -> LayoutSlotId {
    if let Some(free) = files.iter().find(|s| !occupied.contains(s)) {
        return *free;
    }
    let max = files
        .iter()
        .chain(occupied.iter())
        .copied()
        .max()
        .unwrap_or(0);
    max + 1
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn engine_with_workspace_name(name: &str) -> crate::core::CoreState {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        engine.workspaces[0].name = name.to_string();
        engine
    }

    #[test]
    fn ambiguous_workspace_name_across_windows_returns_error() {
        let e1 = engine_with_workspace_name("main");
        let e2 = engine_with_workspace_name("main");
        let w1 = WindowId::from(1u64);
        let w2 = WindowId::from(2u64);

        let err = find_workspace_by_name([(w1, &e1), (w2, &e2)].into_iter(), "main")
            .expect_err("이름이 같은 창이 둘이면 Err여야 한다");
        assert!(
            err.contains("main"),
            "에러 메시지에 workspace 이름이 포함돼야 한다: {err}"
        );
        assert!(
            err.contains('2'),
            "에러 메시지에 매칭 개수가 포함돼야 한다: {err}"
        );
    }

    #[test]
    fn unique_workspace_name_still_resolves() {
        let e1 = engine_with_workspace_name("main");
        let e2 = engine_with_workspace_name("other");
        let w1 = WindowId::from(1u64);
        let w2 = WindowId::from(2u64);

        assert_eq!(
            find_workspace_by_name([(w1, &e1), (w2, &e2)].into_iter(), "other"),
            Ok(Some(w2))
        );
    }

    #[test]
    fn no_match_returns_ok_none() {
        let e1 = engine_with_workspace_name("main");
        let w1 = WindowId::from(1u64);

        assert_eq!(
            find_workspace_by_name([(w1, &e1)].into_iter(), "nonexistent"),
            Ok(None)
        );
    }

    fn parked(names: &[&str]) -> Vec<(crate::state::AppState, crate::core::CoreState)> {
        names
            .iter()
            .map(|name| {
                let (state, mut engine) = crate::state::tests::test_state();
                engine.workspaces[0].name = (*name).to_string();
                (state, engine)
            })
            .collect()
    }

    #[test]
    fn workspace_in_windowed_engine_is_not_orphaned() {
        let windowed = engine_with_workspace_name("mirror");
        let ws = windowed.workspaces[0].id;
        assert!(any_engine_has_workspace([&windowed].into_iter(), &[], ws));
    }

    #[test]
    fn workspace_only_in_parked_engine_is_not_orphaned() {
        let parked = parked(&["mirror"]);
        let ws = parked[0].1.workspaces[0].id;
        assert!(any_engine_has_workspace(std::iter::empty(), &parked, ws));
    }

    #[test]
    fn workspace_in_later_parked_engine_is_not_orphaned() {
        let mut parked = parked(&["first", "second"]);
        // 첫 engine과 겹치지 않는 ID를 두 번째 engine에만 만든다.
        let target = parked[0].1.workspaces[0].id + 5_000;
        parked[1].1.workspaces[0].id = target;
        assert!(any_engine_has_workspace(
            std::iter::empty(),
            &parked,
            target
        ));
    }

    #[test]
    fn workspace_in_no_engine_is_orphaned() {
        let windowed = engine_with_workspace_name("local");
        let parked = parked(&["other"]);
        let missing = windowed.workspaces[0].id + parked[0].1.workspaces[0].id + 1_000;
        assert!(!any_engine_has_workspace(
            [&windowed].into_iter(),
            &parked,
            missing
        ));
    }

    fn slots(v: &[LayoutSlotId]) -> HashSet<LayoutSlotId> {
        v.iter().copied().collect()
    }

    #[test]
    fn picks_lowest_free_slot() {
        assert_eq!(pick_free_slot(&[1, 2, 3], &slots(&[1])), 2);
    }

    #[test]
    fn allocates_new_slot_when_all_occupied() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2])), 3);
    }

    #[test]
    fn starts_at_one_when_nothing_exists() {
        assert_eq!(pick_free_slot(&[], &slots(&[])), 1);
    }

    #[test]
    fn prefers_existing_slot_file_over_lower_unused_number() {
        assert_eq!(pick_free_slot(&[2, 3], &slots(&[])), 2);
    }

    #[test]
    fn new_slot_exceeds_both_files_and_occupancy() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2, 5])), 6);
    }
}
