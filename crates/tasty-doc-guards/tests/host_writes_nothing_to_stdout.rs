//! ADR-0101 의 재검토 조건 하나에 **발화 자리**를 준다 —
//! "host 프로세스가 stdout 에 쓰는 경로가 생긴다(현재는 없음)".
//!
//! 그 ADR 은 EPIPE 처리를 **CLI 클라이언트 갈래에만** 격리했다. host 는 자식 stdin 을
//! `Stdio::piped()` 로 열고 쓰므로 같은 처방을 host 에 적용하면 자식이 먼저 죽을 때
//! host 가 통째로 죽는다. 그 격리가 성립하는 근거가 **host 는 stdout 에 안 쓴다** 는
//! 실측 하나다. 그 전제가 깨지는 날을 사람이 알아채야 발동하는 상태였다.
//!
//! # 좌변
//!
//! 루트 패키지의 **출하되는** `.rs` — `src/**` 에서 파일 단위 test-only 와 인라인
//! `#[cfg(test)]` 줄을 뺀 것. 조건의 주어가 "host **프로세스**가 쓰는 경로" 이므로
//! 출하 밖의 코드는 좌변이 아니다. 실측 2026-09-08: 그것을 빼기 전 거친 계수가 17 이고
//! 뺀 뒤가 0 이다 — 안 빼면 이 시험은 첫날부터 빨갛고, 빨간 이유가 조건과 무관하다.
//!
//! # 이미 있는 판사와 무엇이 다른가
//!
//! `tests/cli_stdout_broken_pipe.rs` 의 `cli_crate_has_no_direct_stdout_print` 는 좌변이
//! `crates/tasty-cli/src` **뿐**이다. host 트리(`src/`)를 안 덮는다. 두 트리는 처방이
//! 반대다 — 저쪽은 `outln!` 로 바꾸라고 하고, 이쪽은 **애초에 쓰지 말라**고 한다
//! (host 에는 EPIPE 를 접을 경계가 없다). 같은 물음에 판사를 둘 만든 것이 아니다.
//!
//! 매크로 호출을 가르는 술어는 [`tasty_doc_guards::source_text::invokes_macro`] **하나**를
//! 양쪽이 쓴다. 사본을 두면 한쪽만 고쳐진 날 답이 갈린다.
//!
//! # 이 가드가 안 보는 것
//!
//! `write!(io::stdout(), …)` 처럼 매크로 이름이 아니라 핸들로 쓰는 자리. 오늘 그 형태는
//! 0 이고, 세려면 핸들 별칭을 따라가야 해서 술어가 근사가 된다. 늘어나면 그때 축을 넓힌다.

use tasty_doc_guards::cfg_predicate::cfg_gated_lines;
use tasty_doc_guards::repo_root;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{invokes_macro, mask_non_code, rust_sources};

/// 훑은 `.rs` 수의 하한. 수집이 죽으면 "쓰는 자리 없음" 은 언제나 참이 된다.
/// 실측 2026-09-08: `src/**` 의 추적 `.rs` 605 개.
const MIN_SOURCES: usize = 400;

/// stdout 에 쓰는 std 매크로. `eprintln!`/`eprint!` 는 **stderr 라 대상이 아니다** —
/// 이 목록에 넣지 마라. 그 둘을 섞는 것이 이 가드가 생긴 계기다.
const STDOUT_MACROS: &[&str] = &["println", "print"];

/// 한 파일 안에서 stdout 에 쓰는 **출하되는** 줄. 반환은 `(1-기준 줄번호, 줄 내용)`.
///
/// 거르는 것이 둘이고 **둘 다 필요하다**:
/// - 주석·문자열 — `mask_non_code`. 안 지우면 "`println!` 을 쓰지 마라" 는 주석 자신이
///   위반이 된다(실측 2026-09-08: 그런 줄이 4 개다).
/// - 인라인 `#[cfg(test)]` 블록 — 그 줄은 출하 산출물에 안 들어가므로 조건의 주어가
///   아니다. 파일 **단위** test-only 는 호출부가 [`test_only_files`] 로 먼저 뺀다.
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
        "루트 패키지 소스를 {} 개만 모았다(하한 {MIN_SOURCES}) — 수집이 죽으면 아래 판정은 \
         빈 집합을 훑고 조용히 통과한다",
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
        "[ADR-0101 좌변] 루트 패키지 `.rs` {} 개 · 출하 밖 {} 개 · 판정 {judged_files} 개",
        sources.len(),
        not_shipped.len()
    );

    assert!(
        offenders.is_empty(),
        "host 가 출하되는 코드에서 stdout 에 직접 쓴다:\n{}\n\
         ★ 이것은 회귀가 아니라 **ADR-0101 의 재검토 조건이 발동한 것**이다 \
         (docs/adr/0101-cli-stdout-broken-pipe-exit-zero.md).\n\
         그 ADR 은 EPIPE 를 종료 코드 0 으로 접는 처방을 **CLI 클라이언트 갈래에만** \
         격리했고, 그 격리의 근거가 \"host 는 stdout 에 안 쓴다\" 는 실측이다. \
         host 에서 쓰기 시작하면 그 경계가 host 를 안 덮는다.\n\
         순서가 있다. (1) 그 자리가 정말 host 갈래인지 본다 — 루트 패키지 안에도 \
         CLI 로 라우팅되는 갈래가 있다. (2) host 갈래면 `tracing` 으로 옮길 수 있는지 \
         먼저 본다(사용자에게 보이는 stdout 이 아니라 진단이면 그게 맞다). \
         (3) 정말 stdout 이어야 하면 그 ADR 의 경계를 다시 쓴다 — **host 의 SIGPIPE 를 \
         되돌리면 자식이 먼저 죽을 때 host 가 통째로 죽는다.**\n\
         ☞ 이 시험을 지워서 통과시키지 마라. 그러면 재검토 조건이 다시 문장이 된다.",
        offenders.join("\n")
    );
}

/// 술어가 `eprintln!` 을 `println` 으로 세지 않는지. 이 픽스처는 이 파일의 상수에서
/// 파생하지 않는다 — 형태만 잰다(R1078).
///
/// 이 자리가 존재하는 이유는 실측 사고다: 부분문자열로 세면 stderr 세 자리가 stdout 으로
/// 잡히고, 그 계수를 근거로 ADR 이 낡았다고 판단하게 된다.
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
        "매크로 술어가 stdout 과 stderr 를 못 가른다:\n{}\n\
         ★ 못 가르면 이 가드는 **더 많이 잡는 쪽으로** 틀리고, 그 계수를 근거로 \
         ADR-0101 이 낡았다는 판정이 나온다. 실측 2026-09-08 에 실제로 그랬다.",
        wrong.join("\n")
    );
}

/// 인라인 `#[cfg(test)]` 필터의 **합성 양성 대조**.
///
/// 실측 2026-09-08: 이 레포에서 그 필터는 **0 건**을 지운다 — 거친 계수 17 의 분해는
/// `eprintln!` 3 · 주석 4 · 실제 호출 10 이고, 그 10 은 **전부 파일 단위** test-only 라
/// 호출부에서 이미 빠진다. 살아 있는 표본이 없는 절은 죽은 채로 초록이므로, 합성
/// 입력으로 양성 대조를 만든다(`shipping_scope` 가 인용하는 R415 와 같은 이유).
///
/// 필터를 지우지 않는 이유: 지우면 출하 안 되는 인라인 test 코드가 위반으로 나가고,
/// 그 실패문의 처방("ADR-0101 의 경계를 다시 써라")이 **실재하지 않는 조건에 대한
/// 처방**이 된다.
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
        "출하되는 줄 하나(2 행)만 잡혀야 한다. 8 행이 함께 잡히면 인라인 \
         `#[cfg(test)]` 필터가 죽은 것이고, 13 행이 잡히면 주석 마스킹이 죽은 것이다."
    );
}
