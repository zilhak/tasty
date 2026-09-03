//! `README.md` / `README.ko.md` 의 Version 배지가 루트 `Cargo.toml` `[package] version`
//! 과 정확히 일치하는지 검증한다. 불일치 시 fail.
//!
//! 배경: shields.io static badge 는 URL 에 값이 박혀 있어 어디서도 파생되지 않는다.
//! 릴리스 절차(`docs/dev-guide/release.md` §1)가 bump 커밋에 배지를 함께 넣도록
//! 요구하지만 절차 문구만으로는 누락을 막지 못했고, 실제로 배지가 여러 마이너 뒤처진
//! 채 방치된 적이 있다. 이 테스트가 그 집행 채널이다 —
//! `cargo test --workspace`(`.github/workflows/test.yml`) 로 CI 강제된다.
//!
//! 선례: `tests/plugin_manifest_version_parity.rs`(plugin `Cargo.toml` ↔
//! `tasty-plugin.toml` lockstep). 같은 형태의 "선언값이 두 곳에 중복 존재" 드리프트다.

use std::path::{Path, PathBuf};

/// 배지 URL 에서 버전 값 앞에 오는 고정 조각. shields.io static badge 문법상
/// `badge/<label>-<message>-<color>` 이므로 값의 끝은 뒤따르는 `-<color>` 다.
const BADGE_PREFIX: &str = "badge/version-";

const READMES: [&str; 2] = ["README.md", "README.ko.md"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 루트 `Cargo.toml` 의 `[package]` 절에서 `version` 을 뽑는다.
///
/// 첫 `version = ` 라인을 그냥 집지 않고 절을 특정한다 — 루트 매니페스트에는
/// `[workspace]` 가 `[package]` 보다 앞에 있고, 나중에 `[workspace.package]` 같은
/// 절이 생기면 첫 매치가 엉뚱한 값을 가리키게 된다.
fn package_version(cargo_toml: &str) -> String {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("version") else {
            continue;
        };
        let Some(after_eq) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        // 값 뒤 인라인 주석(`version = "X" # note`)을 먼저 잘라낸다 — 안 자르면 닫는
        // 따옴표를 못 벗겨 값에 주석이 섞이고, 정상 배지가 불일치로 오탐된다.
        let value = after_eq
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches('"')
            .trim_end_matches('"')
            .to_string();
        assert!(
            !value.is_empty(),
            "루트 Cargo.toml [package] version 이 비어 있음"
        );
        return value;
    }
    panic!("루트 Cargo.toml 에서 [package] version 을 찾지 못함");
}

/// 배지 조각을 담은 라인들 — 실패 메시지에 "지금 뭐라고 적혀 있는지" 를 그대로 보인다.
fn badge_lines(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter(|l| l.contains(BADGE_PREFIX))
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn readme_version_badge_matches_cargo_version() {
    let root = repo_root();
    let version = package_version(&read(&root.join("Cargo.toml")));
    // 값의 끝 경계까지 포함해 비교한다. 뒤의 `-` 가 없으면 `0.1` 이 `0.10.2` 배지에도
    // 걸려 드리프트를 놓친다.
    let expected = format!("{BADGE_PREFIX}{version}-");

    let mut problems: Vec<String> = Vec::new();
    for name in READMES {
        let path = root.join(name);
        let contents = read(&path);
        let lines = badge_lines(&contents);
        let matches = contents.matches(expected.as_str()).count();

        if lines.len() != 1 {
            problems.push(format!(
                "  {name}: Version 배지가 {} 개 (정확히 1 개여야 함):\n{}",
                lines.len(),
                lines
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            continue;
        }
        if matches != 1 {
            problems.push(format!(
                "  {name}: 배지 값이 Cargo.toml 과 다름 — 기대 `{expected}`, 실제:\n      {}",
                lines[0]
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "README Version 배지가 루트 Cargo.toml version({version}) 과 드리프트됨.\n\
         릴리스 절차상 배지는 bump 커밋에 함께 갱신한다 (docs/dev-guide/release.md §1):\n{}",
        problems.join("\n")
    );
}
