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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_buffer;

    fn first<'a>(items: &'a [ParsedItem], kind: &str) -> &'a ParsedItem {
        items.iter().find(|i| i.kind == kind).expect("no item")
    }

    // ----- path -----

    #[test]
    fn path_basic_with_line_col() {
        let items = parse_buffer("error at src/main.rs:42:7 ok", ["path"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["path"], "src/main.rs");
        assert_eq!(items[0].data["line"], 42);
        assert_eq!(items[0].data["col"], 7);
    }

    #[test]
    fn path_line_only() {
        let items = parse_buffer("see src/main.rs:42 ok", ["path"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["path"], "src/main.rs");
        assert_eq!(items[0].data["line"], 42);
        assert!(items[0].data["col"].is_null());
    }

    #[test]
    fn path_no_suffix() {
        let items = parse_buffer("touch README.md ok", ["path"]).unwrap();
        let p = first(&items, "path");
        assert_eq!(p.data["path"], "README.md");
        assert!(p.data["line"].is_null());
    }

    #[test]
    fn path_skips_short_or_non_path() {
        let items = parse_buffer("hello world test", ["path"]).unwrap();
        // "hello"/"world"/"test" 는 separator/확장자 없음 → skip.
        assert!(
            items.is_empty(),
            "expected no paths, got: {:?}",
            items.iter().map(|i| &i.data).collect::<Vec<_>>()
        );
    }

    // ----- url -----

    #[test]
    fn url_https() {
        let items = parse_buffer("see https://example.com/path?x=1", ["url"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["url"], "https://example.com/path?x=1");
        assert_eq!(items[0].data["scheme"], "https");
    }

    #[test]
    fn url_strips_trailing_punct() {
        let items = parse_buffer("visit https://example.com.", ["url"]).unwrap();
        assert_eq!(items[0].data["url"], "https://example.com");
    }

    #[test]
    fn url_ssh_scheme() {
        let items = parse_buffer("clone ssh://git@host/repo.git", ["url"]).unwrap();
        assert_eq!(items[0].data["scheme"], "ssh");
    }

    // ----- prompt_boundary -----

    #[test]
    fn prompt_boundary_a_b_c_d() {
        let text = "\x1b]133;A\x07prompt\x1b]133;B\x07cmd\x1b]133;C\x07out\x1b]133;D;0\x07";
        let items = parse_buffer(text, ["prompt_boundary"]).unwrap();
        let phases: Vec<&str> = items
            .iter()
            .map(|i| i.data["phase"].as_str().unwrap())
            .collect();
        assert_eq!(phases, ["A", "B", "C", "D"]);
    }

    #[test]
    fn prompt_boundary_st_terminator() {
        let text = "\x1b]133;A\x1b\\";
        let items = parse_buffer(text, ["prompt_boundary"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["phase"], "A");
    }

    // ----- exit_code -----

    #[test]
    fn exit_code_zero() {
        let items = parse_buffer("\x1b]133;D;0\x07", ["exit_code"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["code"], 0);
    }

    #[test]
    fn exit_code_nonzero_with_extra_token() {
        let items = parse_buffer("\x1b]133;D;127;some-extra\x07", ["exit_code"]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].data["code"], 127);
    }

    // ----- ordering & multi-parser -----

    #[test]
    fn multiple_parsers_in_single_buffer() {
        let text = "error in src/main.rs:42:7\nsee https://example.com/docs\n";
        let items = parse_buffer(text, ["path", "url"]).unwrap();
        assert!(items.iter().any(|i| i.kind == "path"));
        assert!(items.iter().any(|i| i.kind == "url"));
    }

    #[test]
    fn line_numbers_reflect_input_order() {
        let text = "a\nb\nsrc/main.rs\n";
        let items = parse_buffer(text, ["path"]).unwrap();
        let p = first(&items, "path");
        assert_eq!(p.line, 2);
    }
}
