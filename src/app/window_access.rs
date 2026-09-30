//! 모달을 제외한 MainView와 engine의 대상 자원을 찾는다.
//!
//! engine은 세 자리에 있다. 창(MainView), 창을 모두 닫아 보관한 parked 항목,
//! 창에 배정되기 전의 임시 `App.core_state`다. 이 자리들을 순회하는 코드는
//! [`EngineScan`]·[`EngineScanMut`]을 거친다. engine 소유 구조가 바뀌면 이 두 타입과
//! [`engines_mut!`]만 고친다.

use std::collections::{HashMap, HashSet};

use winit::window::WindowId;

use crate::app::App;
use crate::core::CoreState;
use crate::core::layout_persistence::LayoutSlotId;
use crate::state::AppState;
use crate::view;

type ViewMap = HashMap<WindowId, Box<dyn view::ui::View>>;

/// App의 engine 자리 필드만 가변으로 빌린다.
/// `&mut self` 메서드와 달리 같은 함수에서 `self.core` 같은 다른 필드를 함께 빌릴 수 있다.
macro_rules! engines_mut {
    ($app:expr) => {
        $crate::app::window_access::EngineScanMut::from_fields(
            &mut $app.view.views,
            &mut $app.parked_states,
            &mut $app.core_state,
        )
    };
}
pub(crate) use engines_mut;

/// 창·parked·임시 engine을 읽기 전용으로 순회한다.
///
/// 방문 순서는 창(`views` 순회 순서) → parked(보관 순서) → 임시 engine이다.
/// 한 engine은 세 자리 중 한 곳에만 있으므로 [`all`](Self::all)은 각 engine을 정확히 한 번 방문한다.
/// 모달 View는 engine이 없어 건너뛴다.
#[derive(Clone, Copy)]
pub(crate) struct EngineScan<'a> {
    views: &'a ViewMap,
    parked: &'a [(AppState, CoreState)],
    pending: Option<&'a CoreState>,
}

impl<'a> EngineScan<'a> {
    pub(crate) fn from_fields(
        views: &'a ViewMap,
        parked: &'a [(AppState, CoreState)],
        pending: Option<&'a CoreState>,
    ) -> Self {
        Self {
            views,
            parked,
            pending,
        }
    }

    /// 창 engine과 그 창의 ID.
    pub(crate) fn windows(self) -> impl Iterator<Item = (WindowId, &'a CoreState)> {
        self.views
            .iter()
            .filter_map(|(wid, w)| w.as_main().map(|m| (*wid, &m.core_state)))
    }

    /// 창과 parked 항목의 AppState·engine 쌍. 창 → parked 순서다.
    pub(crate) fn sessions(self) -> impl Iterator<Item = (&'a AppState, &'a CoreState)> {
        self.views
            .values()
            .filter_map(|w| w.as_main().map(|m| (&m.state, &m.core_state)))
            .chain(self.parked_sessions())
    }

    pub(crate) fn parked(self) -> impl Iterator<Item = &'a CoreState> {
        self.parked.iter().map(|(_, e)| e)
    }

    pub(crate) fn parked_sessions(self) -> impl Iterator<Item = (&'a AppState, &'a CoreState)> {
        self.parked.iter().map(|(s, e)| (s, e))
    }

    pub(crate) fn parked_count(self) -> usize {
        self.parked.len()
    }

    /// 창에 배정되기 전의 임시 engine.
    pub(crate) fn pending(self) -> Option<&'a CoreState> {
        self.pending
    }

    /// 임시 engine, 없으면 첫 창 engine. parked engine은 보지 않는다.
    pub(crate) fn primary(self) -> Option<&'a CoreState> {
        self.pending
            .or_else(|| self.windows().next().map(|(_, e)| e))
    }

    /// 창 → parked 순서. 임시 engine은 제외한다.
    pub(crate) fn windowed_and_parked(self) -> impl Iterator<Item = &'a CoreState> {
        self.windows().map(|(_, e)| e).chain(self.parked())
    }

    /// 창 → parked → 임시 순서로 모든 engine을 한 번씩 방문한다.
    pub(crate) fn all(self) -> impl Iterator<Item = &'a CoreState> {
        self.windowed_and_parked().chain(self.pending)
    }
}

/// 창·parked·임시 engine을 가변으로 순회한다. 순서와 1회 방문 성질은 [`EngineScan`]과 같다.
/// 메서드는 핸들을 소비한다. 한 함수에서 여러 번 쓰려면 [`reborrow`](Self::reborrow)한다.
pub(crate) struct EngineScanMut<'a> {
    views: &'a mut ViewMap,
    parked: &'a mut Vec<(AppState, CoreState)>,
    pending: &'a mut Option<CoreState>,
}

impl<'a> EngineScanMut<'a> {
    /// [`engines_mut!`]가 쓴다. 호출부에서 필드를 직접 넘기지 않는다.
    pub(crate) fn from_fields(
        views: &'a mut ViewMap,
        parked: &'a mut Vec<(AppState, CoreState)>,
        pending: &'a mut Option<CoreState>,
    ) -> Self {
        Self {
            views,
            parked,
            pending,
        }
    }

    pub(crate) fn reborrow(&mut self) -> EngineScanMut<'_> {
        EngineScanMut {
            views: self.views,
            parked: self.parked,
            pending: self.pending,
        }
    }

    pub(crate) fn windows(self) -> impl Iterator<Item = (WindowId, &'a mut CoreState)> {
        self.views
            .iter_mut()
            .filter_map(|(wid, w)| w.as_main_mut().map(|m| (*wid, &mut m.core_state)))
    }

    /// 창의 AppState·engine 쌍.
    pub(crate) fn window_sessions(
        self,
    ) -> impl Iterator<Item = (&'a mut AppState, &'a mut CoreState)> {
        self.views
            .values_mut()
            .filter_map(|w| w.as_main_mut().map(|m| (&mut m.state, &mut m.core_state)))
    }

    /// 창과 parked 항목의 AppState·engine 쌍. 창 → parked 순서다.
    pub(crate) fn sessions(self) -> impl Iterator<Item = (&'a mut AppState, &'a mut CoreState)> {
        let Self { views, parked, .. } = self;
        views
            .values_mut()
            .filter_map(|w| w.as_main_mut().map(|m| (&mut m.state, &mut m.core_state)))
            .chain(parked.iter_mut().map(|(s, e)| (s, e)))
    }

    pub(crate) fn parked(self) -> impl Iterator<Item = &'a mut CoreState> {
        self.parked.iter_mut().map(|(_, e)| e)
    }

    /// parked 항목의 AppState·engine 쌍. 위치는 `DispatchSource::Parked`의 index와 같다.
    pub(crate) fn parked_sessions(
        self,
    ) -> impl Iterator<Item = (&'a mut AppState, &'a mut CoreState)> {
        self.parked.iter_mut().map(|(s, e)| (s, e))
    }

    pub(crate) fn parked_session(
        self,
        idx: usize,
    ) -> Option<(&'a mut AppState, &'a mut CoreState)> {
        self.parked.get_mut(idx).map(|(s, e)| (s, e))
    }

    /// 가장 먼저 보관한 parked 항목. 대상 없는 요청의 기본 engine이다.
    pub(crate) fn first_parked_session(self) -> Option<(&'a mut AppState, &'a mut CoreState)> {
        self.parked_session(0)
    }

    /// 요청 대상 자원을 가진 parked 항목. 창의 소유 판정과 같은 `engine_has_resource`를 쓴다.
    pub(crate) fn parked_session_with_resource(
        self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<(&'a mut AppState, &'a mut CoreState)> {
        self.parked_sessions()
            .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
    }

    pub(crate) fn pending(self) -> Option<&'a mut CoreState> {
        self.pending.as_mut()
    }

    /// 임시 engine, 없으면 첫 창 engine. 순서는 [`EngineScan::primary`]와 같다.
    pub(crate) fn primary(self) -> Option<&'a mut CoreState> {
        match self.pending {
            Some(e) => Some(e),
            None => self
                .views
                .values_mut()
                .find_map(|w| w.as_main_mut())
                .map(|m| &mut m.core_state),
        }
    }

    /// 창 → 임시 순서. parked engine은 제외한다.
    pub(crate) fn windows_and_pending(self) -> impl Iterator<Item = &'a mut CoreState> {
        let Self { views, pending, .. } = self;
        views
            .values_mut()
            .filter_map(|w| w.as_main_mut().map(|m| &mut m.core_state))
            .chain(pending.as_mut())
    }

    /// 창 → parked 순서. 임시 engine은 제외한다.
    pub(crate) fn windowed_and_parked(self) -> impl Iterator<Item = &'a mut CoreState> {
        let Self { views, parked, .. } = self;
        views
            .values_mut()
            .filter_map(|w| w.as_main_mut().map(|m| &mut m.core_state))
            .chain(parked.iter_mut().map(|(_, e)| e))
    }

    /// 마지막 창을 닫거나 백그라운드로 보낼 때 engine을 보관한다. 뒤에 붙인다.
    pub(crate) fn park(self, state: AppState, engine: CoreState) {
        self.parked.push((state, engine));
    }

    /// 새 창에 넘길 parked 항목을 가장 먼저 보관한 것부터 꺼낸다.
    pub(crate) fn unpark_first(self) -> Option<(AppState, CoreState)> {
        (!self.parked.is_empty()).then(|| self.parked.remove(0))
    }
}

impl App {
    pub(crate) fn engines(&self) -> EngineScan<'_> {
        EngineScan::from_fields(
            &self.view.views,
            &self.parked_states,
            self.core_state.as_ref(),
        )
    }

    pub(crate) fn engines_mut(&mut self) -> EngineScanMut<'_> {
        engines_mut!(self)
    }
}

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
        self.engines().windowed_and_parked().next()
    }

    /// 점유를 별도로 관리하지 않고 살아 있는 engine의 슬롯을 모은다.
    /// 창에 등록되기 전 임시 App.core_state도 포함해야 중복 배정을 피할 수 있다.
    pub(crate) fn occupied_layout_slots(&self) -> HashSet<LayoutSlotId> {
        occupied_slots(self.engines())
    }

    /// 빈 슬롯을 고르기만 한다. engine이 만들어져야 실제 점유로 센다.
    pub(crate) fn claim_free_layout_slot(&self) -> LayoutSlotId {
        pick_free_slot(
            &crate::core::layout_persistence::list_slots(),
            &self.occupied_layout_slots(),
        )
    }

    pub(crate) fn find_main_with_surface(&self, surface_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.has_surface(surface_id))
            .map(|(wid, _)| wid)
    }

    pub(crate) fn find_main_with_workspace(&self, workspace_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.has_workspace(workspace_id))
            .map(|(wid, _)| wid)
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
        self.engines()
            .windows()
            .find(|(_, e)| e.has_pane(pane_id))
            .map(|(wid, _)| wid)
    }

    pub(crate) fn find_main_with_tab(&self, tab_id: u32) -> Option<WindowId> {
        self.engines()
            .windows()
            .find(|(_, e)| e.find_pane_for_tab(tab_id).is_some())
            .map(|(wid, _)| wid)
    }

    /// PTY 레지스트리는 engine별로 있으므로 ID의 소유 창을 전체 MainView에서 찾는다.
    pub(crate) fn find_main_with_headless_pty(&self, pty_id: u32) -> Option<WindowId> {
        for (wid, w) in &self.view.views {
            if let Some(m) = w.as_main()
                && m.core_state.runtime.pty_registry.contains(pty_id)
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
        self.engines()
            .windows()
            .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
            .map(|(wid, _)| wid)
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
        find_workspace_by_name(self.engines().windows(), target)
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

/// 모든 engine의 슬롯을 모은다. 창·parked·임시 engine을 [`EngineScan::all`]로 한 번씩 본다.
fn occupied_slots(engines: EngineScan<'_>) -> HashSet<LayoutSlotId> {
    engines.all().filter_map(|e| e.layout_slot).collect()
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

    fn names<'a>(engines: impl Iterator<Item = &'a crate::core::CoreState>) -> Vec<String> {
        engines.map(|e| e.workspaces[0].name.clone()).collect()
    }

    #[test]
    fn scan_visits_parked_then_pending_once_each() {
        let parked = parked(&["p0", "p1"]);
        let pending = engine_with_workspace_name("tmp");
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &parked, Some(&pending));
        assert_eq!(names(scan.all()), ["p0", "p1", "tmp"]);
        assert_eq!(names(scan.windowed_and_parked()), ["p0", "p1"]);
        assert_eq!(names(scan.parked()), ["p0", "p1"]);
        assert_eq!(scan.parked_count(), 2);
        assert_eq!(
            names(scan.primary().into_iter()),
            ["tmp"],
            "임시 engine이 창보다 먼저다"
        );
    }

    #[test]
    fn scan_without_pending_has_no_primary() {
        let parked = parked(&["p0"]);
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &parked, None);
        assert!(scan.primary().is_none(), "primary는 parked를 보지 않는다");
        assert_eq!(names(scan.all()), ["p0"]);
    }

    #[test]
    fn occupied_slots_counts_each_engine_once() {
        let mut parked = parked(&["p0", "p1"]);
        parked[0].1.layout_slot = Some(4);
        parked[1].1.layout_slot = Some(2);
        let mut pending = engine_with_workspace_name("tmp");
        pending.layout_slot = Some(7);
        let views = HashMap::new();
        let scan = EngineScan::from_fields(&views, &parked, Some(&pending));
        assert_eq!(occupied_slots(scan), slots(&[2, 4, 7]));
        assert_eq!(
            scan.all().filter(|e| e.layout_slot.is_some()).count(),
            3,
            "슬롯을 가진 engine이 각각 한 번만 보인다"
        );
    }

    #[test]
    fn scan_mut_keeps_order_and_excludes_by_place() {
        let mut parked = parked(&["p0", "p1"]);
        let mut pending = Some(engine_with_workspace_name("tmp"));
        let mut views = HashMap::new();
        let mut scan = EngineScanMut::from_fields(&mut views, &mut parked, &mut pending);
        let visit = |it: &mut dyn Iterator<Item = &mut crate::core::CoreState>| -> Vec<String> {
            it.map(|e| e.workspaces[0].name.clone()).collect()
        };
        assert_eq!(
            visit(&mut scan.reborrow().windowed_and_parked()),
            ["p0", "p1"]
        );
        assert_eq!(visit(&mut scan.reborrow().windows_and_pending()), ["tmp"]);
        assert_eq!(visit(&mut scan.reborrow().parked()), ["p0", "p1"]);
        let sessions: Vec<String> = scan
            .reborrow()
            .sessions()
            .map(|(_, e)| e.workspaces[0].name.clone())
            .collect();
        assert_eq!(
            sessions,
            ["p0", "p1"],
            "sessions는 임시 engine을 넣지 않는다"
        );
        assert_eq!(
            scan.reborrow()
                .primary()
                .map(|e| e.workspaces[0].name.clone()),
            Some("tmp".to_string())
        );
    }

    #[test]
    fn park_appends_and_unpark_takes_the_oldest() {
        let mut parked = parked(&["p0"]);
        let mut pending = None;
        let mut views = HashMap::new();
        let mut scan = EngineScanMut::from_fields(&mut views, &mut parked, &mut pending);
        let (state, mut engine) = crate::state::tests::test_state();
        engine.workspaces[0].name = "p1".to_string();
        scan.reborrow().park(state, engine);
        let (_, first) = scan.reborrow().unpark_first().expect("보관한 항목");
        assert_eq!(first.workspaces[0].name, "p0");
        let (_, second) = scan.reborrow().unpark_first().expect("보관한 항목");
        assert_eq!(second.workspaces[0].name, "p1");
        assert!(scan.unpark_first().is_none());
    }

    #[test]
    fn parked_session_lookups_use_parked_index() {
        let mut parked = parked(&["p0", "p1"]);
        // 첫 항목과 겹치지 않는 ID를 두 번째 항목에만 둔다.
        let target = parked[0].1.workspaces[0].id + 5_000;
        parked[1].1.workspaces[0].id = target;
        let mut pending = None;
        let mut views = HashMap::new();
        let mut scan = EngineScanMut::from_fields(&mut views, &mut parked, &mut pending);
        let (_, e) = scan.reborrow().parked_session(1).expect("두 번째 항목");
        assert_eq!(e.workspaces[0].name, "p1");
        let (_, e) = scan.reborrow().first_parked_session().expect("첫 항목");
        assert_eq!(e.workspaces[0].name, "p0");
        assert!(scan.reborrow().parked_session(2).is_none());
        let rid = crate::core::request_target::ResourceId {
            kind: crate::core::request_target::Kind::Workspace,
            id: u64::from(target),
        };
        let (_, e) = scan
            .parked_session_with_resource(rid)
            .expect("workspace를 가진 parked 항목");
        assert_eq!(e.workspaces[0].name, "p1");
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
