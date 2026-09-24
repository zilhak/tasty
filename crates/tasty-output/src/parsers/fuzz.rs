//! 임의 Unicode 문자열과 깨진 escape 조각으로 파서의 패닉 여부를 검사한다.
//! 정규식·OSC의 선택적 분기를 포함한 입력을 생성하지만 모든 입력의 안전성을 증명하는 시험은 아니다.

use proptest::prelude::*;

use crate::parse_buffer;

/// 신뢰불가 터미널 출력에 등장할 수 있는 조각들. 정규식의 선택적 분기와
/// escape(OSC/CSI) 경로까지 닿도록 의도적으로 부분/깨진 시퀀스를 섞는다.
const FRAGMENTS: &[&str] = &[
    // 일반 텍스트 / 경계 문자
    "",
    " ",
    "\n",
    "\r\n",
    "\t",
    ":",
    ";",
    "/",
    "\\",
    ".",
    "%",
    "->",
    "(",
    ")",
    "[",
    "]",
    "'",
    "\"",
    // 컴파일러 에러 (선택적 code/col group 유도)
    "error",
    "warning",
    "error[E0308]: mismatched types",
    "error: boom",
    "note: see here",
    "fatal error: nope",
    " --> src/main.rs:10:5",
    " --> a",
    "src/foo.c:10:5: error: msg",
    "src/foo.c:10: warning: msg",
    "src/foo.ts(10,5): error TS2345: msg",
    "src/foo.ts(10,5): warning TS1: ",
    // 스택트레이스 (python/rust/node/java)
    "Traceback (most recent call last):",
    "  File \"x.py\", line 3",
    "  File \"x.py\", line 3, in f",
    "    code context",
    "ValueError: boom",
    "Exception",
    "KeyboardInterrupt",
    "thread 'main' panicked at 'oops', src/a.rs:1:2",
    "thread 'x' panicked at src/a.rs:1:2",
    "stack backtrace:",
    "   0: foo::bar::baz",
    "   1: 0xdeadbeef - foo::bar",
    "    at fn (file:1:2)",
    "    at node:internal/x:1:2",
    "    at com.example.Foo.bar(Foo.java:42)",
    "    at com.example.Foo.bar(Foo.java)",
    // test result (cargo/pytest/jest)
    "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s",
    "test result: FAILED. 1 passed; 2 failed",
    "===== 5 passed, 1 failed, 2 skipped in 0.34s =====",
    "===== 5 passed in 0.12s =====",
    "Tests:       1 failed, 2 passed, 3 total",
    "Tests:       all",
    // progress
    "[#####>....] 42%",
    "[====] 45.2%",
    "50 MB / 200 MB",
    "50.5KiB/1.2GiB",
    "  99%  ",
    "100%",
    "999%",
    // links / path
    "see src/main.rs:42:7 ok",
    "https://example.com/path?x=1",
    "ftp://h/p",
    "ssh://h",
    "file:///a/b",
    "README.md",
    "/abs/path",
    "C:/win/path",
    "a.b",
    // OSC / CSI escape 시퀀스 (raw 바이트 — 핵심 escape 경로 + 깨진 변형)
    "\x1b]8;;https://e.com\x07link\x1b]8;;\x07",
    "\x1b]8;id=1;https://e.com\x1b\\text\x1b]8;;\x1b\\",
    "\x1b]8;;\x07\x1b]8;;\x07",
    "\x1b]133;A\x07",
    "\x1b]133;B;payload\x07",
    "\x1b]133;C\x1b\\",
    "\x1b]133;D;0\x07",
    "\x1b]133;D;-1;extra\x07",
    "\x1b]133;D;notanumber\x07",
    "\x1b]9;hello\x07",
    "\x1b]777;notify;title;body\x07",
    "\x1b]777;notify;title\x07",
    "\x1b]777;notify\x07",
    "\x1b]777;\x07",
    "\x1b[31m",
    "\x1b[0m",
    "\x1b]",
    "\x1b",
    "\x07",
];

/// FRAGMENTS 를 0~14개 무작위로 이어붙인 문자열 (escape 경로 집중).
fn fragment_soup() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(FRAGMENTS), 0..14).prop_map(|parts| parts.concat())
}

/// 순수 임의 유니코드 문자열 (정규식 선두만 매치되는 경계 케이스 포함).
fn arbitrary_text() -> impl Strategy<Value = String> {
    prop::string::string_regex(".{0,300}").unwrap()
}

/// 두 전략을 섞은 입력.
fn untrusted_input() -> impl Strategy<Value = String> {
    prop_oneof![arbitrary_text(), fragment_soup()]
}

/// 생성한 입력을 지정 파서에 넣는다. 패닉은 proptest가 재현·축소한다.
fn assert_no_panic(id: &'static str, s: &str) -> Result<(), TestCaseError> {
    // 등록된 ID이므로 조회 실패도 시험 실패다.
    prop_assert!(parse_buffer(s, [id]).is_ok());
    Ok(())
}

proptest! {
    // errors.rs — CompileErrorParser / StackTraceParser
    #[test]
    fn compile_error_never_panics(s in untrusted_input()) {
        assert_no_panic("compile_error", &s)?;
    }

    #[test]
    fn stack_trace_never_panics(s in untrusted_input()) {
        assert_no_panic("stack_trace", &s)?;
    }

    // links.rs — PathParser / UrlParser / OscLinkParser
    #[test]
    fn path_never_panics(s in untrusted_input()) {
        assert_no_panic("path", &s)?;
    }

    #[test]
    fn url_never_panics(s in untrusted_input()) {
        assert_no_panic("url", &s)?;
    }

    #[test]
    fn osc_link_never_panics(s in untrusted_input()) {
        assert_no_panic("osc_link", &s)?;
    }

    // progress.rs — ProgressParser
    #[test]
    fn progress_never_panics(s in untrusted_input()) {
        assert_no_panic("progress", &s)?;
    }

    // shell.rs — PromptBoundaryParser / ExitCodeParser / OscNotificationParser
    #[test]
    fn prompt_boundary_never_panics(s in untrusted_input()) {
        assert_no_panic("prompt_boundary", &s)?;
    }

    #[test]
    fn exit_code_never_panics(s in untrusted_input()) {
        assert_no_panic("exit_code", &s)?;
    }

    #[test]
    fn osc_notification_never_panics(s in untrusted_input()) {
        assert_no_panic("osc_notification", &s)?;
    }

    // test_result.rs — TestResultParser
    #[test]
    fn test_result_never_panics(s in untrusted_input()) {
        assert_no_panic("test_result", &s)?;
    }

    // 전 파서 동시 dispatch — 상호작용/누적 경로까지 패닉 없음.
    #[test]
    fn all_parsers_never_panic(s in untrusted_input()) {
        let ids = [
            "path", "url", "prompt_boundary", "exit_code", "compile_error",
            "stack_trace", "test_result", "progress", "osc_link", "osc_notification",
        ];
        prop_assert!(parse_buffer(&s, ids).is_ok());
    }
}
