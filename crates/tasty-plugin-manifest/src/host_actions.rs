//! `binding_mode = "inherit:<host_action>"` 가 참조할 수 있는 호스트 액션 id
//! 화이트리스트.
//!
//! 이 crate는 `KeybindingSettings`(호스트 설정 타입)에 의존하지 않으므로 id
//! 목록만 소유한다. id → 실제 키 목록 해석(`host_action_for`)은
//! `tasty-host-plugin::host_actions`가 담당 — 이 상수를 그대로 재노출한다.
//!
//! 화이트리스트는 의도적으로 좁게 시작 (clipboard 4종). 추가 요청 시
//! 케이스별로 검토 후 확장한다 — plugin이 임의 호스트 액션에 inherit하면
//! 의미 매핑이 모호해진다.

/// inherit가 허용되는 호스트 액션 id 목록.
pub const INHERITABLE_HOST_ACTIONS: &[&str] = &[
    "clipboard.copy",
    "clipboard.paste",
    "clipboard.cut",
    "select_all",
];

/// 주어진 host action id가 inherit 화이트리스트에 있는지.
pub fn is_inheritable(action_id: &str) -> bool {
    INHERITABLE_HOST_ACTIONS.contains(&action_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_inheritable_matches_constant() {
        assert!(is_inheritable("clipboard.copy"));
        assert!(is_inheritable("select_all"));
        assert!(!is_inheritable("tab.new"));
        assert!(!is_inheritable(""));
    }
}
