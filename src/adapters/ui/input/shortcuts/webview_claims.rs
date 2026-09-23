//! native webview 위에서 host 가 가져갈 콤보를 `KeybindingSettings` 에서 도출한다.
//!
//! webview 키 브리지(`host_api/webview/keys.rs`)는 콤보 목록을 **주입받기만** 한다 — 어느
//! 설정 필드가 host 액션이고 어느 것이 페이지가 자체로 구현하는 액션인지는 이 단축키
//! 계층의 지식이라 여기 둔다. quick-switch 합성 규칙은 디스패치(`numeric.rs`)와 같다.
//! 결정 배경: `docs/adr/0629-webview-host-integration.md`,
//! 주입 경계: `docs/adr/0629-webview-host-integration.md`.

use crate::settings::KeybindingSettings;
use crate::webview::{HostShortcutPolicy, ShortcutSources};

/// 페이지가 소유하는(= host 로 포워딩하지 않는) 단축키 액션의 `GENERAL_BINDING_FIELDS`
/// field id. 콤보가 아니라 **액션 id** 목록이라, 사용자가 그 액션의 콤보를 바꿔도
/// 규칙이 따라간다(하드코딩된 키 문자열이 아니다). `find` 는 host 쪽에도 같은 취지의
/// kind 게이트가 있다(`keybinding.rs`).
const PAGE_RESERVED_FIELDS: &[&str] = &["find", "copy", "cut", "paste", "select_all"];

/// `KeybindingSettings` 전량 + plugin 명령 바인딩에서 webview 키 정책을 만든다. host 쪽은
/// 고정 액션 필드(페이지 예약 제외) + quick-switch 3 축(슬롯/다음/이전) + 사용자 스크립트
/// 바인딩이고, modifier 필터와 예약 동등성 필터는 정책 자신이 건다.
///
/// `plugin_combos` 는 `plugin_bridge::key_dispatch::all_command_bindings` 가 만든
/// effective binding 목록이다(매니페스트 default + 사용자 override 합성 결과).
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

    // quick-switch 3 축. 축 modifier 가 `INDIVIDUAL_SWITCH_MODIFIER` sentinel 이면
    // 슬롯/다음/이전 필드가 이미 완성된 콤보이고, 아니면 `<modifier>+<raw key>` 로
    // 합성한다 — dispatch(`numeric.rs`)와 같은 규칙이다.
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

    /// 기본 설정의 host 액션(`split_surface_vertical` = alt+d)을 정책이 가져간다 —
    /// 설정 → 정책 → 판정의 끝과 끝을 한 번 잇는다.
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

    /// 사용자가 콤보를 바꾸면 정책이 따라간다 — 키 리터럴이 아니라 설정에서 온다는 것의
    /// 직접 증거다.
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

    /// 페이지 예약 액션(find/copy/cut/paste/select_all)의 콤보는 정책에 오르지
    /// 않는다 — 문서의 find-in-page·복사가 계속 페이지 것이어야 한다.
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

    /// plugin 이 페이지 예약 액션의 콤보를 표기만 바꿔 바인딩해도 페이지가 갖는다 —
    /// 예약 목록이 설정에서 정책까지 실제로 흘러 들어가는지를 본다.
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

    /// 사이드바 토글·복사 모드 콤보는 host 가 가져간다.
    ///
    /// 이 셋은 `GENERAL_BINDING_FIELDS` 밖에 있던 동안 정책에 안 올랐고, 그래서
    /// markdown/html webview 에 포커스가 있으면 `ctrl+b` · `ctrl+shift+b` ·
    /// `ctrl+shift+space` 가 페이지로 갔다 — 사이드바 단축키가 그 표면에서만 죽었다.
    /// 페이지 예약(`PAGE_RESERVED_FIELDS`)에 넣지 않은 이유는 그 목록이 **모든 문서가
    /// 자기 구현을 갖는 문서 액션**(찾기·복사·잘라내기·붙여넣기·전체선택)이기 때문이다.
    /// 사이드바와 복사 모드는 창 자체의 동작이라 페이지가 대신 수행할 수 없다.
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

    /// `ctrl+z` 는 페이지가 갖는다.
    ///
    /// host 에 `image_undo` / `image_redo` 필드가 있던 동안 SoT 순회가 그 콤보를 정책에
    /// 올렸고, `capture_key` 는 그것을 host 로 넘겼다. 그런데 host 에는 실행부가 없어
    /// 아무 일도 일어나지 않았다 — 그 사이 **페이지 자신의 undo 까지 죽었다.** 필드를
    /// 걷어낸 뒤로는 페이지가 그대로 받는다. 예약 목록에 넣어 해결한 것이 아니라
    /// (예약은 host 가 그 액션을 가진다는 전제 위에서만 뜻이 있다) 없는 액션을 없앤 것이다.
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

    /// quick-switch 축은 `<modifier>+<slot key>` 로 합성돼 정책에 오른다
    /// (workspace 전환이 "완료 확인 방법" 의 측정 대상 3종 중 하나다).
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
