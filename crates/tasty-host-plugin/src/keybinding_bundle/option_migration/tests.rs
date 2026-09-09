//! `option` 이식 판정 시험.

use super::*;
use tasty_settings::ScriptBinding;

fn plugin_key(plugin: &str, command: &str, combo: &str) -> PluginShortcutOverrides {
    let mut commands = BTreeMap::new();
    commands.insert(
        command.to_string(),
        ShortcutOverride::Key {
            value: vec![combo.to_string()],
        },
    );
    let mut out = PluginShortcutOverrides::new();
    out.insert(plugin.to_string(), commands);
    out
}

/// 다섯 자리에 각각 `option` 을 심은 구성.
fn five_sites() -> (KeybindingSettings, PluginShortcutOverrides) {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.add_binding("new_tab", "alt+option+t".into()); // (1) 일반 필드
    kb.category_switch_modifier = "option+shift".into(); // (2) 축 modifier
    kb.tab_switch_modifier = KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.into();
    kb.set_tab_slot_key(0, "ctrl+option+1"); // (3) 개별 지정 슬롯
    kb.script_bindings.push(ScriptBinding {
        script_id: "s1".into(),
        combo: "option+f5".into(),
    }); // (4) 스크립트
    let overrides = plugin_key("com.example.x", "x.go", "option+g"); // (5) plugin override
    (kb, overrides)
}

#[test]
fn finds_option_in_all_five_places() {
    let (kb, overrides) = five_sites();
    let found = scan_option_bindings(&kb, &overrides, TargetOs::NonMac);
    assert_eq!(found.len(), 5, "빠진 자리: {found:#?}");

    let sites: Vec<String> = found.iter().map(|f| f.site.to_string()).collect();
    assert!(sites.iter().any(|s| s.starts_with("new_tab[")), "{sites:?}");
    assert!(
        sites.contains(&"category.modifier".to_string()),
        "{sites:?}"
    );
    assert!(sites.contains(&"tab.slot[0]".to_string()), "{sites:?}");
    assert!(sites.contains(&"script:s1".to_string()), "{sites:?}");
    assert!(
        sites.contains(&"plugin:com.example.x/x.go[0]".to_string()),
        "{sites:?}"
    );
}

#[test]
fn a_rule_based_axis_holds_raw_keys_which_are_not_combos() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.tab_switch_modifier = "ctrl".into(); // 규칙 기반
    kb.set_tab_slot_key(0, "o"); // raw 키 "o" — 콤보가 아니다
    kb.set_tab_next_key("o");
    assert!(
        scan_option_bindings(&kb, &PluginShortcutOverrides::new(), TargetOs::NonMac).is_empty()
    );
}

/// 개별 지정 축의 다음/이전도 콤보를 담는다 — 슬롯만 보면 이 자리가 샌다.
#[test]
fn an_individual_axis_step_is_scanned_too() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.workspace_switch_modifier = KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.into();
    kb.set_workspace_next_key("option+j");
    let found = scan_option_bindings(&kb, &PluginShortcutOverrides::new(), TargetOs::NonMac);
    assert_eq!(
        found.iter().map(|f| f.site.to_string()).collect::<Vec<_>>(),
        vec!["workspace.next".to_string()]
    );
}

/// 규칙 기반 축의 슬롯은 **콤보로 해석하지 않는다**. 손으로 config 를 고쳐 콤보처럼
/// 생긴 값을 넣어도 그 자리는 raw 키 자리라, 대체 값을 콤보로 받으면 규칙 기반 축의
/// 의미가 깨진다(그 축의 modifier 는 따로 있다). 개별 지정 축과 섞어 판정하면 안 된다.
#[test]
fn a_rule_based_slot_is_never_read_as_a_combo() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.tab_switch_modifier = "ctrl".into(); // 규칙 기반
    kb.set_tab_slot_key(0, "option+1");
    kb.set_tab_next_key("option+l");
    assert!(
        scan_option_bindings(&kb, &PluginShortcutOverrides::new(), TargetOs::NonMac).is_empty()
    );
}

#[test]
fn macos_needs_no_migration() {
    let (kb, overrides) = five_sites();
    assert!(scan_option_bindings(&kb, &overrides, TargetOs::Mac).is_empty());
}

/// `Inherit`/`None` 은 콤보를 담지 않으므로 대상이 아니다.
#[test]
fn non_key_overrides_are_not_scanned() {
    let mut commands = BTreeMap::new();
    commands.insert(
        "a".to_string(),
        ShortcutOverride::Inherit {
            source: "copy".to_string(),
        },
    );
    commands.insert("b".to_string(), ShortcutOverride::None);
    let mut overrides = PluginShortcutOverrides::new();
    overrides.insert("com.example.x".to_string(), commands);
    assert!(
        scan_option_bindings(
            &KeybindingSettings::preset_tasty(),
            &overrides,
            TargetOs::NonMac
        )
        .is_empty()
    );
}

/// 대체 값은 자리마다 종류가 다르다 — 축 modifier 는 modifier 조합, 나머지는 콤보.
#[test]
fn replacement_kind_follows_the_site() {
    let (kb, overrides) = five_sites();
    for f in scan_option_bindings(&kb, &overrides, TargetOs::NonMac) {
        let expected = if matches!(f.site, BindingSite::AxisModifier { .. }) {
            ReplacementKind::ModifierCombo
        } else {
            ReplacementKind::Combo
        };
        assert_eq!(f.site.replacement_kind(), expected, "{}", f.site);
    }
}

/// 자리마다 종류에 맞는 대체 값을 고른 계획.
fn full_plan(kb: &KeybindingSettings, overrides: &PluginShortcutOverrides) -> MigrationPlan {
    let combos = [
        "ctrl+shift+f1",
        "ctrl+shift+f2",
        "ctrl+shift+f3",
        "ctrl+shift+f4",
        "ctrl+shift+f5",
    ];
    let mut plan = MigrationPlan::new();
    let mut next = combos.iter();
    for f in scan_option_bindings(kb, overrides, TargetOs::NonMac) {
        let value = match f.site.replacement_kind() {
            ReplacementKind::ModifierCombo => "ctrl+alt".to_string(),
            ReplacementKind::Combo => {
                (*next.next().expect("콤보 자리가 준비한 값보다 많다")).to_string()
            }
        };
        plan.insert(f.site, value);
    }
    plan
}

#[test]
fn nothing_is_left_after_applying() {
    let (kb, overrides) = five_sites();
    let plan = full_plan(&kb, &overrides);
    let (kb2, ov2) = apply_migration(&kb, &overrides, &plan).unwrap();
    assert!(scan_option_bindings(&kb2, &ov2, TargetOs::NonMac).is_empty());

    // 실제로 값이 들어갔는지 — 빈 결과가 "아무것도 안 했다" 로도 나오기 때문이다.
    assert!(
        kb2.get_bindings("new_tab")
            .unwrap()
            .contains(&"ctrl+shift+f1".to_string())
    );
    assert_eq!(kb2.category_switch_modifier, "ctrl+alt");
    assert!(kb2.tab_slot_key(0).unwrap().starts_with("ctrl+shift+f"));
    assert!(
        kb2.script_binding_combo("s1")
            .unwrap()
            .starts_with("ctrl+shift+f")
    );
    let ShortcutOverride::Key { value } = &ov2["com.example.x"]["x.go"] else {
        panic!("Key 가 아니다");
    };
    assert!(value[0].starts_with("ctrl+shift+f"));
    // 원본은 안 건드린다.
    assert_eq!(kb.category_switch_modifier, "option+shift");
}

#[test]
fn an_incomplete_plan_is_rejected() {
    let (kb, overrides) = five_sites();
    let mut plan = full_plan(&kb, &overrides);
    let dropped = plan.keys().next().cloned().unwrap();
    plan.remove(&dropped);
    match apply_migration(&kb, &overrides, &plan).unwrap_err() {
        MigrationError::Unassigned { sites } => assert_eq!(sites, vec![dropped]),
        other => panic!("예상 밖: {other}"),
    }
}

#[test]
fn a_plan_pointing_at_a_site_that_is_not_there_is_rejected() {
    let (kb, overrides) = five_sites();
    let mut plan = full_plan(&kb, &overrides);
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "ghost".into(),
        },
        "ctrl+f12".into(),
    );
    assert!(matches!(
        apply_migration(&kb, &overrides, &plan).unwrap_err(),
        MigrationError::UnknownSite { .. }
    ));
}

#[test]
fn a_replacement_that_keeps_option_is_rejected() {
    let (kb, overrides) = five_sites();
    for (site, bad) in [
        (
            BindingSite::ScriptBinding {
                script_id: "s1".into(),
            },
            "ctrl+option+f5",
        ),
        (
            BindingSite::AxisModifier {
                axis: SwitchAxis::Category,
            },
            "ctrl+option",
        ),
    ] {
        let mut plan = full_plan(&kb, &overrides);
        plan.insert(site.clone(), bad.to_string());
        match apply_migration(&kb, &overrides, &plan).unwrap_err() {
            MigrationError::ReplacementKeepsOption { site: s, .. } => assert_eq!(s, site),
            other => panic!("{site} 에서 예상 밖: {other}"),
        }
    }
}

#[test]
fn a_value_of_the_wrong_kind_is_rejected() {
    let (kb, overrides) = five_sites();

    // 콤보 자리에 modifier 단독 — 파싱되지 않는다.
    let mut plan = full_plan(&kb, &overrides);
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "s1".into(),
        },
        "ctrl".into(),
    );
    assert!(matches!(
        apply_migration(&kb, &overrides, &plan).unwrap_err(),
        MigrationError::NotACombo { .. }
    ));

    // modifier 자리에 완전 콤보 — modifier 조합이 아니다.
    let mut plan = full_plan(&kb, &overrides);
    plan.insert(
        BindingSite::AxisModifier {
            axis: SwitchAxis::Category,
        },
        "ctrl+t".into(),
    );
    assert!(matches!(
        apply_migration(&kb, &overrides, &plan).unwrap_err(),
        MigrationError::NotAModifierCombo { .. }
    ));

    // 개별 지정 sentinel 도 modifier 조합이 아니다 — 마이그레이션이 축의 모드를 바꾸지 않는다.
    let mut plan = full_plan(&kb, &overrides);
    plan.insert(
        BindingSite::AxisModifier {
            axis: SwitchAxis::Category,
        },
        KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.into(),
    );
    assert!(matches!(
        apply_migration(&kb, &overrides, &plan).unwrap_err(),
        MigrationError::NotAModifierCombo { .. }
    ));
}

#[test]
fn replacements_that_collide_with_each_other_are_rejected() {
    let (kb, overrides) = five_sites();
    let mut plan = full_plan(&kb, &overrides);
    let combo_sites: Vec<BindingSite> = plan
        .keys()
        .filter(|s| s.replacement_kind() == ReplacementKind::Combo)
        .cloned()
        .collect();
    // 호스트 네임스페이스 안의 두 자리에 같은 조합을 준다(plugin 은 별도 네임스페이스라
    // 여기서 골라 쓰면 충돌로 안 잡힌다 — 그것이 규정된 우선순위다).
    let host_sites: Vec<&BindingSite> = combo_sites
        .iter()
        .filter(|s| !matches!(s, BindingSite::PluginOverride { .. }))
        .collect();
    assert!(host_sites.len() >= 2);
    for s in host_sites.iter().take(2) {
        plan.insert((*s).clone(), "ctrl+shift+f11".into());
    }
    match apply_migration(&kb, &overrides, &plan).unwrap_err() {
        MigrationError::Conflicts(c) => {
            assert_eq!(c.len(), 1, "{c:#?}");
            assert!(c[0].to_string().contains("ctrl+shift+f11"));
        }
        other => panic!("예상 밖: {other}"),
    }
}

#[test]
fn a_replacement_that_collides_with_an_existing_binding_is_rejected() {
    let (kb, overrides) = five_sites();
    let existing = kb.get_bindings("new_workspace").unwrap()[0].clone();
    let mut plan = full_plan(&kb, &overrides);
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "s1".into(),
        },
        existing.clone(),
    );
    match apply_migration(&kb, &overrides, &plan).unwrap_err() {
        MigrationError::Conflicts(c) => {
            assert!(
                c.iter().any(|x| bindings_equivalent(&x.combo, &existing)),
                "{c:#?}"
            );
        }
        other => panic!("예상 밖: {other}"),
    }
}

/// 같은 plugin 안의 두 커맨드가 같은 키를 갖는 것은 충돌이다 — 어느 것이 발화할지
/// 가릴 근거가 없다. 반면 호스트↔plugin 은 우선순위가 규정돼 있어 충돌이 아니다.
#[test]
fn the_plugin_namespace_is_separate_from_the_host_one() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.script_bindings.push(ScriptBinding {
        script_id: "s1".into(),
        combo: "option+f5".into(),
    });
    let host_combo = kb.get_bindings("new_workspace").unwrap()[0].clone();

    // (a) plugin override 를 호스트 바인딩과 같은 값으로 바꿔도 통과한다.
    let overrides = plugin_key("com.example.x", "x.go", "option+g");
    let mut plan = MigrationPlan::new();
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "s1".into(),
        },
        "ctrl+shift+f9".into(),
    );
    plan.insert(
        BindingSite::PluginOverride {
            plugin_id: "com.example.x".into(),
            command_id: "x.go".into(),
            index: 0,
        },
        host_combo.clone(),
    );
    assert!(apply_migration(&kb, &overrides, &plan).is_ok());

    // (b) 같은 plugin 의 다른 커맨드와 겹치면 거절한다.
    let mut overrides = plugin_key("com.example.x", "x.go", "option+g");
    overrides.get_mut("com.example.x").unwrap().insert(
        "x.stop".to_string(),
        ShortcutOverride::Key {
            value: vec!["ctrl+shift+f9".to_string()],
        },
    );
    let mut plan = MigrationPlan::new();
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "s1".into(),
        },
        "ctrl+shift+f8".into(),
    );
    plan.insert(
        BindingSite::PluginOverride {
            plugin_id: "com.example.x".into(),
            command_id: "x.go".into(),
            index: 0,
        },
        "ctrl+shift+f9".into(),
    );
    assert!(matches!(
        apply_migration(&kb, &overrides, &plan).unwrap_err(),
        MigrationError::Conflicts(_)
    ));
}

/// 축 modifier 를 바꾸면 그 축의 **슬롯 전부 + next/prev 합성 콤보**가 한꺼번에
/// 움직인다 — 충돌 검사가 값 하나만 봤다면 이 케이스가 샌다.
#[test]
fn changing_an_axis_modifier_is_checked_against_every_composed_combo() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.category_switch_modifier = "option+shift".into();
    // 카테고리 축이 `ctrl+alt` 로 옮겨가면 3번 슬롯의 합성 콤보 `ctrl+alt+3` 이
    // 이 일반 액션과 겹친다.
    kb.set_field("open_tutorial", "ctrl+alt+3");
    assert_eq!(kb.category_slot_key(2), Some("3"));

    let overrides = PluginShortcutOverrides::new();
    let mut plan = MigrationPlan::new();
    plan.insert(
        BindingSite::AxisModifier {
            axis: SwitchAxis::Category,
        },
        "ctrl+alt".into(),
    );
    match apply_migration(&kb, &overrides, &plan).unwrap_err() {
        MigrationError::Conflicts(c) => {
            assert!(
                c.iter().any(|x| x.combo.eq_ignore_ascii_case("ctrl+alt+3")),
                "{c:#?}"
            );
        }
        other => panic!("예상 밖: {other}"),
    }
}

/// 원래부터 있던 중복은 이번 마이그레이션의 책임이 아니다 — 그것까지 거절하면
/// 사용자가 무관한 이유로 막힌다.
#[test]
fn a_conflict_that_was_already_there_does_not_block() {
    let mut kb = KeybindingSettings::preset_tasty();
    kb.script_bindings.push(ScriptBinding {
        script_id: "s1".into(),
        combo: "option+f5".into(),
    });
    // 기존 중복을 심는다 — 두 일반 필드가 같은 콤보.
    let dup = "ctrl+shift+f7";
    kb.set_field("open_tutorial", dup);
    kb.set_field("open_file_picker", dup);

    let overrides = PluginShortcutOverrides::new();
    let mut plan = MigrationPlan::new();
    plan.insert(
        BindingSite::ScriptBinding {
            script_id: "s1".into(),
        },
        "ctrl+shift+f9".into(),
    );
    let (kb2, _) = apply_migration(&kb, &overrides, &plan).unwrap();
    assert_eq!(kb2.get_field("open_tutorial"), Some(dup));
}

#[test]
fn the_host_target_matches_the_build() {
    let expected = if cfg!(target_os = "macos") {
        TargetOs::Mac
    } else {
        TargetOs::NonMac
    };
    assert_eq!(TargetOs::host(), expected);
}
