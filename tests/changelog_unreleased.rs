//! 본체와 plugin-protocol CHANGELOG에 Unreleased 헤더 문자열이 있는지 확인한다. 절의 내용이 비었는지는 검사하지 않는다.

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
