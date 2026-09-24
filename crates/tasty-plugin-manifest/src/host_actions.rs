//! inherit:<host_action>으로 참조할 수 있는 호스트 동작 이름.
//! 실제 키 설정은 tasty-host-plugin::host_actions에서 찾는다.

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
