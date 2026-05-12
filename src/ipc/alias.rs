//! IPC 메서드 이름의 alias 정규화.
//!
//! 옛 이름과 새 이름이 같은 핸들러를 가리키도록 단방향 매핑한다. dispatcher와
//! method_meta는 **새 이름만** 알면 된다 — 옛 이름은 [`canonicalize`]에서
//! 1회 변환된 뒤 사라진다.
//!
//! deprecated 이름은 [`docs/dev-guide/cli-naming.md`](../../docs/dev-guide/cli-naming.md)에
//! 옮긴 시점이 명시되어 있다. 실제 제거는 1.0 tag 직전에 일괄 PR로.

/// 옛 이름 → 새 이름 매핑. 새 이름은 반드시 `method_meta::METHOD_TABLE`에
/// 등록되어 있어야 한다.
const ALIASES: &[(&str, &str)] = &[
    ("surface.meta_set", "surface.meta.set"),
    ("surface.meta_get", "surface.meta.get"),
    ("surface.meta_unset", "surface.meta.unset"),
    ("surface.meta_list", "surface.meta.list"),
];

/// 옛 이름이 들어오면 새 이름으로 정규화한다. 새 이름은 그대로 반환.
pub fn canonicalize(method: &str) -> &str {
    for (old, new) in ALIASES {
        if method == *old {
            return new;
        }
    }
    method
}

/// 호출된 메서드가 deprecated alias인지 검사 — `tracing::warn` 출력 결정용.
pub fn is_deprecated(method: &str) -> bool {
    ALIASES.iter().any(|(old, _)| method == *old)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_alias_normalizes() {
        assert_eq!(canonicalize("surface.meta_set"), "surface.meta.set");
        assert_eq!(canonicalize("surface.meta_unset"), "surface.meta.unset");
    }

    #[test]
    fn unknown_method_passthrough() {
        assert_eq!(canonicalize("surface.list"), "surface.list");
        assert_eq!(canonicalize("some.random.method"), "some.random.method");
    }

    #[test]
    fn deprecated_flag_only_for_aliases() {
        assert!(is_deprecated("surface.meta_set"));
        assert!(!is_deprecated("surface.meta.set"));
        assert!(!is_deprecated("surface.list"));
    }

    #[test]
    fn new_names_are_registered_in_method_table() {
        use crate::ipc::method_meta::METHOD_TABLE;
        for (_old, new) in ALIASES {
            let found = METHOD_TABLE.iter().any(|(name, _)| name == new);
            assert!(
                found,
                "alias target '{new}' must exist in METHOD_TABLE"
            );
        }
    }
}
