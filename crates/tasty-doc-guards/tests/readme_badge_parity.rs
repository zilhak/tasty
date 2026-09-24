//! 두 README의 버전·workspace 배지와 본문의 크레이트 수를 대조한다.
//! 버전은 루트 Cargo.toml, 크레이트 수는 crates 바로 아래의 매니페스트에서 읽는다.
//! root의 tasty는 세지 않으며 workspace.exclude에 있더라도 crates 아래에 있으면 포함한다.
//! 배지가 crates 디렉터리로 연결되므로 그 디렉터리의 개수를 표시하기 때문이다.
//!
//! 정해진 배지·본문 조각이 없으면 실패하지만, 다른 문장으로 복제한 숫자는 검사하지 못한다.
//! README 밖의 아키텍처 문서 개수는 architecture_crate_list_complete에서 검사한다.

use std::path::{Path, PathBuf};

/// 버전 배지 값의 끝은 뒤따르는 색상 앞의 하이픈이다.
const VERSION_PREFIX: &str = "badge/version-";

/// Workspace 배지 값 뒤에는 URL 인코딩 공백이 붙는다.
const WORKSPACE_PREFIX: &str = "badge/workspace-";

/// 부분 숫자가 긴 숫자에 일치하지 않도록 값의 끝까지 비교한다.
const WORKSPACE_SUFFIX: &str = "%20crates-";

/// 두 README가 공유하는 본문 표현의 숫자 뒤 부분.
const BODY_SUFFIX: &str = "-crate workspace";

const READMES: [&str; 2] = ["README.md", "README.ko.md"];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 다른 표의 version을 읽지 않도록 package 절에서만 찾는다.
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
        // 인라인 주석이 버전 값에 섞이지 않게 한다.
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

fn lines_with(contents: &str, needle: &str) -> Vec<String> {
    contents
        .lines()
        .filter(|l| l.contains(needle))
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn readme_version_badge_matches_cargo_version() {
    let root = repo_root();
    let version = package_version(&read(&root.join("Cargo.toml")));
    // 하이픈까지 비교해야 0.1을 0.10.2의 일부로 잘못 인정하지 않는다.
    let expected = format!("{VERSION_PREFIX}{version}-");

    let mut problems: Vec<String> = Vec::new();
    for name in READMES {
        let path = root.join(name);
        let contents = read(&path);
        let lines = lines_with(&contents, VERSION_PREFIX);
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
        "README Version 배지가 Cargo.toml의 {version}과 다르다. 버전을 올릴 때 배지도 함께 갱신한다(docs/dev-guide/release.md):\n{}",
        problems.join("\n")
    );
}

#[test]
fn readme_workspace_badge_matches_crate_directories() {
    let root = repo_root();
    let actual = tasty_doc_guards::crate_layers::crate_dir_names(&root).len();
    let expected = format!("{WORKSPACE_PREFIX}{actual}{WORKSPACE_SUFFIX}");
    let body = format!("{actual}{BODY_SUFFIX}");

    let mut problems: Vec<String> = Vec::new();
    for name in READMES {
        let contents = read(&root.join(name));

        let badges = lines_with(&contents, WORKSPACE_PREFIX);
        if badges.len() != 1 {
            problems.push(format!(
                "  {name}: Workspace 배지가 {} 개 (정확히 1 개여야 함):\n{}",
                badges.len(),
                badges
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        } else if contents.matches(expected.as_str()).count() != 1 {
            problems.push(format!(
                "  {name}: 배지 값이 crates/ 실측과 다름 — 기대 `{expected}`, 실제:\n      {}",
                badges[0]
            ));
        }

        // 본문에서 찾은 모든 개수를 비교하고 표현이 사라진 경우도 보고한다.
        let mentions = lines_with(&contents, BODY_SUFFIX);
        if mentions.is_empty() {
            problems.push(format!(
                "  {name}: 본문에 N{BODY_SUFFIX} 표현이 없다. 문구를 바꿨다면 BODY_SUFFIX도 갱신한다."
            ));
        }
        let stale: Vec<&String> = mentions.iter().filter(|l| !l.contains(&body)).collect();
        if !stale.is_empty() {
            problems.push(format!(
                "  {name}: 본문의 크레이트 수가 실측({actual}) 과 다름 — 기대 `{body}`:\n{}",
                stale
                    .iter()
                    .map(|l| format!("      {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "README의 크레이트 수가 실제 {actual}개와 다르다. 두 README의 배지와 본문을 함께 갱신한다:\n{}",
        problems.join("\n")
    );
}
