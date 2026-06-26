//! 숫자 키 (1~0) + modifier 로 탭/워크스페이스 직접 전환.

use winit::keyboard::Key;

use crate::adapters::ui::switch_overlay::{SwitchTarget, switch_target_for};
use crate::view::main::MainView;

impl MainView {
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
                // 대상 판별은 switch-number overlay 와 **단일 소스** 공유 헬퍼로 한다.
                match switch_target_for(kb, ctrl, shift, alt) {
                    Some(SwitchTarget::Tab) => {
                        let index = if ch == '0' {
                            9
                        } else {
                            (ch as usize) - ('1' as usize)
                        };
                        state.goto_tab_in_pane(engine, index);
                        return true;
                    }
                    Some(SwitchTarget::Workspace) => {
                        if let Some(digit) = ch.to_digit(10)
                            && (1..=9).contains(&digit)
                        {
                            state.switch_workspace(engine, (digit - 1) as usize);
                            return true;
                        }
                    }
                    None => {}
                }
            }
        }

        false
    }
}
