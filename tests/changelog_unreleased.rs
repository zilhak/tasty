//! 본체와 plugin-protocol CHANGELOG에 `[Unreleased]` 절이 존재하는지 검증한다.
//!
//! 릴리스 도구가 `[Unreleased]`를 버전 헤더로 옮긴 뒤 새 `[Unreleased]`를
//! 비어 있는 상태로 재추가하는 것을 강제한다. (절차는 `docs/dev-guide/release.md`)

const ROOT_CHANGELOG: &str = "CHANGELOG.md";
const PLUGIN_PROTOCOL_CHANGELOG: &str = "crates/tasty-plugin-protocol/CHANGELOG.md";

#[test]
fn root_changelog_has_unreleased_section() {
    let changelog = read(ROOT_CHANGELOG);
    assert!(
        changelog.contains("## [Unreleased]"),
        "{ROOT_CHANGELOG} must contain a `## [Unreleased]` section"
    );
}

#[test]
fn plugin_protocol_changelog_has_unreleased_section() {
    let changelog = read(PLUGIN_PROTOCOL_CHANGELOG);
    assert!(
        changelog.contains("## [Unreleased]"),
        "{PLUGIN_PROTOCOL_CHANGELOG} must contain a `## [Unreleased]` section"
    );
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}
