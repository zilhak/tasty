//! `let _ =` 사유 주석 전수 가드 — 프로덕션 코드에서 이유 없이 값을 버리면 fail 한다.
//!
//! 배경: `CLAUDE.md` "에러 처리 (필수)" 와
//! [`docs/dev-guide/error-handling.md`](../docs/dev-guide/error-handling.md) 는
//! "무시는 명시적 정책 결정이어야 하고 그 이유가 주석으로 남는다" 를 요구하는데,
//! 그걸 **전수로** 보는 검사가 지금까지 없었다. 세 층 중 어느 것도 못 본다:
//!
//! - `clippy::let_underscore_must_use` 는 타입은 정확히 보지만 **주석을 모른다** —
//!   사유가 제대로 달린 정상 코드까지 경고하므로 경고 총량이 크고, 신규 위반 한 건이
//!   그 안에 묻힌다.
//! - `.githooks/pre-commit` C.6 은 주석은 보지만 **staged diff 의 추가 라인만** 본다.
//! - 리뷰는 사람이라 전수가 아니다.
//!
//! ## 판정 규칙
//!
//! `let _ =` 한 건마다, **문장 범위 + 그 다음 한 줄** 안에 `//` 주석이 있거나,
//! **바로 앞의 유의미한 줄**(빈 줄과 `#[..]` 속성은 건너뛴다)이 `//` 주석이면 통과다.
//! 문장 범위는 `let` 부터 괄호 깊이가 0 으로 돌아오며 `;` 가 나오는 줄까지다.
//!
//! 이 규칙은 pre-commit C.6 이 인정하는 세 형태(같은 줄 · 윗줄 · 다음 줄)를 **모두
//! 포함하고 조금 더 넓다**(빈 줄·속성 건너뛰기, 멀티라인 문장 내부 주석). 방향이
//! 중요하다 — 훅이 통과시킨 코드를 CI 가 떨어뜨리면 안 되므로 가드가 훅보다 좁아서는
//! 안 된다. 반대 방향(가드가 더 넓다)은 훅이 먼저 막을 뿐이라 안전하다.
//!
//! **블록 상단 주석은 인정하지 않는다.** 몇 줄 위 블록 머리에 적힌 설명을 사유로
//! 인정하려면 "어디까지 거슬러 올라갈 것인가" 를 정해야 하는데, 함수 doc 주석까지
//! 닿을 만큼 넓히면 사실상 모든 `let _ =` 가 통과해 가드가 가드를 그만둔다. 사유는
//! 그 문장 옆에서 읽혀야 한다 — 위에 이미 적었더라도 한 줄을 더 적는 비용이
//! 가드를 유지하는 값보다 싸다.
//!
//! ## 대상 범위
//!
//! 테스트 코드는 제외한다 — `tests/`·`benches/` 디렉토리 아래 전부와, 남은 파일
//! 안의 `#[cfg(test)]` / `#[test]` 아이템 본문, `#![cfg(test)]` 파일 전체다. 테스트에서
//! 값을 버리는 것은 그 자체가 의도라 사유가 자명하고, 여기까지 강제하면 통과시키기
//! 위한 형식적 주석만 늘어난다. 정책이 지키려는 것은 **프로덕션에서 조용히 사라지는
//! 실패**다.
//!
//! 규칙을 "Result 만" 이 아니라 **모든 `let _ =`** 로 넓힌 이유는 텍스트 스캔이
//! 타입을 알 수 없기 때문이다. 변수 바인딩 억제(`let _ = path;`)도 "왜 여기서
//! 안 쓰는가" 가 읽는 사람에게 궁금한 지점이라 한 줄 주석이 손해가 아니다.

use std::fs;
use std::path::{Path, PathBuf};

/// 금지 형태를 담는 것이 본질인 파일. **현재 비어 있다** — 이 가드 자신도 예외가
/// 아니어서, 헬퍼 테스트의 픽스처는 금지 형태를 소스에 그대로 적지 않고 런타임에
/// 조립한다([`ignore_stmt`]). `.githooks/pre-commit` C.6 에는 면제 장치가 없으므로
/// 어차피 그렇게 해야 하고, 덕분에 가드가 자기 자신도 검사한다.
const ALLOWLIST: &[&str] = &[];

const PRUNE_DIRS: &[&str] = &[
    "target",
    "dist",
    "_site",
    ".worktree",
    ".git",
    ".idea",
    "node_modules",
    "assets",
];

/// 테스트 코드로 보는 디렉토리 이름 — 경로 성분 어디에 있어도 제외한다.
const TEST_DIRS: &[&str] = &["tests", "benches"];

fn is_pruned(name: &str) -> bool {
    // gitignored 로컬 폴더는 전부 선행 `.` 을 갖는다.
    (name.starts_with('.') && name != ".githooks") || PRUNE_DIRS.contains(&name)
}

fn gather(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if p.is_dir() {
            if !is_pruned(name) {
                gather(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_test_path(rel: &str) -> bool {
    rel.split('/').any(|seg| TEST_DIRS.contains(&seg))
}

/// `let _ =` 가 코드에 실제로 나타나는 열 위치. 줄 주석(`//`) 뒤는 제외한다.
///
/// 문자열 리터럴 안의 형태까지 배제하려면 렉서가 필요한데, 실제로 그걸 담는 파일은
/// 이 가드 자신뿐이라 [`ALLOWLIST`] 로 처리한다 — 렉서를 들이는 것보다 싸다.
fn has_let_underscore(line: &str) -> bool {
    let code = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    let bytes = code.as_bytes();
    let mut i = 0;
    while let Some(off) = code[i..].find("let") {
        let start = i + off;
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let rest = &code[start + 3..];
        if before_ok {
            let trimmed = rest.trim_start();
            if rest.len() != trimmed.len()
                && let Some(after_underscore) = trimmed.strip_prefix('_')
                && after_underscore.trim_start().starts_with('=')
                && !after_underscore.trim_start().starts_with("==")
            {
                return true;
            }
        }
        i = start + 3;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `#[cfg(test)]` / `#[test]` 아이템 본문에 속하는 줄 번호(0-based). `#![cfg(test)]`
/// 가 있으면 파일 전체를 테스트로 본다.
fn test_regions(lines: &[&str]) -> Vec<bool> {
    let mut marked = vec![false; lines.len()];
    if lines
        .iter()
        .any(|l| l.trim_start().starts_with("#![cfg(test)]"))
    {
        return vec![true; lines.len()];
    }
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("#[cfg(test)]") || t.starts_with("#[test]") {
            let mut depth: i32 = 0;
            let mut started = false;
            let mut j = i;
            while j < lines.len() {
                for ch in lines[j].chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            started = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                marked[j] = true;
                if started && depth <= 0 {
                    break;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    marked
}

/// 문장의 마지막 줄 index — 괄호 깊이가 0 으로 돌아오며 `;` 가 나오는 줄.
fn statement_end(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    for (offset, line) in lines.iter().enumerate().skip(start).take(80) {
        for ch in line.chars() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 && line.contains(';') {
            return offset;
        }
    }
    start
}

/// 바로 앞의 유의미한 줄이 `//` 주석인가 — 빈 줄과 `#[..]` 속성은 건너뛴다.
fn preceded_by_comment(lines: &[&str], start: usize) -> bool {
    let mut j = start;
    while j > 0 {
        j -= 1;
        let t = lines[j].trim();
        if t.is_empty() || t.starts_with("#[") || t.starts_with("#![") {
            continue;
        }
        return t.starts_with("//");
    }
    false
}

fn violations_in(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let in_test = test_regions(&lines);
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if in_test[i] || !has_let_underscore(line) {
            continue;
        }
        let end = statement_end(&lines, i);
        // 문장 범위 + 그 다음 한 줄 (rustfmt 가 trailing 주석을 다음 줄로 밀어내는 형태).
        let scan_to = (end + 1).min(lines.len().saturating_sub(1));
        let inline = lines[i..=scan_to].iter().any(|l| l.contains("//"));
        if inline || preceded_by_comment(&lines, i) {
            continue;
        }
        out.push((i + 1, line.trim().to_string()));
    }
    out
}

#[test]
fn every_let_underscore_in_production_code_says_why() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    gather(root, &mut files);
    files.sort();

    let mut report = Vec::new();
    for file in &files {
        let rel = rel_of(file, root);
        if ALLOWLIST.contains(&rel.as_str()) || is_test_path(&rel) {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (line, snippet) in violations_in(&text) {
            report.push(format!("  {rel}:{line}  {snippet}"));
        }
    }

    assert!(
        report.is_empty(),
        "사유 없이 값을 버리는 `let _` 이 있다 ({} 건).\n{}\n\n\
         왜 값을 버리는지 한 줄 주석을 같은 줄·윗줄·다음 줄 중 한 곳에 단다 \
         (docs/dev-guide/error-handling.md \"의도적 무시\").\n\
         값을 버리면 안 되는 것이었다면 주석 대신 처리하거나 로그를 남긴다.",
        report.len(),
        report.join("\n")
    );
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    /// 픽스처 조립 — 이 파일은 [`ALLOWLIST`] 면제가 없고 `.githooks/pre-commit` C.6
    /// 에는 면제 장치 자체가 없다. 금지 형태를 소스에 그대로 적으면 이 가드와 훅이
    /// 자기 자신을 잡으므로 런타임에 만든다.
    fn ignore_stmt() -> String {
        format!("let {}= g();", "_ ")
    }

    fn scan(src: &str) -> Vec<usize> {
        violations_in(src).into_iter().map(|(l, _)| l).collect()
    }

    /// `{}` 자리에 [`ignore_stmt`] 를 끼운다.
    fn fixture(template: &str) -> Vec<usize> {
        scan(&template.replace("{IGNORE}", &ignore_stmt()))
    }

    #[test]
    fn detects_a_bare_ignore() {
        assert_eq!(fixture("fn f() {\n    {IGNORE}\n}\n"), vec![2]);
    }

    #[test]
    fn same_line_previous_line_and_next_line_comments_all_count() {
        assert!(fixture("fn f() {\n    {IGNORE} // 이유\n}\n").is_empty());
        assert!(fixture("fn f() {\n    // 이유\n    {IGNORE}\n}\n").is_empty());
        assert!(fixture("fn f() {\n    {IGNORE}\n    // 이유\n}\n").is_empty());
    }

    #[test]
    fn blank_lines_and_attributes_are_skipped_when_looking_up() {
        assert!(fixture("fn f() {\n    // 이유\n\n    {IGNORE}\n}\n").is_empty());
        assert!(
            fixture("fn f() {\n    // 이유\n    #[allow(unused)]\n    {IGNORE}\n}\n").is_empty()
        );
    }

    #[test]
    fn a_comment_inside_a_multiline_statement_counts() {
        let src = format!(
            "fn f() {{\n    let {}= g(Bar {{\n        // 이유\n        a: 1,\n    }});\n}}\n",
            "_ "
        );
        assert!(scan(&src).is_empty());
    }

    #[test]
    fn a_block_comment_far_above_does_not_count() {
        assert_eq!(
            fixture("fn f() {\n    // 블록 상단 설명\n    h();\n    {IGNORE}\n}\n"),
            vec![4]
        );
    }

    #[test]
    fn test_bodies_are_exempt() {
        assert!(fixture("#[test]\nfn t() {\n    {IGNORE}\n}\n").is_empty());
        assert!(
            fixture("#[cfg(test)]\nmod tests {\n    fn t() {\n        {IGNORE}\n    }\n}\n")
                .is_empty()
        );
    }

    #[test]
    fn a_whole_file_marked_cfg_test_is_exempt() {
        assert!(fixture("#![cfg(test)]\nfn f() {\n    {IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn identifier_lookalikes_are_not_matched() {
        // `let _y` 바인딩, `==` 비교, 식별자 꼬리(`outlet _ =`)는 대상이 아니다.
        assert!(scan("fn f() {\n    let _y = g();\n}\n").is_empty());
        assert!(scan("fn f() {\n    if _ == g() {}\n}\n").is_empty());
        assert!(fixture("fn f() {\n    out{IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn a_form_inside_a_line_comment_is_not_a_violation() {
        assert!(fixture("fn f() {\n    // 예: {IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn test_dirs_are_recognised_by_any_path_segment() {
        assert!(is_test_path("tests/common/mod.rs"));
        assert!(is_test_path(
            "crates/tasty-agent/tests/runner_integration.rs"
        ));
        assert!(!is_test_path("src/core/agent/task.rs"));
    }
}
