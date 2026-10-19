//! 창 최소화·최대화·닫기 단축키의 판정. macOS는 각 동작의 첫 조합을 Window 메뉴의 key
//! equivalent로 등록하므로(`tasty_platform::macos_delegate`), 그 조합은 AppKit이 실행하고 키
//! 경로는 나머지 조합만 실행한다. 다른 OS는 모든 조합을 키 경로가 실행한다.

use winit::keyboard::{Key, ModifiersState};

use crate::settings::KeybindingSettings;

/// 이 빌드에서 OS 메뉴가 첫 조합을 실행하는가.
pub(super) const OS_MENU_OWNS_FIRST: bool = cfg!(target_os = "macos");

/// 눌린 키가 실행할 창 제어 동작 ID.
pub(super) fn window_control_action(
    kb: &KeybindingSettings,
    key: &Key,
    mods: ModifiersState,
    menu_owns_first: bool,
) -> Option<&'static str> {
    [
        (&kb.minimize_window, "minimize_window"),
        (&kb.maximize_window, "maximize_window"),
        (&kb.close_window, "close_window"),
    ]
    .into_iter()
    .find(|(bindings, _)| {
        super::matches_any_binding(key_path_bindings(bindings, menu_owns_first), key, mods)
    })
    .map(|(_, action_id)| action_id)
}

/// 키 경로가 맡는 조합. 메뉴가 첫 조합을 key equivalent로 나타낼 수 없으면 메뉴 항목에 단축키가
/// 없으므로 첫 조합도 키 경로가 맡는다.
fn key_path_bindings(bindings: &[String], menu_owns_first: bool) -> &[String] {
    match bindings.split_first() {
        Some((first, rest))
            if menu_owns_first
                && !tasty_platform::menu_key_equivalent::menu_key_equivalent(first)
                    .key
                    .is_empty() =>
        {
            rest
        }
        _ => bindings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn kb(close: &[&str]) -> KeybindingSettings {
        KeybindingSettings {
            minimize_window: vec!["ctrl+alt+m".into()],
            maximize_window: Vec::new(),
            close_window: close.iter().map(|s| s.to_string()).collect(),
            ..KeybindingSettings::default()
        }
    }

    fn ch(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    /// 바인딩의 `alt`가 이 빌드에서 대응하는 winit modifier.
    fn alt() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::ALT
        }
    }

    #[test]
    fn menu_owned_first_combo_is_left_to_the_menu_and_the_rest_run() {
        let kb = kb(&["alt+shift+w", "ctrl+alt+w"]);
        let w = ch("w");
        assert_eq!(
            window_control_action(&kb, &w, alt() | ModifiersState::SHIFT, true),
            None,
            "the menu key equivalent already runs the first combo"
        );
        assert_eq!(
            window_control_action(&kb, &w, alt() | ModifiersState::CONTROL, true),
            Some("close_window")
        );
    }

    #[test]
    fn without_a_menu_every_combo_runs_in_order() {
        let kb = kb(&["alt+shift+w", "ctrl+alt+w"]);
        let w = ch("w");
        assert_eq!(
            window_control_action(&kb, &w, alt() | ModifiersState::SHIFT, false),
            Some("close_window")
        );
        assert_eq!(
            window_control_action(&kb, &w, alt() | ModifiersState::CONTROL, false),
            Some("close_window")
        );
        assert_eq!(
            window_control_action(&kb, &ch("m"), alt() | ModifiersState::CONTROL, false),
            Some("minimize_window")
        );
    }

    #[test]
    fn first_combo_the_menu_cannot_show_stays_on_the_key_path() {
        // 수식키만 있는 값은 메뉴 key equivalent가 비므로 키 경로가 그대로 맡는다.
        assert_eq!(key_path_bindings(&["alt+".to_string()], true).len(), 1);
        let both = ["alt+shift+w".to_string(), "ctrl+alt+w".to_string()];
        assert_eq!(key_path_bindings(&both, true), &both[1..]);
        assert_eq!(key_path_bindings(&both, false), &both[..]);
        assert!(key_path_bindings(&[], true).is_empty());
    }

    #[test]
    fn this_build_matches_its_menu_ownership() {
        assert_eq!(OS_MENU_OWNS_FIRST, cfg!(target_os = "macos"));
    }
}
