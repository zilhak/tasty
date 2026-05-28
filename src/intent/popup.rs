//! Popup 도메인 Intent 핸들러.
//!
//! 정책 차이 (TODO 01 결정 13 — `docs/design/action-dispatch.md` 참조):
//! dispatcher 는 origin 정책을 강제하지 않는다. 호출자가 적절한 `OpenPopupMode` 를
//! 선택하고 (예: agent origin 인데 focus 가 필요 없다면 `Default` 또는 `CenteredFocused`
//! 대신 focus 없는 변형을 발화), PR 리뷰에서 정책 위반을 잡는다.
//!
//! Dedup: 같은 popup id 에 OpenPopup 이 중복 들어오면 이미 열려있을 때 무시.

use super::{DispatchedIntent, Intent, OpenPopupMode};
use crate::state::AppState;

/// popup 도메인 분기 핸들러. `dispatch_pending_intents` 에서 호출.
pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    match &intent.body {
        Intent::OpenPopup { id, mode } => open(state, id, mode),
        Intent::ClosePopup { id } => state.popups.close(id),
        Intent::TogglePopup { id, mode } => {
            if state.popups.is_open(id) {
                state.popups.close(id);
            } else {
                open(state, id, mode);
            }
        }
        _ => {}
    }
}

fn open(state: &mut AppState, id: &'static str, mode: &OpenPopupMode) {
    // Dedup: 이미 열려있으면 두 번째 OpenPopup 무시.
    if state.popups.is_open(id) {
        return;
    }
    match mode {
        OpenPopupMode::Default => state.popups.open(id),
        OpenPopupMode::CenteredFocused => state.popups.open_centered_focused(id),
        OpenPopupMode::WithScope(scope) => state.popups.open_with_scope(id, scope.clone()),
        OpenPopupMode::AtTopOfScope(scope) => state.popups.open_at_top_of_scope(id, scope.clone()),
        OpenPopupMode::AtFocused(pos) => state.popups.open_at_focused(id, *pos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::popup::{PopupScope, PopupState};

    fn make_state() -> AppState {
        let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::engine_state::CoreState::new(80, 24, waker).unwrap();
        let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
            tasty_presets::PresetStore::load_default(),
        ));
        let mut state = AppState::new(&mut engine, preset_store);
        // 테스트 대상 popup 을 PopupManager 에 등록.
        state.popups.register(PopupState::new(
            "test_popup",
            "Test".to_string(),
            egui::vec2(200.0, 100.0),
        ));
        state
    }

    fn dispatched_open(id: &'static str, mode: OpenPopupMode) -> DispatchedIntent {
        Intent::OpenPopup { id, mode }.from_user_shortcut("test")
    }

    fn dispatched_close(id: &'static str) -> DispatchedIntent {
        Intent::ClosePopup { id }.from_user_shortcut("test")
    }

    #[test]
    fn second_open_intent_for_same_id_is_deduped() {
        let mut state = make_state();
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::Default),
        );
        assert!(state.popups.is_open("test_popup"));
        // 두 번째 동일 id OpenPopup — dedup 무시 (state 변동 없음).
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::CenteredFocused),
        );
        assert!(state.popups.is_open("test_popup"));
    }

    #[test]
    fn close_intent_closes_popup() {
        let mut state = make_state();
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::Default),
        );
        assert!(state.popups.is_open("test_popup"));
        handle(&mut state, &dispatched_close("test_popup"));
        assert!(!state.popups.is_open("test_popup"));
    }

    #[test]
    fn toggle_opens_when_closed_closes_when_open() {
        let mut state = make_state();
        let toggle = Intent::TogglePopup {
            id: "test_popup",
            mode: OpenPopupMode::Default,
        }
        .from_user_shortcut("test");
        handle(&mut state, &toggle);
        assert!(state.popups.is_open("test_popup"));
        handle(&mut state, &toggle);
        assert!(!state.popups.is_open("test_popup"));
    }

    #[test]
    fn open_with_scope_uses_requested_scope() {
        let mut state = make_state();
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::WithScope(PopupScope::Window)),
        );
        assert!(state.popups.is_open("test_popup"));
    }
}
