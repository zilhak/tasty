//! 로드 종료 신호가 스크립트 게이트가 시작한 마지막 로드의 것인지 판정한다(ADR-0053).
//! 앞 로드의 늦은 종료가 새 로드의 대기 값을 지우지 않게 한다. WebView2는 `NavigationId`, macOS는 `WKNavigation`을 세대로 쓴다.
//! chrome의 `NavState`도 같은 판정으로 마지막에 시작한 main frame 탐색의 종료만 따른다.

use super::NavState;

/// 종료 신호가 게이트가 시작한 마지막 로드의 것인지. ID를 알 수 없는 쪽이 있으면 현재 로드로 본다.
pub(super) fn is_current_load(current: Option<u64>, ended: Option<u64>) -> bool {
    match (current, ended) {
        (Some(c), Some(e)) => c == e,
        _ => true,
    }
}

/// 로드 종료 뒤 chrome 상태. `started`는 마지막에 시작한 main frame 탐색의 세대다.
/// 앞 로드가 늦게 끝나면 새 로드의 Loading을 Done이나 Failed로 덮지 않도록 None을 돌려준다.
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

#[cfg(test)]
mod tests {
    use super::{NavState, is_current_load, nav_state_after_end};

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
