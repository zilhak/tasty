#![forbid(unsafe_code)]

//! 터미널 출력에서 경로·URL·셸 마커·오류 등을 추출한다.
//! ANSI를 포함한 문자열을 받으며 필요한 파서가 escape를 제거한다.
//! 기본 파서 네 종류와 명시적으로 선택할 파서 여섯 종류는 registry에 등록한다.

pub mod parsers;

use std::sync::LazyLock;

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

/// 상태를 보관하지 않는 출력 파서. 단일 행 파서는 parse_line을, 여러 행의 문맥이
/// 필요한 파서는 parse_block을 구현한다. 스트리밍 옵저버는 parse_line만 호출하므로
/// 여러 행 파서는 배치 parse_buffer에서 사용한다.
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

/// 지정한 파서로 text를 처리한다. 알 수 없는 ID는 Err로 반환한다.
/// 기본 목록을 쓰려면 호출자가 DEFAULT_PARSER_IDS를 전달한다.
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

/// tasty-ansi의 공통 구현으로 ANSI escape를 제거한다.
pub(crate) use tasty_ansi::strip_ansi;

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
}
