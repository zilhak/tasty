//! docs 마크다운 체크박스 재유입 가드 — `docs/**/*.md` 에 마크다운 task list
//! 체크박스(`[ ]` / `[x]` 로 시작하는 목록 항목)가 다시 들어오면 fail 한다.
//!
//! 배경: `docs/documentation-model.md` §6 "작성 규칙 요약" 은 docs 문서에 체크박스를
//! 두지 않는다고 규정한다. 체크 상태(했다/안 했다)는 본질이 진행 추적이라 같은 절의
//! transient(빌드/로드맵 상태) 금지와 충돌하고, 실제로도 체크 상태에 정해진 의미가
//! 없어 정보를 담지 못한다. Acceptance Criteria 는 평문 `Given … When … Then …` 불릿,
//! 검증·절차 항목은 평문 불릿이나 번호 목록으로 적는다.
//!
//! **탐지 규칙 — 행 시작의 목록 마커만 본다.** 선행 공백 → 목록 마커(`-` / `*` / `+`)
//! → 공백(1 개 이상, GFM 이 같은 항목으로 렌더하는 2~4 개 포함) → `[` → 공백·`x`·`X` 한 글자 → `]` 로 시작하는 행이 대상이다. 부분문자열
//! 검사는 인라인 설명("`[x]` 접두를 쓰지 않는다")이나 링크 텍스트까지 오탐하므로
//! 쓰지 않는다. 규칙 본문(documentation-model · 템플릿 주석 · `CLAUDE.md`)은 금지
//! 형태를 인라인 백틱으로만 언급하고 행 시작 목록으로는 쓰지 않아 allowlist 가
//! 필요 없다.
//!
//! **코드 펜스 안팎을 구분하지 않는다.** 규칙이 "docs 문서에 체크리스트를 넣지
//! 않는다" 이므로 펜스 안의 예시도 금지 대상이다.
//!
//! 선례: `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`(docs 스캔 구조) · `tests/no_emoji_in_source.rs`.

use std::path::{Path, PathBuf};

/// 스캔에서 제외할 파일(repo-relative). 현재 비어 있다 — 규칙 본문은 금지 형태를
/// 행 시작 목록으로 쓰지 않는 방식으로 이 가드를 통과하므로 등록할 파일이 없다.
/// 체크박스를 **담는 것이 본질** 인 파일(마크다운 렌더 테스트 픽스처 등)이 docs 에
/// 생기면 여기에 등록한다.
const ALLOWLIST_FILES: &[&str] = &[];

/// 순회에서 통째로 가지치기할 디렉토리명. 빌드 산출물·워크트리·VCS·의존성 +
/// gitignored 로컬 작업 폴더(worktree 에서는 레포 밖으로 향하는 심볼릭 링크일 수 있다).
const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 폴더 이름의 조각. 리터럴로 두면 이 파일이 비-git 경로 참조 금지
/// (`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`) 를 어긴다 — 인용이
/// 아니라 순회 입력이지만, 조각으로 조립하면 예외 등록 없이 규칙을 지킬 수 있다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 가지치기 대상 디렉토리인지 — 빌드 산출물 + gitignored 로컬 폴더(선행 `.`).
fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

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
    // GFM 은 마커 뒤 공백 1~4 개를 모두 같은 목록 항목으로 렌더한다 — 남은 공백도 흡수.
    let rest = rest.trim_start_matches(' ');
    let Some(rest) = rest.strip_prefix('[') else {
        return false;
    };
    let Some(rest) = rest.strip_prefix([' ', 'x', 'X']) else {
        return false;
    };
    rest.starts_with(']')
}

/// 스캔 대상 파일인지 — repo-relative 경로 기준. `docs/` 하위 `.md` 전부.
fn is_scan_target(rel: &str) -> bool {
    rel.starts_with("docs/") && rel.ends_with(".md")
}

/// `path` 하위를 재귀 순회하며 스캔 대상 파일을 모은다. `is_pruned` 는 가지치기.
fn gather(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        let rel = rel_of(path, root);
        if is_scan_target(&rel) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // ★ **이름으로 하는 가지치기는 종류를 묻지 않는다.** worktree 에서 `.git` 은
        // 디렉토리가 아니라 `gitdir:` 한 줄이 든 **파일**이다 — 종류를 먼저 물으면
        // 그 파일이 가지치기를 빠져나가 모집단에 들고, 같은 커밋이 worktree 와 메인
        // 체크아웃에서 서로 다른 파일을 보게 된다. 모집단이 환경을 읽으면 답도
        // 언젠가 환경을 읽는다.
        if is_pruned(name) {
            continue;
        }
        gather(&p, root, out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn no_checkbox_in_docs() {
    let root = &tasty_doc_guards::repo_root();
    let mut files = Vec::new();
    gather(&root.join("docs"), root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "docs/ 아래 스캔 대상 .md 가 하나도 없다 — 순회 경로가 잘못됐다"
    );

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        if ALLOWLIST_FILES.contains(&rel.as_str()) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue; // 비-UTF8 은 마크다운 문서가 아니다.
        };
        for (i, line) in contents.lines().enumerate() {
            if is_checkbox_item(line) {
                violations.push(format!("  {}:{} — `{}`", rel, i + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "docs 문서에 마크다운 체크박스(task list) 목록 항목이 있다 — docs 는 현재 상태만 \
         기술하며 체크 상태(했다/안 했다)는 진행 추적이라 두지 않는다 \
         (docs/documentation-model.md §6 \"작성 규칙 요약\").\n\
         체크박스 접두를 떼고 평문 불릿(`- Given … When … Then …`)이나 번호 목록으로 쓸 것. \
         미구현 범위는 Status 줄과 본문 문장으로 적는다.\n\
         체크박스를 담는 것이 본질인 파일이면 ALLOWLIST_FILES 에 추가:\n{}",
        violations.join("\n")
    );
}

#[test]
fn checkbox_matcher_hits_only_line_start_list_items() {
    // 잡아야 하는 형태 — 마커 3 종, 체크 상태 3 종, 선행 공백, 중첩.
    assert!(is_checkbox_item("- [ ] Given a When b Then c"));
    assert!(is_checkbox_item("- [x] done"));
    assert!(is_checkbox_item("* [X] done"));
    assert!(is_checkbox_item("+ [ ] item"));
    assert!(is_checkbox_item("  - [ ] nested"));
    assert!(is_checkbox_item("\t- [x] tab-indented"));
    assert!(is_checkbox_item("- [ ]"));
    // 마커 뒤 공백 2~4 개 — GFM 은 같은 task item 으로 렌더하므로 회피 형태가 아니다.
    assert!(is_checkbox_item("-  [ ] two spaces"));
    assert!(is_checkbox_item("-   [x] three spaces"));
    assert!(is_checkbox_item("*    [ ] four spaces"));

    // 잡지 않아야 하는 형태 — 평문 불릿, 번호 목록, 인라인 언급, 링크, 마커 없음.
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
