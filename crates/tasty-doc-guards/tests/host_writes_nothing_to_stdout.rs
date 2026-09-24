//! 루트 패키지 제품 코드의 print·println 사용을 찾아 stdout 정책을 재검토하게 한다(ADR-0043).
//! CLI의 broken pipe 종료 처리를 host에 그대로 적용하면 자식 stdin 파이프 오류로 host까지 종료될 수 있다.
//! 주석·문자열과 파일·인라인 테스트 코드는 제외한다. CLI 크레이트의 출력은 별도 검사 대상이다.
//! stdout 핸들로 직접 쓰는 경우는 찾지 못하고, 루트 코드가 실제 host 경로인지도 사람이 확인해야 한다.

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{invokes_macro, mask_non_code, rust_sources};

/// 2026-09-08 추적 src Rust 파일 605개를 기준으로 둔 수집 하한.
const MIN_SOURCES: usize = 400;

/// stdout 매크로만 등록한다. eprint·eprintln은 stderr라 제외한다.
const STDOUT_MACROS: &[&str] = &["println", "print"];

/// 마스킹한 코드에서 인라인 테스트를 제외해 stdout 매크로 호출 줄을 찾는다. 파일 단위 테스트는 호출부가 제외한다.
fn stdout_write_lines(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if gated[i] {
            continue;
        }
        let code = mask_non_code(line);
        if STDOUT_MACROS.iter().any(|m| invokes_macro(&code, m)) {
            out.push((i + 1, line.trim().to_string()));
        }
    }
    out
}

#[test]
fn the_shipped_host_has_no_direct_stdout_write() {
    let root = repo_root();
    let sources = rust_sources(&root, &["src"]);
    assert!(
        sources.len() >= MIN_SOURCES,
        "루트 패키지 소스를 {}개만 수집했다(하한 {MIN_SOURCES}). 수집 범위를 확인한다.",
        sources.len()
    );

    let not_shipped = test_only_files(&root, &sources);
    let mut judged_files = 0usize;
    let mut offenders: Vec<String> = Vec::new();
    for (rel, text) in &sources {
        if not_shipped.contains(rel) {
            continue;
        }
        judged_files += 1;
        for (i, line) in stdout_write_lines(text) {
            offenders.push(format!("  {}:{} — {}", rel.display(), i, line));
        }
    }

    println!(
        "[ADR-0043 검사 범위] 루트 패키지 `.rs` {} 개 · test 전용 {} 개 · 판정 {judged_files} 개",
        sources.len(),
        not_shipped.len()
    );

    assert!(
        offenders.is_empty(),
        "루트 제품 코드에서 stdout 매크로 호출을 찾았다:\n{}\n실제 host 경로인지 확인한다. 진단이면 tracing을 사용하고 stdout이 필요하다면 ADR-0043의 CLI·host 오류 처리 경계를 재검토한다. host의 SIGPIPE 처리를 바꾸면 자식 파이프 오류가 host를 종료시킬 수 있다.",
        offenders.join("\n")
    );
}

#[test]
fn the_predicate_tells_stdout_macros_from_stderr_ones() {
    let cases: &[(&str, bool)] = &[
        ("println!(\"x\");", true),
        ("print!(\"x\");", true),
        ("eprintln!(\"x\");", false),
        ("eprint!(\"x\");", false),
        ("let printed = 1;", false),
        ("foo_println!(\"x\");", false),
    ];
    let mut wrong = Vec::new();
    for (code, want) in cases {
        let got = STDOUT_MACROS.iter().any(|m| invokes_macro(code, m));
        if got != *want {
            wrong.push(format!("  `{code}` → {got} (기대 {want})"));
        }
    }
    assert!(
        wrong.is_empty(),
        "stdout과 stderr 매크로를 구별하지 못했다:\n{}",
        wrong.join("\n")
    );
}

/// 실제 소스에서 해당 입력이 없어도 인라인 테스트와 주석 제외가 작동하는지 합성 입력으로 확인한다.
#[test]
fn inline_cfg_test_blocks_are_not_shipped_lines() {
    let src = "\
fn shipped() {
    print!(\"a\");
}

#[cfg(test)]
mod t {
    fn helper() {
        println!(\"b\");
    }
}

// println! 이 주석 안에 있다
";
    let got: Vec<usize> = stdout_write_lines(src)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        got,
        vec![2],
        "test 전용이 아닌 줄 하나(2 행)만 잡혀야 한다. 8 행이 함께 잡히면 인라인 \
         `#[cfg(test)]` 필터가 적용되지 않은 것이고, 13 행이 잡히면 주석 마스킹이 적용되지 않은 것이다."
    );
}
