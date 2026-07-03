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
    assert_eq!(KeybindingSettings::GENERAL_BINDING_FIELDS.len(), 52);
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

#[test]
fn format_display_parts_tokenizes() {
    assert_eq!(
        KeybindingSettings::format_display_parts("alt+n"),
        vec!["Alt", "N"]
    );
    assert_eq!(
        KeybindingSettings::format_display_parts("ctrl+shift+v"),
        vec!["Ctrl", "Shift", "V"]
    );
}

#[test]
fn format_display_parts_plus_key_not_split() {
    // "ctrl++"는 Ctrl + `+`키 — 키캡 2개 [Ctrl][+] 로 분해되어야 한다.
    assert_eq!(
        KeybindingSettings::format_display_parts("ctrl++"),
        vec!["Ctrl", "+"]
    );
    assert_eq!(
        KeybindingSettings::format_display_parts("ctrl+plus"),
        vec!["Ctrl", "+"]
    );
}

#[test]
fn format_display_parts_empty() {
    assert!(KeybindingSettings::format_display_parts("").is_empty());
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

#[test]
fn script_binding_set_get_remove() {
    let mut kb = KeybindingSettings::default();
    assert!(kb.script_binding_combo("script-0").is_none());
    kb.set_script_binding("script-0", "ctrl+alt+r".to_string());
    assert_eq!(kb.script_binding_combo("script-0"), Some("ctrl+alt+r"));
    // 같은 script_id 재설정은 교체(중복 누적 안 함).
    kb.set_script_binding("script-0", "ctrl+alt+t".to_string());
    assert_eq!(kb.script_binding_combo("script-0"), Some("ctrl+alt+t"));
    assert_eq!(kb.script_bindings.len(), 1);
    // 빈 combo 는 제거.
    kb.set_script_binding("script-0", String::new());
    assert!(kb.script_binding_combo("script-0").is_none());
    // remove.
    kb.set_script_binding("script-1", "ctrl+alt+y".to_string());
    assert!(kb.remove_script_binding("script-1"));
    assert!(!kb.remove_script_binding("script-1"));
}

#[test]
fn combo_conflict_detects_fixed_and_script() {
    let mut kb = KeybindingSettings::default();
    // 고정 액션(new_tab)의 combo 를 잡아 충돌 확인.
    let taken = kb
        .new_tab
        .first()
        .cloned()
        .unwrap_or_else(|| "ctrl+t".to_string());
    assert!(kb.combo_conflict(&taken, None).is_some());
    // 스크립트끼리 충돌.
    kb.set_script_binding("script-0", "ctrl+alt+9".to_string());
    assert_eq!(
        kb.combo_conflict("ctrl+alt+9", None).as_deref(),
        Some("script:script-0")
    );
    // 자기 자신 재바인딩은 충돌 아님(except).
    assert!(kb.combo_conflict("ctrl+alt+9", Some("script-0")).is_none());
    // 미사용 combo 는 충돌 없음.
    assert!(kb.combo_conflict("ctrl+alt+0", None).is_none());
}

#[test]
fn script_bindings_serde_default_when_absent() {
    // script_bindings 키 없는 기존 config 조각 → 빈 목록.
    let kb: KeybindingSettings = toml::from_str("new_tab = [\"ctrl+t\"]").unwrap();
    assert!(kb.script_bindings.is_empty());
}

// ── quick-switch raw 키 필드 (quickswitch-02) ─────────────────────

#[test]
fn preset_tasty_has_vim_style_quick_switch_defaults() {
    let kb = KeybindingSettings::preset_tasty();
    assert_eq!(
        kb.tab_switch_slot_keys,
        ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
    );
    assert_eq!(
        kb.workspace_switch_slot_keys,
        ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
    );
    assert_eq!(kb.tab_switch_next_key, "l");
    assert_eq!(kb.tab_switch_prev_key, "h");
    assert_eq!(kb.workspace_switch_next_key, "j");
    assert_eq!(kb.workspace_switch_prev_key, "k");
    // 무변경 확인: next_tab/prev_tab 는 quick-switch 와 별개로 빈 채 유지.
    assert!(kb.next_tab.is_empty());
    assert!(kb.prev_tab.is_empty());
}

#[test]
fn missing_new_fields_in_toml_falls_back_to_defaults() {
    // 신규 필드가 없는 구버전 TOML 문자열을 역직렬화 → 필드별 default fn 으로 복원.
    let toml_str = "tab_switch_modifier = \"ctrl\"\nworkspace_switch_modifier = \"alt\"\n";
    let kb: KeybindingSettings = toml::from_str(toml_str).unwrap();
    assert_eq!(
        kb.tab_switch_slot_keys,
        ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
    );
    assert_eq!(
        kb.workspace_switch_slot_keys,
        ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
    );
    assert_eq!(kb.tab_switch_next_key, "l");
    assert_eq!(kb.tab_switch_prev_key, "h");
    assert_eq!(kb.workspace_switch_next_key, "j");
    assert_eq!(kb.workspace_switch_prev_key, "k");
}

/// 모든 프리셋이 quick-switch 기본값을 동일하게 갖는지(공통 vim 키 적용) 확인.
#[test]
fn all_presets_share_quick_switch_defaults() {
    for name in KeybindingSettings::preset_names() {
        let kb = KeybindingSettings::preset_by_name(name).unwrap();
        assert_eq!(
            kb.tab_switch_slot_keys,
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
            "preset '{name}' tab slot keys"
        );
        assert_eq!(
            kb.workspace_switch_slot_keys,
            ["1", "2", "3", "4", "5", "6", "7", "8", "9"],
            "preset '{name}' workspace slot keys"
        );
        assert_eq!(kb.tab_switch_next_key, "l", "preset '{name}' tab next");
        assert_eq!(kb.tab_switch_prev_key, "h", "preset '{name}' tab prev");
        assert_eq!(
            kb.workspace_switch_next_key, "j",
            "preset '{name}' workspace next"
        );
        assert_eq!(
            kb.workspace_switch_prev_key, "k",
            "preset '{name}' workspace prev"
        );
    }
}

/// index 기반 slot/raw-key accessor round-trip.
#[test]
fn quick_switch_accessors_roundtrip() {
    let mut kb = KeybindingSettings::preset_tasty();

    // 슬롯 getter 기본값.
    assert_eq!(kb.tab_slot_key(0), Some("1"));
    assert_eq!(kb.tab_slot_key(9), Some("0"));
    assert_eq!(kb.tab_slot_key(10), None); // 범위 밖
    assert_eq!(kb.workspace_slot_key(8), Some("9"));
    assert_eq!(kb.workspace_slot_key(9), None); // 0번 슬롯 없음

    // 슬롯 setter.
    assert!(kb.set_tab_slot_key(4, "q"));
    assert_eq!(kb.tab_slot_key(4), Some("q"));
    assert!(!kb.set_tab_slot_key(10, "q")); // 범위 밖 → false
    assert!(kb.set_workspace_slot_key(0, "z"));
    assert_eq!(kb.workspace_slot_key(0), Some("z"));
    assert!(!kb.set_workspace_slot_key(9, "z"));

    // next/prev getter·setter.
    assert_eq!(kb.tab_next_key(), "l");
    assert_eq!(kb.tab_prev_key(), "h");
    assert_eq!(kb.workspace_next_key(), "j");
    assert_eq!(kb.workspace_prev_key(), "k");
    kb.set_tab_next_key("n");
    kb.set_tab_prev_key("p");
    kb.set_workspace_next_key("d");
    kb.set_workspace_prev_key("u");
    assert_eq!(kb.tab_next_key(), "n");
    assert_eq!(kb.tab_prev_key(), "p");
    assert_eq!(kb.workspace_next_key(), "d");
    assert_eq!(kb.workspace_prev_key(), "u");
}

/// 직렬화 → 역직렬화 후 quick-switch 필드가 보존되는지(round-trip).
#[test]
fn quick_switch_fields_serde_roundtrip() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.set_tab_slot_key(0, "q");
    kb.set_workspace_slot_key(0, "z");
    kb.set_tab_next_key("n");
    kb.set_workspace_prev_key("u");

    let serialized = toml::to_string(&kb).unwrap();
    let restored: KeybindingSettings = toml::from_str(&serialized).unwrap();

    assert_eq!(restored.tab_switch_slot_keys, kb.tab_switch_slot_keys);
    assert_eq!(
        restored.workspace_switch_slot_keys,
        kb.workspace_switch_slot_keys
    );
    assert_eq!(restored.tab_switch_next_key, "n");
    assert_eq!(restored.tab_switch_prev_key, kb.tab_switch_prev_key);
    assert_eq!(
        restored.workspace_switch_next_key,
        kb.workspace_switch_next_key
    );
    assert_eq!(restored.workspace_switch_prev_key, "u");
}

/// 신규 필드를 GENERAL_BINDING_FIELDS 에 넣지 않았음을 회귀 방지로 고정.
#[test]
fn quick_switch_fields_not_in_general_bindings() {
    for id in [
        "tab_switch_slot_keys",
        "workspace_switch_slot_keys",
        "tab_switch_next_key",
        "tab_switch_prev_key",
        "workspace_switch_next_key",
        "workspace_switch_prev_key",
    ] {
        assert!(
            KeybindingSettings::GENERAL_BINDING_FIELDS
                .iter()
                .all(|(fid, _)| *fid != id),
            "{id} 는 콤보가 아닌 raw 키이므로 GENERAL_BINDING_FIELDS 에 없어야 함"
        );
    }
    // count 는 여전히 52.
    assert_eq!(KeybindingSettings::GENERAL_BINDING_FIELDS.len(), 52);
}
