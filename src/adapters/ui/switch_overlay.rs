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
//! 2. **키캡 그리기** ([`paint_keycap`]) — 갤러리 `switch_overlay::num_cap` 형상과 동일
//!    (본체 `kbd()` 키캡 + active accent 변종). 좌표 painting 이라 탭 스트립·사이드바
//!    어디서든 정해진 slot 에 그릴 수 있다.
//!
//! **사용자↔에이전트 분리**: modifier 상태는 egui `ctx.input(...).modifiers` — 실제 사용자
//! 키 입력(winit→egui raw_input)만 반영한다. IPC/CLI/에이전트 경로는 egui raw_input 에
//! 주입할 수 없으므로 이 오버레이를 강제 표시할 수 없다(순수 미리보기).

use tasty_type_appearance::theme::Theme;

use crate::settings::KeybindingSettings;

/// 키캡 한 변 (= 디자인 `switch-overlay-size` = `kbd-size` = size-16). 갤러리 num_cap·
/// 본체 `kbd()`(chip.rs `KBD_HEIGHT`) 와 동일.
const KEYCAP_SIZE: f32 = 16.0;
/// 키캡 하단 3D edge (= `switch-overlay-shadow-depth` = size-2). chip.rs `KBD_BOTTOM_BORDER`.
const KEYCAP_BOTTOM_BORDER: f32 = 2.0;

/// 현재 눌린 modifier 가 가리키는 switch-number overlay 의 대상.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchTarget {
    /// 탭 전환 (`tab_switch_modifier`, 기본 Ctrl).
    Tab,
    /// 워크스페이스 전환 (`workspace_switch_modifier`, 기본 Alt).
    Workspace,
    /// 카테고리 전환 (`workspace_switch_modifier` + Shift, 기본 Alt+Shift, T4WS ②⑤).
    /// folders 기능 on 일 때만 의미. modifier-exclusive: Workspace(단독)와 동시 렌더 없음.
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
/// 단축키 소비처 `input/shortcuts/numeric.rs` 와 **완전히 동일한 조건/우선순위**:
/// tab 을 먼저 검사하고, 아니면 워크스페이스축(`workspace_switch_modifier`)을 본다 —
/// 그 키 + Shift = `Category`, 단독 = `Workspace`(Shift 유무로 상호 배타). 축과 무관한
/// modifier 가 섞이면 `None`. `Category` 의 folders 기능 게이트는 호출측이 판단한다.
/// `ctrl`/`shift`/`alt` 는 플랫폼 정규화가 끝난 값을 받는다 (macOS super→alt 매핑은
/// 호출측 `dispatch.rs`/래퍼에서 처리됨).
pub fn switch_target_for(
    kb: &KeybindingSettings,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<SwitchTarget> {
    let tab_matches = match kb.tab_switch_modifier.to_lowercase().as_str() {
        "alt" => alt && !ctrl && !shift,
        // 기본 ctrl.
        _ => ctrl && !shift && !alt,
    };
    if tab_matches {
        return Some(SwitchTarget::Tab);
    }
    // 워크스페이스축 물리 키(토큰 기준)와 그 반대 축 키. Category = 이 키 + Shift,
    // Workspace = 이 키 단독. Shift 유무로 두 대상이 상호 배타(modifier-exclusive).
    let (ws_key, ws_other) = match kb.workspace_switch_modifier.to_lowercase().as_str() {
        "ctrl" => (ctrl, alt),
        // 기본 alt.
        _ => (alt, ctrl),
    };
    if ws_key && !ws_other {
        if shift {
            return Some(SwitchTarget::Category);
        }
        return Some(SwitchTarget::Workspace);
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
    let alt = mods.mac_cmd;
    #[cfg(not(target_os = "macos"))]
    let alt = mods.alt;
    switch_target_for(kb, mods.ctrl, mods.shift, alt) == Some(SwitchTarget::Workspace)
}

/// 현재 눌린 modifier 가 카테고리 전환(`workspace_switch_modifier` + Shift, 기본 Alt+Shift)과
/// 일치하는지 (사이드바 카테고리 키캡). [`workspace_switch_held`] 와 동일한 정규화
/// (`"alt"` 토큰 = macOS Command)를 거쳐 [`switch_target_for`] 에 넘긴다. folders 기능
/// 게이트는 호출측이 별도로 판단한다(이 함수는 modifier 만 본다).
pub fn category_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    #[cfg(target_os = "macos")]
    let alt = mods.mac_cmd;
    #[cfg(not(target_os = "macos"))]
    let alt = mods.alt;
    switch_target_for(kb, mods.ctrl, mods.shift, alt) == Some(SwitchTarget::Category)
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
pub fn keycap_size() -> f32 {
    KEYCAP_SIZE
}

/// 한 자리 숫자 키캡을 `center` 기준 16px slot 에 그린다.
///
/// `active`(현재 탭/워크스페이스) = `accent_primary()` fill + `text_on_accent()` 숫자,
/// 비active = `surface_raised` fill + `border_strong` edge + `text_secondary` 숫자 (본체
/// `kbd()` 키캡과 동일 레시피). 갤러리 `switch_overlay::num_cap` 와 1:1.
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
    let rect = egui::Rect::from_center_size(center, egui::vec2(KEYCAP_SIZE, KEYCAP_SIZE));
    let radius = theme.corner_radius_sm.value();
    let bw = theme.border_width.value();
    let (fill, edge, fg): (egui::Color32, egui::Color32, egui::Color32) = if active {
        (
            theme.accent_primary().into(),
            theme.accent_primary().into(),
            theme.text_on_accent().into(),
        )
    } else {
        (
            theme.surface_raised().into(),
            theme.border_strong().into(),
            theme.text_secondary().into(),
        )
    };
    // 등장 페이드 — 세 색에 동일 알파 적용(키캡 전체가 함께 떠오름).
    let (fill, edge, fg) = (
        fill.gamma_multiply(alpha),
        edge.gamma_multiply(alpha),
        fg.gamma_multiply(alpha),
    );
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, edge),
        egui::StrokeKind::Inside,
    );
    // 하단 2px edge — Kbd 키캡과 동일.
    painter.line_segment(
        [
            egui::pos2(rect.left() + radius, rect.bottom() - bw),
            egui::pos2(rect.right() - radius, rect.bottom() - bw),
        ],
        egui::Stroke::new(KEYCAP_BOTTOM_BORDER, edge),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        digit,
        egui::FontId::monospace(theme.font_size_micro.value()),
        fg,
    );
}

/// switch-number overlay 등장 페이드 계수(0..=1).
///
/// `visible`(modifier held + 이 오버레이 인스턴스가 표시 대상) 가 false→true 로 바뀌면
/// `motion-ui-fast`(90ms) 동안 0→1 로 올라오고, 놓으면 같은 시간으로 0 으로 내려간다.
/// egui 애니메이션 상태를 양방향으로 갱신하려면 **프레임마다**(키캡을 그리지 않는
/// 프레임 포함) 호출해야 한다. `id_salt` 로 오버레이 인스턴스(pane / sidebar)를 구분한다.
pub fn appear_fade(
    ctx: &egui::Context,
    theme: &Theme,
    id_salt: impl std::hash::Hash,
    visible: bool,
) -> f32 {
    ctx.animate_bool_with_time(
        egui::Id::new("switch_overlay_fade").with(id_salt),
        visible,
        theme.motion_ui_fast_ms() / 1000.0,
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
        // ⌘+⌥ → 표시됨 — 실제 전환도 super_key 기준이라 발동한다(표시=동작 일치).
        assert!(workspace_switch_held(
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
    fn switch_target_default_ctrl_is_tab() {
        let kb = kb_with("ctrl", "alt");
        // ctrl 단독 → Tab, alt 단독 → Workspace.
        assert_eq!(
            switch_target_for(&kb, true, false, false),
            Some(SwitchTarget::Tab)
        );
        assert_eq!(
            switch_target_for(&kb, false, false, true),
            Some(SwitchTarget::Workspace)
        );
    }

    #[test]
    fn switch_target_mixed_modifier_is_none() {
        let kb = kb_with("ctrl", "alt");
        // 다른 modifier 가 섞이면 단축키와 동일하게 미표시(None).
        assert_eq!(switch_target_for(&kb, true, true, false), None);
        assert_eq!(switch_target_for(&kb, true, false, true), None);
        assert_eq!(switch_target_for(&kb, false, false, false), None);
    }

    #[test]
    fn switch_target_rebind_swaps() {
        // tab=alt / ws=ctrl 로 재바인딩하면 대상도 반대로.
        let kb = kb_with("alt", "ctrl");
        assert_eq!(
            switch_target_for(&kb, false, false, true),
            Some(SwitchTarget::Tab)
        );
        assert_eq!(
            switch_target_for(&kb, true, false, false),
            Some(SwitchTarget::Workspace)
        );
    }

    #[test]
    fn switch_target_alt_shift_is_category() {
        let kb = kb_with("ctrl", "alt"); // tab=ctrl, ws=alt
        // alt 단독 → Workspace, alt+shift → Category (Shift 유무로 상호 배타).
        assert_eq!(
            switch_target_for(&kb, false, false, true),
            Some(SwitchTarget::Workspace)
        );
        assert_eq!(
            switch_target_for(&kb, false, true, true),
            Some(SwitchTarget::Category)
        );
        // ctrl+shift 는 워크스페이스축(alt)이 아니므로 카테고리 아님.
        assert_eq!(switch_target_for(&kb, true, true, false), None);
    }

    #[test]
    fn switch_target_category_follows_rebound_ws_modifier() {
        // ws=ctrl 로 재바인딩하면 카테고리는 ctrl+shift.
        let kb = kb_with("alt", "ctrl");
        assert_eq!(
            switch_target_for(&kb, true, true, false),
            Some(SwitchTarget::Category)
        );
        // alt+shift 는 이제 tab 축(alt)+shift → tab 아님, ws 축(ctrl) 아님 → None.
        assert_eq!(switch_target_for(&kb, false, true, true), None);
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
}
