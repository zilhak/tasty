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

/// 체크박스를 금지할 문서인지 — `docs/` 하위 `.md` 전부.
///
/// 이름을 물음으로 적는다: 다른 가드의 같은 이름 `is_scan_target` 들과 grep 에서 뭉쳐
/// 보이지만, 이 가드의 물음은 "체크박스 검사 대상 문서인가" 로 그들과 다르다 — 다른
/// 물음이라 위임할 정본이 없다(ADR-0180: 같은 이름 다른 물음은 이름을 갈라 세운다).
fn is_checkbox_doc(rel: &str) -> bool {
    rel.starts_with("docs/") && rel.ends_with(".md")
}

/// `path` 하위를 재귀 순회하며 스캔 대상 파일을 모은다.
///
/// **가지치기가 없다 — 이 자리에는 가지칠 것이 원리적으로 없다.** 순회 루트가
/// `docs/` 하나이고 빌드 산출물도 gitignored 로컬 폴더도 전부 그 밖에 있다.
/// 한때 이름 기반 가지치기가 여기에도 있었으나 **한 번도 참이 된 적이 없었다**
/// (2026-09-06 실측: 순회 중 가지쳐진 디렉토리 0 개). 죽은 가지는 코드가 없는
/// 것보다 나쁘다 — 읽는 사람에게 "이 가드는 그 경우를 고려했다" 는 거짓 안심을
/// 주면서 그 판정은 한 번도 돌지 않는다.
///
/// **이 단정이 깨지는 날: `docs/` 아래에 빌드 산출물이나 gitignored 폴더가 처음
/// 생기는 날.** 그날 이 순회는 그것을 그대로 들여다본다. 그때 가지치기를 되살릴지
/// 아니면 그 안의 `.md` 도 검사 대상이 맞는지를 그 자리에서 다시 판단한다 —
/// 지금 미리 넣어두면 그날까지 또 죽어 있다.
fn gather(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        let rel = rel_of(path, root);
        if is_checkbox_doc(&rel) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        gather(&entry.path(), root, out);
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
