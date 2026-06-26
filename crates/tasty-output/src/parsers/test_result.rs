//! tasty-output parsers — sub-module 별로 분리.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser, strip_ansi};

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
static PYTEST_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<n>\d+)\s+(?P<kind>passed|failed|skipped|error|errors|xfailed|xpassed|deselected|warnings?)").unwrap()
});

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
            // status/passed/failed 는 비선택 group → unwrap 안전.
            // ignored/measured/filtered/dur 는 선택적 → .and_then 으로 graceful.
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
            // body 는 비선택 group → unwrap 안전.
            let body = caps.name("body").unwrap().as_str();
            let mut counts = serde_json::Map::new();
            for c in PYTEST_TOKEN_RE.captures_iter(body) {
                // n/kind 는 PYTEST_TOKEN_RE 의 비선택 group → unwrap 안전.
                let n: u32 = c.name("n").unwrap().as_str().parse().unwrap_or(0);
                let kind = c.name("kind").unwrap().as_str();
                let key = if kind == "errors" { "error" } else { kind };
                counts.insert(key.to_string(), json!(n));
            }
            if counts.is_empty() {
                return;
            }
            let failed = counts.get("failed").and_then(|v| v.as_u64()).unwrap_or(0);
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
            // body 는 비선택 group → unwrap 안전.
            let body = caps.name("body").unwrap().as_str();
            let mut counts = serde_json::Map::new();
            for c in JEST_TOKEN_RE.captures_iter(body) {
                // n/kind 는 JEST_TOKEN_RE 의 비선택 group → unwrap 안전.
                let n: u32 = c.name("n").unwrap().as_str().parse().unwrap_or(0);
                let kind = c.name("kind").unwrap().as_str();
                counts.insert(kind.to_string(), json!(n));
            }
            if counts.is_empty() {
                return;
            }
            let failed = counts.get("failed").and_then(|v| v.as_u64()).unwrap_or(0);
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
