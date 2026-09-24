//! 팝업 Intent 처리. 요청의 origin은 여기서 검사하지 않으므로
//! 호출자가 요청 출처에 맞는 OpenPopupMode를 골라야 한다.
//! 정책: docs/design/flows/action-dispatch.md.

use super::DispatchedIntent;
#[cfg(feature = "gui")]
use super::{Intent, OpenPopupMode, UiIntent};
use crate::state::AppState;

/// 헤드리스에서는 팝업을 표시할 수 없어 요청을 무시한다.
pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    #[cfg(feature = "gui")]
    {
        let Intent::Ui(ui) = &intent.body else {
            return;
        };
        match ui {
            UiIntent::OpenPopup { id, mode } => open(state, id, mode),
            UiIntent::ClosePopup { id } => state.popups.close(id),
            UiIntent::TogglePopup { id, mode } => {
                if state.popups.is_open(id) {
                    state.popups.close(id);
                } else {
                    open(state, id, mode);
                }
            }
            // App이 별도로 처리하는 테마 변경이다.
            UiIntent::AppearanceChanged => {}
        }
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = (state, intent); // reason: 헤드리스에서는 팝업 요청을 처리하지 않는다.
    }
}

#[cfg(feature = "gui")]
fn open(state: &mut AppState, id: &'static str, mode: &OpenPopupMode) {
    if state.popups.is_open(id) {
        return;
    }
    if id == crate::adapters::ui::tutorial::topic_popup::TUTORIAL_TOPICS_POPUP_ID {
        crate::adapters::ui::tutorial::open_catalog(state);
    }
    if id == crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID {
        state
            .tutorial
            .observe(crate::adapters::ui::tutorial::PracticeEvent::OpenPalette);
    }
    match mode {
        OpenPopupMode::Default => state.popups.open(id),
        OpenPopupMode::CenteredFocused => state.popups.open_centered_focused(id),
        OpenPopupMode::WithScope(scope) => state.popups.open_with_scope(id, scope.clone()),
        OpenPopupMode::AtTopOfScope(scope) => state.popups.open_at_top_of_scope(id, scope.clone()),
        OpenPopupMode::AtFocused(pos) => state.popups.open_at_focused(id, *pos),
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;
    use crate::adapters::ui::popup::{PopupScope, PopupState};

    fn make_state() -> AppState {
        let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
            tasty_presets::PresetStore::load_default(),
        ));
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            ));
        let mut state = AppState::new(&mut engine, preset_store, memory);
        state.popups.register(PopupState::new(
            "test_popup",
            "Test".to_string(),
            egui::vec2(200.0, 100.0),
        ));
        state
    }

    fn dispatched_open(id: &'static str, mode: OpenPopupMode) -> DispatchedIntent {
        UiIntent::OpenPopup { id, mode }.from_user_shortcut("test")
    }

    fn dispatched_close(id: &'static str) -> DispatchedIntent {
        UiIntent::ClosePopup { id }.from_user_shortcut("test")
    }

    #[test]
    fn second_open_intent_for_same_id_is_deduped() {
        let mut state = make_state();
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::Default),
        );
        assert!(state.popups.is_open("test_popup"));
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
        let toggle = UiIntent::TogglePopup {
            id: "test_popup",
            mode: OpenPopupMode::Default,
        }
        .from_user_shortcut("test");
        handle(&mut state, &toggle);
        assert!(state.popups.is_open("test_popup"));
        handle(&mut state, &toggle);
        assert!(!state.popups.is_open("test_popup"));
    }

    // 닫기 큐에 기록해야 후속 프레임의 정리가 실행된다.
    #[test]
    fn close_intent_pushes_to_closed_queue() {
        let mut state = make_state();
        handle(
            &mut state,
            &dispatched_open("test_popup", OpenPopupMode::Default),
        );
        handle(&mut state, &dispatched_close("test_popup"));
        assert_eq!(state.popups.take_closed_queue(), vec!["test_popup"]);
    }

    #[test]
    fn toggle_close_branch_pushes_to_closed_queue() {
        let mut state = make_state();
        let toggle = UiIntent::TogglePopup {
            id: "test_popup",
            mode: OpenPopupMode::Default,
        }
        .from_user_shortcut("test");
        handle(&mut state, &toggle);
        assert!(state.popups.take_closed_queue().is_empty());
        handle(&mut state, &toggle);
        assert_eq!(state.popups.take_closed_queue(), vec!["test_popup"]);
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
