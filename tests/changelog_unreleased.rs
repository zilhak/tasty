//! 본체와 plugin-protocol CHANGELOG에 `[Unreleased]` 절이 존재하는지 검증한다.
//!
//! 릴리스 도구가 `[Unreleased]`를 버전 헤더로 옮긴 뒤 새 `[Unreleased]`를
//! 비어 있는 상태로 재추가하는 것을 강제한다. (절차는 `docs/dev-guide/release.md`)
//!
//! 추가로 0.7.x SemVer 가드: `[Unreleased]` 의 bullet entry 에 `(BREAK)` 토큰이
//! 들어가 있으면서 직전 release 와 major 가 동일하면 fail 한다. major bump
//! (X.0.0) 없이 break 가 누적되는 사고를 차단.

const ROOT_CHANGELOG: &str = "CHANGELOG.md";
const PLUGIN_PROTOCOL_CHANGELOG: &str = "crates/tasty-plugin-protocol/CHANGELOG.md";
const ROOT_CARGO: &str = "Cargo.toml";
const PLUGIN_PROTOCOL_CARGO: &str = "crates/tasty-plugin-protocol/Cargo.toml";

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

#[test]
fn root_unreleased_has_no_break_when_major_unchanged() {
    assert_no_break_without_major_bump(ROOT_CHANGELOG, ROOT_CARGO);
}

#[test]
fn plugin_protocol_unreleased_has_no_break_when_major_unchanged() {
    assert_no_break_without_major_bump(PLUGIN_PROTOCOL_CHANGELOG, PLUGIN_PROTOCOL_CARGO);
}

fn assert_no_break_without_major_bump(changelog_path: &str, cargo_path: &str) {
    let cl = read(changelog_path);
    let cargo = read(cargo_path);

    let unreleased = extract_section(&cl, "## [Unreleased]")
        .unwrap_or_else(|| panic!("{changelog_path} missing `## [Unreleased]` section"));
    let last_released_major = parse_first_versioned_major(&cl);
    let cargo_major = parse_cargo_major(&cargo)
        .unwrap_or_else(|| panic!("{cargo_path}: cannot parse `version = \"X.Y.Z\"`"));

    let Some(last_major) = last_released_major else {
        return;
    };

    if cargo_major == last_major && has_break_bullet(unreleased) {
        panic!(
            "{changelog_path} [Unreleased] 에 `(BREAK)` bullet entry 가 있으나 \
             {cargo_path} major ({cargo_major}) 가 직전 release ({last_major}) 와 동일. \
             SemVer 위반 — major bump 가 필요 또는 deprecation 으로 전환할 것 \
             (docs/dev-guide/ipc-stability.md)."
        );
    }
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn extract_section<'a>(text: &'a str, header: &str) -> Option<&'a str> {
    let start = text.find(header)?;
    let rest = &text[start + header.len()..];
    let end = rest
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or_else(|| rest.len());
    Some(&rest[..end])
}

fn parse_first_versioned_major(changelog: &str) -> Option<u64> {
    for line in changelog.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("## [") else {
            continue;
        };
        if rest.starts_with("Unreleased]") {
            continue;
        }
        let close = rest.find(']')?;
        let ver = &rest[..close];
        return parse_major(ver);
    }
    None
}

fn parse_cargo_major(cargo: &str) -> Option<u64> {
    for line in cargo.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("version") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim().trim_matches('"');
        return parse_major(rest);
    }
    None
}

fn parse_major(ver: &str) -> Option<u64> {
    ver.split('.').next()?.parse::<u64>().ok()
}

fn has_break_bullet(section: &str) -> bool {
    for line in section.lines() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("- ") || trimmed.starts_with("* ")) && trimmed.contains("(BREAK)") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn extract_section_handles_eof() {
        let s = "## [Unreleased]\nbody\n";
        assert_eq!(extract_section(s, "## [Unreleased]"), Some("\nbody\n"));
    }

    #[test]
    fn extract_section_stops_at_next_header() {
        let s = "## [Unreleased]\nbody\n## [0.7.0]\nold\n";
        assert_eq!(extract_section(s, "## [Unreleased]"), Some("\nbody\n"));
    }

    #[test]
    fn parse_first_versioned_major_skips_unreleased() {
        let s = "## [Unreleased]\n## [1.2.3]\n";
        assert_eq!(parse_first_versioned_major(s), Some(1));
    }

    #[test]
    fn parse_cargo_major_basic() {
        assert_eq!(parse_cargo_major("version = \"2.5.7\""), Some(2));
    }

    #[test]
    fn has_break_bullet_detects_dash() {
        assert!(has_break_bullet("- (BREAK) foo\n"));
        assert!(has_break_bullet("  - (BREAK) bar\n"));
    }

    #[test]
    fn has_break_bullet_ignores_prose() {
        assert!(!has_break_bullet("(BREAK) 0.6.0 → 0.7.0 was the start\n"));
        assert!(!has_break_bullet("> (BREAK) note\n"));
    }
}
