//! 로드 종료 신호가 스크립트 게이트가 시작한 마지막 로드의 것인지 판정한다(ADR-0053).
//! 앞 로드의 늦은 종료가 새 로드의 대기 값을 지우지 않게 한다. WebView2는 `NavigationId`, macOS는 `WKNavigation`을 세대로 쓴다.
//! chrome의 `NavState`도 같은 판정으로 마지막에 시작한 main frame 탐색의 종료만 따른다.
//! WebKitGTK 종료 신호에는 navigation 식별자가 없어 Linux는 신호 순서로 세대를 매긴다(`SignalOrderLoads`).

use super::NavState;

/// 종료 신호가 게이트가 시작한 마지막 로드의 것인지. ID를 알 수 없는 쪽이 있으면 현재 로드로 본다.
#[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
pub(super) fn is_current_load(current: Option<u64>, ended: Option<u64>) -> bool {
    match (current, ended) {
        (Some(c), Some(e)) => c == e,
        _ => true,
    }
}

/// 로드 종료 뒤 chrome 상태. `started`는 마지막에 시작한 main frame 탐색의 세대다.
/// 앞 로드가 늦게 끝나면 새 로드의 Loading을 Done이나 Failed로 덮지 않도록 None을 돌려준다.
#[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
pub(super) fn nav_state_after_end(
    started: Option<u64>,
    ended: Option<u64>,
    success: bool,
) -> Option<NavState> {
    if !is_current_load(started, ended) {
        return None;
    }
    Some(if success {
        NavState::Done
    } else {
        NavState::Failed
    })
}

/// 식별자 없는 종료 신호를 받는 백엔드(WebKitGTK)의 chrome 세대.
/// 종료 신호는 마지막 `STARTED`를 받은 로드(진행 중 로드)의 것으로 본다. `load_url`·`load_html`은 새 세대를
/// 현재 로드로 두므로, 그 뒤에 오는 진행 중 로드의 취소 실패와 `FINISHED`는 앞 로드의 늦은 종료가 된다.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Default)]
pub(super) struct SignalOrderLoads {
    next: u64,
    /// 마지막에 요청했거나 시작한 로드.
    current: Option<u64>,
    /// 요청한 로드가 아직 `STARTED`를 받지 않았다.
    requested: bool,
    /// `STARTED`를 받고 `FINISHED`를 받지 않은 로드. 종료 신호를 이 로드에 돌린다.
    in_flight: Option<u64>,
}

#[cfg(any(target_os = "linux", test))]
impl SignalOrderLoads {
    fn fresh(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// `load_url`·`load_html`. 시작 신호 전부터 이 로드가 현재 로드다.
    pub(super) fn requested(&mut self) {
        self.current = Some(self.fresh());
        self.requested = true;
    }

    /// main frame `STARTED`. 요청한 로드가 있으면 그 로드이고, 없으면(링크·뒤로 가기) 새 로드다.
    pub(super) fn started(&mut self) {
        if !std::mem::take(&mut self.requested) {
            self.current = Some(self.fresh());
        }
        self.in_flight = self.current;
    }

    /// `load-failed`. 뒤따르는 `FINISHED`도 같은 로드의 것이라 진행 중 로드를 그대로 둔다.
    pub(super) fn failed(&self) -> Option<NavState> {
        nav_state_after_end(self.current, self.in_flight, false)
    }

    /// `FINISHED`. 진행 중 로드를 비운다.
    pub(super) fn finished(&mut self) -> Option<NavState> {
        let state = nav_state_after_end(self.current, self.in_flight, true);
        self.in_flight = None;
        state
    }

    /// web process 종료. `FINISHED`가 오지 않으므로 진행 중 로드를 비운다. chrome은 세대와 관계없이 Failed다.
    pub(super) fn terminated(&mut self) {
        self.in_flight = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{NavState, SignalOrderLoads, is_current_load, nav_state_after_end};

    /// WebKitGTK 2.50.4에서 측정한 순서: 진행 중 로드를 `load_uri`로 바꾸면 앞 로드의 취소 실패와
    /// `FINISHED`가 새 로드의 `STARTED`보다 먼저 온다.
    #[test]
    fn a_load_cancelled_by_a_new_request_does_not_fail_the_chrome() {
        let mut loads = SignalOrderLoads::default();
        loads.requested();
        loads.started();
        loads.requested();
        assert_eq!(loads.failed(), None);
        assert_eq!(loads.finished(), None);
        loads.started();
        assert_eq!(loads.finished(), Some(NavState::Done));
    }

    #[test]
    fn the_current_load_failing_fails_the_chrome() {
        let mut loads = SignalOrderLoads::default();
        loads.requested();
        loads.started();
        assert_eq!(loads.failed(), Some(NavState::Failed));
        // 실패 뒤 FINISHED의 Done은 호출자가 Failed를 덮지 않게 거른다.
        assert_eq!(loads.finished(), Some(NavState::Done));
    }

    #[test]
    fn a_navigation_without_a_request_is_its_own_load() {
        let mut loads = SignalOrderLoads::default();
        loads.started();
        assert_eq!(loads.failed(), Some(NavState::Failed));
        loads.finished();
        loads.started();
        assert_eq!(loads.finished(), Some(NavState::Done));
    }

    #[test]
    fn a_request_failing_before_its_start_fails_the_chrome() {
        let mut loads = SignalOrderLoads::default();
        loads.requested();
        assert_eq!(loads.failed(), Some(NavState::Failed));
        let mut after_finish = SignalOrderLoads::default();
        after_finish.requested();
        after_finish.started();
        after_finish.finished();
        after_finish.requested();
        assert_eq!(after_finish.failed(), Some(NavState::Failed));
    }

    #[test]
    fn a_terminated_web_process_leaves_no_load_in_flight() {
        let mut loads = SignalOrderLoads::default();
        loads.requested();
        loads.started();
        loads.terminated();
        loads.requested();
        assert_eq!(loads.failed(), Some(NavState::Failed));
    }

    #[test]
    fn the_end_of_the_load_the_gate_started_is_current() {
        assert!(is_current_load(Some(7), Some(7)));
    }

    #[test]
    fn the_late_end_of_a_superseded_load_is_not_current() {
        assert!(!is_current_load(Some(8), Some(7)));
    }

    #[test]
    fn an_unknown_id_on_either_side_falls_back_to_the_current_load() {
        assert!(is_current_load(Some(8), None));
        assert!(is_current_load(None, Some(7)));
        assert!(is_current_load(None, None));
    }

    #[test]
    fn the_end_of_the_last_started_load_sets_the_chrome_state() {
        assert_eq!(
            nav_state_after_end(Some(7), Some(7), true),
            Some(NavState::Done)
        );
        assert_eq!(
            nav_state_after_end(Some(7), Some(7), false),
            Some(NavState::Failed)
        );
    }

    #[test]
    fn a_superseded_load_ending_late_keeps_the_new_load_state() {
        assert_eq!(nav_state_after_end(Some(8), Some(7), true), None);
        assert_eq!(nav_state_after_end(Some(8), Some(7), false), None);
    }

    #[test]
    fn an_unknown_id_ends_as_the_current_load() {
        assert_eq!(
            nav_state_after_end(None, Some(7), true),
            Some(NavState::Done)
        );
        assert_eq!(
            nav_state_after_end(Some(8), None, false),
            Some(NavState::Failed)
        );
    }

    #[test]
    fn an_end_after_a_load_request_without_a_start_signal_is_applied() {
        // load_url·load_html이 chrome 세대를 비운 뒤 시작 신호 없이 끝나는 로드.
        assert_eq!(
            nav_state_after_end(None, Some(9), false),
            Some(NavState::Failed)
        );
        assert_eq!(
            nav_state_after_end(None, Some(9), true),
            Some(NavState::Done)
        );
        assert_eq!(
            nav_state_after_end(None, None, false),
            Some(NavState::Failed)
        );
    }
}
