//! 본체와 plugin-protocol CHANGELOG에 `[Unreleased]` 절이 존재하는지 검증한다.
//!
//! 릴리스 도구가 `[Unreleased]`를 버전 헤더로 옮긴 뒤 새 `[Unreleased]`를
//! 비어 있는 상태로 재추가하는 것을 강제한다. (절차는 `docs/dev-guide/release.md`)

#[test]
fn root_changelog_has_unreleased_section() {
    let path = "CHANGELOG.md";
    let changelog =
        std::fs::read_to_string(path).expect("CHANGELOG.md must exist at workspace root");
    assert!(
        changelog.contains("## [Unreleased]"),
        "{path} must contain a `## [Unreleased]` section"
    );
}

#[test]
fn plugin_protocol_changelog_has_unreleased_section() {
    let path = "crates/tasty-plugin-protocol/CHANGELOG.md";
    let changelog = std::fs::read_to_string(path).expect("plugin-protocol CHANGELOG must exist");
    assert!(
        changelog.contains("## [Unreleased]"),
        "{path} must contain a `## [Unreleased]` section"
    );
}
