//! 휘발성 로컬 문서 인용 재유입 가드 — 커밋되는 파일이 git 에 올라가지 않는
//! 로컬 작업 문서를 인용하면 fail 한다.
//!
//! 배경: `CLAUDE.md` "소스 주석의 TODO 파일 및 디자인 changelog 인용 금지" 와
//! `docs/adr/template.md` "외부(비-git) 위치 문서 참조 금지" 가 이미 규정한 것을
//! 실제로 강제한다. 로컬 작업 폴더는 `.gitignore` 대상이라 커밋되지 않고 완료된
//! 항목은 관례상 파일 자체가 삭제되므로, 그 번호·경로는 **로컬 세션에서만 유효한
//! 휘발성 식별자**다. 저장소를 새로 clone 한 사람에게 그 좌표는 존재한 적이 없다.
//!
//! 실제로 번호 재사용이 일어나 인용이 *무관한 문서* 로 해석되는 사례까지 나왔다 —
//! 죽은 참조를 넘어 오도하는 참조가 된다. 같은 문제를 진단한 선례는
//! [ADR-0027](../docs/adr/0027-figma-planning-sot-naming-derived-index.md) 의
//! "세션/트랙 식별자 누수" 항목이다.
//!
//! **대체 수단(위반 시 이 중 하나를 쓴다)**:
//! 1. 이유가 자명하면 — 번호/경로 대신 이유를 주석에 직접 서술
//! 2. 설계 결정이 크면 — `docs/adr/` 에 ADR 을 쓰고 그 경로를 인용
//! 3. 기능 동작 설명이면 — `docs/`(dev-guide / features / plugins) 문서를 참조
//!
//! **탐지 패턴 5 종** (하나만 잡는 정규식으로는 절반도 못 거른다):
//! - P1 번호 인용 — 대문자 `TODO` + 선택적 공백/하이픈 + 숫자
//! - P2 conductor 번호 인용 — `todo-conductor`(대소문자 무시) + 선택적 구분자 + 숫자
//! - P3 경로 인용 — 로컬 작업 폴더 + `todo` / `todo-conductor` / `plans` / `conductor`
//! - P4 디자인 changelog slug — `YYYY-MM-DD-<소문자-slug>`. 원격 Claude Design
//!   프로젝트 내부에만 존재해 로컬 파일시스템에 흔적조차 없으므로 더 휘발적이다.
//! - P5 앵커 슬러그 변형 — 마크다운 앵커(`#...`) 안에 소문자로 굳은 번호
//!   (`-todo-<숫자>`). 제목에 번호를 달면 그 번호가 앵커의 일부가 되어 링크·주석
//!   으로 퍼지고, 나중에 제목에서 번호를 떼는 순간 그 참조가 **전부 깨진다**
//!   (실제로 한 번 발생했다 — 제목 하나를 고치자 링크 1곳과 주석 15줄이 죽은
//!   좌표가 됐다). 대문자 `TODO` 가 아니라 P1 이 못 잡는 별도 형태다.
//!
//! **오탐 회피**: 금지 대상은 로컬 작업 폴더 뒤에 위 4 개 하위 디렉토리가 오는
//! 경우로 **한정**한다. 임시 파일 위치(`temp/` 하위)와 폴더 자체를 언급하는 것은
//! `CLAUDE.md` 가 오히려 규정한 정당한 사용이므로 잡지 않는다. P4 는 도입 시점에
//! 레포 전체를 스캔해 규칙 본문(`CLAUDE.md`) 외 오탐 0 건을 확인하고 채택했다.
//!
//! 선례: `tests/no_emoji_in_source.rs`(구조 템플릿) · `tests/design_token_adherence.rs`.

use std::path::{Path, PathBuf};

/// 스캔에서 제외할 파일(repo-relative) — 금지 형태를 **담는 것이 본질** 인 곳.
/// - `docs/adr/template.md`: "외부(비-git) 위치 문서 참조 금지" 규칙 본문의 거처.
/// - `docs/adr/0027-...`: 휘발 경로 누수를 *문제로 서술* 하는 예시(참조가 아니다).
///   게다가 Accepted ADR 의 Context 본문이라 template 규칙상 수정 대상도 아니다.
/// - `tests/no_emoji_in_source.rs`: 가지치기 목록에 로컬 작업 폴더명을 보유.
/// - 이 파일: 위 패턴을 설명·구현한다.
///
/// `CLAUDE.md`(규칙 본문이 금지 형태를 예시 인용) 와 `.gitignore`(제외 항목 그
/// 자체) 는 애초에 스캔 대상이 아니라 별도 항목이 필요 없다.
const ALLOWLIST_FILES: &[&str] = &[
    "docs/adr/template.md",
    "docs/adr/0027-figma-planning-sot-naming-derived-index.md",
    "tests/no_emoji_in_source.rs",
    "tests/no_todo_file_citation.rs",
];

/// 순회에서 통째로 가지치기할 디렉토리명. 빌드 산출물·워크트리·VCS·의존성 +
/// gitignored 로컬 작업 폴더(그 안의 문서는 커밋 대상이 아니라 스캔 의미가 없다).
///
/// 로컬 작업 폴더는 worktree 에 **심볼릭 링크**로 걸려 있을 수 있다. `is_dir()` 은
/// 링크를 따라가므로, 가지치기하지 않으면 순회가 레포 밖 실제 경로까지 새어나간다.
const PRUNE_DIRS: &[&str] = &[
    "target",
    "dist",
    ".worktree",
    ".git",
    "node_modules",
    ".claude",
    ".claude-workspace",
];

/// 로컬 작업 폴더명. 이 파일 자신이 P3 에 걸리지 않도록 조각으로 두고 조립한다.
const WORKSPACE_DIR: &str = "claude-workspace/";

/// 금지되는 하위 디렉토리 — 이 넷 뒤에 오는 경로만 잡는다(`temp/` 등은 정당).
const FORBIDDEN_SUBDIRS: &[&str] = &["todo-conductor", "todo", "plans", "conductor"];

/// P1 — 대문자 `TODO` + 선택적 공백/하이픈 + 숫자. 번호 없는 평범한 `TODO:` 주석은
/// 대상이 아니다(금지 대상은 *파일 번호 인용* 이지 할 일 표시가 아니다).
fn find_p1(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(pos) = line[from..].find("TODO") {
        let start = from + pos;
        let mut i = start + 4;
        if i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'-') {
            i += 1;
        }
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            let mut end = i;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            return Some(line[start..end].to_string());
        }
        from = start + 4;
    }
    None
}

/// P2 — `todo-conductor`(대소문자 무시) + 선택적 구분자 + 숫자.
fn find_p2(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let needle = "todo-conductor";
    let mut from = 0;
    while let Some(pos) = lower[from..].find(needle) {
        let start = from + pos;
        let mut i = start + needle.len();
        if i < bytes.len() && matches!(bytes[i], b' ' | b'/' | b'_' | b'-') {
            i += 1;
        }
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            let mut end = i;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            return Some(line[start..end].to_string());
        }
        from = start + needle.len();
    }
    None
}

/// P3 — 로컬 작업 폴더 + 금지 하위 디렉토리. `temp/` 하위나 폴더 단독 언급은 통과.
fn find_p3(line: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = line[from..].find(WORKSPACE_DIR) {
        let start = from + pos;
        let after = &line[start + WORKSPACE_DIR.len()..];
        if let Some(sub) = FORBIDDEN_SUBDIRS.iter().find(|s| after.starts_with(**s)) {
            return Some(format!("{WORKSPACE_DIR}{sub}"));
        }
        from = start + WORKSPACE_DIR.len();
    }
    None
}

/// P4 — 디자인 changelog 판정 slug(`YYYY-MM-DD-<소문자-slug>`).
fn find_p4(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let is_d = |i: usize| i < bytes.len() && bytes[i].is_ascii_digit();
    let is_dash = |i: usize| i < bytes.len() && bytes[i] == b'-';
    for start in 0..bytes.len() {
        // 앞 글자가 숫자면 연도 4 자리의 시작이 아니다(더 긴 숫자열의 중간).
        if start > 0 && bytes[start - 1].is_ascii_digit() {
            continue;
        }
        if !(is_d(start) && is_d(start + 1) && is_d(start + 2) && is_d(start + 3)) {
            continue;
        }
        if !(is_dash(start + 4) && is_d(start + 5) && is_d(start + 6)) {
            continue;
        }
        if !(is_dash(start + 7) && is_d(start + 8) && is_d(start + 9)) {
            continue;
        }
        if !is_dash(start + 10) {
            continue;
        }
        let mut end = start + 11;
        if !(end < bytes.len() && bytes[end].is_ascii_lowercase()) {
            continue;
        }
        while end < bytes.len() && (bytes[end].is_ascii_lowercase() || bytes[end] == b'-') {
            end += 1;
        }
        return Some(line[start..end].to_string());
    }
    None
}

/// 앵커 슬러그 안의 소문자 번호(`todo-<숫자>`)를 찾는다. 슬러그 시작이거나 `-`
/// 뒤에 와야 한다 — `todo-conductor` 는 뒤가 숫자가 아니라 걸리지 않는다.
fn slug_todo_number(slug: &str) -> Option<String> {
    let bytes = slug.as_bytes();
    let mut from = 0;
    while let Some(pos) = slug[from..].find("todo-") {
        let start = from + pos;
        // `start - 1` 이 멀티바이트 연속 바이트여도 `-`(ASCII) 와는 절대 같지 않다.
        let at_boundary = start == 0 || bytes[start - 1] == b'-';
        let digits_start = start + "todo-".len();
        let mut end = digits_start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if at_boundary && end > digits_start {
            return Some(slug[start..end].to_string());
        }
        from = start + "todo-".len();
    }
    None
}

/// P5 — 마크다운 앵커(`#<슬러그>`) 안에 소문자로 굳은 번호.
fn find_p5(line: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = line[from..].find('#') {
        let start = from + pos;
        let rest = &line[start + 1..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '`' | ',' | '"' | '\''))
            .map(|i| start + 1 + i)
            .unwrap_or(line.len());
        let slug = &line[start + 1..end];
        if let Some(hit) = slug_todo_number(slug) {
            return Some(format!("#{slug} ({hit})"));
        }
        from = start + 1;
    }
    None
}

/// 스캔 대상 파일인지 — repo-relative 경로 기준.
/// `.rs`(`src/` · `tests/` · `crates/*/src/` · `crates/*/tests/`) +
/// `.toml`(루트 `Cargo.toml`/`deny.toml` · `crates/*/Cargo.toml` · `tasty-plugin.toml` ·
/// `src/**/*.toml`) + `docs/**/*.md` + 어디에 있든 `CHANGELOG.md`.
fn is_scan_target(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or("");
    if name == "CHANGELOG.md" || name == "tasty-plugin.toml" {
        return true;
    }
    if rel.starts_with("docs/") && rel.ends_with(".md") {
        return true;
    }
    if rel == "Cargo.toml" || rel == "deny.toml" {
        return true;
    }
    let crate_sub = |suffix: &str| -> bool {
        rel.strip_prefix("crates/")
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(_name, after)| {
                if suffix == "Cargo.toml" {
                    after == suffix
                } else {
                    after.starts_with(suffix)
                }
            })
    };
    if rel.ends_with(".rs") {
        return rel.starts_with("src/")
            || rel.starts_with("tests/")
            || crate_sub("src/")
            || crate_sub("tests/");
    }
    if rel.ends_with(".toml") {
        return rel.starts_with("src/") || crate_sub("Cargo.toml");
    }
    false
}

/// `path` 하위를 재귀 순회하며 스캔 대상 파일을 모은다. PRUNE_DIRS 는 가지치기.
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
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if PRUNE_DIRS.contains(&name) {
                continue;
            }
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
fn no_todo_file_citation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    gather(root, root, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        if ALLOWLIST_FILES.contains(&rel.as_str()) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue; // 비-UTF8 은 인용을 담을 수 없다.
        };
        for (i, line) in contents.lines().enumerate() {
            let hit = find_p1(line)
                .map(|m| ("P1 번호 인용", m))
                .or_else(|| find_p2(line).map(|m| ("P2 conductor 번호 인용", m)))
                .or_else(|| find_p3(line).map(|m| ("P3 경로 인용", m)))
                .or_else(|| find_p4(line).map(|m| ("P4 디자인 changelog slug", m)))
                .or_else(|| find_p5(line).map(|m| ("P5 앵커 슬러그 번호", m)));
            if let Some((kind, matched)) = hit {
                violations.push(format!("  {}:{} — {kind}: `{matched}`", rel, i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "커밋되는 파일이 git 에 올라가지 않는 로컬 작업 문서(로컬 작업 폴더의 todo / \
         todo-conductor / plans / conductor)나 디자인 changelog slug 를 인용했다 — 그 좌표는 \
         clone 한 사람에게 존재한 적이 없고, 번호는 재사용되어 무관한 문서로 해석된다.\n\
         대체 수단 3 가지 중 하나를 쓸 것: (1) 이유가 자명하면 번호 대신 이유를 직접 서술 \
         (2) 설계 결정이 크면 `docs/adr/` 에 ADR 을 쓰고 그 경로를 인용 \
         (3) 기능 동작 설명이면 `docs/`(dev-guide / features / plugins) 문서를 참조.\n\
         앵커(P5)면 제목에서 번호를 떼고 그 제목을 가리키던 참조도 함께 고칠 것 — \
         제목의 번호는 앵커로 굳어 링크·주석으로 퍼진다.\n\
         금지 형태를 담는 것이 본질인 파일이면 ALLOWLIST_FILES 에 추가:\n{}",
        violations.join("\n")
    );
}
