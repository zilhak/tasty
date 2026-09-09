//! 단축키 이식 번들 코덱 시험.

use super::*;
use tasty_settings::keybindings::ScriptBinding;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

/// 일반 콤보 · quick-switch 축 modifier · quick-switch 슬롯 · next/prev ·
/// `script_bindings` 를 전부 손댄 구성. round-trip 이 한 부류라도 빠뜨리면 깨진다.
fn heavily_edited() -> KeybindingSettings {
    let mut kb = KeybindingSettings::preset_mac();
    assert!(kb.add_binding("new_tab", "ctrl+alt+t".into()));
    assert!(kb.set_tab_slot_key(0, "q"));
    assert!(kb.set_workspace_slot_key(8, "z"));
    assert!(kb.set_category_slot_key(9, "p"));
    kb.set_tab_next_key("]");
    kb.set_category_prev_key("u");
    kb.category_switch_modifier = "option+shift".into();
    kb.tab_switch_modifier = KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.into();
    kb.script_bindings.push(ScriptBinding {
        script_id: "s1".into(),
        combo: "ctrl+f9".into(),
    });
    kb
}

fn overrides() -> PluginShortcutOverrides {
    let mut per_command = BTreeMap::new();
    per_command.insert(
        "explorer.refresh".to_string(),
        ShortcutOverride::Key {
            value: vec!["F6".to_string()],
        },
    );
    per_command.insert(
        "explorer.copy_paths".to_string(),
        ShortcutOverride::Inherit {
            source: "copy".to_string(),
        },
    );
    per_command.insert("explorer.mute".to_string(), ShortcutOverride::None);
    let mut out = PluginShortcutOverrides::new();
    out.insert("com.example.explorer".to_string(), per_command);
    out
}

#[test]
fn round_trips_the_whole_host_section() {
    let kb = heavily_edited();
    let ov = overrides();
    let text = encode(&kb, &ov).unwrap();

    let installed = ids(&["com.example.explorer"]);
    let scripts = ids(&["s1"]);
    let decoded = decode(
        &text,
        &DecodeEnv {
            installed_plugin_ids: &installed,
            known_script_ids: Some(&scripts),
        },
    )
    .unwrap();

    assert_eq!(decoded.keybindings, kb);
    assert_eq!(decoded.plugin_keybindings, ov);
    assert!(decoded.warnings.is_empty(), "{:#?}", decoded.warnings);
}

#[test]
fn drops_overrides_of_plugins_that_are_not_installed() {
    let mut ov = overrides();
    let mut ghost_commands = BTreeMap::new();
    ghost_commands.insert(
        "ghost.do".to_string(),
        ShortcutOverride::Key {
            value: vec!["ctrl+g".to_string()],
        },
    );
    ov.insert("ghost".to_string(), ghost_commands);

    let text = encode(&heavily_edited(), &ov).unwrap();
    let installed = ids(&["com.example.explorer"]);
    let scripts = ids(&["s1"]);
    let decoded = decode(
        &text,
        &DecodeEnv {
            installed_plugin_ids: &installed,
            known_script_ids: Some(&scripts),
        },
    )
    .unwrap();

    assert!(!decoded.plugin_keybindings.contains_key("ghost"));
    assert!(
        decoded
            .plugin_keybindings
            .contains_key("com.example.explorer")
    );
    assert_eq!(
        decoded.warnings,
        vec![BundleWarning::DroppedUninstalledPlugin {
            plugin_id: "ghost".to_string(),
            commands: 1,
        }]
    );
    assert!(decoded.warnings[0].to_string().contains("ghost"));
}

#[test]
fn drops_script_bindings_whose_script_is_gone() {
    let text = encode(&heavily_edited(), &PluginShortcutOverrides::new()).unwrap();
    let decoded = decode(
        &text,
        &DecodeEnv {
            installed_plugin_ids: &[],
            known_script_ids: Some(&[]),
        },
    )
    .unwrap();

    assert!(decoded.keybindings.script_bindings.is_empty());
    assert_eq!(
        decoded.warnings,
        vec![BundleWarning::DroppedUnknownScriptBinding {
            script_id: "s1".to_string(),
            combo: "ctrl+f9".to_string(),
        }]
    );
}

#[test]
fn keeps_script_bindings_when_the_registry_is_not_visible() {
    // `known_script_ids: None` — 레지스트리를 못 보는 호출자는 전량을 잃지 않는다.
    let text = encode(&heavily_edited(), &PluginShortcutOverrides::new()).unwrap();
    let decoded = decode(&text, &DecodeEnv::default()).unwrap();
    assert_eq!(decoded.keybindings.script_bindings.len(), 1);
    assert!(decoded.warnings.is_empty(), "{:#?}", decoded.warnings);
}

#[test]
fn rejects_a_toml_that_is_not_a_bundle() {
    let err = decode("[appearance]\ntheme = \"mocha\"\n", &DecodeEnv::default()).unwrap_err();
    assert!(matches!(
        err,
        BundleError::NotABundle {
            found: SchemaFound::Missing,
            ..
        }
    ));
    // 실제 config.toml 을 골랐을 때도 같은 갈래여야 한다(패닉 없음).
    let cfg = "[keybindings]\nnew_tab = [\"ctrl+t\"]\n\n[appearance]\ntheme = \"mocha\"\n";
    assert!(matches!(
        decode(cfg, &DecodeEnv::default()).unwrap_err(),
        BundleError::NotABundle { .. }
    ));
}

#[test]
fn rejects_a_bundle_with_a_different_schema() {
    let text = "schema = \"tasty.themes\"\nversion = 1\n";
    match decode(text, &DecodeEnv::default()).unwrap_err() {
        BundleError::NotABundle {
            found: SchemaFound::Other(s),
            ..
        } => assert_eq!(s, "tasty.themes"),
        other => panic!("예상 밖: {other}"),
    }
}

#[test]
fn broken_toml_is_an_error_not_a_panic() {
    assert!(matches!(
        decode("schema = \"tasty.keybindings\"\n[[[", &DecodeEnv::default()).unwrap_err(),
        BundleError::Toml(_)
    ));
}

#[test]
fn unknown_fields_are_ignored_and_reported() {
    let text = format!(
        "schema = \"{BUNDLE_SCHEMA}\"\n\
         version = {BUNDLE_VERSION}\n\
         future_section = 3\n\
         \n[keybindings]\n\
         new_tab = [\"ctrl+alt+n\"]\n\
         teleport_to_mars = [\"ctrl+m\"]\n"
    );
    let decoded = decode(&text, &DecodeEnv::default()).unwrap();

    assert_eq!(decoded.keybindings.new_tab, vec!["ctrl+alt+n".to_string()]);
    assert!(
        decoded
            .warnings
            .contains(&BundleWarning::UnknownTopLevelKey {
                key: "future_section".to_string(),
            })
    );
    assert!(
        decoded
            .warnings
            .contains(&BundleWarning::UnknownKeybindingField {
                field: "teleport_to_mars".to_string(),
            })
    );
    // 나머지 필드는 기본값으로 살아 있다.
    assert_eq!(
        decoded.keybindings.tab_switch_slot_keys,
        KeybindingSettings::default().tab_switch_slot_keys
    );
}

#[test]
fn a_fixed_array_of_the_wrong_length_falls_back_to_the_default() {
    // 구버전 번들(슬롯이 9개뿐)·신버전 번들(11개) 어느 쪽도 패닉하지 않는다.
    for slots in [
        "[\"1\",\"2\"]",
        "[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"0\",\"a\"]",
    ] {
        let text = format!(
            "schema = \"{BUNDLE_SCHEMA}\"\n\
             version = {BUNDLE_VERSION}\n\
             \n[keybindings]\n\
             tab_switch_slot_keys = {slots}\n\
             new_tab = [\"ctrl+alt+n\"]\n"
        );
        let decoded = decode(&text, &DecodeEnv::default()).unwrap();
        assert_eq!(
            decoded.keybindings.tab_switch_slot_keys,
            KeybindingSettings::default().tab_switch_slot_keys,
            "{slots}"
        );
        // 같은 절의 다른 필드는 그대로 복원된다 — 되돌리는 단위가 필드다.
        assert_eq!(decoded.keybindings.new_tab, vec!["ctrl+alt+n".to_string()]);
        assert!(
            decoded.warnings.iter().any(
                |w| matches!(w, BundleWarning::KeybindingFieldShapeMismatch { field, .. }
                    if field == "tab_switch_slot_keys")
            ),
            "{:#?}",
            decoded.warnings
        );
    }
}

#[test]
fn a_field_of_the_wrong_type_falls_back_to_the_default() {
    let text = format!(
        "schema = \"{BUNDLE_SCHEMA}\"\n\
         version = {BUNDLE_VERSION}\n\
         \n[keybindings]\n\
         new_tab = \"ctrl+t\"\n"
    );
    let decoded = decode(&text, &DecodeEnv::default()).unwrap();
    assert_eq!(
        decoded.keybindings.new_tab,
        KeybindingSettings::default().new_tab
    );
    assert!(
        decoded
            .warnings
            .iter()
            .any(|w| matches!(w, BundleWarning::KeybindingFieldShapeMismatch { .. }))
    );
}

#[test]
fn an_unreadable_override_entry_is_dropped_alone() {
    let text = format!(
        "schema = \"{BUNDLE_SCHEMA}\"\n\
         version = {BUNDLE_VERSION}\n\
         \n[keybindings]\n\
         \n[plugin_keybindings.\"com.example.explorer\".\"a\"]\n\
         mode = \"key\"\n\
         value = [\"F6\"]\n\
         \n[plugin_keybindings.\"com.example.explorer\".\"b\"]\n\
         mode = \"telepathy\"\n"
    );
    let installed = ids(&["com.example.explorer"]);
    let decoded = decode(
        &text,
        &DecodeEnv {
            installed_plugin_ids: &installed,
            known_script_ids: None,
        },
    )
    .unwrap();

    let kept = &decoded.plugin_keybindings["com.example.explorer"];
    assert!(kept.contains_key("a"));
    assert!(!kept.contains_key("b"));
    assert!(decoded.warnings.iter().any(
        |w| matches!(w, BundleWarning::UnreadablePluginOverride { command_id, .. }
                if command_id == "b")
    ));
}

#[test]
fn a_newer_version_is_read_best_effort_with_a_warning() {
    let text = format!(
        "schema = \"{BUNDLE_SCHEMA}\"\n\
         version = 99\n\
         \n[keybindings]\n\
         new_tab = [\"ctrl+alt+n\"]\n"
    );
    let decoded = decode(&text, &DecodeEnv::default()).unwrap();
    assert_eq!(decoded.keybindings.new_tab, vec!["ctrl+alt+n".to_string()]);
    assert!(decoded.warnings.contains(&BundleWarning::NewerVersion {
        found: 99,
        known: BUNDLE_VERSION,
    }));
}

/// 최상위 키 명부는 손으로 든 것이라, 구조체가 늘면 여기서 잡힌다.
#[test]
fn top_level_keys_match_the_struct() {
    let text = encode(
        &KeybindingSettings::default(),
        &PluginShortcutOverrides::new(),
    )
    .unwrap();
    let table: toml::Table = toml::from_str(&text).unwrap();
    let mut serialized: Vec<&str> = table.keys().map(String::as_str).collect();
    serialized.sort_unstable();
    let mut known: Vec<&str> = BUNDLE_KEYS.to_vec();
    known.sort_unstable();
    assert_eq!(serialized, known);
}

/// export 파일은 사용자가 열어 고칠 수 있어야 한다 — 헤더가 맨 앞이고 절 이름이
/// 사람이 읽는 형태인지 본다.
#[test]
fn the_encoded_file_is_human_readable() {
    let text = encode(&heavily_edited(), &overrides()).unwrap();
    assert!(text.starts_with(&format!("schema = \"{BUNDLE_SCHEMA}\"\n")));
    assert!(text.contains("[keybindings]"));
    assert!(text.contains("[plugin_keybindings.\"com.example.explorer\"."));
    assert!(text.contains("[[keybindings.script_bindings]]"));
}
