//! quick-switch 키 (설정된 슬롯 키 + next/prev) + modifier 로 탭/워크스페이스 전환.
//!
//! 슬롯 키·next/prev 키는 모두 `KeybindingSettings` 에서 읽는다(하드코딩 금지). 대상
//! (Tab/Workspace) 판정은 switch-number overlay 와 **단일 소스**([`switch_target_for`])를
//! 공유하고, modifier 는 `tab_switch_modifier`/`workspace_switch_modifier` 를 이 판정에서
//! 조합한다. 우선순위는 **next/prev 를 슬롯보다 먼저** 검사한다(커스텀 슬롯 키가
//! next/prev 키와 겹칠 때 next/prev 가 이긴다).
//!
//! **"개별 지정" 축(S-9)**: 축 modifier 가 `INDIVIDUAL_SWITCH_MODIFIER` sentinel 이면
//! `switch_target_for` 는 그 축을 절대 반환하지 않는다(sentinel 은 `Combo::parse_modifiers`
//! 에서 파싱 실패 → `None`). 이 축의 슬롯/다음/이전 필드는 이미 완성된 콤보 문자열이므로
//! (`view/settings/ui/keybindings_tab/quick_switch.rs` 참고) `matches_binding` 으로 직접
//! 매칭하는 별도 경로([`find_matching_individual_slot`])를 규칙 기반 경로보다 **먼저**
//! 검사한다 — 개별 지정 콤보는 문자 키가 아닐 수도 있어(F5, 화살표 등) `Key::Character`
//! 가드보다 앞서야 한다.
//!
//! **원칙 1/3**: 여기서 호출하는 `goto_tab_in_pane` / `next_workspace_in_active_category`
//! 등은 focused pane 의 active_tab · active_workspace(= 사용자 포커스 상태)를 바꾼다.
//! 따라서 **사용자 키 입력 경로(`handle_shortcut`)에서만** 호출되며 release IPC/CLI 로
//! 노출되지 않는다.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_binding;
use crate::adapters::ui::switch_overlay::{SwitchTarget, switch_target_for};
use crate::settings::KeybindingSettings;
use crate::view::main::MainView;

/// 개별 지정 슬롯(완전 콤보) 배열을 순회해 `key`/`mods` 와 일치하는 첫 슬롯의 인덱스를
/// 반환한다. `matches_any_binding` 은 불리언만 반환하므로, 어느 슬롯이 맞았는지 알아야
/// 하는 개별 지정 디스패치는 슬롯별로 하나씩 `matches_binding` 을 호출하는 이 헬퍼가
/// 필요하다(S-9 분석검증 구현자 노트 5).
fn find_matching_individual_slot(
    slots: &[String],
    key: &Key,
    mods: ModifiersState,
) -> Option<usize> {
    slots
        .iter()
        .position(|combo| !combo.is_empty() && matches_binding(combo, key, mods))
}

impl MainView {
    #[allow(clippy::too_many_arguments)] // reason: quick-switch dispatch context(정규화된 modifier bool 4개 + 원본 ModifiersState)
    pub(super) fn handle_numeric_switch_shortcuts(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        ctrl: bool,
        shift: bool,
        alt: bool,
        option: bool,
    ) -> bool {
        // 개별 지정 축 매칭 — 규칙 기반 경로(Character 전용)보다 먼저 검사한다. 세 축을
        // 순서대로 시도(표기 순서일 뿐 배타적이므로 어느 순서든 결과는 같다 — 두 축이
        // 동시에 개별 지정이어도 서로 다른 콤보를 쓰도록 충돌 검사가 저장 단계에서 막는다).
        if kb.tab_switch_modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER {
            if matches_binding(kb.tab_next_key(), key, mods) {
                state.next_tab_in_pane(engine);
                return true;
            }
            if matches_binding(kb.tab_prev_key(), key, mods) {
                state.prev_tab_in_pane(engine);
                return true;
            }
            if let Some(index) = find_matching_individual_slot(&kb.tab_switch_slot_keys, key, mods)
            {
                state.goto_tab_in_pane(engine, index);
                return true;
            }
        }
        if kb.workspace_switch_modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER {
            if matches_binding(kb.workspace_next_key(), key, mods) {
                state.next_workspace_in_active_category(engine);
                return true;
            }
            if matches_binding(kb.workspace_prev_key(), key, mods) {
                state.prev_workspace_in_active_category(engine);
                return true;
            }
            if let Some(local) =
                find_matching_individual_slot(&kb.workspace_switch_slot_keys, key, mods)
            {
                if engine.settings.general.workspace_categories_enabled {
                    state.switch_workspace_in_active_category(engine, local);
                } else {
                    state.switch_workspace(engine, local);
                }
                return true;
            }
        }
        if kb.category_switch_modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER
            && engine.settings.general.workspace_categories_enabled
        {
            if matches_binding(kb.category_next_key(), key, mods) {
                state.next_category(engine);
                return true;
            }
            if matches_binding(kb.category_prev_key(), key, mods) {
                state.prev_category(engine);
                return true;
            }
            if let Some(section) =
                find_matching_individual_slot(&kb.category_switch_slot_keys, key, mods)
            {
                state.switch_to_category(engine, section);
                return true;
            }
        }

        // 슬롯 키가 문자(`"q"` 등)일 수도 있으므로 `Key::Character` 이면 무조건 검색을
        // 시도하고, 매칭이 없으면 조용히 false 를 돌려 다른 단축키 매칭으로 넘긴다.
        let Key::Character(c) = key else {
            return false;
        };
        let ch = c.as_str();

        // 대상 판별은 switch-number overlay 와 **단일 소스** 공유 헬퍼로 한다
        // (정규화된 ctrl/shift/alt + modifier 필수 게이트 → 맨 키 오검출 없음). 개별
        // 지정 축은 sentinel 이 파싱 실패하므로 이 판정에서 절대 나오지 않는다(위에서
        // 이미 별도 경로로 처리됨) — 규칙 기반 축만 여기 도달한다.
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
                // next/prev 우선 검사(겹칠 때 next/prev 가 슬롯을 이긴다) — 탭/워크스페이스와 동일 순서.
                if ch == kb.category_next_key() {
                    state.next_category(engine);
                    return true;
                }
                if ch == kb.category_prev_key() {
                    state.prev_category(engine);
                    return true;
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
