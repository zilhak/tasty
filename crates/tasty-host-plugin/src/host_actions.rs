//! plugin이 상속할 수 있는 호스트 단축키를 설정에서 읽는다.
//! 허용된 액션 ID 목록은 매니페스트 검증과 공유하며 tasty-plugin-manifest가 소유한다.

use tasty_settings::KeybindingSettings;

pub use tasty_plugin_manifest::{INHERITABLE_HOST_ACTIONS, is_inheritable};

/// 주어진 host action id에 매핑되는 `KeybindingSettings`의 키 목록.
///
/// inherit 가능한 4종에만 매핑이 존재한다. 그 외 id는 `None`.
pub fn host_action_for<'a>(kb: &'a KeybindingSettings, action_id: &str) -> Option<&'a Vec<String>> {
    match action_id {
        "clipboard.copy" => Some(&kb.copy),
        "clipboard.paste" => Some(&kb.paste),
        "clipboard.cut" => Some(&kb.cut),
        "select_all" => Some(&kb.select_all),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb() -> KeybindingSettings {
        KeybindingSettings::preset_tasty()
    }

    #[test]
    fn inheritable_set_matches_resolver() {
        // 화이트리스트의 모든 id가 host_action_for로 해석된다.
        let kb = kb();
        for id in INHERITABLE_HOST_ACTIONS {
            assert!(
                host_action_for(&kb, id).is_some(),
                "missing mapping for {id}"
            );
        }
    }

    #[test]
    fn unknown_action_returns_none() {
        let kb = kb();
        assert!(host_action_for(&kb, "tab.new").is_none());
        assert!(host_action_for(&kb, "").is_none());
        assert!(host_action_for(&kb, "explorer.refresh").is_none());
    }

    #[test]
    fn copy_resolves_to_copy_field() {
        let kb = kb();
        let copy = host_action_for(&kb, "clipboard.copy").unwrap();
        assert_eq!(copy, &kb.copy);
    }

    #[test]
    fn is_inheritable_matches_constant() {
        assert!(is_inheritable("clipboard.copy"));
        assert!(is_inheritable("select_all"));
        assert!(!is_inheritable("tab.new"));
        assert!(!is_inheritable(""));
    }
}
