//! 수식키 더블탭 바인딩(`shift+shift`·`ctrl+ctrl`·`alt+alt`)을 받는 동작의 집합.
//! 메인 창의 실행 경로와 설정 화면의 녹화·경고가 이 집합 하나를 쓴다. 일반 바인딩 파서는 더블탭
//! 표기를 거절하므로(`parse::parse_binding`) 이 집합 밖의 칸에 저장된 더블탭은 실행되지 않는다.

use super::KeybindingSettings;

impl KeybindingSettings {
    /// 더블탭 표기. 토큰의 실제 키는 키 매핑 정책을 따른다(macOS `alt+alt`는 Command 두 번).
    pub const DOUBLE_TAP_SPELLINGS: &'static [&'static str] =
        &["shift+shift", "ctrl+ctrl", "alt+alt"];

    /// 메인 창에서 더블탭으로 실행하는 동작 field id. 설정 창 안에서는 `toggle_settings`만 받는다.
    pub const DOUBLE_TAP_FIELDS: &'static [&'static str] = &[
        "toggle_settings",
        "toggle_notifications",
        "new_workspace",
        "close_workspace",
        "new_tab",
        "close_pane",
        "split_pane_vertical",
        "split_pane_horizontal",
        "split_surface_vertical",
        "split_surface_horizontal",
        "focus_pane_next",
        "focus_pane_prev",
        "focus_surface_next",
        "focus_surface_prev",
        "close_surface",
        "open_markdown",
        "open_explorer",
        "convert_surface",
        "convert_to_markdown",
        "convert_to_explorer",
        "close_active",
        "next_tab",
        "prev_tab",
        "restore_closed",
        "quit",
        "quit_immediate",
        "quit_minimize",
    ];

    /// 저장 값이 더블탭 표기인가. 대소문자는 구분하지 않는다.
    pub fn is_double_tap_spelling(binding: &str) -> bool {
        let lower = binding.trim().to_ascii_lowercase();
        Self::DOUBLE_TAP_SPELLINGS.contains(&lower.as_str())
    }

    /// 이 field id 칸에 저장한 더블탭이 실행되는가.
    pub fn accepts_double_tap(field_id: &str) -> bool {
        Self::DOUBLE_TAP_FIELDS.contains(&field_id)
    }

    /// 실행되지 않는 칸에 저장된 더블탭이 있는 일반·범위 한정 field id와 그 값.
    pub fn unusable_double_taps(&self) -> Vec<(&'static str, String)> {
        Self::binding_fields()
            .filter(|(field, _)| !Self::accepts_double_tap(field))
            .flat_map(|(field, _)| {
                self.get_bindings(field)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|b| Self::is_double_tap_spelling(b))
                    .map(move |b| (*field, b.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_double_tap_field_is_a_general_binding_field() {
        for field in KeybindingSettings::DOUBLE_TAP_FIELDS {
            assert!(
                KeybindingSettings::GENERAL_BINDING_FIELDS
                    .iter()
                    .any(|(id, _)| id == field),
                "{field} is not a general binding field"
            );
        }
    }

    #[test]
    fn scoped_fields_do_not_take_double_taps() {
        for (field, _) in KeybindingSettings::TEXT_FIELD_BINDING_FIELDS
            .iter()
            .chain(KeybindingSettings::EXPLORER_LIST_BINDING_FIELDS)
        {
            assert!(!KeybindingSettings::accepts_double_tap(field), "{field}");
        }
    }

    #[test]
    fn double_tap_spellings_are_recognized_case_insensitively() {
        assert!(KeybindingSettings::is_double_tap_spelling("Shift+Shift"));
        assert!(KeybindingSettings::is_double_tap_spelling("alt+alt"));
        assert!(!KeybindingSettings::is_double_tap_spelling("shift+s"));
        assert!(!KeybindingSettings::is_double_tap_spelling("option+option"));
    }

    #[test]
    fn unusable_double_taps_lists_only_fields_that_do_not_run_them() {
        let mut kb = KeybindingSettings {
            toggle_command_palette: vec!["shift+shift".into(), "ctrl+shift+p".into()],
            new_tab: vec!["ctrl+ctrl".into()],
            ..KeybindingSettings::default()
        };
        assert_eq!(
            kb.unusable_double_taps(),
            vec![("toggle_command_palette", "shift+shift".to_string())]
        );
        kb.toggle_command_palette = vec!["ctrl+shift+p".into()];
        assert!(kb.unusable_double_taps().is_empty());
    }

    #[test]
    fn default_presets_hold_no_unusable_double_tap() {
        assert!(
            KeybindingSettings::default()
                .unusable_double_taps()
                .is_empty()
        );
    }
}
