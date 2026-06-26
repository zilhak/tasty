//! tasty-output parsers — sub-module 별로 분리.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser, strip_ansi};

pub struct CompileErrorParser;

static RUSTC_HEAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    // error[E0308]: mismatched types  |  warning: unused variable: `x`
    Regex::new(r"^(?P<sev>error|warning)(?:\[(?P<code>[A-Z]\d+)\])?:\s*(?P<msg>.+)$").unwrap()
});

static RUSTC_LOC_RE: LazyLock<Regex> = LazyLock::new(|| {
    // " --> src/main.rs:10:5"
    Regex::new(r"^\s*-->\s*(?P<path>[^\s:][^:]*):(?P<line>\d+):(?P<col>\d+)\s*$").unwrap()
});

static GCC_RE: LazyLock<Regex> = LazyLock::new(|| {
    // src/foo.c:10:5: error: msg   |   src/foo.c:10: warning: msg
    Regex::new(
        r"^(?P<path>[^:\s][^:]*):(?P<line>\d+)(?::(?P<col>\d+))?:\s*(?P<sev>error|warning|note|fatal error):\s*(?P<msg>.+)$",
    )
    .unwrap()
});

static TSC_RE: LazyLock<Regex> = LazyLock::new(|| {
    // src/foo.ts(10,5): error TS2345: msg
    Regex::new(
        r"^(?P<path>[^\(\s][^\(]*)\((?P<line>\d+),(?P<col>\d+)\):\s*(?P<sev>error|warning)\s+(?P<code>TS\d+):\s*(?P<msg>.+)$",
    )
    .unwrap()
});

impl Parser for CompileErrorParser {
    fn id(&self) -> &'static str {
        "compile_error"
    }

    fn parse_block(&self, text: &str, out: &mut Vec<ParsedItem>) {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut i = 0;
        while i < lines.len() {
            let raw = lines[i];
            let stripped = strip_ansi(raw);
            let line = stripped.trim_end_matches(['\n', '\r']);

            // rustc: header + (선택적) --> location
            if let Some(caps) = RUSTC_HEAD_RE.captures(line) {
                let mut path: Option<String> = None;
                let mut ln: Option<u32> = None;
                let mut col: Option<u32> = None;
                let mut consumed = 1usize;
                if let Some(next) = lines.get(i + 1) {
                    let next_clean = strip_ansi(next);
                    let next_trim = next_clean.trim_end_matches(['\n', '\r']);
                    if let Some(loc) = RUSTC_LOC_RE.captures(next_trim) {
                        // path/line/col 은 RUSTC_LOC_RE 의 비선택 group → 매치 성공 시 항상 Some.
                        path = Some(loc.name("path").unwrap().as_str().to_string());
                        ln = loc.name("line").unwrap().as_str().parse().ok();
                        col = loc.name("col").unwrap().as_str().parse().ok();
                        consumed = 2;
                    }
                }
                out.push(ParsedItem {
                    kind: "compile_error",
                    line: i as u32,
                    byte_start: 0,
                    byte_end: line.len(),
                    data: json!({
                        "kind": "compile_error",
                        "tool": "rustc",
                        // sev/msg 는 RUSTC_HEAD_RE 의 비선택 group → unwrap 안전.
                        // code 는 선택적 `(?:\[...\])?` 이라 .map 으로 graceful.
                        "severity": caps.name("sev").unwrap().as_str(),
                        "code": caps.name("code").map(|m| m.as_str()),
                        "message": caps.name("msg").unwrap().as_str(),
                        "path": path,
                        "line_number": ln,
                        "column": col,
                    }),
                });
                i += consumed;
                continue;
            }

            // tsc
            if let Some(caps) = TSC_RE.captures(line) {
                out.push(ParsedItem {
                    kind: "compile_error",
                    line: i as u32,
                    byte_start: 0,
                    byte_end: line.len(),
                    data: json!({
                        "kind": "compile_error",
                        "tool": "tsc",
                        // TSC_RE 의 path/line/col/sev/code/msg 는 전부 비선택 group → unwrap 안전.
                        "severity": caps.name("sev").unwrap().as_str(),
                        "code": caps.name("code").unwrap().as_str(),
                        "message": caps.name("msg").unwrap().as_str(),
                        "path": caps.name("path").unwrap().as_str(),
                        "line_number": caps.name("line").unwrap().as_str().parse::<u32>().ok(),
                        "column": caps.name("col").unwrap().as_str().parse::<u32>().ok(),
                    }),
                });
                i += 1;
                continue;
            }

            // gcc/clang
            if let Some(caps) = GCC_RE.captures(line) {
                out.push(ParsedItem {
                    kind: "compile_error",
                    line: i as u32,
                    byte_start: 0,
                    byte_end: line.len(),
                    data: json!({
                        "kind": "compile_error",
                        "tool": "gcc",
                        // sev/msg/path/line 은 GCC_RE 의 비선택 group → unwrap 안전.
                        // col 은 선택적 `(?::(?P<col>...))?` 이라 .and_then 으로 graceful.
                        "severity": caps.name("sev").unwrap().as_str(),
                        "code": serde_json::Value::Null,
                        "message": caps.name("msg").unwrap().as_str(),
                        "path": caps.name("path").unwrap().as_str(),
                        "line_number": caps.name("line").unwrap().as_str().parse::<u32>().ok(),
                        "column": caps.name("col").and_then(|m| m.as_str().parse::<u32>().ok()),
                    }),
                });
                i += 1;
                continue;
            }

            i += 1;
        }
    }
}

// ============================================================
// stack_trace (multi-line)
// ============================================================

pub struct StackTraceParser;

static PY_TRACEBACK_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^Traceback \(most recent call last\):\s*$"#).unwrap());
static PY_FRAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*File "(?P<path>[^"]+)", line (?P<line>\d+)(?:, in (?P<func>.+))?\s*$"#)
        .unwrap()
});
static PY_EXC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<exc>[A-Z][A-Za-z0-9_.]*Error|[A-Z][A-Za-z0-9_.]*Exception|Exception|KeyboardInterrupt|SystemExit|StopIteration|GeneratorExit)(?::\s*(?P<msg>.+))?$").unwrap()
});

static RUST_PANIC_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    // thread 'main' panicked at 'msg', src/main.rs:10:5
    Regex::new(
        r"^thread\s+'(?P<thread>[^']+)'\s+panicked\s+at\s+(?:'(?P<msg>[^']*)',\s*)?(?P<path>[^:\s]+):(?P<line>\d+):(?P<col>\d+)\s*$",
    )
    .unwrap()
});
static RUST_BACKTRACE_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^stack backtrace:\s*$").unwrap());
static RUST_FRAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // "   0: foo::bar::baz"  또는 "   0: 0x55... - foo::bar"
    Regex::new(r"^\s*\d+:\s+(?:0x[0-9a-f]+\s+-\s+)?(?P<func>.+?)\s*$").unwrap()
});

static NODE_FRAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // "    at funcName (file:line:col)"  |  "    at file:line:col"
    // path 에 콜론이 포함될 수 있음 (예: node:internal/...). lazy 매칭 + 끝 anchor 로
    // 마지막 ":line:col" 두 그룹을 분리한다.
    Regex::new(
        r"^\s*at\s+(?:(?P<func>[^\s(]+)\s+\()?(?P<path>.+?):(?P<line>\d+):(?P<col>\d+)\)?\s*$",
    )
    .unwrap()
});

static JAVA_FRAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // "    at com.example.Foo.bar(Foo.java:42)"
    Regex::new(r"^\s*at\s+(?P<func>[\w.$<>]+)\((?P<file>[^:)]+)(?::(?P<line>\d+))?\)\s*$").unwrap()
});

impl Parser for StackTraceParser {
    fn id(&self) -> &'static str {
        "stack_trace"
    }

    fn parse_block(&self, text: &str, out: &mut Vec<ParsedItem>) {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let clean: Vec<String> = lines
            .iter()
            .map(|l| {
                let s = strip_ansi(l);
                s.trim_end_matches(['\n', '\r']).to_string()
            })
            .collect();

        let mut i = 0;
        while i < clean.len() {
            let line = &clean[i];

            // Python traceback
            if PY_TRACEBACK_HEAD.is_match(line) {
                let start = i;
                let mut frames = Vec::new();
                i += 1;
                while i < clean.len() {
                    if let Some(caps) = PY_FRAME_RE.captures(&clean[i]) {
                        frames.push(json!({
                            // path/line 은 비선택 group → unwrap 안전. func 는 선택적 → .map.
                            "path": caps.name("path").unwrap().as_str(),
                            "line": caps.name("line").unwrap().as_str().parse::<u32>().ok(),
                            "func": caps.name("func").map(|m| m.as_str().trim().to_string()),
                        }));
                        i += 1;
                        // 다음 줄이 코드 컨텍스트 (들여쓰기) 면 skip.
                        if i < clean.len()
                            && !PY_FRAME_RE.is_match(&clean[i])
                            && !PY_EXC_RE.is_match(&clean[i])
                            && clean[i].starts_with("    ")
                        {
                            i += 1;
                        }
                    } else {
                        break;
                    }
                }
                let (exception, message) = if i < clean.len()
                    && let Some(caps) = PY_EXC_RE.captures(&clean[i])
                {
                    // exc 는 PY_EXC_RE 의 비선택 group → unwrap 안전. msg 는 선택적 → .map.
                    let exc = caps.name("exc").unwrap().as_str().to_string();
                    let msg = caps.name("msg").map(|m| m.as_str().to_string());
                    i += 1;
                    (Some(exc), msg)
                } else {
                    (None, None)
                };
                out.push(ParsedItem {
                    kind: "stack_trace",
                    line: start as u32,
                    byte_start: 0,
                    byte_end: line.len(),
                    data: json!({
                        "kind": "stack_trace",
                        "language": "python",
                        "exception": exception,
                        "message": message,
                        "frames": frames,
                    }),
                });
                continue;
            }

            // Rust panic (단일 라인) + optional stack backtrace
            if let Some(caps) = RUST_PANIC_HEAD.captures(line) {
                let start = i;
                // thread/path/line/col 은 RUST_PANIC_HEAD 의 비선택 group → unwrap 안전.
                // msg 는 선택적 `(?:'...',\s*)?` 분기라 .map 으로 graceful.
                let panic_path = caps.name("path").unwrap().as_str().to_string();
                let panic_line: Option<u32> = caps.name("line").unwrap().as_str().parse().ok();
                let panic_col: Option<u32> = caps.name("col").unwrap().as_str().parse().ok();
                let thread = caps.name("thread").unwrap().as_str().to_string();
                let msg = caps.name("msg").map(|m| m.as_str().to_string());
                i += 1;
                let mut frames = Vec::new();
                if i < clean.len() && RUST_BACKTRACE_HEAD.is_match(&clean[i]) {
                    i += 1;
                    while i < clean.len() {
                        if let Some(fc) = RUST_FRAME_RE.captures(&clean[i]) {
                            frames.push(json!({
                                // func 는 RUST_FRAME_RE 의 비선택 group → unwrap 안전.
                                "func": fc.name("func").unwrap().as_str(),
                            }));
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
                out.push(ParsedItem {
                    kind: "stack_trace",
                    line: start as u32,
                    byte_start: 0,
                    byte_end: line.len(),
                    data: json!({
                        "kind": "stack_trace",
                        "language": "rust",
                        "thread": thread,
                        "message": msg,
                        "path": panic_path,
                        "line_number": panic_line,
                        "column": panic_col,
                        "frames": frames,
                    }),
                });
                continue;
            }

            // Node / Java — at-frame 연속 블록
            if NODE_FRAME_RE.is_match(line) || JAVA_FRAME_RE.is_match(line) {
                let start = i;
                let mut frames = Vec::new();
                let mut lang_hint = "node";
                while i < clean.len() {
                    let l = &clean[i];
                    if let Some(c) = NODE_FRAME_RE.captures(l) {
                        frames.push(json!({
                            // path/line/col 은 비선택 group → unwrap 안전. func 는 선택적 → .map.
                            "func": c.name("func").map(|m| m.as_str()),
                            "path": c.name("path").unwrap().as_str(),
                            "line": c.name("line").unwrap().as_str().parse::<u32>().ok(),
                            "col": c.name("col").unwrap().as_str().parse::<u32>().ok(),
                        }));
                        i += 1;
                    } else if let Some(c) = JAVA_FRAME_RE.captures(l) {
                        lang_hint = "java";
                        frames.push(json!({
                            // func/file 은 비선택 group → unwrap 안전. line 은 선택적 → .and_then.
                            "func": c.name("func").unwrap().as_str(),
                            "file": c.name("file").unwrap().as_str(),
                            "line": c.name("line").and_then(|m| m.as_str().parse::<u32>().ok()),
                        }));
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !frames.is_empty() {
                    out.push(ParsedItem {
                        kind: "stack_trace",
                        line: start as u32,
                        byte_start: 0,
                        byte_end: line.len(),
                        data: json!({
                            "kind": "stack_trace",
                            "language": lang_hint,
                            "frames": frames,
                        }),
                    });
                }
                continue;
            }

            i += 1;
        }
    }
}

// ============================================================
// test_result (single-line summaries, but parser-level)
// ============================================================
