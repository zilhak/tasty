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

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _` 무시는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};

/// 스캔에서 제외할 파일(repo-relative). 현재 비어 있다 — 규칙 본문은 금지 형태를
/// 행 시작 목록으로 쓰지 않는 방식으로 이 가드를 통과하므로 등록할 파일이 없다.
/// 체크박스를 **담는 것이 본질** 인 파일(마크다운 렌더 테스트 픽스처 등)이 docs 에
/// 생기면 여기에 등록한다.
const ALLOWLIST_FILES: &[&str] = &[];

/// 순회가 실제로 `docs/` 를 봤음을 보장하는 하한 — 값 하나가 아니라 **무엇의 함수인지**와
/// 함께 선언한다. 이 형태와 그 이유는 `tasty_doc_guards::floored_walk` 에 있다.
const DOCS_FLOOR: Floor = Floor {
    min: 250,
    measured: 380,
    measured_on: "2026-09-06",
    why_this_gap: "이 모수는 `docs/` 아래 `.md` 문서의 수다. 문서는 ADR 이 쌓이면서 단조 \
                   증가해 왔고, 한 번에 수십 개가 사라지는 변경은 없었다 — 그래서 여유를 \
                   좁게 둔다. 넓게 두면 순회가 절반 죽어도 통과하는데, 이 가드가 겨냥하는 \
                   사고가 정확히 그것(순회 루트 오타 · 재귀 중단)이라 넓은 여유는 가드를 \
                   자기 목적에서 멀어지게 한다.",
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

/// `docs/` 아래 스캔 대상을 모은다.
///
/// **가지치기가 없다.** 순회 루트가 `docs/` 하나이고 빌드 산출물도 로컬 작업 폴더도
/// 전부 그 밖에 있다. 죽은 가지는 코드가 없는 것보다 나쁘다 — 읽는 사람에게 "이
/// 가드는 그 경우를 고려했다" 는 거짓 안심을 주면서 그 판정은 한 번도 돌지 않는다.
///
/// **그 사실은 여기 적힌 문장이 아니라 `docs_holds_no_prunable_directory` 가 지킨다.**
/// 한때 이름 기반 가지치기가 이 자리에 있었고 지울 때 근거로 쓴 것은 "한 번도 참이
/// 된 적이 없다" 였다 — 그 값은 적는 순간 낡고, 낡아도 아무도 모른다. 그래서 값을
/// 문장으로 남기지 않고 재는 법을 테스트로 남긴다: `docs/` 아래에 가지쳐야 할
/// 디렉토리가 처음 생기는 날 이 순회는 그것을 그대로 들여다보고 그 아래 `.md` 까지
/// 검사 대상으로 삼는데, 그날 빨개지는 것은 이 주석이 아니라 그 테스트다.
///
/// 하한은 공용 순회가 강제한다 — 여기서 빠뜨릴 수 없고, 실패문도 거기서 나온다.
fn gather_docs(root: &Path) -> Result<Vec<PathBuf>, String> {
    walk_with_floor(&root.join("docs"), &DOCS_FLOOR, Descend::Everything, &|p| {
        is_checkbox_doc(&rel_of(p, root))
    })
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
    let files = gather_docs(root).unwrap_or_else(|why| panic!("{why}"));

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

/// `docs/` 아래에 있으면 안 되는 디렉토리 이름 — 빌드 산출물과 워크트리·git 내부.
///
/// **이 목록은 가지치기를 하지 않는다.** `gather` 가 무가지 순회라는 사실을 지키기
/// 위해서만 존재한다 — 값이 아니라 재는 법이다.
const PRUNABLE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 작업 폴더 이름의 조각. 리터럴로 두면 이 파일이 비-git 경로 참조
/// 금지(`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`) 를 어긴다 — 인용이
/// 아니라 판정 입력이지만, 조각으로 조립하면 예외 등록 없이 규칙을 지킬 수 있다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 디렉토리 이름이 순회에서 가지쳐야 할 것인지 — 빌드 산출물 이름 또는 선행 `.` 이
/// 붙은 로컬 작업 폴더. 두 갈래는 서로 다른 축이라 둘 다 대조가 필요하다.
fn is_prunable_dir(name: &str) -> bool {
    PRUNABLE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

/// `docs_root` 하위를 순회하며 `is_prunable_dir` 이 참인 디렉토리를 모은다.
/// 가지친 자리 아래로도 계속 내려간다 — 세는 것이 목적이지 자르는 것이 아니다.
fn prunable_dirs_under(docs_root: &Path, rel_root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(docs_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if is_prunable_dir(&name) {
            out.push(rel_of(&path, rel_root));
        }
        prunable_dirs_under(&path, rel_root, out);
    }
}

#[test]
fn docs_holds_no_prunable_directory() {
    let root = &tasty_doc_guards::repo_root();
    let mut found = Vec::new();
    prunable_dirs_under(&root.join("docs"), root, &mut found);
    found.sort();
    assert!(
        found.is_empty(),
        "docs/ 아래에 순회에서 가지쳐야 할 디렉토리가 있다 — 이 가드의 순회(`gather`)는 \
         가지치기를 하지 않으므로 그 안의 `.md` 까지 체크박스 검사 대상이 된다.\n\
         셋 중 하나를 골라라. (1) 그 디렉토리를 `docs/` 밖으로 옮긴다. (2) 그 안의 문서도 \
         검사 대상이 맞다면 이 테스트를 지우고 `gather` 주석의 무가지 근거를 다시 쓴다. \
         (3) 검사 대상이 아니라면 `gather` 에 가지치기를 되살리고 이 테스트를 그 가지의 \
         대조로 바꾼다.\n\
         PRUNABLE_DIRS 에서 이름을 빼거나 이 테스트를 지워서 통과시키지 마라 — 그러면 \
         `gather` 가 가지 없이 도는 것이 옳다는 사실을 지키는 것이 아무것도 안 남는다.\n\
         빨간 경로가 네 변경과 무관해 보이면 먼저 만든 쪽을 찾아라 — 이 검사는 레포 실물 \
         `docs/` 를 보고, 함께 도는 다른 테스트 바이너리가 거기에 디렉토리를 만들면 그 \
         타깃의 뒷정리 누락이 여기서 빨개진다: \
         grep -rn --include='*.rs' create_dir tests crates | grep docs\n{}",
        found.join("\n")
    );
}

#[test]
fn the_prunable_check_reacts_to_a_planted_tree() {
    // 술어 축 — 두 갈래가 각각 산다.
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

    // 순회 축 — 실제 디렉토리를 심어서 부른다. 술어만 부르면 순회가 죽어도 초록이다.
    let stamp = format!(
        "tasty-checkbox-guard-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let base = std::env::temp_dir().join(stamp);
    let docs = base.join("docs");
    std::fs::create_dir_all(docs.join("guide").join("target").join("deep")).unwrap();
    std::fs::create_dir_all(docs.join(format!(".{LOCAL_HEAD}"))).unwrap();
    std::fs::create_dir_all(docs.join("adr")).unwrap();

    let mut found = Vec::new();
    prunable_dirs_under(&docs, &base, &mut found);
    found.sort();
    // 정리 실패는 무시한다 — 판정은 위에서 이미 끝났고, 여기서 `?` 나 `unwrap` 을
    // 쓰면 임시 디렉토리 삭제 실패가 가드의 빨강으로 둔갑한다. 남아도 임시 경로다.
    let _ = std::fs::remove_dir_all(&base);

    assert_eq!(
        found,
        vec![
            format!("docs/.{LOCAL_HEAD}"),
            "docs/guide/target".to_string()
        ],
        "심은 트리에서 가지칠 디렉토리를 정확히 두 개 집어야 한다 — 얕은 자리만 보거나 \
         평범한 디렉토리까지 집으면 이 목록이 달라진다"
    );
}
