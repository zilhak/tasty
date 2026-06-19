//! IPC 메서드 이름의 alias 정규화.
//!
//! 옛 이름과 새 이름이 같은 핸들러를 가리키도록 단방향 매핑한다. dispatcher와
//! method_meta는 **새 이름만** 알면 된다 — 옛 이름은 [`canonicalize`]에서
//! 1회 변환된 뒤 사라진다.
//!
//! deprecated 이름은 [`docs/dev-guide/cli-naming.md`](../../docs/dev-guide/cli-naming.md)에
//! 옮긴 시점이 명시되어 있다. 0.7.0 에서 `surface.meta_*` 4 종이 제거되었고,
//! 현재는 `tool.ssh.*`(1세대) 와 `ssh.profile.*`(2세대)가 `remote.profile.*` 로
//! alias 한시 호환 중이다 (구이름 실제 제거는 다음 minor tag 직전).
//!
//! `canonicalize` 는 **1회 변환**이라 2세대 구이름(`ssh.profile.*`)도 최종 새 이름으로
//! 직접 매핑한다(중간 이름 `tool.ssh.*` 를 거치지 않는다).

/// 옛 이름 → 새 이름 매핑. 새 이름은 반드시 `method_meta::METHOD_TABLE`에
/// 등록되어 있어야 한다.
const ALIASES: &[(&str, &str)] = &[
    // 1세대(tool.ssh.*) → 새 이름.
    ("tool.ssh.list", "remote.profile.list"),
    ("tool.ssh.get", "remote.profile.get"),
    ("tool.ssh.add", "remote.profile.add"),
    ("tool.ssh.detect", "remote.profile.detect"),
    ("tool.ssh.remove", "remote.profile.remove"),
    // 2세대(ssh.profile.*) → 새 이름 직접(1회 변환이라 중간 이름 미경유).
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
        // 0.7.0 에서 `surface.meta_*` 4 종 alias 제거 → 더 이상 정규화되지 않고
        // 그대로 통과한다 (= unknown_method).
        assert_eq!(canonicalize("surface.meta_set"), "surface.meta_set");
        assert!(!is_deprecated("surface.meta_set"));
        assert!(!is_deprecated("surface.meta.set"));
    }

    #[test]
    fn remote_profile_aliases_canonicalize() {
        // 1세대(tool.ssh.*)·2세대(ssh.profile.*) 모두 새 이름으로 1회 정규화.
        assert_eq!(canonicalize("tool.ssh.list"), "remote.profile.list");
        assert_eq!(canonicalize("tool.ssh.detect"), "remote.profile.detect");
        assert_eq!(canonicalize("ssh.profile.list"), "remote.profile.list");
        assert_eq!(canonicalize("ssh.profile.remove"), "remote.profile.remove");
        assert!(is_deprecated("tool.ssh.list"));
        assert!(is_deprecated("ssh.profile.list"));
        // 새 이름은 deprecated 가 아니며 그대로 통과한다.
        assert_eq!(canonicalize("remote.profile.list"), "remote.profile.list");
        assert!(!is_deprecated("remote.profile.list"));
    }

    // F.B.4-3: 본 테스트는 method_meta 가 아직 본 바이너리에 잔존하여 이 crate
    // 에서 직접 검증 불가. method_meta 가 tasty-ipc 로 이동한 뒤
    // (B.6 / B.7 통과 후 후속 substep) 같은 cross-validation 을 재도입한다.
    // (대안: 본 바이너리 alias_integration.rs 통합 테스트.)
}
