//! tasty-output parsers — sub-module 별로 분리.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::{ParsedItem, Parser};

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

