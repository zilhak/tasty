//! 설정과 플러그인 바인딩에서 webview 위에서도 host가 처리할 키 조합을 만든다.
//! 페이지 액션은 제외하며 quick-switch 조합은 numeric.rs와 같은 규칙을 따른다.
//! 배경: `docs/design/systems/webview.md`.

use crate::settings::KeybindingSettings;
use crate::webview::{HostShortcutPolicy, ShortcutSources, WebViewEditAction};

/// 페이지가 처리할 액션 ID. 키 조합을 바꿔도 해당 액션은 host로 보내지 않는다.
/// 편집 액션은 `ShortcutSources::edit`로 키를 받은 WebView의 엔진 편집 명령이 된다.
const PAGE_RESERVED_FIELDS: &[&str] = &[
    "find",
    "copy",
    "cut",
    "paste",
    "select_all",
    "page_undo",
    "page_redo",
];
/// explorer 에 포커스가 있을 때만 동작하는 액션. webview 위에서는 실행되지 않으므로 host 가
/// 가져가지 않는다. 특히 실행 취소 키는 페이지의 undo/redo 와, 뒤로·앞으로 키(Alt+←·Alt+→)는
/// 페이지의 뒤로·앞으로와 같다. 주소창 포커스(`explorer_focus_address`)는 주소 띠가 있는
/// html 페이지에서만 host 가 가져간다(`ShortcutSources::address`).
const EXPLORER_ONLY_FIELDS: &[&str] = &[
    "explorer_undo",
    "explorer_redo",
    "explorer_back",
    "explorer_forward",
    "explorer_focus_address",
    "explorer_new_folder",
    "explorer_new_file",
];

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
        address: kb
            .get_bindings("explorer_focus_address")
            .unwrap_or(&[])
            .to_vec(),
        refresh: kb.refresh.clone(),
        edit: WebViewEditAction::ALL
            .into_iter()
            .map(|a| (a, kb.get_bindings(a.field_id()).unwrap_or(&[]).to_vec()))
            .collect(),
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
        // 공통 새로고침은 `ShortcutSources::refresh`로만 넣는다. 여기서 넣으면 페이지 예약 조합 검사를 건너뛴다.
        if PAGE_RESERVED_FIELDS.contains(field_id)
            || EXPLORER_ONLY_FIELDS.contains(field_id)
            || *field_id == "refresh"
        {
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

    /// explorer 전용 실행 취소·다시 실행은 다른 키로 바꿔도 webview 위에서 host 가 가져가지 않는다.
    #[test]
    fn explorer_only_actions_are_never_claimed_over_a_page() {
        let mut kb = kb();
        assert!(kb.set_field("explorer_undo", "ctrl+shift+u"));
        assert!(kb.set_field("explorer_redo", "ctrl+shift+o"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        for combo in ["ctrl+shift+u", "ctrl+shift+o"] {
            assert!(
                !policy.combos().iter().any(|c| c == combo),
                "{combo} 를 host 가 가져간다"
            );
        }
    }

    /// 주소창 포커스는 주소 띠가 있는 html 페이지에서만 host가 가져가고 다른 WebView 페이지에는
    /// 남긴다. 키를 바꾸면 새 키만 가져가고 이전 키는 페이지로 간다.
    #[test]
    fn the_address_focus_key_is_claimed_only_over_an_html_page_and_follows_a_rebind() {
        use crate::host_api::webview::keys::{WebViewKeyBridge, WebViewKeySink};
        let unknown = || {
            winit::keyboard::PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified)
        };
        let l = || Key::Character("l".into());
        let k = || Key::Character("k".into());
        let (html, other) = (1, 2);
        let bridge = WebViewKeyBridge::new();
        bridge.set_address_surfaces([html].into_iter().collect());
        let mut kb = kb();
        bridge.set_policy(webview_shortcut_policy(&kb, Vec::new()));
        assert!(bridge.capture_key(html, l(), unknown(), alt_mod()));
        assert!(!bridge.capture_key(other, l(), unknown(), alt_mod()));
        assert!(kb.set_field("explorer_focus_address", "ctrl+alt+k"));
        bridge.set_policy(webview_shortcut_policy(&kb, Vec::new()));
        let ctrl_alt = ModifiersState::CONTROL | alt_mod();
        assert!(bridge.capture_key(html, k(), unknown(), ctrl_alt));
        assert!(!bridge.capture_key(html, l(), unknown(), alt_mod()));
        assert!(!bridge.capture_key(other, k(), unknown(), ctrl_alt));
    }

    /// 편집 네 액션은 host 조합이 아니라 현재 설정의 편집 명령이 되고, 재지정·해제를 따른다.
    #[test]
    fn edit_bindings_follow_the_settings_and_are_not_host_claims() {
        let mut kb = kb();
        assert!(kb.clear_field("copy"));
        assert!(kb.add_binding("copy", "ctrl+shift+c".into()));
        assert!(kb.clear_field("select_all"));
        assert!(kb.add_binding("select_all", "ctrl+alt+a".into()));
        assert!(kb.clear_field("paste"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(
            policy.edit_action(&Key::Character("c".into()), ctrl_shift),
            Some(WebViewEditAction::Copy)
        );
        assert!(!policy.claims(&Key::Character("c".into()), ctrl_shift));
        assert_eq!(
            policy.edit_action(
                &Key::Character("a".into()),
                ModifiersState::CONTROL | alt_mod()
            ),
            Some(WebViewEditAction::SelectAll)
        );
        assert_eq!(
            policy.edit_action(&Key::Character("v".into()), alt_mod()),
            None,
            "해제한 paste 는 편집 명령을 만들지 않는다"
        );
        assert_eq!(
            policy.edit_action(&Key::Character("c".into()), alt_mod()),
            None,
            "이전 copy 조합 alt+c 가 남아 있다"
        );
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

    /// 공통 새로고침의 F5 는 보조키가 없어도 webview 위에서 host 가 가져간다. 키를 바꾸면 새 키만,
    /// 해제하면 아무것도 가져가지 않는다. 보조키 없는 문자 키로 바꾸면 페이지에 남긴다.
    #[test]
    fn the_refresh_function_key_is_claimed_and_follows_a_rebind() {
        use winit::keyboard::NamedKey;
        let f5 = Key::Named(NamedKey::F5);
        let none = ModifiersState::empty();
        let mut kb = kb();
        // 탐색기 F5 는 webview 위에서 동작하지 않으므로 공통 키만 남겨 둔다.
        kb.explorer_refresh.clear();
        assert!(webview_shortcut_policy(&kb, Vec::new()).claims(&f5, none));

        assert!(kb.set_field("refresh", "ctrl+alt+r"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(!policy.claims(&f5, none), "{:?}", policy.combos());
        assert!(policy.claims(
            &Key::Character("r".into()),
            ModifiersState::CONTROL | alt_mod()
        ));

        assert!(kb.set_field("refresh", "r"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(!policy.claims(&Key::Character("r".into()), none));

        assert!(kb.clear_field("refresh"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(!policy.claims(&f5, none));
    }

    /// 새로고침을 페이지 편집 조합(엔진 기본 Paste인 Ctrl+V)에 묶어도 host가 가져가지 않는다.
    /// 그 키는 엔진 붙여넣기로 페이지에 남는다. 다른 host 조합 경로로 새로고침이 들어가지 않는다.
    #[test]
    fn a_refresh_binding_on_a_page_edit_key_stays_with_the_page() {
        let mut kb = kb();
        kb.paste = vec!["ctrl+v".into()];
        kb.refresh = vec!["ctrl+v".into()];
        let policy = webview_shortcut_policy(&kb, Vec::new());
        let v = Key::Character("v".into());
        assert!(
            !policy.claims(&v, ModifiersState::CONTROL),
            "{:?}",
            policy.combos()
        );
        assert!(!host_combos(&kb).iter().any(|c| c == "ctrl+v"));
        if !cfg!(target_os = "macos") {
            assert_eq!(
                policy.edit_action(&v, ModifiersState::CONTROL),
                None,
                "엔진 기본 Paste는 엔진이 처리한다"
            );
        }
        assert!(policy.matches_edit_binding(&v, ModifiersState::CONTROL));
    }

    /// 기능키 예외는 공통 새로고침에만 있다. 다른 host 동작의 보조키 없는 F 키는 페이지에 남긴다.
    #[test]
    fn other_modifierless_function_keys_stay_with_the_page() {
        let mut kb = kb();
        kb.refresh.clear();
        assert!(kb.set_field("toggle_settings", "f9"));
        let policy = webview_shortcut_policy(&kb, Vec::new());
        assert!(!policy.claims(
            &Key::Named(winit::keyboard::NamedKey::F9),
            ModifiersState::empty()
        ));
    }
}
