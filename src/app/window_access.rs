//! 활성 윈도우 접근 헬퍼.
//!
//! IPC / 키보드 라우팅의 일반 대상은 모달이 아닌 `MainView`. 모달 활성 여부와는
//! 별개로 `view.focused_view_id` 로 추적되는 윈도우만 반환한다.

use winit::window::WindowId;

use crate::app::App;
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
}
