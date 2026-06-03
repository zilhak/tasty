//! `keybindings_tests` 단위 테스트.

use super::*;
use std::collections::HashSet;

#[test]
fn preset_by_name_matches_preset_tasty() {
    let by_name = KeybindingSettings::preset_by_name("Tasty").unwrap();
    let direct = KeybindingSettings::preset_tasty();
    for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        assert_eq!(
            by_name.get_bindings(id),
            direct.get_bindings(id),
            "field {id} mismatch"
        );
    }
    assert!(KeybindingSettings::preset_by_name("Unknown").is_none());
}

#[test]
fn preset_by_name_matches_preset_mac() {
    let by_name = KeybindingSettings::preset_by_name("Mac").unwrap();
    let direct = KeybindingSettings::preset_mac();
    for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        assert_eq!(
            by_name.get_bindings(id),
            direct.get_bindings(id),
            "field {id} mismatch"
        );
    }
}

#[test]
fn preset_by_name_matches_preset_windows() {
    let by_name = KeybindingSettings::preset_by_name("Windows").unwrap();
    let direct = KeybindingSettings::preset_windows();
    for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        assert_eq!(
            by_name.get_bindings(id),
            direct.get_bindings(id),
            "field {id} mismatch"
        );
    }
}

#[test]
fn preset_by_name_matches_preset_linux() {
    let by_name = KeybindingSettings::preset_by_name("Linux").unwrap();
    let direct = KeybindingSettings::preset_linux();
    for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        assert_eq!(
            by_name.get_bindings(id),
            direct.get_bindings(id),
            "field {id} mismatch"
        );
    }
}

#[test]
fn preset_names_lists_all_four() {
    let names = KeybindingSettings::preset_names();
    assert_eq!(names, &["Tasty", "Mac", "Windows", "Linux"]);
}

/// 각 프리셋 내부에 바인딩 충돌이 없는지 검증.
#[test]
fn no_conflicts_within_presets() {
    for name in KeybindingSettings::preset_names() {
        let kb = KeybindingSettings::preset_by_name(name).unwrap();
        for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            if let Some(bindings) = kb.get_bindings(id) {
                for combo in bindings {
                    if combo.is_empty() {
                        continue;
                    }
                    let conflict = kb.find_conflict(id, combo);
                    assert_eq!(
                        conflict, None,
                        "preset '{name}': field '{id}' combo '{combo}' conflicts with {:?}",
                        conflict
                    );
                }
            }
        }
    }
}

#[test]
fn find_conflict_detects_duplicate_across_fields() {
    let kb = KeybindingSettings::preset_tasty();
    assert_eq!(
        kb.find_conflict("toggle_settings", "ctrl+shift+w"),
        Some(("close_pane", 0))
    );
}

#[test]
fn find_conflict_ignores_self() {
    let kb = KeybindingSettings::preset_tasty();
    assert_eq!(kb.find_conflict("close_pane", "ctrl+shift+w"), None);
}

#[test]
fn find_conflict_empty_combo_is_none() {
    let kb = KeybindingSettings::preset_tasty();
    assert_eq!(kb.find_conflict("close_pane", ""), None);
}

#[test]
fn set_and_get_field_roundtrip() {
    let mut kb = KeybindingSettings::preset_tasty();
    assert!(kb.set_field("new_tab", "alt+x"));
    assert_eq!(kb.get_field("new_tab"), Some("alt+x"));
    assert!(kb.clear_field("new_tab"));
    assert_eq!(kb.get_field("new_tab"), Some(""));
}

#[test]
fn add_remove_binding_roundtrip() {
    let mut kb = KeybindingSettings::preset_tasty();
    assert!(kb.clear_field("copy"));
    assert!(kb.add_binding("copy", "ctrl+c".into()));
    assert!(kb.add_binding("copy", "ctrl+shift+c".into()));
    // 중복 추가는 실패.
    assert!(!kb.add_binding("copy", "ctrl+c".into()));
    assert_eq!(
        kb.get_bindings("copy"),
        Some(&["ctrl+c".to_string(), "ctrl+shift+c".to_string()][..])
    );
    assert!(kb.remove_binding("copy", 0));
    assert_eq!(
        kb.get_bindings("copy"),
        Some(&["ctrl+shift+c".to_string()][..])
    );
    // 범위 밖 idx는 실패.
    assert!(!kb.remove_binding("copy", 5));
}

#[test]
fn add_binding_rejects_empty() {
    let mut kb = KeybindingSettings::preset_tasty();
    assert!(!kb.add_binding("new_tab", String::new()));
}

#[test]
fn set_field_unknown_returns_false() {
    let mut kb = KeybindingSettings::preset_tasty();
    assert!(!kb.set_field("nonexistent", "ctrl+x"));
    assert_eq!(kb.get_field("nonexistent"), None);
}

#[test]
fn general_binding_fields_count() {
    assert_eq!(KeybindingSettings::GENERAL_BINDING_FIELDS.len(), 47);
}

#[test]
fn all_general_fields_have_getters_and_setters() {
    let mut kb = KeybindingSettings::preset_tasty();
    for (id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        assert!(
            kb.get_bindings(id).is_some(),
            "get_bindings missing for {id}"
        );
        assert!(kb.set_field(id, "x"), "set_field missing for {id}");
        assert_eq!(kb.get_field(id), Some("x"));
    }
}

#[test]
fn label_key_for_returns_correct_key() {
    assert_eq!(
        KeybindingSettings::label_key_for("close_pane"),
        Some("settings.keybindings.close_pane_label")
    );
    assert_eq!(
        KeybindingSettings::label_key_for("copy"),
        Some("settings.keybindings.copy_label")
    );
    assert_eq!(KeybindingSettings::label_key_for("nonexistent"), None);
}

/// 0.4 fresh-start: 단일 string 형식은 reject (Vec 필수).
#[test]
fn single_string_keybinding_rejected() {
    let toml_str = r#"new_tab = "alt+x""#;
    let result: Result<KeybindingSettings, _> = toml::from_str(toml_str);
    assert!(
        result.is_err(),
        "single string should be rejected, got: {:?}",
        result
    );
}

// ── format_display: `+` 구분자/키 충돌 해소 ───────────────────────

#[test]
fn format_display_basic() {
    assert_eq!(
        KeybindingSettings::format_display("ctrl+shift+n"),
        "Ctrl+Shift+N"
    );
}

#[test]
fn format_display_ctrl_plus_key() {
    // "ctrl++"는 Ctrl + `+` 키. 표시상 "Ctrl++"여야 한다.
    assert_eq!(KeybindingSettings::format_display("ctrl++"), "Ctrl++");
}

#[test]
fn format_display_symbol_aliases() {
    assert_eq!(KeybindingSettings::format_display("ctrl+plus"), "Ctrl++");
    assert_eq!(KeybindingSettings::format_display("ctrl+minus"), "Ctrl+-");
    assert_eq!(KeybindingSettings::format_display("ctrl+equals"), "Ctrl+=");
}

#[test]
fn format_display_empty_and_minus() {
    assert_eq!(KeybindingSettings::format_display(""), "");
    assert_eq!(KeybindingSettings::format_display("ctrl+-"), "Ctrl+-");
    assert_eq!(KeybindingSettings::format_display("ctrl+="), "Ctrl+=");
}

/// TOML에 일부 필드만 있고 나머지가 누락된 경우,
/// 누락된 필드가 preset_tasty() 기본값을 따르는지 확인.
#[test]
fn missing_fields_fall_back_to_preset_not_empty() {
    let toml = r#"new_workspace = ["alt+n"]"#;
    let kb: KeybindingSettings = toml::from_str(toml).unwrap();
    let preset = KeybindingSettings::preset_tasty();

    for (field_id, _label) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        let deserialized = kb.get_bindings(field_id).unwrap();
        let expected = preset.get_bindings(field_id).unwrap();
        assert_eq!(
            deserialized, expected,
            "필드 '{field_id}'가 누락 시 preset 기본값이 아닌 빈 값이 됨"
        );
    }
}

/// 사용자가 설정한 바인딩과 충돌하는 기본값 바인딩이 제거되는지 확인.
#[test]
fn remove_conflicts_from_defaults_strips_conflicting_combos() {
    // image_undo의 기본값은 ["ctrl+z", "alt+z"].
    // 사용자가 new_tab = ["ctrl+z"]를 설정하면,
    // image_undo에서 "ctrl+z"가 제거되어야 한다.
    let toml = r#"new_tab = ["ctrl+z"]"#;
    let mut kb: KeybindingSettings = toml::from_str(toml).unwrap();
    let existing_keys: HashSet<String> = ["new_tab".to_string()].into_iter().collect();
    kb.remove_conflicts_from_defaults(&existing_keys);

    // new_tab은 사용자 설정이므로 그대로
    assert_eq!(kb.new_tab, vec!["ctrl+z".to_string()]);
    // image_undo에서 "ctrl+z"가 제거되고 "alt+z"만 남아야 함
    assert!(
        !kb.image_undo.contains(&"ctrl+z".to_string()),
        "기본값 필드에서 사용자 바인딩과 충돌하는 combo가 제거되지 않음"
    );
    assert!(
        kb.image_undo.contains(&"alt+z".to_string()),
        "충돌하지 않는 combo까지 제거됨"
    );
}
