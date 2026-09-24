//! 전환 modifier의 대상 판정과 슬롯 키캡 표시. 단축키 처리와 같은 switch_target_for를 사용한다.
//! 키캡은 공용 위젯으로 그리며 표시 상태는 사용자 modifier 입력에서 읽는다.

use tasty_type_appearance::theme::Theme;

use crate::adapters::ui::input::shortcuts::modifier_hint::Combo;
use crate::settings::KeybindingSettings;

/// 현재 눌린 modifier 가 가리키는 switch-number overlay 의 대상.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchTarget {
    /// 탭 전환 (`tab_switch_modifier`, 기본 Ctrl).
    Tab,
    /// 워크스페이스 전환 (`workspace_switch_modifier`, 기본 Alt).
    Workspace,
    /// 카테고리 전환 (`category_switch_modifier`, 기본 `ctrl+shift`).
    /// folders 기능 on 일 때만 의미. modifier-exclusive: 세 축 조합이 충돌 없이 배타.
    Category,
}

/// draw 경로(탭 바 / 사이드바)가 매 프레임 읽는 switch-number overlay 스냅샷.
///
/// `MainView` 가 `ModifiersChanged` 마다 [`switch_target_for`] 로 갱신한다. 창
/// 비활성/포커스 상실 시 `None` 으로 clear 된다. `pane_id` 는 `Tab` 대상일 때만
/// `Some` — 탭 바 draw 경로가 오버레이를 그릴 focused pane 을 식별하는 데 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchOverlayState {
    /// 현재 held modifier 가 가리키는 전환 대상.
    pub target: SwitchTarget,
    /// `Tab` 대상일 때 오버레이를 그릴 focused pane id. `Workspace` 면 `None`.
    pub pane_id: Option<u32>,
}

/// 설정된 탭·워크스페이스·카테고리 modifier 조합과 정확히 일치하는 대상을 반환한다.
/// 카테고리 기능의 사용 여부는 호출부에서 확인한다.
/// 입력은 정규화된 값이다. alt 토큰은 macOS의 Command, 나머지 플랫폼의 Alt이며
/// option은 macOS Option이고 다른 플랫폼에서는 false다.
pub fn switch_target_for(
    kb: &KeybindingSettings,
    ctrl: bool,
    shift: bool,
    alt: bool,
    option: bool,
) -> Option<SwitchTarget> {
    let held = Combo {
        ctrl,
        alt,
        option,
        shift,
    };
    if Combo::parse_modifiers(&kb.tab_switch_modifier) == Some(held) {
        return Some(SwitchTarget::Tab);
    }
    if Combo::parse_modifiers(&kb.workspace_switch_modifier) == Some(held) {
        return Some(SwitchTarget::Workspace);
    }
    if Combo::parse_modifiers(&kb.category_switch_modifier) == Some(held) {
        return Some(SwitchTarget::Category);
    }
    None
}

/// 워크스페이스 전환 조합인지 확인한다. macOS의 alt 토큰은 egui.mac_cmd에 대응한다.
pub fn workspace_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    #[cfg(target_os = "macos")]
    let (alt, option) = (mods.mac_cmd, mods.alt);
    #[cfg(not(target_os = "macos"))]
    let (alt, option) = (mods.alt, false);
    switch_target_for(kb, mods.ctrl, mods.shift, alt, option) == Some(SwitchTarget::Workspace)
}

/// 같은 정규화로 카테고리 전환 조합을 확인한다. 카테고리 기능 사용 여부는 호출부에서 확인한다.
pub fn category_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    #[cfg(target_os = "macos")]
    let (alt, option) = (mods.mac_cmd, mods.alt);
    #[cfg(not(target_os = "macos"))]
    let (alt, option) = (mods.alt, false);
    switch_target_for(kb, mods.ctrl, mods.shift, alt, option) == Some(SwitchTarget::Category)
}

/// 탭 슬롯 `index` 에 표시할 키캡 문자. 설정된 `tab_switch_slot_keys[index]` 를 그대로
/// 돌려준다(하드코딩 상수 없음 — "표시=동작 일치"). 슬롯 범위 밖(index ≥ 10)이거나
/// 슬롯 키가 비어 있으면(= 미바인딩) `None`.
pub fn tab_digit(kb: &KeybindingSettings, index: usize) -> Option<&str> {
    kb.tab_slot_key(index).filter(|s| !s.is_empty())
}

/// 실제 전환 대상인 포커스된 pane의 탭에만 키캡을 표시한다.
pub fn tab_keycap_for(
    kb: &KeybindingSettings,
    overlay_pane: Option<u32>,
    pane_id: u32,
    index: usize,
) -> Option<&str> {
    if overlay_pane == Some(pane_id) {
        tab_digit(kb, index)
    } else {
        None
    }
}

/// 워크스페이스 슬롯 **로컬 인덱스**(카테고리 토글 on 이면 active 카테고리 내 로컬 순서,
/// off 면 전역 순서) → 키캡 문자. 설정된 `workspace_switch_slot_keys[local_idx]` 를
/// 그대로 돌려준다. 슬롯 범위 밖(local_idx ≥ 9)이거나 비어 있으면 `None`.
pub fn workspace_digit(kb: &KeybindingSettings, local_idx: usize) -> Option<&str> {
    kb.workspace_slot_key(local_idx).filter(|s| !s.is_empty())
}

/// 카테고리 슬롯 **섹션 인덱스**(0=reserved normal, 1.. = 사용자 카테고리) → 키캡 문자.
/// 설정된 `category_switch_slot_keys[index]`(1..10)를 그대로 돌려준다. 범위 밖(index ≥ 10)
/// 이거나 비어 있으면 `None` — 11번째+ 카테고리는 키캡 없음(누를 키 없는 번호 미표시).
pub fn category_digit(kb: &KeybindingSettings, index: usize) -> Option<&str> {
    kb.category_slot_key(index).filter(|s| !s.is_empty())
}

/// 배율이 반영된 Theme의 키캡 크기.
pub fn keycap_size(theme: &Theme) -> f32 {
    theme.switch_overlay_size().value()
}

/// 공용 위젯으로 슬롯 가운데에 키캡을 그린다. alpha는 호출부에서 계산한 페이드 값이다.
pub fn paint_keycap(
    painter: &egui::Painter,
    theme: &Theme,
    center: egui::Pos2,
    digit: &str,
    active: bool,
    alpha: f32,
) {
    tasty_ui_widgets::paint_num_keycap(painter, theme, center, digit, active, alpha);
}

/// 표시 여부에 따라 양방향 페이드를 갱신한다. 그리지 않는 프레임에도 호출해야 한다.
/// 인스턴스마다 id_salt를 구분하고 모션 감소 설정이면 즉시 전환한다.
pub fn appear_fade(
    ctx: &egui::Context,
    theme: &Theme,
    id_salt: impl std::hash::Hash,
    visible: bool,
) -> f32 {
    ctx.animate_bool_with_time(
        egui::Id::new("switch_overlay_fade").with(id_salt),
        visible,
        if theme.reduced_motion {
            0.0
        } else {
            theme.switch_overlay_fade().to_secs_f32()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb_with(tab: &str, ws: &str) -> KeybindingSettings {
        let mut kb = KeybindingSettings::default();
        kb.tab_switch_modifier = tab.to_string();
        kb.workspace_switch_modifier = ws.to_string();
        kb
    }

    fn mods(ctrl: bool, alt: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers {
            ctrl,
            alt,
            shift,
            ..Default::default()
        }
    }

    /// macOS 전용: `"alt"` 토큰의 물리 키인 Command(⌘=`mac_cmd`)와 Option(egui `alt`)을
    /// 구분해 세팅한다. `mods()` 는 `mac_cmd` 를 못 세팅하므로 별도 헬퍼.
    #[cfg(target_os = "macos")]
    fn mods_mac(ctrl: bool, mac_cmd: bool, option: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers {
            ctrl,
            alt: option,
            shift,
            mac_cmd,
            ..Default::default()
        }
    }

    // egui `mods.alt` 는 macOS 에서 Option 이라 `"alt"` 토큰(=Cmd)이 아니다. 이 테스트는
    // non-macOS(여기서 `mods.alt` = 실제 Alt = workspace modifier) 경로만 검증한다.
    // macOS Cmd 경로는 `workspace_held_matches_mac_cmd_not_option` 가 담당한다.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn workspace_held_matches_default_alt_alone() {
        let kb = kb_with("ctrl", "alt");
        assert!(workspace_switch_held(mods(false, true, false), &kb));
        assert!(!workspace_switch_held(mods(true, true, false), &kb));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_held_matches_mac_cmd_not_option() {
        let kb = kb_with("ctrl", "alt"); // ws=alt(default) → macOS Cmd
        assert!(workspace_switch_held(
            mods_mac(false, true, false, false),
            &kb
        ));
        assert!(!workspace_switch_held(
            mods_mac(false, false, true, false),
            &kb
        ));
        assert!(!workspace_switch_held(
            mods_mac(true, true, false, false),
            &kb
        ));
        assert!(!workspace_switch_held(
            mods_mac(false, true, true, false),
            &kb
        ));
    }

    #[test]
    fn rebound_modifiers_follow_settings() {
        let kb = kb_with("alt", "ctrl");
        assert!(workspace_switch_held(mods(true, false, false), &kb));
        assert!(!workspace_switch_held(mods(false, true, false), &kb));
    }

    #[test]
    fn switch_target_default_axes() {
        let kb = KeybindingSettings::default();
        assert_eq!(
            switch_target_for(&kb, true, false, false, false),
            Some(SwitchTarget::Tab)
        );
        assert_eq!(
            switch_target_for(&kb, false, false, true, false),
            Some(SwitchTarget::Workspace)
        );
        assert_eq!(
            switch_target_for(&kb, true, true, false, false),
            Some(SwitchTarget::Category)
        );
        assert_ne!(
            switch_target_for(&kb, true, false, false, false),
            Some(SwitchTarget::Category)
        );
    }

    #[test]
    fn switch_target_mixed_modifier_is_none() {
        let kb = KeybindingSettings::default(); // 탭=ctrl, ws=alt, cat=ctrl+shift
        assert_eq!(switch_target_for(&kb, true, false, true, false), None); // ctrl+alt
        assert_eq!(switch_target_for(&kb, false, true, false, false), None); // shift 단독
        assert_eq!(switch_target_for(&kb, false, false, false, false), None); // 무 modifier
    }

    #[test]
    fn switch_target_rebind_swaps() {
        let kb = kb_with("alt", "ctrl");
        assert_eq!(
            switch_target_for(&kb, false, false, true, false),
            Some(SwitchTarget::Tab)
        );
        assert_eq!(
            switch_target_for(&kb, true, false, false, false),
            Some(SwitchTarget::Workspace)
        );
    }

    #[test]
    fn switch_target_category_is_independent_axis() {
        let mut kb = kb_with("ctrl", "alt");
        kb.category_switch_modifier = "alt+shift".into();
        assert_eq!(
            switch_target_for(&kb, false, true, true, false),
            Some(SwitchTarget::Category)
        );
        assert_eq!(switch_target_for(&kb, true, true, false, false), None);
    }

    /// 개별 지정 설정은 규칙 기반 modifier 판정에서 제외돼 자동 오버레이가 나오지 않는다.
    #[test]
    fn individual_axis_never_matched_by_switch_target_for() {
        let kb = KeybindingSettings {
            tab_switch_modifier: KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.to_string(),
            ..Default::default()
        }; // ws=alt, cat=ctrl+shift 는 기본값 유지.
        assert_eq!(switch_target_for(&kb, true, false, false, false), None);
        assert_eq!(
            switch_target_for(&kb, false, false, true, false),
            Some(SwitchTarget::Workspace)
        );
        assert_eq!(
            switch_target_for(&kb, true, true, false, false),
            Some(SwitchTarget::Category)
        );
    }

    /// 세 축 모두 개별 지정이면 어떤 modifier 조합을 눌러도 `switch_target_for` 는 항상
    /// `None` — 규칙 기반 판정 경로가 완전히 비활성화된다.
    #[test]
    fn all_axes_individual_means_switch_target_for_always_none() {
        let individual = KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.to_string();
        let kb = KeybindingSettings {
            tab_switch_modifier: individual.clone(),
            workspace_switch_modifier: individual.clone(),
            category_switch_modifier: individual,
            ..Default::default()
        };
        for (ctrl, shift, alt, option) in [
            (true, false, false, false),
            (false, false, true, false),
            (true, true, false, false),
            (false, true, false, false),
        ] {
            assert_eq!(switch_target_for(&kb, ctrl, shift, alt, option), None);
        }
    }

    #[test]
    fn category_digit_range() {
        let kb = KeybindingSettings::default();
        assert_eq!(category_digit(&kb, 0), Some("1")); // reserved normal = 1
        assert_eq!(category_digit(&kb, 8), Some("9"));
        assert_eq!(category_digit(&kb, 9), Some("0")); // 10번째 = 0
        assert_eq!(category_digit(&kb, 10), None); // 11번째+ 키캡 없음
    }

    #[test]
    fn tab_digit_range() {
        let kb = KeybindingSettings::default();
        assert_eq!(tab_digit(&kb, 0), Some("1"));
        assert_eq!(tab_digit(&kb, 8), Some("9"));
        assert_eq!(tab_digit(&kb, 9), Some("0")); // 10번째 = Ctrl+0
        assert_eq!(tab_digit(&kb, 10), None); // 11번째부터 키캡 없음(슬롯 배열 밖)
    }

    #[test]
    fn tab_digit_reflects_custom_slot_key() {
        let mut kb = KeybindingSettings::default();
        kb.set_tab_slot_key(4, "q");
        assert_eq!(tab_digit(&kb, 4), Some("q"));
        assert_eq!(tab_digit(&kb, 0), Some("1")); // 나머지는 그대로
    }

    #[test]
    fn tab_digit_empty_slot_is_none() {
        let mut kb = KeybindingSettings::default();
        kb.set_tab_slot_key(3, "");
        assert_eq!(tab_digit(&kb, 3), None);
    }

    #[test]
    fn tab_keycap_only_on_focused_pane() {
        let kb = KeybindingSettings::default();
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 0), Some("1"));
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 9), Some("0"));
        assert_eq!(tab_keycap_for(&kb, Some(7), 3, 0), None); // 비-focused pane
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 10), None);
    }

    #[test]
    fn tab_keycap_none_when_not_held() {
        let kb = KeybindingSettings::default();
        assert_eq!(tab_keycap_for(&kb, None, 7, 0), None);
        assert_eq!(tab_keycap_for(&kb, None, 3, 0), None);
    }

    #[test]
    fn workspace_digit_range() {
        let kb = KeybindingSettings::default();
        assert_eq!(workspace_digit(&kb, 0), Some("1"));
        assert_eq!(workspace_digit(&kb, 8), Some("9"));
        assert_eq!(workspace_digit(&kb, 9), None); // 슬롯 9개 → 10번째부터 없음
    }

    #[test]
    fn workspace_digit_reflects_custom_slot_key() {
        let mut kb = KeybindingSettings::default();
        kb.set_workspace_slot_key(2, "w");
        assert_eq!(workspace_digit(&kb, 2), Some("w"));
        assert_eq!(workspace_digit(&kb, 0), Some("1"));
    }

    /// 기본 배율뿐 아니라 다른 배율에서도 Theme의 키캡 크기를 사용하는지 확인한다.
    #[test]
    fn keycap_side_is_a_theme_token_not_a_local_constant() {
        let one = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let two = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 2.0);
        assert_eq!(keycap_size(&one), one.switch_overlay_size().value());
        assert_eq!(keycap_size(&two), two.switch_overlay_size().value());
        assert!(
            keycap_size(&two) > keycap_size(&one),
            "배율을 안 타면 지역 상수로 되돌아간 것이다"
        );
    }

    /// 렌더 구현을 중복하지 않고 공용 위젯을 호출하는지 검사한다.
    /// 이 파일 자체를 읽으므로 주석·문자열은 mask_non_code로 제외한다.
    #[test]
    fn this_file_does_not_paint_the_keycap_itself() {
        let code = crate::source_guards::mask_non_code(include_str!("switch_overlay.rs"));
        for needle in [
            "rect_filled",
            "rect_stroke",
            "line_segment",
            "circle_filled",
            "layout_no_wrap",
        ] {
            assert!(
                !code.contains(needle),
                "`{needle}` 호출이 이 파일에 있다 — 키캡 형상이 두 벌이 됐다. \
                 tasty_ui_widgets::paint_num_keycap 에 위임해라"
            );
        }
    }
}
