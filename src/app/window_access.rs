//! 활성 윈도우 접근 헬퍼.
//!
//! IPC / 키보드 라우팅의 일반 대상은 모달이 아닌 `MainView`. 모달 활성 여부와는
//! 별개로 `view.focused_view_id` 로 추적되는 윈도우만 반환한다.

use std::collections::HashSet;

use winit::window::WindowId;

use crate::app::App;
use crate::core::layout_persistence::LayoutSlotId;
use crate::view;

impl App {
    /// Get the focused main window, if any.
    /// 모달이 아닌 MainView만 반환한다 — IPC/키보드 라우팅의 일반적 대상.
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

    /// 모든 MainView를 순회. 모달은 제외된다.
    pub(crate) fn main_windows_iter_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut view::main::MainView> {
        self.view.views.values_mut().filter_map(|w| w.as_main_mut())
    }

    /// 열려 있는 MainView(모달 제외) 개수 — "다중 윈도우 세션인가"를 판단하는
    /// 공용 질의. `request_close_window`(event_handler.rs)의 판단과
    /// [`request_owner::find_request_owner`](super::request_owner) 의 다중 윈도우
    /// 모호성 판정이 같은 정의를 공유한다.
    pub(crate) fn main_window_count(&self) -> usize {
        self.view
            .views
            .values()
            .filter(|w| w.as_main().is_some())
            .count()
    }

    /// 살아있는 CoreState 중 하나(아무거나)를 참조로 반환. windows main → parked
    /// 순으로 찾는다. 두 번째 main window 생성 시 첫 engine 의 Arc 들 (surface_registry /
    /// file_format / file_handler / preset_store / identify_worker / approval_store /
    /// telemetry_seq / anomaly_detector / agent_seq) 을 공유시키기 위해 사용.
    pub(crate) fn any_main_engine(&self) -> Option<&crate::core::CoreState> {
        for w in self.view.views.values() {
            if let Some(m) = w.as_main() {
                return Some(&m.core_state);
            }
        }
        self.parked_states.first().map(|(_, e)| e)
    }

    /// 지금 살아있는 engine 들이 점유한 레이아웃 슬롯 집합.
    ///
    /// 점유를 별도 레지스트리로 들지 않는다 — 점유자는 언제나 살아있는 engine 이라
    /// 각 engine 의 `layout_slot` 을 모으면 그것이 곧 점유 집합이다. 별도 `HashSet`
    /// 을 유지하면 창 닫힘·parking·drop 경로마다 동기화가 필요해지고 한 군데만
    /// 빠져도 슬롯이 영구 누수된다.
    ///
    /// 순회 대상이 `any_main_engine`(views → parked) 보다 하나 많다 —
    /// `self.core_state` 도 본다. `ensure_engine_and_plugins` 가 만든 engine 은
    /// `register_window` 로 `views` 에 들어가기 전까지 거기 임시로 머물기 때문에,
    /// 그 구간에서 스캔이 놓치면 같은 슬롯이 두 번 배정된다.
    ///
    /// 포커스 독립(CLAUDE.md 원칙 3): `focused_view_id` 를 보지 않고 전 engine 을
    /// 본다.
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

    /// 새 창(engine)이 쓸 free 슬롯을 고른다. `&self` 만 읽고 아무 것도 mutate 하지
    /// 않는다 — 점유는 engine 이 실제로 만들어지면서 확정된다.
    pub(crate) fn claim_free_layout_slot(&self) -> LayoutSlotId {
        pick_free_slot(
            &crate::core::layout_persistence::list_slots(),
            &self.occupied_layout_slots(),
        )
    }

    /// Surface 를 가진 MainView 의 WindowId 를 반환. windows main 순회 후 못 찾으면
    /// None (parked 는 별도로 fallback 처리).
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

    /// Workspace 를 가진 MainView 의 WindowId 를 반환.
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

    /// Pane 을 가진 MainView 의 WindowId 를 반환.
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

    /// Workspace 를 (id 또는 표시 이름) 문자열로 여러 window 에 걸쳐 찾는다 —
    /// `terminal::resolve_workspace_id` 와 동일한 우선순위(숫자 id exact match 우선,
    /// 실패 시 name exact match)를 단일 engine 이 아니라 **모든 main window** 에
    /// 걸쳐 적용한다. 라우팅 시점엔 아직 어느 window 대상인지 몰라 engine 하나를
    /// 먼저 고를 수 없으므로 `resolve_workspace_id(engine, ...)` 를 그대로 못 쓴다.
    ///
    /// name exact match 는 window 간 유일성이 보장되지 않는다(예: 두 window 모두
    /// 기본 workspace 이름 "main") — 2개 이상 window 가 일치하면 `self.view.views`
    /// (`HashMap`, 순회 순서 비결정적) 순서에 따라 임의의 window 가 골라지는 모호성이
    /// 생긴다. 포커스 독립 원칙상 이런 자명하지 않은 임의 선택을 조용히 하기보다,
    /// 모호하면 `Err` 로 명확히 거부한다.
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

/// `find_main_with_workspace_target`의 name-exact-match 부분 — window ↔ CoreState
/// 페어들에서 `target`이라는 이름의 workspace 를 가진 window 를 찾는다. 정확히
/// 하나만 일치하면 그 window, 없으면 `Ok(None)`(호출자가 다른 폴백을 시도할 수
/// 있게), 2개 이상 일치하면 어느 것도 임의로 고르지 않고 `Err` 로 모호함을
/// 알린다. `App`/`MainView` 의존 없이 단위 테스트 가능하도록 분리한 순수 함수.
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

/// `claim_free_layout_slot` 의 순수 본문 — 디스크에 실제로 존재하는 슬롯 파일
/// 번호(`files`, 오름차순)와 지금 점유된 슬롯(`occupied`)만으로 결정한다.
///
/// - 기존 슬롯 파일 중 점유되지 않은 **첫 번째**를 쓴다. 번호 공백은 채우지
///   않는다 — 파일이 `[2, 3]` 이고 점유가 없으면 답은 `1` 이 아니라 `2` 다.
///   "가장 작은 미점유 정수" 가 아니라 "가장 작은 미점유 **기존 슬롯**" 이며,
///   저장된 레이아웃을 건너뛰고 빈 창을 띄우지 않기 위한 것이다.
/// - 기존 슬롯이 전부 점유면 `max(files ∪ occupied) + 1` — 파일 없는 새 슬롯이라
///   로드 대상이 없고 기본 워크스페이스로 시작한다. `occupied` 까지 함께 보는
///   이유는 파일 없이 배정된 새 슬롯이 이미 있을 수 있어서다.
/// - 파일도 점유도 없으면 `1`.
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

    /// 기본 workspace 하나를 가진 `CoreState`를 만들고 그 이름을 `name`으로 바꾼다.
    /// 여러 main window 를 흉내내기 위해 서로 다른 `WindowId`와 짝지어 쓴다 — 실제
    /// `App`/`MainView`(EventLoopProxy 등 GUI 의존)를 띄우지 않고도 `CoreState.workspaces`
    /// 매칭 로직을 있는 그대로 실행한다.
    fn engine_with_workspace_name(name: &str) -> crate::core::CoreState {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        engine.workspaces[0].name = name.to_string();
        engine
    }

    /// 두 window 가 각각 같은 이름("main")의 workspace 를 가지면(둘 다 기본
    /// workspace 이름을 그대로 쓰는 흔한 케이스), 문자열 이름 매칭은 `HashMap`
    /// 순회 순서에 따라 임의의 window 를 고르지 않고 모호함을 `Err` 로 보고해야
    /// 한다(Gate4 리뷰 판단필요 항목 — 사용자 결정: 모호하면 에러로 거부).
    #[test]
    fn ambiguous_workspace_name_across_windows_returns_error() {
        let e1 = engine_with_workspace_name("main");
        let e2 = engine_with_workspace_name("main");
        let w1 = WindowId::from(1u64);
        let w2 = WindowId::from(2u64);

        let err = find_workspace_by_name([(w1, &e1), (w2, &e2)].into_iter(), "main")
            .expect_err("2개 window 가 일치하면 Err 이어야 한다");
        assert!(
            err.contains("main"),
            "에러 메시지에 workspace 이름이 포함돼야 한다: {err}"
        );
        assert!(
            err.contains('2'),
            "에러 메시지에 매칭 개수가 포함돼야 한다: {err}"
        );
    }

    /// 단일 매치는 회귀 없이 그대로 동작해야 한다.
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

    /// 아무 window 도 일치하지 않으면 `Ok(None)` — 호출자가 다른 폴백 경로로
    /// 넘어갈 수 있어야 한다(모호함과 구분되는 별도 케이스).
    #[test]
    fn no_match_returns_ok_none() {
        let e1 = engine_with_workspace_name("main");
        let w1 = WindowId::from(1u64);

        assert_eq!(
            find_workspace_by_name([(w1, &e1)].into_iter(), "nonexistent"),
            Ok(None)
        );
    }

    fn slots(v: &[LayoutSlotId]) -> HashSet<LayoutSlotId> {
        v.iter().copied().collect()
    }

    /// 기존 슬롯 파일 중 점유되지 않은 가장 낮은 번호를 쓴다.
    #[test]
    fn picks_lowest_free_slot() {
        assert_eq!(pick_free_slot(&[1, 2, 3], &slots(&[1])), 2);
    }

    /// 기존 슬롯이 전부 점유면 파일 없는 새 슬롯을 만든다.
    #[test]
    fn allocates_new_slot_when_all_occupied() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2])), 3);
    }

    /// 첫 설치(슬롯 파일도 점유도 없음)는 1 부터.
    #[test]
    fn starts_at_one_when_nothing_exists() {
        assert_eq!(pick_free_slot(&[], &slots(&[])), 1);
    }

    /// 파일이 2,3 뿐이고 점유가 없으면 1 이 아니라 2 — 저장된 레이아웃을 건너뛰고
    /// 빈 슬롯을 새로 만들지 않는다.
    #[test]
    fn prefers_existing_slot_file_over_lower_unused_number() {
        assert_eq!(pick_free_slot(&[2, 3], &slots(&[])), 2);
    }

    /// 새 슬롯 번호는 파일 집합과 점유 집합 **양쪽**을 넘어야 한다 — 파일 없이
    /// 배정된 슬롯(5)과 충돌하면 두 창이 같은 파일에 쓴다.
    #[test]
    fn new_slot_exceeds_both_files_and_occupancy() {
        assert_eq!(pick_free_slot(&[1, 2], &slots(&[1, 2, 5])), 6);
    }
}
