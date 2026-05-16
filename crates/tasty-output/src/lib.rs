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

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

pub mod parsers;

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
pub trait Parser: Send + Sync {
    /// 파서 id. CLI/IPC 의 `--parsers` 리스트에서 사용.
    fn id(&self) -> &'static str;

    /// `line_idx` 번 라인을 파싱해 발견한 항목을 `out` 에 push.
    fn parse_line(&self, line: &str, line_idx: u32, out: &mut Vec<ParsedItem>);
}

/// 빌트인 파서 카탈로그. `ID` 문자열로 조회.
pub fn registry() -> &'static [&'static dyn Parser] {
    static ENTRIES: LazyLock<Vec<&'static dyn Parser>> = LazyLock::new(|| {
        vec![
            &parsers::PathParser as &'static dyn Parser,
            &parsers::UrlParser,
            &parsers::PromptBoundaryParser,
            &parsers::ExitCodeParser,
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
pub fn parse_buffer_with(text: &str, parsers: &[&'static dyn Parser]) -> Vec<ParsedItem> {
    let mut out = Vec::new();
    for (idx, line) in text.split_inclusive('\n').enumerate() {
        // `split_inclusive` 가 trailing `\n` 도 줄 끝에 남기므로 strip.
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
        for p in parsers {
            p.parse_line(trimmed, idx as u32, &mut out);
        }
    }
    out
}

/// ANSI escape (`\x1b[...m`, `\x1b]...\x07`, `\x1b]...\x1b\\`) 를 제거한 라인을
/// 돌려준다. 파서 내부에서 plain text 매칭이 필요한 곳에서 사용. raw 라인의
/// byte offset 매핑은 보존되지 않는다 (offset 은 stripped 결과 기준).
pub(crate) fn strip_ansi(s: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").unwrap()
    });
    RE.replace_all(s, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_four_parsers() {
        let ids: Vec<&str> = registry().iter().map(|p| p.id()).collect();
        assert_eq!(ids, ["path", "url", "prompt_boundary", "exit_code"]);
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
}
