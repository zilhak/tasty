//! `docs/architecture/index.md` 의 크레이트 열거가 `crates/*/` 와 어긋나지 않는지
//! 검증한다.
//!
//! 아키텍처 문서는 에이전트가 크레이트의 역할을 찾는 첫 진입점이라, 목록에 없는
//! 크레이트는 존재 자체가 발견되지 않는다. 수치와 목록이 손으로 복제되어 있어
//! 크레이트가 추가될 때 문서가 따라오지 않는 drift 가 반복됐다 — 이 테스트가
//! 그 갱신을 강제한다. 이 타깃은 `doc-guards.yml` 이 main push · PR 마다 돌리고
//! `check-headless` 의 전체 스위트에서도 돈다(`docs/dev-guide/ci-gates.md`). 자동 잡은
//! push 된 커밋만 보므로, 크레이트를 추가·삭제했으면 커밋 전에 직접 돌려라.
//!
//! 검사 4 종:
//! - 모든 `crates/<name>/Cargo.toml` 의 `<name>` 이 문서에 `` `<name>` `` 형태로
//!   등장한다(번들 plugin 도 축약 없이 풀네임).
//! - 절 제목 `## 워크스페이스 크레이트 (N)` 의 N 이 디렉토리 수와 같다.
//! - 개요 문장의 "N 개 크레이트(`crates/*`)" 도 같은 값이다.
//! - 레포 루트 `CLAUDE.md` "빌드" 절의 "`crates/*` N 개" 도 같은 값이다.
//!
//! 마지막 항목이 여기 있는 이유: 그 문장은 정본(이 문서)의 **복제본**인데 채널이
//! 없었다. `docs/dev-guide/build.md` 는 같은 자리에서 수를 아예 복제하지 않는 쪽을
//! 골랐지만(그 문서가 그 판단을 본문에 적어 두었다), `CLAUDE.md` 는 에이전트가 매
//! 세션 읽는 진입점이라 규모를 그 자리에서 알려주는 값이 필요하다. 그래서 복제를
//! 남기는 대신 대조를 붙인다.
//!
//! 역방향(문서에만 있고 디렉토리에 없는 이름)은 검사하지 않는다 — `tasty-tui-sim`
//! 같은 바이너리 이름이 정당하게 등장한다.
//!
//! **README 의 같은 수는 여기서 안 본다** — 배지와 본문이라 형태가 달라
//! `crates/tasty-doc-guards/tests/readme_badge_parity.rs` 가 본다. 좌변 함수는 공유한다.
//!
//! 선례: `tests/changelog_unreleased.rs` · `crates/tasty-doc-guards/tests/plugin_manifest_version_parity.rs`.

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

/// `crates/` 바로 아래에서 `Cargo.toml` 을 가진 디렉토리 이름 — 좌변의 정의와 빈 결과
/// 거부는 [`tasty_doc_guards::crate_layers::crate_dir_names`] 한 곳에 있다. 같은 좌변을
/// `readme_badge_parity` 도 쓰므로 여기서 다시 구현하지 않는다(두 벌이면 갈린다).
fn crate_dir_names() -> Vec<String> {
    tasty_doc_guards::crate_layers::crate_dir_names(&root())
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

/// `CLAUDE.md` 의 복제본은 정본과 문장 형태가 다르다 — 정본은 "N 개 크레이트(`crates/*`)"
/// 이고 여기는 어순이 뒤집힌 "`crates/*` N 개" 다. 그래서 같은 술어로 못 찾는다.
const CLAUDE_MD: &str = "CLAUDE.md";

#[test]
fn claude_md_crate_count_matches_directories() {
    let actual = crate_dir_names().len();
    let expected = format!("`crates/*` {actual} 개");
    let contents = read(CLAUDE_MD);
    assert!(
        contents.contains(&expected),
        "{CLAUDE_MD} 의 \"빌드\" 절이 `{expected}` 라고 말해야 한다 ({CRATES_DIR}/ 실측 \
         {actual}). 지금 그 형태로 적힌 줄:\n{}",
        contents
            .lines()
            .filter(|l| l.contains("`crates/*`"))
            .map(|l| format!("      {}", l.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
