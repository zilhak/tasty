#![forbid(unsafe_code)]

//! Semantic output parsers for tasty surfaces.
//!
//! 라인 단위 stateless 파서들로 구성. `parse_buffer` 가 입력 텍스트를 줄로 쪼개
//! 각 활성 파서에 dispatch 하고 [`ParsedItem`] 들을 누적한다. ANSI escape 가
//! 섞인 raw 라인을 그대로 받으므로, 파서는 필요하면 자체적으로 strip 한다.
//!
//! 빌트인 4종 (`Phase 2.1` 기준):
//! - `path` — 파일 경로 (선택적 line/column suffix `:N` / `:N:C`)
//! - `url` — `http`/`https`/`ftp`/`ssh`/`file` URL
//! - `prompt_boundary` — OSC 133 `\x1b]133;A` / `B` / `C` / `D` 마커
//! - `exit_code` — OSC 133 `D;<code>` 페이로드

pub mod parsers;

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// 파싱된 의미 단위 한 건. `kind` 가 파서 ID 와 일치하며 `data` 는 파서별
/// 구조화된 페이로드 (JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedItem {
    /// 파서 id (`path`, `url`, ...).
    pub kind: &'static str,
    /// 0-based 라인 번호 (입력 텍스트 안에서).
    pub line: u32,
    /// 매치된 byte range (라인 내 offset). 멀티라인 페이로드는 첫 줄 기준.
    pub byte_start: usize,
    pub byte_end: usize,
    /// 파서별 구조화 페이로드.
    pub data: serde_json::Value,
}

/// 라인 단위 stateless 파서.
///
/// 단일 라인 파서는 [`parse_line`] 만 구현하면 된다 (기본 [`parse_block`] 이
/// 라인별 dispatch). 멀티라인 파서 (compile_error, stack_trace 등) 는
/// [`parse_block`] 을 override 하고 [`parse_line`] 은 no-op 으로 둔다.
/// 옵저버 스트리밍 경로는 [`parse_line`] 만 호출하므로, 멀티라인 파서는
/// `parse_buffer` (batch) 환경에서만 발화한다.
pub trait Parser: Send + Sync {
    /// 파서 id. CLI/IPC 의 `--parsers` 리스트에서 사용.
    fn id(&self) -> &'static str;

    /// 단일 라인 dispatch. stateless 단일 라인 파서가 구현.
    fn parse_line(&self, _line: &str, _line_idx: u32, _out: &mut Vec<ParsedItem>) {}

    /// 블록 dispatch. 기본은 `text` 를 `\n` 으로 쪼개 [`parse_line`] 반복.
    /// 멀티라인 컨텍스트가 필요한 파서는 override.
    fn parse_block(&self, text: &str, out: &mut Vec<ParsedItem>) {
        for (idx, line) in text.split_inclusive('\n').enumerate() {
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
            self.parse_line(trimmed, idx as u32, out);
        }
    }
}

/// 빌트인 파서 카탈로그. `ID` 문자열로 조회.
///
/// 처음 4종 (`path`/`url`/`prompt_boundary`/`exit_code`) 은 [`DEFAULT_PARSER_IDS`] 에
/// 포함되어 기본 활성화된다. 나머지 6종 (`compile_error`/`stack_trace`/`test_result`/
/// `progress`/`osc_link`/`osc_notification`) 은 false-positive 위험 또는 도메인 특수성
/// 때문에 명시적으로 opt-in 해야 한다.
pub fn registry() -> &'static [&'static dyn Parser] {
    static ENTRIES: LazyLock<Vec<&'static dyn Parser>> = LazyLock::new(|| {
        vec![
            &parsers::PathParser as &'static dyn Parser,
            &parsers::UrlParser,
            &parsers::PromptBoundaryParser,
            &parsers::ExitCodeParser,
            &parsers::CompileErrorParser,
            &parsers::StackTraceParser,
            &parsers::TestResultParser,
            &parsers::ProgressParser,
            &parsers::OscLinkParser,
            &parsers::OscNotificationParser,
        ]
    });
    &ENTRIES
}

/// 기본 활성 파서 id 들. CLI/IPC 가 `--parsers` 를 생략하면 이 리스트가 쓰인다.
pub const DEFAULT_PARSER_IDS: &[&str] = &["path", "url", "prompt_boundary", "exit_code"];

/// id 로 빌트인 파서 lookup. 없으면 `None`.
pub fn lookup(id: &str) -> Option<&'static dyn Parser> {
    registry().iter().copied().find(|p| p.id() == id)
}

/// `text` 를 라인 단위로 쪼개 활성화된 파서들로 dispatch. `parser_ids` 가
/// `None` 이면 [`DEFAULT_PARSER_IDS`] 사용. 알 수 없는 id 는 `Err`.
pub fn parse_buffer<'a, I>(text: &str, parser_ids: I) -> Result<Vec<ParsedItem>, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let parsers: Vec<&'static dyn Parser> = parser_ids
        .into_iter()
        .map(|id| lookup(id).ok_or_else(|| id.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(parse_buffer_with(text, &parsers))
}

/// `parse_buffer` 와 동일하나 미리 lookup 한 parser 슬라이스를 받는다.
/// 각 파서의 [`Parser::parse_block`] 을 호출 — 단일 라인 파서는 기본
/// 구현으로 라인별 dispatch, 멀티라인 파서는 자체 처리.
pub fn parse_buffer_with(text: &str, parsers: &[&'static dyn Parser]) -> Vec<ParsedItem> {
    let mut out = Vec::new();
    for p in parsers {
        p.parse_block(text, &mut out);
    }
    out
}

/// ANSI escape (`\x1b[...m`, `\x1b]...\x07`, `\x1b]...\x1b\\`) 를 제거한 라인을
/// 돌려준다. 파서 내부에서 plain text 매칭이 필요한 곳에서 사용. raw 라인의
/// byte offset 매핑은 보존되지 않는다 (offset 은 stripped 결과 기준).
///
/// 정규식은 `tasty-terminal` 의 `ANSI_ESCAPE_RE` 와 **문자 단위로 같다** —
/// 파라미터 문자군의 근거는 그쪽 주석에 있고, 동등은
/// `tests/strip_ansi_regex_parity.rs` 가 강제한다.
pub(crate) fn strip_ansi(s: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\x1b\[[0-9:;<=>?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").unwrap()
    });
    RE.replace_all(s, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_builtin_parsers() {
        let ids: Vec<&str> = registry().iter().map(|p| p.id()).collect();
        assert_eq!(
            ids,
            [
                "path",
                "url",
                "prompt_boundary",
                "exit_code",
                "compile_error",
                "stack_trace",
                "test_result",
                "progress",
                "osc_link",
                "osc_notification",
            ]
        );
    }

    #[test]
    fn default_parser_ids_is_subset_of_registry() {
        let registry_ids: std::collections::HashSet<&str> =
            registry().iter().map(|p| p.id()).collect();
        for id in DEFAULT_PARSER_IDS {
            assert!(registry_ids.contains(id), "missing default id: {id}");
        }
    }

    #[test]
    fn lookup_returns_some_for_known() {
        assert!(lookup("path").is_some());
        assert!(lookup("nonexistent").is_none());
    }

    #[test]
    fn parse_buffer_rejects_unknown_parser() {
        let err = parse_buffer("anything", ["bogus"]).unwrap_err();
        assert_eq!(err, "bogus");
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        let s = "\x1b[31mred\x1b[0m \x1b]0;title\x07after";
        assert_eq!(strip_ansi(s), "red after");
    }

    /// 파라미터 바이트는 `0x30-0x3F` 전체다. 파서가 먹는 것은 컴파일러 진단·테스트
    /// 출력·진행률인데, 최근 rustc/gcc 진단은 곱슬 밑줄 `\x1b[4:3m` 을 쓰고 nvim 은
    /// `\x1b[>4;1m` 을 방출한다 — 남으면 파서의 plain text 매칭이 그만큼 어긋난다.
    /// 사본이 둘이라 회귀 핀도 양쪽에 둔다.
    #[test]
    fn strip_ansi_removes_colon_and_private_prefix_parameters() {
        assert_eq!(strip_ansi("\x1b[4:3mwavy\x1b[0m"), "wavy");
        assert_eq!(strip_ansi("\x1b[38:2::255:0:0mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[>4;1mx"), "x");
        assert_eq!(strip_ansi("\x1b[=1cy"), "y");
        assert_eq!(strip_ansi("\x1b[<0;12;3Mz"), "z");
        // 본문의 같은 글자는 건드리지 않는다.
        assert_eq!(strip_ansi("a<b >c =d ratio 3:4"), "a<b >c =d ratio 3:4");
    }
}
