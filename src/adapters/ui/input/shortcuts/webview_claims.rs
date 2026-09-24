//! 설정과 플러그인 바인딩에서 webview 위에서도 host가 처리할 키 조합을 만든다.
//! 페이지 액션은 제외하며 quick-switch 조합은 numeric.rs와 같은 규칙을 따른다.
//! 배경: `docs/adr/0029-webview-host-integration.md`.

use crate::settings::KeybindingSettings;
use crate::webview::{HostShortcutPolicy, ShortcutSources};

/// 페이지가 처리할 액션 ID. 키 조합을 바꿔도 해당 액션은 host로 보내지 않는다.
const PAGE_RESERVED_FIELDS: &[&str] = &["find", "copy", "cut", "paste", "select_all"];

/// 설정의 host 액션·quick-switch·사용자 스크립트와 플러그인 바인딩으로 키 정책을 만든다.
/// `plugin_combos`는 매니페스트 기본값에 사용자 설정을 적용한 목록이다.
/// modifier와 페이지 예약 키의 중복은 정책 객체에서 검사한다.
pub(crate) fn webview_shortcut_policy(
    kb: &KeybindingSettings,
    plugin_combos: Vec<String>,
) -> HostShortcutPolicy {
    HostShortcutPolicy::new(ShortcutSources {
        host: host_combos(kb),
        page_reserved: page_reserved_combos(kb),
        plugin: plugin_combos,
    })
}

fn page_reserved_combos(kb: &KeybindingSettings) -> Vec<String> {
    PAGE_RESERVED_FIELDS
        .iter()
        .flat_map(|f| kb.get_bindings(f).unwrap_or(&[]))
        .cloned()
        .collect()
}

fn host_combos(kb: &KeybindingSettings) -> Vec<String> {
    let mut combos: Vec<String> = Vec::new();

    for (field_id, _label) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        if PAGE_RESERVED_FIELDS.contains(field_id) {
            continue;
        }
        combos.extend(kb.get_bindings(field_id).unwrap_or(&[]).iter().cloned());
    }

    // 개별 지정은 완성된 조합을, 나머지는 modifier와 키를 합쳐 사용한다.
    let axes: [(&str, Vec<&str>); 3] = [
        (
            kb.tab_switch_modifier.as_str(),
            kb.tab_switch_slot_keys
                .iter()
                .map(|s| s.as_str())
                .chain([kb.tab_next_key(), kb.tab_prev_key()])
                .collect(),
        ),
        (
            kb.workspace_switch_modifier.as_str(),
            kb.workspace_switch_slot_keys
                .iter()
                .map(|s| s.as_str())
                .chain([kb.workspace_next_key(), kb.workspace_prev_key()])
                .collect(),
        ),
        (
            kb.category_switch_modifier.as_str(),
            kb.category_switch_slot_keys
                .iter()
                .map(|s| s.as_str())
                .chain([kb.category_next_key(), kb.category_prev_key()])
                .collect(),
        ),
    ];
    for (modifier, raw_keys) in axes {
        for raw in raw_keys {
            if raw.is_empty() {
                continue;
            }
            if modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER {
                combos.push(raw.to_string());
            } else {
                combos.push(format!("{modifier}+{raw}"));
            }
        }
    }

    combos.extend(kb.script_bindings.iter().map(|b| b.combo.clone()));
    combos
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{Key, ModifiersState};

    fn kb() -> KeybindingSettings {
        KeybindingSettings::default()
    }

    /// 바인딩 토큰 `alt` 의 실제 modifier(macOS 는 Command).
    fn alt_mod() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::ALT
        }
    }

    #[test]
    fn a_default_host_action_is_claimed_end_to_end() {
        let kb = kb();
        assert_eq!(
            kb.get_bindings("split_surface_vertical"),
            Some(&["alt+d".to_string()][..]),
            "sanity: 이 시험은 기본값 alt+d 를 전제한다"
        );
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(policy.claims(&Key::Character("d".into()), alt_mod()));
    }

    #[test]
    fn a_rebound_host_action_moves_the_claim() {
        let mut kb = kb();
        assert!(kb.replace_binding_at("split_surface_vertical", 0, "ctrl+alt+y".to_string()));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(
            policy.combos().iter().any(|c| c == "ctrl+alt+y"),
            "{:?}",
            policy.combos()
        );
        assert!(
            !policy.claims(&Key::Character("d".into()), alt_mod()),
            "옛 콤보 alt+d 가 남아 있다: {:?}",
            policy.combos()
        );
    }

    #[test]
    fn page_reserved_actions_are_not_claimed() {
        let kb = kb();
        let policy = webview_shortcut_policy(&kb, Vec::new());
        for field in PAGE_RESERVED_FIELDS {
            for binding in kb.get_bindings(field).unwrap_or(&[]) {
                assert!(
                    !policy.combos().iter().any(|c| c == binding),
                    "page-reserved binding {binding} ({field}) must not be claimed"
                );
            }
        }
        assert!(!policy.combos().is_empty(), "policy should not be empty");
    }

    /// 표기가 달라도 페이지 예약 키와 같은 조합이면 페이지에 남긴다.
    #[test]
    fn plugin_binding_of_a_reserved_action_stays_with_the_page() {
        let kb = kb();
        let find = kb
            .get_bindings("find")
            .and_then(|b| b.first())
            .cloned()
            .expect("sanity: find 에 기본 바인딩이 있어야 한다");
        let policy = webview_shortcut_policy(&kb, vec![find.to_ascii_uppercase()]);
        assert!(
            !policy
                .combos()
                .iter()
                .any(|c| tasty_key_match::bindings_equivalent(c, &find)),
            "{find} 를 host 가 가져간다: {:?}",
            policy.combos()
        );
    }

    /// 사이드바와 복사 모드는 페이지 액션이 아니라 창의 동작이다.
    #[test]
    fn window_chrome_combos_are_claimed() {
        let kb = kb();
        let policy = webview_shortcut_policy(&kb, Vec::new());
        for field in [
            "toggle_sidebar",
            "toggle_sidebar_collapse",
            "enter_copy_mode",
        ] {
            for binding in kb.get_bindings(field).unwrap_or(&[]) {
                assert!(
                    policy.combos().iter().any(|c| c == binding),
                    "{binding} ({field}) 가 정책에 없다: {:?}",
                    policy.combos()
                );
            }
        }
    }

    /// 페이지의 undo/redo 키를 host가 가로채지 않는다.
    #[test]
    fn ctrl_z_belongs_to_the_page() {
        let kb = kb();
        let policy = webview_shortcut_policy(&kb, Vec::new());
        for combo in ["ctrl+z", "alt+z", "ctrl+shift+z", "alt+shift+z"] {
            assert!(
                !policy.combos().iter().any(|c| c == combo),
                "{combo} 를 host 가 가져간다: {:?}",
                policy.combos()
            );
        }
    }

    #[test]
    fn quick_switch_axis_combos_are_claimed() {
        let kb = kb();
        let policy = webview_shortcut_policy(&kb, Vec::new());
        let slot = kb.workspace_slot_key(1).expect("slot 1");
        let expected = format!("{}+{}", kb.workspace_switch_modifier, slot);
        assert!(
            policy.combos().contains(&expected),
            "expected {expected} in policy: {:?}",
            policy.combos()
        );
    }
}
