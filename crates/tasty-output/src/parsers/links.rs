//! tasty-output parsers — sub-module 별로 분리.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser, strip_ansi};

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
            // path 는 PATH_RE 의 최상위 비선택 group (마지막 segment `+` 필수) → unwrap 안전.
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
            let line_n = caps
                .name("line")
                .and_then(|m| m.as_str().parse::<u32>().ok());
            let col_n = caps
                .name("col")
                .and_then(|m| m.as_str().parse::<u32>().ok());
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

static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?P<scheme>https?|ftp|ssh|file)://[^\s'"<>()\[\]]+"#).unwrap());

impl Parser for UrlParser {
    fn id(&self) -> &'static str {
        "url"
    }

    fn parse_line(&self, raw: &str, line_idx: u32, out: &mut Vec<ParsedItem>) {
        let line = strip_ansi(raw);
        for caps in URL_RE.captures_iter(&line) {
            // group 0 (전체 매치) + scheme 은 항상 참여 → unwrap 안전.
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
            // group 0/params/url/text 는 전부 비선택 group (`*` 라도 group 은 참여) → unwrap 안전.
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
