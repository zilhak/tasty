//! 호스트의 의미론적 단축키 액션 카탈로그.
//!
//! plugin이 매니페스트에서 `binding_mode = "inherit:<host_action>"`를 선언할 때
//! 참조할 수 있는 액션 id 화이트리스트와, 해당 id에 대응하는 호스트
//! `KeybindingSettings` 필드를 조회하는 헬퍼를 제공한다.
//!
//! id 화이트리스트 자체(`INHERITABLE_HOST_ACTIONS`)는 `tasty-plugin-manifest`가
//! 소유한다 — 매니페스트 로드 시점 validate(`validate_contributed_commands`)도
//! 같은 목록을 참조해야 하는데, 그 crate는 `KeybindingSettings`(호스트 설정)에
//! 의존하지 않으므로 id 목록만 그쪽에 두고 여기서 재노출한다. 이 모듈은 id →
//! 실제 키 목록 해석(`host_action_for`)만 담당.
//!
//! 화이트리스트는 의도적으로 좁게 시작 (clipboard 4종). 추가 요청 시
//! 케이스별로 검토 후 확장한다 — plugin이 임의 호스트 액션에 inherit하면
//! 의미 매핑이 모호해진다.
//!
//! # inherit 가능한 액션 (현재)
//!
//! - `clipboard.copy`
//! - `clipboard.paste`
//! - `clipboard.cut`
//! - `select_all`

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
