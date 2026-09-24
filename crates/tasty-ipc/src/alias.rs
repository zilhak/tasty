//! 호환용 메서드 이름을 최종 이름으로 한 번만 바꾼다.
//! tool.ssh.*와 ssh.profile.*는 remote.profile.*로 직접 연결한다.
//! 별칭 제거 규칙은 [API 규약](../../../docs/dev-guide/api-conventions.md)을 따른다.

/// 옛 이름 → 새 이름 매핑. 새 이름은 반드시 `method_meta::METHOD_TABLE`에
/// 등록되어 있어야 한다.
const ALIASES: &[(&str, &str)] = &[
    ("tool.ssh.list", "remote.profile.list"),
    ("tool.ssh.get", "remote.profile.get"),
    ("tool.ssh.add", "remote.profile.add"),
    ("tool.ssh.detect", "remote.profile.detect"),
    ("tool.ssh.remove", "remote.profile.remove"),
    ("ssh.profile.list", "remote.profile.list"),
    ("ssh.profile.get", "remote.profile.get"),
    ("ssh.profile.add", "remote.profile.add"),
    ("ssh.profile.remove", "remote.profile.remove"),
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
    fn unknown_method_passthrough() {
        assert_eq!(canonicalize("surface.list"), "surface.list");
        assert_eq!(canonicalize("some.random.method"), "some.random.method");
    }

    #[test]
    fn removed_aliases_passthrough() {
        assert_eq!(canonicalize("surface.meta_set"), "surface.meta_set");
        assert!(!is_deprecated("surface.meta_set"));
        assert!(!is_deprecated("surface.meta.set"));
    }

    #[test]
    fn remote_profile_aliases_canonicalize() {
        assert_eq!(canonicalize("tool.ssh.list"), "remote.profile.list");
        assert_eq!(canonicalize("tool.ssh.detect"), "remote.profile.detect");
        assert_eq!(canonicalize("ssh.profile.list"), "remote.profile.list");
        assert_eq!(canonicalize("ssh.profile.remove"), "remote.profile.remove");
        assert!(is_deprecated("tool.ssh.list"));
        assert!(is_deprecated("ssh.profile.list"));
        assert_eq!(canonicalize("remote.profile.list"), "remote.profile.list");
        assert!(!is_deprecated("remote.profile.list"));
    }
}
