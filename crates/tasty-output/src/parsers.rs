//! Builtin parsers (Phase 2.1).
//!
//! - `path` — `src/main.rs`, `src/main.rs:42`, `src/main.rs:42:7`
//! - `url` — `http://`, `https://`, `ftp://`, `ssh://`, `file://`
//! - `prompt_boundary` — OSC 133 (`\x1b]133;A|B|C|D ...\x07` or `\x1b\\`)
//! - `exit_code` — OSC 133 `D;<code>` (선택적 `;` 추가 토큰)

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser, strip_ansi};

// ============================================================
// path
// ============================================================

pub struct PathParser;

static PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 경로 후보: 슬래시·점·하이픈·언더스코어·영숫자 시퀀스, 슬래시 또는 확장자 점 포함.
    // 끝에 옵션 `:LINE` 또는 `:LINE:COL` 캡처.
    Regex::new(
        r"(?x)
        (?P<path>
            (?:\.{1,2}/|/|[A-Za-z]:[\\/])?            # 선택적 root: ./  ../  /  C:/
            (?:[A-Za-z0-9._\-]+[\\/])*                # 디렉터리 세그먼트
            [A-Za-z0-9_\-]+                           # 마지막 segment 기본
            (?:\.[A-Za-z0-9]{1,6})?                   # 선택적 확장자
        )
        (?::(?P<line>\d+)(?::(?P<col>\d+))?)?
        ",
    )
    .unwrap()
});

impl Parser for PathParser {
    fn id(&self) -> &'static str {
        "path"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        let line = strip_ansi(raw);
        for caps in PATH_RE.captures_iter(&line) {
            let m = caps.name("path").unwrap();
            let s = m.as_str();
            // 너무 짧거나 디렉터리 separator + 확장자 도 없으면 휴리스틱 제외.
            if s.len() < 3 {
                continue;
            }
            let looks_path = s.contains('/') || s.contains('\\') || s.contains('.');
            if !looks_path {
                continue;
            }
            // 한 글자 segment (예: "a.b") 도 일단 받지만 모두 영문자/숫자만이면 path 같지
            // 않을 때 — 점 표기 식별자와 구분이 어렵다. 디렉터리 separator 가 있으면
            // 무조건 path 로 본다. 없고 확장자만 있으면 last segment 가 3+ 자일 때만.
            let has_sep = s.contains('/') || s.contains('\\');
            if !has_sep {
                let last_dot = s.rfind('.');
                let Some(dot) = last_dot else {
                    continue;
                };
                let basename_len = dot;
                if basename_len < 2 {
                    continue;
                }
            }
            let line_n = caps.name("line").and_then(|m| m.as_str().parse::<u32>().ok());
            let col_n = caps.name("col").and_then(|m| m.as_str().parse::<u32>().ok());
            out.push(ParsedItem {
                kind: "path",
                line: line_idx,
                byte_start: m.start(),
                byte_end: caps.get(0).unwrap().end(),
                data: json!({
                    "kind": "path",
                    "path": s,
                    "line": line_n,
                    "col": col_n,
                }),
            });
        }
    }
}

// ============================================================
// url
// ============================================================

pub struct UrlParser;

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?P<scheme>https?|ftp|ssh|file)://[^\s'"<>()\[\]]+"#).unwrap()
});

impl Parser for UrlParser {
    fn id(&self) -> &'static str {
        "url"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        let line = strip_ansi(raw);
        for caps in URL_RE.captures_iter(&line) {
            let m = caps.get(0).unwrap();
            // trailing punctuation 정리: 마침표/쉼표/세미콜론/콜론은 URL 의도 X.
            let raw_match = m.as_str();
            let trimmed = raw_match.trim_end_matches(|c: char| ".,;:!?)\"'".contains(c));
            let trim_diff = raw_match.len() - trimmed.len();
            out.push(ParsedItem {
                kind: "url",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end() - trim_diff,
                data: json!({
                    "kind": "url",
                    "url": trimmed,
                    "scheme": caps.name("scheme").unwrap().as_str(),
                }),
            });
        }
    }
}

// ============================================================
// prompt_boundary (OSC 133)
// ============================================================

pub struct PromptBoundaryParser;

static PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    // OSC 133 ; <A|B|C|D> [; payload ...] (BEL or ST terminator).
    // raw 라인 (ANSI 미제거) 에 대해 매칭한다.
    Regex::new(r"\x1b\]133;(?P<phase>[ABCD])(?:;(?P<payload>[^\x07\x1b]*))?(?:\x07|\x1b\\)")
        .unwrap()
});

impl Parser for PromptBoundaryParser {
    fn id(&self) -> &'static str {
        "prompt_boundary"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        for caps in PROMPT_RE.captures_iter(raw) {
            let m = caps.get(0).unwrap();
            let phase = caps.name("phase").unwrap().as_str();
            let payload = caps.name("payload").map(|m| m.as_str());
            out.push(ParsedItem {
                kind: "prompt_boundary",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "prompt_boundary",
                    "phase": phase,
                    "payload": payload,
                }),
            });
        }
    }
}

// ============================================================
// exit_code (OSC 133 D;<code>)
// ============================================================

pub struct ExitCodeParser;

static EXIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\]133;D;(?P<code>-?\d+)(?:;[^\x07\x1b]*)?(?:\x07|\x1b\\)").unwrap()
});

impl Parser for ExitCodeParser {
    fn id(&self) -> &'static str {
        "exit_code"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        for caps in EXIT_RE.captures_iter(raw) {
            let m = caps.get(0).unwrap();
            let code: i32 = caps.name("code").unwrap().as_str().parse().unwrap_or(0);
            out.push(ParsedItem {
                kind: "exit_code",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "exit_code",
                    "code": code,
                }),
            });
        }
    }
}

// ============================================================
// osc_link (OSC 8)
// ============================================================

pub struct OscLinkParser;

static OSC_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    // \x1b]8;<params>;<url>(BEL|ST)<text>\x1b]8;;(BEL|ST)
    // params 는 비어있거나 key=value;... 형태일 수 있음.
    Regex::new(
        r"\x1b\]8;(?P<params>[^;\x07\x1b]*);(?P<url>[^\x07\x1b]*)(?:\x07|\x1b\\)(?P<text>[^\x1b]*)\x1b\]8;;(?:\x07|\x1b\\)",
    )
    .unwrap()
});

impl Parser for OscLinkParser {
    fn id(&self) -> &'static str {
        "osc_link"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        for caps in OSC_LINK_RE.captures_iter(raw) {
            let m = caps.get(0).unwrap();
            let url = caps.name("url").unwrap().as_str();
            let text = caps.name("text").unwrap().as_str();
            let params = caps.name("params").unwrap().as_str();
            out.push(ParsedItem {
                kind: "osc_link",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "osc_link",
                    "url": url,
                    "text": text,
                    "params": if params.is_empty() { None } else { Some(params) },
                }),
            });
        }
    }
}

// ============================================================
// osc_notification (OSC 9 / OSC 777)
// ============================================================

pub struct OscNotificationParser;

static OSC_NOTIFY_RE: LazyLock<Regex> = LazyLock::new(|| {
    // OSC 9 ; body  (iTerm2 style user notification)
    // OSC 777 ; notify ; title ; body  (rxvt-unicode)
    Regex::new(r"\x1b\](?P<id>9|777);(?P<body>[^\x07\x1b]*)(?:\x07|\x1b\\)").unwrap()
});

impl Parser for OscNotificationParser {
    fn id(&self) -> &'static str {
        "osc_notification"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        for caps in OSC_NOTIFY_RE.captures_iter(raw) {
            let m = caps.get(0).unwrap();
            let id = caps.name("id").unwrap().as_str();
            let body = caps.name("body").unwrap().as_str();
            // OSC 777 은 `notify;title;body` 형태가 표준. 분해 시도.
            let (kind_field, title, message) = if id == "777" {
                let parts: Vec<&str> = body.splitn(3, ';').collect();
                match parts.as_slice() {
                    [action, title, msg] => (Some(*action), Some(*title), Some(*msg)),
                    [action, title] => (Some(*action), Some(*title), None),
                    [action] => (Some(*action), None, None),
                    _ => (None, None, None),
                }
            } else {
                (None, None, Some(body))
            };
            out.push(ParsedItem {
                kind: "osc_notification",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "osc_notification",
                    "osc": id.parse::<u32>().ok(),
                    "action": kind_field,
                    "title": title,
                    "message": message.or(Some(body)),
                }),
            });
        }
    }
}

// ============================================================
// progress
// ============================================================

pub struct ProgressParser;

static PROGRESS_BAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    // [#####>....]  N%  / [====]   45.2%
    Regex::new(r"\[(?P<bar>[#=>\-\.\s]{3,})\]\s*(?P<pct>\d{1,3}(?:\.\d+)?)\s*%").unwrap()
});

static PROGRESS_PCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<pct>\d{1,3}(?:\.\d+)?)\s*%").unwrap());

static PROGRESS_SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 50 MB / 200 MB,  50MiB/200MiB,  50.5 KB / 1.2 GB
    Regex::new(
        r"(?P<cur>\d+(?:\.\d+)?)\s*(?P<u1>[KMGTP]i?B|B)\s*/\s*(?P<tot>\d+(?:\.\d+)?)\s*(?P<u2>[KMGTP]i?B|B)",
    )
    .unwrap()
});

impl Parser for ProgressParser {
    fn id(&self) -> &'static str {
        "progress"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        let line = strip_ansi(raw);

        // 1) bar + percent (가장 구체적)
        if let Some(caps) = PROGRESS_BAR_RE.captures(&line) {
            let m = caps.get(0).unwrap();
            let pct: f64 = caps.name("pct").unwrap().as_str().parse().unwrap_or(0.0);
            out.push(ParsedItem {
                kind: "progress",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "progress",
                    "style": "bar",
                    "percent": pct,
                    "bar": caps.name("bar").unwrap().as_str(),
                }),
            });
            return;
        }

        // 2) size / total
        if let Some(caps) = PROGRESS_SIZE_RE.captures(&line) {
            let m = caps.get(0).unwrap();
            let cur: f64 = caps.name("cur").unwrap().as_str().parse().unwrap_or(0.0);
            let tot: f64 = caps.name("tot").unwrap().as_str().parse().unwrap_or(0.0);
            let pct = if tot > 0.0 {
                Some((cur / tot * 100.0).round())
            } else {
                None
            };
            out.push(ParsedItem {
                kind: "progress",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "progress",
                    "style": "size",
                    "current": cur,
                    "current_unit": caps.name("u1").unwrap().as_str(),
                    "total": tot,
                    "total_unit": caps.name("u2").unwrap().as_str(),
                    "percent": pct,
                }),
            });
            return;
        }

        // 3) 단순 N% — false-positive 가 높으므로 라인이 짧고 % 가 끝에 가까울 때만.
        if line.len() <= 80
            && let Some(caps) = PROGRESS_PCT_RE.captures(&line)
        {
            let m = caps.get(0).unwrap();
            // 백분율 뒤에 다른 토큰이 거의 없을 때만.
            let tail = line[m.end()..].trim();
            if tail.len() > 8 {
                return;
            }
            let pct: f64 = caps.name("pct").unwrap().as_str().parse().unwrap_or(0.0);
            if pct > 100.0 {
                return;
            }
            out.push(ParsedItem {
                kind: "progress",
                line: line_idx,
                byte_start: m.start(),
                byte_end: m.end(),
                data: json!({
                    "kind": "progress",
                    "style": "percent",
                    "percent": pct,
                }),
            });
        }
    }
}

// ============================================================
// compile_error (multi-line)
// ============================================================

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
static PY_EXC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<exc>[A-Z][A-Za-z0-9_.]*Error|[A-Z][A-Za-z0-9_.]*Exception|Exception|KeyboardInterrupt|SystemExit|StopIteration|GeneratorExit)(?::\s*(?P<msg>.+))?$").unwrap());

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
    Regex::new(
        r"^\s*at\s+(?P<func>[\w.$<>]+)\((?P<file>[^:)]+)(?::(?P<line>\d+))?\)\s*$",
    )
    .unwrap()
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
                let panic_path = caps.name("path").unwrap().as_str().to_string();
                let panic_line: Option<u32> =
                    caps.name("line").unwrap().as_str().parse().ok();
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
                            "func": c.name("func").map(|m| m.as_str()),
                            "path": c.name("path").unwrap().as_str(),
                            "line": c.name("line").unwrap().as_str().parse::<u32>().ok(),
                            "col": c.name("col").unwrap().as_str().parse::<u32>().ok(),
                        }));
                        i += 1;
                    } else if let Some(c) = JAVA_FRAME_RE.captures(l) {
                        lang_hint = "java";
                        frames.push(json!({
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

pub struct TestResultParser;

static CARGO_TEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    // test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
    Regex::new(
        r"^test result:\s*(?P<status>ok|FAILED)\.\s*(?P<passed>\d+)\s+passed;\s*(?P<failed>\d+)\s+failed(?:;\s*(?P<ignored>\d+)\s+ignored)?(?:;\s*(?P<measured>\d+)\s+measured)?(?:;\s*(?P<filtered>\d+)\s+filtered out)?(?:;\s*finished in\s*(?P<dur>[0-9.]+)s)?",
    )
    .unwrap()
});

static PYTEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    // ===== 5 passed, 1 failed, 2 skipped in 0.34s =====
    // 또는 ===== 5 passed in 0.12s =====
    Regex::new(r"=+\s*(?P<body>[^=]+?)\s+in\s+(?P<dur>[0-9.]+)\s*s\s*=+").unwrap()
});
static PYTEST_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<n>\d+)\s+(?P<kind>passed|failed|skipped|error|errors|xfailed|xpassed|deselected|warnings?)").unwrap());

static JEST_TESTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    // "Tests:       1 failed, 2 passed, 3 total"
    Regex::new(r"^Tests:\s+(?P<body>.+?)$").unwrap()
});
static JEST_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<n>\d+)\s+(?P<kind>passed|failed|skipped|todo|total)").unwrap()
});

impl Parser for TestResultParser {
    fn id(&self) -> &'static str {
        "test_result"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        let stripped = strip_ansi(raw);
        let line = stripped.trim();

        if let Some(caps) = CARGO_TEST_RE.captures(line) {
            let status = caps.name("status").unwrap().as_str();
            out.push(ParsedItem {
                kind: "test_result",
                line: line_idx,
                byte_start: 0,
                byte_end: raw.len(),
                data: json!({
                    "kind": "test_result",
                    "framework": "cargo",
                    "status": if status == "ok" { "passed" } else { "failed" },
                    "passed": caps.name("passed").unwrap().as_str().parse::<u32>().ok(),
                    "failed": caps.name("failed").unwrap().as_str().parse::<u32>().ok(),
                    "ignored": caps.name("ignored").and_then(|m| m.as_str().parse::<u32>().ok()),
                    "measured": caps.name("measured").and_then(|m| m.as_str().parse::<u32>().ok()),
                    "filtered": caps.name("filtered").and_then(|m| m.as_str().parse::<u32>().ok()),
                    "duration_seconds": caps.name("dur").and_then(|m| m.as_str().parse::<f64>().ok()),
                }),
            });
            return;
        }

        if let Some(caps) = PYTEST_RE.captures(line) {
            let body = caps.name("body").unwrap().as_str();
            let mut counts = serde_json::Map::new();
            for c in PYTEST_TOKEN_RE.captures_iter(body) {
                let n: u32 = c.name("n").unwrap().as_str().parse().unwrap_or(0);
                let kind = c.name("kind").unwrap().as_str();
                let key = if kind == "errors" { "error" } else { kind };
                counts.insert(key.to_string(), json!(n));
            }
            if counts.is_empty() {
                return;
            }
            let failed = counts
                .get("failed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let errors = counts.get("error").and_then(|v| v.as_u64()).unwrap_or(0);
            let status = if failed > 0 || errors > 0 {
                "failed"
            } else {
                "passed"
            };
            out.push(ParsedItem {
                kind: "test_result",
                line: line_idx,
                byte_start: 0,
                byte_end: raw.len(),
                data: json!({
                    "kind": "test_result",
                    "framework": "pytest",
                    "status": status,
                    "counts": counts,
                    "duration_seconds": caps.name("dur").and_then(|m| m.as_str().parse::<f64>().ok()),
                }),
            });
            return;
        }

        if let Some(caps) = JEST_TESTS_RE.captures(line) {
            let body = caps.name("body").unwrap().as_str();
            let mut counts = serde_json::Map::new();
            for c in JEST_TOKEN_RE.captures_iter(body) {
                let n: u32 = c.name("n").unwrap().as_str().parse().unwrap_or(0);
                let kind = c.name("kind").unwrap().as_str();
                counts.insert(kind.to_string(), json!(n));
            }
            if counts.is_empty() {
                return;
            }
            let failed = counts
                .get("failed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let status = if failed > 0 { "failed" } else { "passed" };
            out.push(ParsedItem {
                kind: "test_result",
                line: line_idx,
                byte_start: 0,
                byte_end: raw.len(),
                data: json!({
                    "kind": "test_result",
                    "framework": "jest",
                    "status": status,
                    "counts": counts,
                }),
            });
        }
    }
}


#[cfg(test)]
#[path = "parsers_tests.rs"]
mod tests;
