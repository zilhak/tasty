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
                            let local = (digit - 1) as usize;
                            // 카테고리 토글 on 이면 active 카테고리 내 로컬 인덱스로,
                            // off 면 현행 전역 인덱스로(무회귀).
                            if engine.settings.general.workspace_categories_enabled {
                                state.switch_workspace_in_active_category(engine, local);
                            } else {
                                state.switch_workspace(engine, local);
                            }
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
