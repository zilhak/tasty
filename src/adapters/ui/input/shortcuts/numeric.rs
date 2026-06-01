//! 숫자 키 (1~0) + modifier 로 탭/워크스페이스 직접 전환.

use winit::keyboard::Key;

use crate::adapters::ui::window::main::MainWindow;

impl MainWindow {
    pub(super) fn handle_numeric_switch_shortcuts(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        if let Key::Character(c) = key {
            let ch = c.chars().next().unwrap_or('\0');
            if ch.is_ascii_digit() {
                let tab_mod = kb.tab_switch_modifier.to_lowercase();
                let tab_mod_matches = match tab_mod.as_str() {
                    "alt" => alt && !ctrl && !shift,
                    _ => ctrl && !shift && !alt,
                };
                if tab_mod_matches {
                    let index = if ch == '0' {
                        9
                    } else {
                        (ch as usize) - ('1' as usize)
                    };
                    state.goto_tab_in_pane(engine, index);
                    return true;
                }

                let ws_mod = kb.workspace_switch_modifier.to_lowercase();
                let ws_mod_matches = match ws_mod.as_str() {
                    "ctrl" => ctrl && !shift && !alt,
                    _ => alt && !ctrl && !shift,
                };
                if ws_mod_matches {
                    if let Some(digit) = ch.to_digit(10) {
                        if digit >= 1 && digit <= 9 {
                            state.switch_workspace(engine, (digit - 1) as usize);
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}
