//! Switch-number overlay — 공통 배선 (P2a 탭 + P2b 사이드바 공유).
//!
//! 사용자가 `tab_switch_modifier`(기본 Ctrl) / `workspace_switch_modifier`(기본 Alt)를
//! **누르고 있는 동안** 탭/워크스페이스의 leading indicator 를 숫자 키캡 미리보기로
//! in-place 교체하기 위한 보조. 두 가지를 한 곳에 모은다:
//!
//! 1. **modifier 일치 판정** ([`tab_switch_held`] / [`workspace_switch_held`]) — 현재 눌린
//!    egui modifier 가 해당 단축키 modifier 와 *단독 일치* 하는지. 단축키 소비처
//!    `input/shortcuts/numeric.rs` 의 `tab_mod_matches`/`ws_mod_matches` 와 동일 규칙.
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

/// 탭 단축키 숫자: 0..=8 → "1".."9", 9 → "0"(10번째 탭, Ctrl+0), 10번째 밖 → None.
const TAB_DIGITS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

/// 현재 눌린 modifier 가 `tab_switch_modifier` 와 단독 일치하는지 (numeric.rs 와 동일 규칙).
pub fn tab_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    match kb.tab_switch_modifier.to_lowercase().as_str() {
        "alt" => mods.alt && !mods.ctrl && !mods.shift,
        // 기본 ctrl.
        _ => mods.ctrl && !mods.shift && !mods.alt,
    }
}

/// 현재 눌린 modifier 가 `workspace_switch_modifier` 와 단독 일치하는지 (사이드바 오버레이).
pub fn workspace_switch_held(mods: egui::Modifiers, kb: &KeybindingSettings) -> bool {
    match kb.workspace_switch_modifier.to_lowercase().as_str() {
        "ctrl" => mods.ctrl && !mods.shift && !mods.alt,
        // 기본 alt.
        _ => mods.alt && !mods.ctrl && !mods.shift,
    }
}

/// 탭 index → 표시할 숫자 키캡 문자. 단축키가 없는 11번째 탭(index ≥ 10)부터 None.
pub fn tab_digit(index: usize) -> Option<&'static str> {
    TAB_DIGITS.get(index).copied()
}

/// 워크스페이스 index → 숫자 키캡 문자. 1–9 만(0 없음) → index ≥ 9 부터 None (사이드바 오버레이).
pub fn workspace_digit(index: usize) -> Option<&'static str> {
    if index < 9 {
        TAB_DIGITS.get(index).copied()
    } else {
        None
    }
}

/// 한 자리 숫자 키캡을 `center` 기준 16px slot 에 그린다.
///
/// `active`(현재 탭/워크스페이스) = `accent_primary()` fill + `text_on_accent()` 숫자,
/// 비active = `surface_raised` fill + `border_strong` edge + `text_secondary` 숫자 (본체
/// `kbd()` 키캡과 동일 레시피). 갤러리 `switch_overlay::num_cap` 와 1:1.
pub fn paint_keycap(
    painter: &egui::Painter,
    theme: &Theme,
    center: egui::Pos2,
    digit: &str,
    active: bool,
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

    #[test]
    fn tab_held_matches_default_ctrl_alone() {
        let kb = kb_with("ctrl", "alt");
        assert!(tab_switch_held(mods(true, false, false), &kb));
        // 다른 modifier 가 섞이면 불일치 (단축키와 동일).
        assert!(!tab_switch_held(mods(true, false, true), &kb));
        assert!(!tab_switch_held(mods(false, true, false), &kb));
    }

    #[test]
    fn workspace_held_matches_default_alt_alone() {
        let kb = kb_with("ctrl", "alt");
        assert!(workspace_switch_held(mods(false, true, false), &kb));
        assert!(!workspace_switch_held(mods(true, true, false), &kb));
    }

    #[test]
    fn rebound_modifiers_follow_settings() {
        // tab=alt / ws=ctrl 로 재바인딩하면 판정도 따라간다.
        let kb = kb_with("alt", "ctrl");
        assert!(tab_switch_held(mods(false, true, false), &kb));
        assert!(workspace_switch_held(mods(true, false, false), &kb));
    }

    #[test]
    fn tab_digit_range() {
        assert_eq!(tab_digit(0), Some("1"));
        assert_eq!(tab_digit(8), Some("9"));
        assert_eq!(tab_digit(9), Some("0")); // 10번째 = Ctrl+0
        assert_eq!(tab_digit(10), None); // 11번째부터 키캡 없음
    }

    #[test]
    fn workspace_digit_range() {
        assert_eq!(workspace_digit(0), Some("1"));
        assert_eq!(workspace_digit(8), Some("9"));
        assert_eq!(workspace_digit(9), None); // 0 없음 → 10번째부터 없음
    }
}
