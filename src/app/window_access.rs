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

    /// 사용자 안내(InfoModal / toast)를 띄울 메인 창.
    ///
    /// 포커스된 뷰가 모달(설정 · 플러그인 · 종료 확인)이면 [`Self::focused_window_mut`]
    /// 은 `None` 을 준다 — 그때도 메인 창이 남아 있으면 그 중 하나로 폴백한다. 안내가
    /// "포커스가 마침 모달에 있었다" 는 이유로 조용히 사라지지 않게 하는 것이 요점이다
    /// (`docs/adr/0117-window-and-modal-creation-failure-policy.md`).
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

    /// mirror 워크스페이스를 들고 있는 engine 이 아직 살아 있는가 — attach 세션의
    /// **고아 판정 전용** 술어(`detach_orphaned_mirror_sessions`).
    ///
    /// `find_main_with_workspace` 를 쓰지 않는 이유: 그 헬퍼는 *창(WindowId)* 을
    /// 찾는 것이 본질이라 `self.view.views` 만 본다. 하지만 마지막 창을 닫거나
    /// (macOS 는 최소화도) engine 은 사라지지 않고 `parked_states` 로 옮겨가 그대로
    /// 살아 있다(ADR-0087 — parked engine 은 레이아웃 슬롯 점유를 유지한다). 창
    /// 유무로 고아를 판정하면 사용자가 창을 최소화했을 뿐인데 원격 attach 점유가
    /// 조용히 풀린다. 여기서 묻는 것은 "창이 있는가"가 아니라 "그 워크스페이스를
    /// 들고 있는 engine 이 살아 있는가"다.
    ///
    /// 순회 범위는 `attach_client::cleanup_mirror_workspace`(정리)·
    /// `attach_client::mirror_output_host`(mirror 이벤트 적용 대상 탐색)와 **같아야**
    /// 한다 — 판정이 살아 있다고 본 engine 을 정리가 못 찾으면 잔류가 생기고, 적용이
    /// 못 찾으면 그 구간에 도착한 출력이 조용히 유실된다([ADR-0110](../../docs/adr/0110-mirror-events-apply-to-parked-engines.md)).
    ///
    /// **`App.core_state` 는 의도적으로 제외한다.** 바로 위 `occupied_layout_slots`
    /// 는 `views`/`parked_states` 에 더해 그 자리(첫 MainView 등록 전 engine 이 임시로
    /// 머무는 곳)까지 보지만, 여기서는 보지 않는다. 이유는 두 가지다 — ① mirror
    /// 워크스페이스는 `attach_client::start_gui_attach` 가 `focused_window_mut()` 의
    /// engine 에만 push 하므로 그 임시 engine 에는 애초에 들어갈 수 없다(= 지금 이
    /// 판정으로 도달 가능한 상태가 아니다). ② 그 자리에는 짝이 되는 `AppState` 가
    /// 없어 정리 쪽의 `active_workspace` 클램프를 대칭으로 맞출 수 없다. 판정과 정리의
    /// 순회 범위는 같아야 하므로 양쪽에서 함께 뺀다. mirror 워크스페이스가 그 임시
    /// engine 에도 만들어질 수 있게 바뀐다면 이 제외와 정리 쪽 순회를 함께 손봐야 한다.
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

    /// 탭을 가진 MainView 의 WindowId 를 반환.
    ///
    /// 탭은 창에 직접 매이지 않는다 — `find_pane_for_tab` 이 그 탭을 담은 pane 을
    /// 찾고, pane 이 engine 에 있으면 그 창이 주인이다.
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

    /// headless pty 를 가진 MainView 의 WindowId 를 반환.
    ///
    /// `pty_registry` 는 **engine 마다 따로**다(`MainView::core_state`). 창이 둘이면
    /// 한쪽에서 spawn 한 pty 는 다른 쪽 registry 에 없으므로, id 만 들고 온 요청은
    /// 창을 건너 찾아야 주인을 만난다.
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

    /// [`ResourceId`](crate::core::request_target::ResourceId) 하나를 주인 창으로 푼다 —
    /// kind 별 분기를 한 곳에만 둔다. 새 kind 를 더하면 여기서 컴파일이 깨지므로,
    /// 라우팅 경로와 parked 경로가 서로 다른 집합을 보는 사고가 안 난다.
    pub(crate) fn find_main_with_resource(
        &self,
        rid: crate::core::request_target::ResourceId,
    ) -> Option<WindowId> {
        use crate::core::request_target::Kind;
        // 창에 매인 리소스 id 는 `u32` 다. 안 들어가는 값은 그 종류의 id 일 수 없으므로
        // 주인이 없다 — 좁히면서 자르지 않고 여기서 판정한다.
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

    /// engine 소유 리소스(surface hook · global hook · observer · workspace category)를
    /// 가진 MainView.
    ///
    /// `find_main_with_*` 를 하나씩 더하지 않는 이유: 이들은 술어가
    /// [`engine_has_resource`](crate::core::request_target::engine_has_resource) 에 이미
    /// 있고, 창 순회와 parked 순회가 **같은 술어**를 보는 것이 이 축의 요점이다.
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

/// `mirror_workspace_engine_alive` 의 순수 본문 — 창 있는 engine(`main`)과 창 없는
/// parked engine(`parked`) 중 어느 하나라도 그 워크스페이스를 들고 있으면 `true`.
/// 두 컬렉션을 별도 인자로 받는 이유는 "parked 도 본다"는 것이 이 술어의 요점이라
/// 단위 테스트에서 두 축을 독립적으로 세울 수 있어야 하기 때문이다.
///
/// `parked` 는 `App.parked_states` 와 **같은 타입**(`&[(AppState, CoreState)]`)으로
/// 받는다 — 호출부가 `.map(|(_, e)| e)` 같은 어댑터를 끼우지 않고 필드를 그대로
/// 넘기므로, 이 함수를 검증하는 테스트가 실제 배선까지 함께 덮는다.
fn any_engine_has_workspace<'a>(
    main: impl Iterator<Item = &'a crate::core::CoreState>,
    parked: &'a [(crate::state::AppState, crate::core::CoreState)],
    workspace_id: u32,
) -> bool {
    main.chain(parked.iter().map(|(_, e)| e))
        .any(|e| e.has_workspace(workspace_id))
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

    /// `App.parked_states` 와 같은 모양의 parked 목록을 만든다 — 판정 헬퍼가 실제로
    /// 받는 타입 그대로 넘겨야 배선까지 함께 검증된다.
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

    /// 워크스페이스가 **창 있는** engine 에 있으면 고아가 아니다(기존 동작 회귀).
    #[test]
    fn workspace_in_windowed_engine_is_not_orphaned() {
        let windowed = engine_with_workspace_name("mirror");
        let ws = windowed.workspaces[0].id;
        assert!(any_engine_has_workspace([&windowed].into_iter(), &[], ws));
    }

    /// 워크스페이스가 **parked engine 에만** 있어도 고아가 아니다 — 창이 없을 뿐
    /// engine 은 살아 있다(마지막 창 닫기 / macOS 최소화). 창만 보던 옛 판정은
    /// 여기서 고아로 오판해 attach 세션을 끊었다.
    #[test]
    fn workspace_only_in_parked_engine_is_not_orphaned() {
        let parked = parked(&["mirror"]);
        let ws = parked[0].1.workspaces[0].id;
        assert!(any_engine_has_workspace(std::iter::empty(), &parked, ws));
    }

    /// 첫 parked 엔트리에서 멈추지 않고 **`parked_states` 전체를 순회**해야 한다 —
    /// 창을 여럿 닫으면 parked engine 도 여럿 쌓이고, mirror 를 들고 있는 것이
    /// 첫 항목이라는 보장이 없다.
    #[test]
    fn workspace_in_later_parked_engine_is_not_orphaned() {
        let mut parked = parked(&["first", "second"]);
        // 두 번째 parked engine 에만 있는 워크스페이스 id 를 만든다(첫 항목과 충돌 회피).
        let target = parked[0].1.workspaces[0].id + 5_000;
        parked[1].1.workspaces[0].id = target;
        assert!(any_engine_has_workspace(
            std::iter::empty(),
            &parked,
            target
        ));
    }

    /// 어느 engine 에도 없으면(사용자가 mirror 워크스페이스를 직접 닫음) 고아다 —
    /// 이 경우에만 세션을 정리하고 원격 점유를 푼다.
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
