//! `docs/architecture/index.md` 의 크레이트 열거가 `crates/*/` 와 어긋나지 않는지
//! 검증한다.
//!
//! 아키텍처 문서는 에이전트가 크레이트의 역할을 찾는 첫 진입점이라, 목록에 없는
//! 크레이트는 존재 자체가 발견되지 않는다. 수치와 목록이 손으로 복제되어 있어
//! 크레이트가 추가될 때 문서가 따라오지 않는 drift 가 반복됐다 — 이 테스트가
//! 그 갱신을 강제한다. 통합 테스트라 자동 실행 채널이 없으니(컴파일만 자동으로
//! 검사된다 — `docs/dev-guide/ci-gates.md`)
//! 크레이트를 추가·삭제했으면 직접 돌려야 잡힌다.
//!
//! 검사 3 종:
//! - 모든 `crates/<name>/Cargo.toml` 의 `<name>` 이 문서에 `` `<name>` `` 형태로
//!   등장한다(번들 plugin 도 축약 없이 풀네임).
//! - 절 제목 `## 워크스페이스 크레이트 (N)` 의 N 이 디렉토리 수와 같다.
//! - 개요 문장의 "N 개 크레이트(`crates/*`)" 도 같은 값이다.
//!
//! 역방향(문서에만 있고 디렉토리에 없는 이름)은 검사하지 않는다 — `tasty-tui-sim`
//! 같은 바이너리 이름이 정당하게 등장한다.
//!
//! 선례: `tests/changelog_unreleased.rs` · `tests/plugin_manifest_version_parity.rs`.

use std::path::PathBuf;

const DOC: &str = "docs/architecture/index.md";
const CRATES_DIR: &str = "crates";
const SECTION_HEADER: &str = "## 워크스페이스 크레이트 (";

/// 레포 루트 — 이 크레이트가 `crates/` 아래 살아서 `CARGO_MANIFEST_DIR` 이 레포 루트가
/// 아니다. 해석과 검증을 [`tasty_doc_guards::repo_root`] 한 곳에 모은다(ADR-0138).
fn root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `crates/` 바로 아래에서 `Cargo.toml` 을 가진 디렉토리 이름을 정렬해 돌려준다.
/// workspace `exclude` 여부와 무관하게 디렉토리 기준이다 — 문서의 수치 정의와 같다.
fn crate_dir_names() -> Vec<String> {
    let dir = root().join(CRATES_DIR);
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn section_count(doc: &str) -> usize {
    let line = doc
        .lines()
        .find(|line| line.starts_with(SECTION_HEADER))
        .unwrap_or_else(|| panic!("{DOC} must contain a `{SECTION_HEADER}N)` header"));
    let digits = line[SECTION_HEADER.len()..].trim_end_matches(')');
    digits
        .parse()
        .unwrap_or_else(|e| panic!("{DOC}: crate count in `{line}` is not a number: {e}"))
}

#[test]
fn every_crate_directory_is_listed() {
    let doc = read(DOC);
    let names = crate_dir_names();
    assert!(!names.is_empty(), "{CRATES_DIR}/ has no crate directories");
    let missing: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !doc.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "{DOC} does not list these crates (write each as `name` in the workspace crate section): {missing:?}"
    );
}

#[test]
fn stated_crate_count_matches_directories() {
    let doc = read(DOC);
    let actual = crate_dir_names().len();
    let stated = section_count(&doc);
    assert_eq!(
        stated, actual,
        "{DOC}: `{SECTION_HEADER}{stated})` but {CRATES_DIR}/ holds {actual} crates"
    );
    let overview = format!("{actual} 개 크레이트(`crates/*`)");
    assert!(
        doc.contains(&overview),
        "{DOC}: the overview sentence must say `{overview}` ({CRATES_DIR}/ holds {actual} crates)"
    );
}
