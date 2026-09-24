//! 출하 코드의 `let _ =`에 값을 버리는 이유를 적었는지 확인한다.
//! 타입을 알 수 없는 텍스트 검사이므로 Result 외의 값도 대상이다.
//! 정책은 docs/dev-guide/error-handling.md의 의도적 무시 절에 있다.
//!
//! 문장 내부·바로 다음 줄 또는 앞의 유의미한 줄에 `//` 주석이 있으면 통과한다.
//! 앞줄을 찾을 때 빈 줄과 속성은 건너뛰지만, 멀리 떨어진 블록 설명은 인정하지 않는다.
//! 이 범위는 pre-commit C.6이 허용하는 위치를 포함한다. 주석의 타당성은 사람이 검토한다.
//!
//! `tests/`·`benches/`, 테스트 아이템과 test를 요구하는 cfg 범위는 제외한다.
//! `all(test, ...)`는 제외하지만 `any(test, ...)`는 출하 코드일 수 있어 제외하지 않는다.
//! 따라서 테스트에서 값을 버려 검증을 빠뜨리는 문제는 이 검사로 찾을 수 없다.
//!
//! 코드 검색에서는 주석·리터럴을 지우고, 사유 검색에서는 리터럴만 지운다.
//! 문자열의 `//`는 사유로 인정하지 않는다. pre-commit C.6의 줄 단위 처리는
//! 여러 줄 문자열을 정확히 구분하지 못하므로 이 검사와 결과가 다를 수 있다.

use std::fs;
use std::path::{Path, PathBuf};

/// (경로, 허용 코드 조각). 해당 조각을 담은 줄만 면제한다.
/// vendor/tiny_http의 상류 코드를 불필요하게 수정하지 않기 위한 예외다(ADR-0032).
/// 같은 파일의 다른 위반은 계속 검사한다. pre-commit C.6은 이 예외를 사용하지 않는다.
const ALLOWLIST: &[(&str, &[&str])] = &[(
    "vendor/tiny_http/src/lib.rs",
    &[
        "let _ = stream.shutdown(Shutdown::Both);",
        "let _ = std::fs::remove_file(path);",
    ],
)];

fn is_allowlisted(entries: &[(&str, &[&str])], rel: &str, line: &str) -> bool {
    entries
        .iter()
        .any(|(path, snippets)| *path == rel && snippets.iter().any(|s| line.contains(s)))
}

const PRUNE_DIRS: &[&str] = &[
    "target",
    "dist",
    "_site",
    ".worktree",
    ".git",
    ".idea",
    // Git 추적 여부와 무관하게 개발 도구의 캐시는 제외한다.
    ".serena",
    ".playwright-mcp",
    "node_modules",
    "assets",
];

/// 경로 성분 어디에 있든 제외할 테스트 디렉터리.
const TEST_DIRS: &[&str] = &["tests", "benches"];

/// 로컬 경로를 문서 인용으로 오인하지 않도록 순회용 이름을 조립한다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 추적되는 숨김 디렉터리도 있으므로 점으로 시작하는 이름을 모두 제외하지 않는다.
fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
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
            // 이름이 다른 CARGO_TARGET_DIR도 빌드 캐시 표식으로 제외한다.
            if !is_pruned(name) && !tasty_doc_guards::is_build_cache_dir(&p) {
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

/// mask_non_code로 주석·리터럴을 지운 줄을 받는다.
fn has_let_underscore(code: &str) -> bool {
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

/// test 또는 이를 포함한 all 조건만 테스트 전용으로 본다.
/// any(test, ...)는 다른 조건으로 출하될 수 있으므로 제외하지 않는다.
fn cfg_requires_test(pred: &str) -> bool {
    let p = pred.trim();
    if p == "test" {
        return true;
    }
    match p.strip_prefix("all(").and_then(|r| r.strip_suffix(')')) {
        Some(inner) => cfg_args(inner).iter().any(|a| cfg_requires_test(a)),
        None => false,
    }
}

/// 최상위 쉼표로만 자른다 — 중첩 괄호 안의 쉼표는 인자 경계가 아니다.
fn cfg_args(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in inner.chars() {
        match ch {
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// `#[cfg(<test 를 요구하는 술어>)]` 또는 `#[test]` 인가. 여는 형태(`#!`)도 함께 본다.
fn is_test_attr(line: &str, inner_attr: bool) -> bool {
    let t = line.trim_start();
    let prefix = if inner_attr { "#![cfg(" } else { "#[cfg(" };
    if !inner_attr && t.starts_with("#[test]") {
        return true;
    }
    t.strip_prefix(prefix)
        .and_then(|r| r.trim_end().strip_suffix(")]"))
        .is_some_and(cfg_requires_test)
}

/// `#[cfg(test)]` / `#[test]` 아이템 본문에 속하는 줄 번호(0-based). 파일 단위
/// `#![cfg(<test 요구>)]` 가 있으면 파일 전체를 테스트로 본다.
fn test_regions(lines: &[&str]) -> Vec<bool> {
    let mut marked = vec![false; lines.len()];
    if lines.iter().any(|l| is_test_attr(l, true)) {
        return vec![true; lines.len()];
    }
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if is_test_attr(t, false) {
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
    let raw: Vec<&str> = text.lines().collect();
    // 코드 탐색과 사유 탐색에 필요한 마스킹 범위가 다르다.
    let code_src = tasty_doc_guards::source_text::mask_non_code(text);
    let lit_src = tasty_doc_guards::source_text::mask_literals(text);
    let code: Vec<&str> = code_src.lines().collect();
    let lit: Vec<&str> = lit_src.lines().collect();
    let in_test = test_regions(&code);
    let mut out = Vec::new();
    for (i, line) in code.iter().enumerate() {
        if in_test[i] || !has_let_underscore(line) {
            continue;
        }
        let end = statement_end(&code, i);
        // 문장 범위 + 그 다음 한 줄 (rustfmt 가 trailing 주석을 다음 줄로 밀어내는 형태).
        let scan_to = (end + 1).min(code.len().saturating_sub(1));
        let inline = lit[i..=scan_to].iter().any(|l| l.contains("//"));
        if inline || preceded_by_comment(&lit, i) {
            continue;
        }
        out.push((i + 1, raw[i].trim().to_string()));
    }
    out
}

/// 순회 실패로 빈 결과가 통과하지 않게 하는 하한이다(ADR-0048).
/// 2026-09-05에 gather가 수집한 Rust 파일은 1180개였다.
/// 파일 정리를 허용할 여유를 두며, 하한 통과가 수집의 완전성을 보장하지는 않는다.
const MIN_SCANNED_FILES: usize = 700;

fn scan_is_credible(found: usize) -> bool {
    found >= MIN_SCANNED_FILES
}

#[test]
fn the_scan_refuses_to_report_zero_from_an_empty_walk() {
    assert!(!scan_is_credible(0), "빈 스캔을 믿을 만하다고 판정했다");
    assert!(!scan_is_credible(MIN_SCANNED_FILES - 1));
    assert!(scan_is_credible(MIN_SCANNED_FILES));
}

#[test]
fn every_let_underscore_in_production_code_says_why() {
    let root = tasty_doc_guards::repo_root();
    let root = root.as_path();
    let mut files = Vec::new();
    gather(root, &mut files);
    files.sort();
    assert!(
        scan_is_credible(files.len()),
        "Rust 파일을 {}개만 수집했다(하한 {MIN_SCANNED_FILES}). 경로와 PRUNE_DIRS를 확인한다. `git ls-files '*.rs' | wc -l`로 추적 파일 수와 비교하되 제외 디렉터리의 차이를 고려한다. 실제 수집 범위가 줄었다면 다시 측정한 근거와 함께 하한을 조정한다.",
        files.len()
    );

    let mut report = Vec::new();
    for file in &files {
        let rel = rel_of(file, root);
        if is_test_path(&rel) {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (line, snippet) in violations_in(&text) {
            if is_allowlisted(ALLOWLIST, &rel, &snippet) {
                continue;
            }
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

const TARGET_EXEMPTION: &str = "#![allow(clippy::let_underscore_must_use)]";

/// 테스트에서만 lint를 면제해 출하 코드의 목록은 유지한다.
const CRATE_EXEMPTION: &str = "#![cfg_attr(test, allow(clippy::let_underscore_must_use))]";

fn mentions_exemption(line: &str) -> bool {
    line.contains("allow(clippy::let_underscore_must_use)")
}

/// 통합 테스트와 벤치는 별도 타깃이므로 크레이트 루트의 면제가 적용되지 않는다.
fn is_own_target(rel: &str) -> bool {
    is_test_path(rel)
}

/// `rel` 이 속한 크레이트의 루트 소스 경로들(있는 것만).
fn crate_roots(root: &Path, rel: &str) -> Vec<PathBuf> {
    let dir = if let Some(rest) = rel.strip_prefix("crates/") {
        match rest.split_once('/') {
            Some((name, _)) => root.join("crates").join(name),
            None => root.to_path_buf(),
        }
    } else {
        root.to_path_buf()
    };
    ["src/lib.rs", "src/main.rs"]
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.is_file())
        .collect()
}

/// 출하 코드 검사와 같은 마스킹·테스트 범위 판정을 쓴다.
/// 통합 테스트는 #[test] 밖의 헬퍼도 테스트 코드이므로 whole_file로 포함한다.
fn test_scope_sites(text: &str, whole_file: bool) -> Vec<usize> {
    let code_src = tasty_doc_guards::source_text::mask_non_code(text);
    let code: Vec<&str> = code_src.lines().collect();
    let in_test = test_regions(&code);
    let mut out = Vec::new();
    for (i, line) in code.iter().enumerate() {
        if (whole_file || in_test[i]) && has_let_underscore(line) {
            out.push(i);
        }
    }
    out
}

/// 그 줄을 감싸는 테스트 아이템에 `#[allow(..)]` 이 붙어 있는가. 아이템의 속성 묶음은
/// 연속된 `#[..]` 줄이라, 표식 줄 위아래로 붙어 있는 것만 본다.
fn item_is_exempt(code: &[&str], line: usize) -> bool {
    let mut i = line;
    loop {
        let t = code[i].trim_start();
        if is_test_attr(t, false) {
            let mut a = i;
            while a > 0 && code[a - 1].trim_start().starts_with("#[") {
                a -= 1;
            }
            let mut b = i;
            while b + 1 < code.len() && code[b + 1].trim_start().starts_with("#[") {
                b += 1;
            }
            return code[a..=b].iter().any(|l| mentions_exemption(l));
        }
        if i == 0 {
            return false;
        }
        i -= 1;
    }
}

/// 출하 코드의 값 무시 목록에 테스트가 섞이지 않도록 lint 면제를 확인한다.
/// 타입을 모르는 검사이므로 각 문장 대신 크레이트 루트나 테스트 타깃에 면제를 요구한다.
#[test]
fn test_scope_stays_out_of_the_lint_roster() {
    let root = tasty_doc_guards::repo_root();
    let root = root.as_path();
    let mut files = Vec::new();
    gather(root, &mut files);
    files.sort();
    assert!(
        scan_is_credible(files.len()),
        "스캔한 `.rs` 가 {}개다(하한 {MIN_SCANNED_FILES}) — 순회가 깨졌다",
        files.len()
    );

    // 경로 기반·cfg 기반 수집을 따로 확인해야 한쪽 실패를 다른 쪽 결과가 가리지 않는다.
    let mut seen_by_path = 0usize;
    let mut seen_by_region = 0usize;
    let mut report = Vec::new();
    for file in &files {
        let rel = rel_of(file, root);
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        let own_target = is_own_target(&rel);
        let sites = test_scope_sites(&text, own_target);
        if sites.is_empty() {
            continue;
        }
        if own_target {
            seen_by_path += sites.len();
        } else {
            seen_by_region += sites.len();
        }
        let code_src = tasty_doc_guards::source_text::mask_non_code(&text);
        let code: Vec<&str> = code_src.lines().collect();
        if code.iter().any(|l| {
            let t = l.trim_start();
            t.starts_with("#![") && mentions_exemption(t)
        }) {
            continue;
        }
        if own_target {
            report.push(format!("  {rel}  ← 파일 머리에 `{TARGET_EXEMPTION}`"));
            continue;
        }
        let uncovered: Vec<usize> = sites
            .iter()
            .copied()
            .filter(|&i| !item_is_exempt(&code, i))
            .collect();
        if uncovered.is_empty() {
            continue;
        }
        let roots = crate_roots(root, &rel);
        let covered = roots.iter().any(|p| {
            fs::read_to_string(p).is_ok_and(|t| {
                tasty_doc_guards::source_text::mask_non_code(&t)
                    .lines()
                    .any(|l| l.trim_start().starts_with("#![") && mentions_exemption(l))
            })
        });
        if covered {
            continue;
        }
        let where_to = roots
            .first()
            .map(|p| rel_of(p, root))
            .unwrap_or_else(|| "(크레이트 루트를 못 찾았다)".to_string());
        report.push(format!(
            "  {rel}:{}  ← 크레이트 루트 `{where_to}` 에 `{CRATE_EXEMPTION}`",
            uncovered[0] + 1
        ));
    }

    assert!(
        seen_by_path > 0,
        "테스트 타깃(`tests/`·`benches/`)에서 `let _` 무시를 한 자리도 못 봤다 — 순회가 깨졌다"
    );
    assert!(
        seen_by_region > 0,
        "프로덕션 파일 안의 `#[cfg(test)]`/`#[test]` 본문에서 `let _` 무시를 한 자리도 \
         못 봤다 — 범위 판정(`test_regions`)이 깨졌다"
    );

    report.sort();
    report.dedup();
    assert!(
        report.is_empty(),
        "테스트 코드의 `let _`에 lint 면제가 없다({}곳).\n{}\n출하 코드의 값 무시 목록에 테스트가 섞이지 않도록 표시된 타깃이나 크레이트 루트에 면제를 추가한다(docs/dev-guide/error-handling.md).",
        report.len(),
        report.join("\n")
    );
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    /// pre-commit C.6에 면제 기능이 없으므로 금지 형태를 런타임에 조립한다.
    fn ignore_stmt() -> String {
        format!("let {}= g();", "_ ")
    }

    fn scan(src: &str) -> Vec<usize> {
        violations_in(src).into_iter().map(|(l, _)| l).collect()
    }

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
    fn a_cfg_that_requires_test_among_other_predicates_is_still_test() {
        assert!(
            fixture(
                "#[cfg(all(test, unix))]\nmod tests {\n    fn t() {\n        {IGNORE}\n    }\n}\n"
            )
            .is_empty()
        );
        assert!(
            fixture(
                "#[cfg(all(unix, all(test, feature = \"gui\")))]\nmod tests {\n    fn t() {\n        {IGNORE}\n    }\n}\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_cfg_that_merely_mentions_test_is_not_test_scope() {
        assert_eq!(
            fixture("#[cfg(any(unix, test))]\nmod m {\n    fn f() {\n        {IGNORE}\n    }\n}\n"),
            vec![4]
        );
        assert_eq!(
            fixture("#[cfg(not(test))]\nmod m {\n    fn f() {\n        {IGNORE}\n    }\n}\n"),
            vec![4]
        );
    }

    #[test]
    fn a_file_level_cfg_that_requires_test_exempts_the_whole_file() {
        assert!(fixture("#![cfg(all(test, unix))]\n\nfn f() {\n    {IGNORE}\n}\n").is_empty());
        assert_eq!(
            fixture("#![cfg(unix)]\n\nfn f() {\n    {IGNORE}\n}\n"),
            vec![4]
        );
    }

    #[test]
    fn a_whole_file_marked_cfg_test_is_exempt() {
        assert!(fixture("#![cfg(test)]\nfn f() {\n    {IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn identifier_lookalikes_are_not_matched() {
        assert!(scan("fn f() {\n    let _y = g();\n}\n").is_empty());
        assert!(scan("fn f() {\n    if _ == g() {}\n}\n").is_empty());
        assert!(fixture("fn f() {\n    out{IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn a_form_inside_a_string_literal_is_not_a_violation() {
        let src = format!(
            "fn f() {{\n    let s = \"{}\";\n    drop(s);\n}}\n",
            ignore_stmt()
        );
        assert!(scan(&src).is_empty());
    }

    #[test]
    fn a_slash_slash_inside_a_string_is_not_a_reason() {
        let src = format!("fn f() {{\n    let {}= get(\"http://x\");\n}}\n", "_ ");
        assert_eq!(scan(&src), vec![2]);
    }

    #[test]
    fn a_form_inside_a_line_comment_is_not_a_violation() {
        assert!(fixture("fn f() {\n    // 예: {IGNORE}\n}\n").is_empty());
    }

    #[test]
    fn the_allowlist_exempts_a_registered_snippet_not_the_whole_file() {
        let entries: &[(&str, &[&str])] = &[("src/x.rs", &["RULE_TEXT"])];

        assert!(is_allowlisted(
            entries,
            "src/x.rs",
            "let s = \"RULE_TEXT\";"
        ));
        assert!(
            !is_allowlisted(entries, "src/x.rs", &ignore_stmt()),
            "등록 파일의 다른 위반까지 면제되면 파일 통째 면제와 같아진다"
        );
        assert!(
            !is_allowlisted(entries, "src/y.rs", "let s = \"RULE_TEXT\";"),
            "같은 조각이어도 등록되지 않은 파일은 면제가 아니다"
        );
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
