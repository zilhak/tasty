//! docs/**/*.md의 Markdown 체크박스 목록을 금지한다.
//! docs/documentation-model.md의 작성 규칙에 따라 절차·검증 항목은 평문 목록으로 쓴다.
//! 행 시작의 목록 마커(-/*/+), 공백, [ ]/[x]/[X] 형태만 찾으며 코드펜스 안도 검사한다.
//! 인라인 언급과 링크는 대상이 아니다.

// 이유: 테스트의 값 무시를 제품 코드의 lint 목록에서 제외한다.
#![allow(clippy::let_underscore_must_use)]

use std::path::Path;
use tasty_doc_guards::floored_walk::{
    Descend, Floor, Pick, Walked, walk_dirs_with_floor, walk_with_floor,
};

/// 체크박스 자체가 필요한 렌더링 예제 등에만 파일 예외를 등록한다.
const ALLOWLIST_FILES: &[&str] = &[];

/// 문서 수집 실패로 빈 결과가 통과하지 않게 한다.
const DOCS_FLOOR: Floor = Floor {
    min: 162,
    measured: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.counted_on,
    why_this_gap: "실측 214개에서 가장 큰 비-ADR 분류인 docs/features의 52개만큼 여유를 둔다. \
                   한 분류를 통합하는 작업은 허용하되 더 큰 수집 누락은 실패시킨다. \
                   ADR 구성 방식이나 검사 범위를 바꾸면 모수와 하한을 함께 다시 검토한다.",
};

/// 행이 마크다운 체크박스 목록 항목으로 시작하는지.
/// 선행 공백 · 목록 마커 · 공백(1 개 이상) · `[` · (공백|x|X) · `]` 순서만 본다. `]` 뒤는 보지 않는다.
fn is_checkbox_item(line: &str) -> bool {
    let rest = line.trim_start();
    let Some(rest) = rest.strip_prefix(['-', '*', '+']) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(' ') else {
        return false;
    };
    let rest = rest.trim_start_matches(' ');
    let Some(rest) = rest.strip_prefix('[') else {
        return false;
    };
    let Some(rest) = rest.strip_prefix([' ', 'x', 'X']) else {
        return false;
    };
    rest.starts_with(']')
}

fn is_checkbox_doc(rel: &str) -> bool {
    rel.starts_with("docs/") && rel.ends_with(".md")
}

/// docs에는 제외할 빌드·로컬 폴더가 없다는 전제로 모든 하위 문서를 수집한다.
/// 그 전제는 docs_holds_no_prunable_directory에서 별도로 확인한다.
fn gather_docs(root: &Path) -> Result<Vec<Walked>, String> {
    walk_with_floor(
        &root.join("docs"),
        root,
        &DOCS_FLOOR,
        Descend::Everything,
        &|found| is_checkbox_doc(&found.rel),
    )
}

#[test]
fn no_checkbox_in_docs() {
    let root = &tasty_doc_guards::repo_root();
    let files = gather_docs(root).unwrap_or_else(|why| panic!("{why}"));

    let mut violations = Vec::new();
    for file in &files {
        let rel = &file.rel;
        if ALLOWLIST_FILES.contains(&rel.as_str()) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file.path) else {
            continue; // UTF-8로 읽지 못한 파일은 검사하지 않는다.
        };
        for (i, line) in contents.lines().enumerate() {
            if is_checkbox_item(line) {
                violations.push(format!("  {}:{} — `{}`", rel, i + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "docs에 Markdown 체크박스가 있다. 평문 불릿이나 번호 목록으로 바꾸고 미구현 범위는 Status와 본문에 적는다(docs/documentation-model.md §6). 체크박스 자체가 필요한 파일만 ALLOWLIST_FILES에 등록한다:\n{}",
        violations.join("\n")
    );
}

#[test]
fn checkbox_matcher_hits_only_line_start_list_items() {
    assert!(is_checkbox_item("- [ ] Given a When b Then c"));
    assert!(is_checkbox_item("- [x] done"));
    assert!(is_checkbox_item("* [X] done"));
    assert!(is_checkbox_item("+ [ ] item"));
    assert!(is_checkbox_item("  - [ ] nested"));
    assert!(is_checkbox_item("\t- [x] tab-indented"));
    assert!(is_checkbox_item("- [ ]"));
    assert!(is_checkbox_item("-  [ ] two spaces"));
    assert!(is_checkbox_item("-   [x] three spaces"));
    assert!(is_checkbox_item("*    [ ] four spaces"));

    assert!(!is_checkbox_item("- Given a When b Then c"));
    assert!(!is_checkbox_item(
        "1. [ ] numbered lists are not task lists"
    ));
    assert!(!is_checkbox_item(
        "- 목록 항목을 `[ ]`·`[x]` 로 시작하는 형식은 쓰지 않는다"
    ));
    assert!(!is_checkbox_item("- [link](target.md)"));
    assert!(!is_checkbox_item("- [xx] not a checkbox"));
    assert!(!is_checkbox_item("-[ ] no space after marker"));
    assert!(!is_checkbox_item("[ ] no list marker"));
    assert!(!is_checkbox_item(""));
}

/// 이 이름이 docs 아래에 생기면 제외 없는 순회가 여전히 적절한지 검토해야 한다.
const PRUNABLE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// 로컬 경로를 문서 인용으로 오인하지 않도록 판정용 이름을 조립한다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

fn is_prunable_dir(name: &str) -> bool {
    PRUNABLE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

/// 검사할 디렉터리 수의 하한이다. 제외 대상 디렉터리는 0개여야 한다.
const DOCS_DIR_FLOOR: Floor = Floor {
    min: 59,
    measured: 71,
    measured_on: "2026-09-23",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "cc2e5e72e에서 추적되는 docs 하위 디렉터리를 측정했다.",
    ),
    why_this_gap: "docs 자체를 제외한 하위 디렉터리 수다. cc2e5e72e에서 71개였고, plugins 하위 12개가 한 번에 정리되는 경우를 허용해 하한을 59로 정했다. features 전체의 제거는 문서 구조 변경이므로 재측정해야 한다. git ls-tree -r -d --name-only HEAD docs 결과에서 /를 포함한 행을 센다. 작업 트리에는 삭제 후 빈 디렉터리가 남을 수 있어 측정 근거는 Git 추적 목록으로 남긴다.",
};

/// 발견한 제외 대상 아래도 계속 검사한다.
/// 합성 테스트도 같은 순회를 쓰도록 하한을 인자로 받는다.
fn prunable_dirs_under(root: &Path, rel_root: &Path, floor: &Floor) -> Result<Vec<String>, String> {
    walk_dirs_with_floor(root, rel_root, floor, &|found| {
        let name = found
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if is_prunable_dir(&name) {
            Pick::Take
        } else {
            Pick::Skip
        }
    })
    .map(|dirs| dirs.into_iter().map(|d| d.rel).collect())
}

#[test]
fn docs_holds_no_prunable_directory() {
    let root = &tasty_doc_guards::repo_root();
    let found = prunable_dirs_under(&root.join("docs"), root, &DOCS_DIR_FLOOR)
        .unwrap_or_else(|why| panic!("{why}"));
    assert!(
        found.is_empty(),
        "docs/ 아래에 제외 대상으로 분류한 디렉터리가 있다. 현재 순회는 이 안의 Markdown도 검사한다. 디렉터리를 docs 밖으로 옮기거나, 포함·제외 정책을 정해 순회와 검사를 함께 고친다. 이름만 목록에서 지워 통과시키지 않는다. 다른 테스트가 만든 경로라면 해당 테스트의 정리 누락을 확인한다:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_prunable_check_reacts_to_a_planted_tree() {
    assert!(is_prunable_dir("target"), "빌드 산출물 이름을 안 잡는다");
    assert!(
        is_prunable_dir("node_modules"),
        "빌드 산출물 이름을 안 잡는다"
    );
    assert!(
        is_prunable_dir(&format!(".{LOCAL_HEAD}")),
        "선행 `.` 로컬 작업 폴더를 안 잡는다"
    );
    assert!(
        is_prunable_dir(&format!(".{LOCAL_HEAD}{LOCAL_TAIL}")),
        "선행 `.` 로컬 작업 폴더를 안 잡는다"
    );
    assert!(
        !is_prunable_dir("adr"),
        "평범한 docs 하위 디렉토리를 잡는다"
    );
    assert!(
        !is_prunable_dir("targets"),
        "이름이 겹치는 다른 디렉토리를 잡는다"
    );

    // 플랫폼의 시계 해상도에 의존하지 않는 이름을 만든다.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let stamp = format!(
        "tasty-checkbox-guard-probe-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let base = std::env::temp_dir().join(stamp);
    let docs = base.join("docs");
    std::fs::create_dir_all(docs.join("guide").join("target").join("deep")).unwrap();
    std::fs::create_dir_all(docs.join(format!(".{LOCAL_HEAD}"))).unwrap();
    std::fs::create_dir_all(docs.join("adr")).unwrap();

    // 같은 순회를 작은 합성 트리의 하한으로 검사한다.
    let probe_floor = Floor {
        min: 3,
        measured: 5,
        measured_on: "2026-09-06",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "작은 합성 트리의 하한이다. 트리 구성의 사소한 변경을 허용할 여유를 둔다.",
    };
    let found = prunable_dirs_under(&docs, &base, &probe_floor)
        .unwrap_or_else(|why| panic!("대조 트리 순회가 하한에 걸렸다: {why}"));
    // 임시 디렉터리 정리 실패가 검사 결과를 가리지 않게 한다.
    let _ = std::fs::remove_dir_all(&base);

    assert_eq!(
        found,
        vec![
            format!("docs/.{LOCAL_HEAD}"),
            "docs/guide/target".to_string()
        ],
        "합성 트리에서 제외 대상 디렉터리 두 개를 정확히 찾지 못했다"
    );
}
