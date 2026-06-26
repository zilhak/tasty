//! tasty-output parsers — sub-module 별로 분리.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser, strip_ansi};

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
            // group 0/bar/pct 는 전부 비선택 group → unwrap 안전. parse 는 unwrap_or 로 graceful.
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
            // cur/u1/tot/u2 + group 0 은 전부 비선택 group → unwrap 안전.
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
            // group 0/pct 는 비선택 group → unwrap 안전.
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
