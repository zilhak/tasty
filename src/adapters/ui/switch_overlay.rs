//! Switch-number overlay — 공통 배선 (P2a 탭 + P2b 사이드바 공유).
//!
//! 사용자가 `tab_switch_modifier`(기본 Ctrl) / `workspace_switch_modifier`(기본 Alt)를
//! **누르고 있는 동안** 탭/워크스페이스의 leading indicator 를 숫자 키캡 미리보기로
//! in-place 교체하기 위한 보조. 두 가지를 한 곳에 모은다:
//!
//! 1. **modifier↔대상 판정** ([`switch_target_for`]) — 현재 눌린 modifier 가 tab/workspace
//!    전환 단축키와 단독 일치하면 그 대상([`SwitchTarget`])을 돌려주는 단일 소스.
//!    단축키 소비처 `input/shortcuts/numeric.rs` 와 동일 조건/우선순위를 공유한다(중복
//!    구현 없음). 사이드바 draw 경로용 egui modifier 래퍼 [`workspace_switch_held`] 도 이
//!    함수를 통해 판정한다. 탭 draw 경로가 매 프레임 읽는 스냅샷은 [`SwitchOverlayState`]
//!    (focused pane 한정 표시를 위해 `pane_id` 를 함께 담는다).
//! 2. **키캡 자리 잡기** ([`keycap_size`] · [`paint_keycap`]) — 탭 스트립·사이드바가
//!    정해진 slot 에 겹쳐 그릴 수 있도록 좌표를 받는 얇은 배선. **모양은 여기 없다** —
//!    `tasty_ui_widgets::paint_num_keycap` 한 벌이고 갤러리 specimen 도 같은 함수를
//!    부른다. 치수도 `switch-overlay-*` 토큰에서만 온다(지역 상수 금지 — 아래).
//!
//! **사용자↔에이전트 분리**: modifier 상태는 egui `ctx.input(...).modifiers` — 실제 사용자
//! 키 입력(winit→egui raw_input)만 반영한다. IPC/CLI/에이전트 경로는 egui raw_input 에
//! 주입할 수 없으므로 이 오버레이를 강제 표시할 수 없다(순수 미리보기).

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

/// draw 경로(04 탭 / 05 사이드바)가 매 프레임 읽는 switch-number overlay 스냅샷.
///
/// `MainView` 가 `ModifiersChanged` 마다 [`switch_target_for`] 로 갱신한다. 창
/// 비활성/포커스 상실 시 `None` 으로 clear 된다. `pane_id` 는 `Tab` 대상일 때만
/// `Some` — 04 가 오버레이를 그릴 focused pane 을 식별하는 데 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchOverlayState {
    /// 현재 held modifier 가 가리키는 전환 대상.
    pub target: SwitchTarget,
    /// `Tab` 대상일 때 오버레이를 그릴 focused pane id. `Workspace` 면 `None`.
    pub pane_id: Option<u32>,
}

/// 현재 눌린 modifier 조합으로 switch overlay 의 대상을 판정하는 **단일 소스**.
///
/// 세 축(탭/워크스페이스/카테고리) 각각의 독립 modifier **조합**([`Combo::parse_modifiers`]
/// 로 파싱)과 현재 눌린 조합이 **정확히 일치**할 때만 그 대상을 돌려준다. 단일 토큰
/// (`"ctrl"`)은 조합의 부분집합이므로 그대로 동작한다. 정확 일치라 축이 서로 새지 않고
/// (`ctrl` 단독 ≠ `ctrl+shift`), 우선순위 로직이 필요 없다 — 두 축이 같은 조합을 갖는
/// 상태는 설정 단계 충돌 차단으로 애초에 저장되지 않는다. `Category` 의 folders 기능
/// 게이트는 호출측이 판단한다.
///
/// `ctrl`/`shift`/`alt`/`option` 은 플랫폼 정규화가 끝난 값을 받는다: `alt` 는 `"alt"`
/// 토큰(macOS 물리 ⌘=super, 그 외 Alt), `option` 은 `"option"` 토큰(macOS 물리 ⌥, 그
/// 외 항상 false). 정규화는 호출측 `dispatch.rs`/`main.rs`/래퍼에서 처리된다.
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
    // 축 순서(탭→워크스페이스→카테고리)는 표기용일 뿐 — 충돌 차단으로 배타가 보장돼
    // 어느 순서든 결과가 같다.
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

/// 현재 눌린 modifier 가 `workspace_switch_modifier` 와 단독 일치하는지 (사이드바 오버레이).
///
/// egui `Modifiers` → [`switch_target_for`] 가 받는 **정규화된** `alt` 로 변환한다.
/// `"alt"` 토큰의 물리 키는 macOS 에서 Command(⌘), 그 외에서 Alt 다(위치 기반 추상화).
/// egui 에서 Command 는 `mac_cmd`, winit `super_key()` 와 대응한다 — 실제 전환 경로
/// (`dispatch.rs` numeric)·탭 스냅샷 경로(`view/main.rs` ModifiersChanged)가 둘 다
/// macOS 에서 `super_key()` 로 정규화해 같은 `switch_target_for` 에 넘기므로, 이 래퍼도
/// 동일하게 `mac_cmd` 를 넘겨야 "표시 판정"과 "실제 전환"이 일치한다. macOS Option
/// (egui `mods.alt`)은 `"alt"` 토큰이 아니므로 무시한다.
pub fn workspace_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    #[cfg(target_os = "macos")]
    let (alt, option) = (mods.mac_cmd, mods.alt);
    #[cfg(not(target_os = "macos"))]
    let (alt, option) = (mods.alt, false);
    switch_target_for(kb, mods.ctrl, mods.shift, alt, option) == Some(SwitchTarget::Workspace)
}

/// 현재 눌린 modifier 가 카테고리 전환(`category_switch_modifier`, 기본 `ctrl+shift`)과
/// 일치하는지 (사이드바 카테고리 키캡). [`workspace_switch_held`] 와 동일한 정규화
/// (`"alt"` 토큰 = macOS Command, `"option"` = macOS Option)를 거쳐 [`switch_target_for`]
/// 에 넘긴다. folders 기능 게이트는 호출측이 별도로 판단한다(이 함수는 modifier 만 본다).
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

/// 이 pane 의 탭 `index` 에 그릴 키캡 문자.
///
/// `tab_switch_modifier` hold 시 [`SwitchOverlayState::pane_id`] 가 가리키는 **focused
/// pane 의 탭바에서만** 키캡을 그린다. 단축키(`goto_tab_in_pane`)는 focused pane 의 탭만
/// 전환하므로, 비-focused pane 탭바에 번호를 띄우면 "눌러도 거기로 안 가는" 거짓 안내가
/// 된다 → `overlay_pane != Some(pane_id)` 이면 held 여도 `None`(아이콘 유지).
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

/// 키캡 한 변(px) — 우측정렬·중앙정렬 배치 계산용(카테고리 헤더/레일 키캡).
///
/// **테마에서 온다.** `switch-overlay-size` 는 `ui_zoom` 이 곱해진 값이라, 여기 16 을
/// 다시 적으면 배율에서만 갈린다 — 기본 배율에서는 두 값이 같아 눈에도 테스트에도
/// 안 잡히고, 사용자가 UI 를 키우는 순간 갤러리만 커진다.
pub fn keycap_size(theme: &Theme) -> f32 {
    theme.switch_overlay_size().value()
}

/// 한 자리 숫자 키캡을 `center` 기준 slot 에 그린다.
///
/// **그림은 여기 없다** — `tasty_ui_widgets::paint_num_keycap` 한 벌이고 갤러리
/// specimen 도 같은 함수를 부르므로 두 화면이 갈릴 수 없다. 여기에 형상을 다시 적으면
/// 그 순간 두 벌이 되고, 갈리는 첫 축은 배율이다: `switch-overlay-*` 치수는 `ui_zoom`
/// 을 타는데 손으로 박은 상수는 안 탄다. `keycap_side_is_a_theme_token_not_a_local_constant`
/// 와 `this_file_does_not_paint_the_keycap_itself` 가 그 두 형태를 각각 못박는다.
///
/// `alpha` 는 등장 페이드 계수(0..=1) — modifier 홀드 시작 시 `motion-ui-fast`(90ms)
/// 동안 0→1 로 올라온다. UI 오버레이 chrome 한정 모션이며 터미널 grid 0ms 불변식과
/// 무관하다. 호출측이 `Context::animate_bool_with_time` 으로 프레임마다 계산해 넘긴다.
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

/// switch-number overlay 등장 페이드 계수(0..=1).
///
/// `visible`(modifier held + 이 오버레이 인스턴스가 표시 대상) 가 false→true 로 바뀌면
/// `motion-ui-fast`(90ms) 동안 0→1 로 올라오고, 놓으면 같은 시간으로 0 으로 내려간다.
/// egui 애니메이션 상태를 양방향으로 갱신하려면 **프레임마다**(키캡을 그리지 않는
/// 프레임 포함) 호출해야 한다. `id_salt` 로 오버레이 인스턴스(pane / sidebar)를 구분한다.
///
/// 접근성 "모션 감소"(`theme.reduced_motion`)면 지속시간이 0 이 되어 페이드 없이
/// 즉시 나타났다 사라진다 — 설정 설명이 약속하는 "모든 UI 페이드/슬라이드를 즉시
/// 끝낸다" 가 이 자리에도 걸린다. 값을 `Theme` 에서 읽는 이유는 ADR-0174.
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
            theme.motion_ui_fast_ms() / 1000.0
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

    // macOS: `"alt"`(default ws) 토큰은 Command(⌘)에 매핑된다. 실제 전환(⌘+1..9)·탭
    // 스냅샷 경로가 `super_key()` 로 정규화하는 것과 대칭 — 표시 판정도 ⌘ 기준이어야 한다.
    #[cfg(target_os = "macos")]
    #[test]
    fn workspace_held_matches_mac_cmd_not_option() {
        let kb = kb_with("ctrl", "alt"); // ws=alt(default) → macOS Cmd
        // ⌘(mac_cmd) 단독 → 오버레이 표시 (실제 ⌘+1..9 전환과 일치).
        assert!(workspace_switch_held(
            mods_mac(false, true, false, false),
            &kb
        ));
        // ⌥(Option=egui alt) 단독 → 표시 안 됨 (이게 수정 전의 잘못된 동작이었다).
        assert!(!workspace_switch_held(
            mods_mac(false, false, true, false),
            &kb
        ));
        // ⌘+Ctrl 혼합 → 표시 안 됨 (단축키 단독-modifier 조건과 동일).
        assert!(!workspace_switch_held(
            mods_mac(true, true, false, false),
            &kb
        ));
        // ⌘+⌥ → 표시 안 됨 — option 이 1급 축이 된 뒤로 held=alt+option 은 ws("alt")와
        // 정확 일치하지 않는다. 실제 전환도 option 을 축으로 넘기므로 표시=동작 일치.
        assert!(!workspace_switch_held(
            mods_mac(false, true, true, false),
            &kb
        ));
    }

    #[test]
    fn rebound_modifiers_follow_settings() {
        // tab=alt / ws=ctrl 로 재바인딩하면 판정도 따라간다.
        let kb = kb_with("alt", "ctrl");
        assert!(workspace_switch_held(mods(true, false, false), &kb));
        assert!(!workspace_switch_held(mods(false, true, false), &kb));
    }

    #[test]
    fn switch_target_default_axes() {
        // 기본 프리셋: 탭=ctrl, 워크스페이스=alt, 카테고리=ctrl+shift.
        let kb = KeybindingSettings::default();
        assert_eq!(
            switch_target_for(&kb, true, false, false, false),
            Some(SwitchTarget::Tab)
        );
        assert_eq!(
            switch_target_for(&kb, false, false, true, false),
            Some(SwitchTarget::Workspace)
        );
        // ctrl+shift → Category (독립 축).
        assert_eq!(
            switch_target_for(&kb, true, true, false, false),
            Some(SwitchTarget::Category)
        );
        // ctrl 단독은 카테고리(ctrl+shift)와 정확 일치하지 않으므로 Tab 이지 Category 아님.
        assert_ne!(
            switch_target_for(&kb, true, false, false, false),
            Some(SwitchTarget::Category)
        );
    }

    #[test]
    fn switch_target_mixed_modifier_is_none() {
        let kb = KeybindingSettings::default(); // 탭=ctrl, ws=alt, cat=ctrl+shift
        // 어느 축과도 정확 일치하지 않는 조합 → None.
        assert_eq!(switch_target_for(&kb, true, false, true, false), None); // ctrl+alt
        assert_eq!(switch_target_for(&kb, false, true, false, false), None); // shift 단독
        assert_eq!(switch_target_for(&kb, false, false, false, false), None); // 무 modifier
    }

    #[test]
    fn switch_target_rebind_swaps() {
        // tab=alt / ws=ctrl 로 재바인딩하면 대상도 반대로.
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
        // 카테고리 modifier 를 독립적으로 재바인딩(ws 파생 아님).
        let mut kb = kb_with("ctrl", "alt");
        kb.category_switch_modifier = "alt+shift".into();
        // alt+shift → Category, ctrl+shift 는 이제 어느 축도 아님 → None.
        assert_eq!(
            switch_target_for(&kb, false, true, true, false),
            Some(SwitchTarget::Category)
        );
        assert_eq!(switch_target_for(&kb, true, true, false, false), None);
    }

    /// S-9: "개별 지정" sentinel 로 바뀐 축은 `switch_target_for` 가 **절대** 그 대상을
    /// 반환하지 않는다 — sentinel 문자열이 `Combo::parse_modifiers` 에서 파싱 실패
    /// (`None`)하므로 규칙 기반 판정에서 구조적으로 제외된다. switch-number 오버레이가
    /// 이 함수를 단일 소스로 공유하므로, 이 결과는 곧 "개별 지정 축엔 오버레이가 자동으로
    /// 안 뜬다"는 의도된 부작용을 고정하는 회귀 테스트다.
    #[test]
    fn individual_axis_never_matched_by_switch_target_for() {
        let kb = KeybindingSettings {
            tab_switch_modifier: KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.to_string(),
            ..Default::default()
        }; // ws=alt, cat=ctrl+shift 는 기본값 유지.
        // ctrl 단독을 눌러도 더 이상 Tab 을 반환하지 않는다(다른 축과도 일치하지 않으므로 None).
        assert_eq!(switch_target_for(&kb, true, false, false, false), None);
        // 워크스페이스/카테고리 축은 여전히 정상 동작(개별 지정은 탭 축에만 적용됨).
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
        // 사용자가 5번째 슬롯(index 4)을 "q" 로 재바인딩하면 키캡도 "q" 로 표시(표시=동작).
        let mut kb = KeybindingSettings::default();
        kb.set_tab_slot_key(4, "q");
        assert_eq!(tab_digit(&kb, 4), Some("q"));
        assert_eq!(tab_digit(&kb, 0), Some("1")); // 나머지는 그대로
    }

    #[test]
    fn tab_digit_empty_slot_is_none() {
        // 슬롯을 비우면 미바인딩 → 키캡 없음(빈 키캡 박스 방지).
        let mut kb = KeybindingSettings::default();
        kb.set_tab_slot_key(3, "");
        assert_eq!(tab_digit(&kb, 3), None);
    }

    #[test]
    fn tab_keycap_only_on_focused_pane() {
        let kb = KeybindingSettings::default();
        // overlay 대상 = pane 7 (focused). pane 7 탭바만 키캡, pane 3 은 아이콘 유지.
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 0), Some("1"));
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 9), Some("0"));
        assert_eq!(tab_keycap_for(&kb, Some(7), 3, 0), None); // 비-focused pane
        // 범위 가드는 focused pane 에서도 그대로(11번째+ 키캡 없음).
        assert_eq!(tab_keycap_for(&kb, Some(7), 7, 10), None);
    }

    #[test]
    fn tab_keycap_none_when_not_held() {
        let kb = KeybindingSettings::default();
        // held 아님(overlay None) → 어느 pane 도 키캡 없음.
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

    /// 키캡 한 변은 **토큰**에서 온다 — 지역 상수로 되박으면 `ui_zoom` 에서 갈린다.
    ///
    /// 기본 배율만 재면 상수와 토큰이 같은 값이라 통과한다. 그래서 배율을 두 개 잡고
    /// **함께 움직이는가**를 묻는다.
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

    /// 이 파일은 키캡을 **직접 그리지 않는다** — 그림은 `tasty_ui_widgets` 한 벌이다.
    ///
    /// 술어를 "두 그림이 같은가" 가 아니라 **"그림이 하나뿐인가"** 로 세운다. 같은지를
    /// 물으면 두 벌이 있어도 초록일 수 있어, 갈리기 시작한 뒤에야 빨개진다.
    ///
    /// 스캔 대상이 **이 파일 자신**이라 주석·문자열을 그대로 두면 설명하려고 적은
    /// 이름이 위반으로 잡힌다 — 그래서 `mask_non_code` 로 덮은 사본 위에서만 판정한다.
    /// 위 바늘들이 이 테스트 본문에 리터럴로 적혀 있어도 자기를 잡지 않는 이유다.
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
