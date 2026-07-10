//! quick-switch 키 (설정된 슬롯 키 + next/prev) + modifier 로 탭/워크스페이스 전환.
//!
//! 슬롯 키·next/prev 키는 모두 `KeybindingSettings` 에서 읽는다(하드코딩 금지). 대상
//! (Tab/Workspace) 판정은 switch-number overlay 와 **단일 소스**([`switch_target_for`])를
//! 공유하고, modifier 는 `tab_switch_modifier`/`workspace_switch_modifier` 를 이 판정에서
//! 조합한다. 우선순위는 **next/prev 를 슬롯보다 먼저** 검사한다(커스텀 슬롯 키가
//! next/prev 키와 겹칠 때 next/prev 가 이긴다).
//!
//! **원칙 1/3**: 여기서 호출하는 `goto_tab_in_pane` / `next_workspace_in_active_category`
//! 등은 focused pane 의 active_tab · active_workspace(= 사용자 포커스 상태)를 바꾼다.
//! 따라서 **사용자 키 입력 경로(`handle_shortcut`)에서만** 호출되며 release IPC/CLI 로
//! 노출되지 않는다.

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
        option: bool,
    ) -> bool {
        // 슬롯 키가 문자(`"q"` 등)일 수도 있으므로 `Key::Character` 이면 무조건 검색을
        // 시도하고, 매칭이 없으면 조용히 false 를 돌려 다른 단축키 매칭으로 넘긴다.
        let Key::Character(c) = key else {
            return false;
        };
        let ch = c.as_str();

        // 대상 판별은 switch-number overlay 와 **단일 소스** 공유 헬퍼로 한다
        // (정규화된 ctrl/shift/alt + modifier 필수 게이트 → 맨 키 오검출 없음).
        match switch_target_for(kb, ctrl, shift, alt, option) {
            Some(SwitchTarget::Tab) => {
                // next/prev 우선 검사(겹칠 때 next/prev 가 슬롯을 이긴다).
                if ch == kb.tab_next_key() {
                    state.next_tab_in_pane(engine);
                    return true;
                }
                if ch == kb.tab_prev_key() {
                    state.prev_tab_in_pane(engine);
                    return true;
                }
                if let Some(index) = kb.tab_switch_slot_keys.iter().position(|k| k == ch) {
                    state.goto_tab_in_pane(engine, index);
                    return true;
                }
            }
            Some(SwitchTarget::Workspace) => {
                if ch == kb.workspace_next_key() {
                    state.next_workspace_in_active_category(engine);
                    return true;
                }
                if ch == kb.workspace_prev_key() {
                    state.prev_workspace_in_active_category(engine);
                    return true;
                }
                if let Some(local) = kb.workspace_switch_slot_keys.iter().position(|k| k == ch) {
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
            Some(SwitchTarget::Category) => {
                // folders 기능 off 면 카테고리 조합+숫자 는 역할 없음(무시).
                if !engine.settings.general.workspace_categories_enabled {
                    return false;
                }
                if let Some(section) = kb.category_switch_slot_keys.iter().position(|k| k == ch) {
                    state.switch_to_category(engine, section);
                    return true;
                }
            }
            None => {}
        }

        false
    }
}
