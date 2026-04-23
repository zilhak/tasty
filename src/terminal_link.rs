//! 터미널 셀 내용에서 클릭 가능한 링크(URL)를 검출한다.
//!
//! 두 가지 경로:
//! 1. **OSC 8 hyperlink**: termwiz `CellAttributes::hyperlink()`로 얻는다.
//!    셀에 이미 URI가 붙어 있으므로 그대로 사용.
//! 2. **Plain text URL**: 라인을 문자열로 이어 붙인 뒤 regex로 검출한다.
//!
//! 링크 좌표는 표시 컬럼(display column) 단위이며 와이드 문자는 2칸을 차지한다.
//! scrollback 라인과 screen 라인 양쪽을 지원한다.

use std::sync::OnceLock;

use regex::Regex;
use termwiz::cell::CellAttributes;

use crate::renderer::unicode_width;

/// 검출된 링크 1건.
#[derive(Debug, Clone)]
pub struct LinkSpan {
    /// 시작 컬럼(포함).
    pub start_col: usize,
    /// 끝 컬럼(포함).
    pub end_col: usize,
    /// 링크 대상 URI.
    pub uri: String,
    /// 절대 row (scrollback 0 = 가장 오래된 라인).
    pub absolute_row: usize,
}

impl LinkSpan {
    /// (col, absolute_row)가 이 링크 범위 안에 있는지.
    pub fn contains(&self, col: usize, absolute_row: usize) -> bool {
        self.absolute_row == absolute_row && col >= self.start_col && col <= self.end_col
    }
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // 스킴 + 호스트/경로. URL 끝에 붙을 수 있는 구두점(. , ; : ) ] } " ' !) 은 후처리에서 제거.
        Regex::new(r"(?i)\b(?:https?|ftp|file)://[^\s<>\[\]\{\}\\^`|]+")
            .expect("URL regex compile")
    })
}

/// URL 뒤에 딸려 붙기 쉬운 구두점을 잘라낸다.
/// 예: "see https://a.com/x." → "https://a.com/x"
fn trim_trailing_punct(s: &str) -> &str {
    let trimmed = s.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
    // 닫는 괄호는 짝이 맞을 때만 보존.
    let mut end = trimmed.len();
    for (ch, open) in [(')', '('), (']', '['), ('}', '{'), ('>', '<')] {
        while trimmed[..end].ends_with(ch) {
            let inner = &trimmed[..end - 1];
            let opens = inner.chars().filter(|c| *c == open).count();
            let closes = inner.chars().filter(|c| *c == ch).count();
            if closes >= opens {
                end -= 1;
            } else {
                break;
            }
        }
    }
    &trimmed[..end]
}

/// scrollback 라인(Vec<(String, CellAttributes)>)에서 링크를 검출한다.
pub fn detect_scrollback_line(
    line: &[(String, CellAttributes)],
    absolute_row: usize,
) -> Vec<LinkSpan> {
    // 1) OSC 8 hyperlink 수집 (연속된 동일 uri 셀을 하나로 묶음).
    let mut result = Vec::new();
    let mut col_of_byte: Vec<usize> = Vec::with_capacity(line.len());
    let mut text = String::new();
    let mut col: usize = 0;
    let mut current: Option<(String, usize, usize)> = None; // (uri, start_col, end_col)
    for (cell_text, attrs) in line {
        col_of_byte.extend(std::iter::repeat(col).take(cell_text.len()));
        text.push_str(cell_text);
        let ch = cell_text.chars().next().unwrap_or(' ');
        let width = unicode_width(ch);
        let end_col = col + width.saturating_sub(1);
        let uri = attrs.hyperlink().map(|h| h.uri().to_string());
        match (&mut current, uri) {
            (Some(cur), Some(u)) if cur.0 == u => cur.2 = end_col,
            (Some(cur), Some(u)) => {
                result.push(LinkSpan {
                    start_col: cur.1,
                    end_col: cur.2,
                    uri: cur.0.clone(),
                    absolute_row,
                });
                *cur = (u, col, end_col);
            }
            (Some(cur), None) => {
                result.push(LinkSpan {
                    start_col: cur.1,
                    end_col: cur.2,
                    uri: cur.0.clone(),
                    absolute_row,
                });
                current = None;
            }
            (None, Some(u)) => current = Some((u, col, end_col)),
            (None, None) => {}
        }
        col += width;
    }
    if let Some(cur) = current {
        result.push(LinkSpan {
            start_col: cur.1,
            end_col: cur.2,
            uri: cur.0,
            absolute_row,
        });
    }

    // 2) 일반 텍스트에서 URL regex 검출. OSC8 범위와 겹치지 않는 경우만 추가.
    append_regex_matches(&text, &col_of_byte, absolute_row, &mut result);
    result
}

/// screen 라인(termwiz Line)에서 링크를 검출한다.
pub fn detect_screen_line(
    line: &termwiz::surface::line::Line,
    absolute_row: usize,
) -> Vec<LinkSpan> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut col_of_byte: Vec<usize> = Vec::new();
    let mut current: Option<(String, usize, usize)> = None;
    for cell_ref in line.visible_cells() {
        let col = cell_ref.cell_index();
        let cell_text = cell_ref.str();
        col_of_byte.extend(std::iter::repeat(col).take(cell_text.len()));
        text.push_str(cell_text);
        let ch = cell_text.chars().next().unwrap_or(' ');
        let width = unicode_width(ch);
        let end_col = col + width.saturating_sub(1);
        let uri = cell_ref.attrs().hyperlink().map(|h| h.uri().to_string());
        match (&mut current, uri) {
            (Some(cur), Some(u)) if cur.0 == u => cur.2 = end_col,
            (Some(cur), Some(u)) => {
                result.push(LinkSpan {
                    start_col: cur.1,
                    end_col: cur.2,
                    uri: cur.0.clone(),
                    absolute_row,
                });
                *cur = (u, col, end_col);
            }
            (Some(cur), None) => {
                result.push(LinkSpan {
                    start_col: cur.1,
                    end_col: cur.2,
                    uri: cur.0.clone(),
                    absolute_row,
                });
                current = None;
            }
            (None, Some(u)) => current = Some((u, col, end_col)),
            (None, None) => {}
        }
    }
    if let Some(cur) = current {
        result.push(LinkSpan {
            start_col: cur.1,
            end_col: cur.2,
            uri: cur.0,
            absolute_row,
        });
    }

    append_regex_matches(&text, &col_of_byte, absolute_row, &mut result);
    result
}

fn append_regex_matches(
    text: &str,
    col_of_byte: &[usize],
    absolute_row: usize,
    out: &mut Vec<LinkSpan>,
) {
    for m in url_regex().find_iter(text) {
        let raw = m.as_str();
        let trimmed = trim_trailing_punct(raw);
        if trimmed.is_empty() {
            continue;
        }
        let start_byte = m.start();
        let end_byte = start_byte + trimmed.len();
        // col_of_byte는 셀 첫 바이트에만 column을 찍고 나머지는 동일한 값이 쌓임.
        // end_byte - 1에 해당하는 문자의 끝 column을 구해야 정확함.
        let Some(&start_col) = col_of_byte.get(start_byte) else {
            continue;
        };
        let last_byte = end_byte.saturating_sub(1);
        let Some(&last_start_col) = col_of_byte.get(last_byte) else {
            continue;
        };
        // 마지막 셀의 width 계산: 원문에서 해당 char를 얻어야 함.
        let end_ch = text[..end_byte]
            .chars()
            .next_back()
            .unwrap_or(' ');
        let end_col = last_start_col + unicode_width(end_ch).saturating_sub(1);

        // 이미 OSC8이 덮고 있으면 스킵.
        let overlap = out.iter().any(|s| {
            s.absolute_row == absolute_row
                && !(end_col < s.start_col || start_col > s.end_col)
        });
        if overlap {
            continue;
        }
        out.push(LinkSpan {
            start_col,
            end_col,
            uri: trimmed.to_string(),
            absolute_row,
        });
    }
}

/// 주어진 (col, absolute_row)에 있는 링크를 터미널에서 찾는다.
/// scrollback과 screen 양쪽을 처리.
pub fn link_at(
    terminal: &tasty_terminal::Terminal,
    col: usize,
    absolute_row: usize,
) -> Option<LinkSpan> {
    let scrollback_len = terminal.scrollback_len();
    let spans = if absolute_row < scrollback_len {
        let line = terminal.scrollback_line_owned(absolute_row)?;
        detect_scrollback_line(&line, absolute_row)
    } else {
        let screen_row = absolute_row - scrollback_len;
        let surface = terminal.surface();
        let lines = surface.screen_lines();
        let line = lines.get(screen_row)?;
        detect_screen_line(line, absolute_row)
    };
    spans.into_iter().find(|s| s.contains(col, absolute_row))
}

/// 렌더러에 전달하는 링크 하이라이트 정보. hovered된 단일 링크를
/// 해당 셀 범위에 대해 fg/bg 색으로 오버라이드한다.
#[derive(Debug, Clone)]
pub struct LinkHighlight {
    pub start_col: usize,
    pub end_col: usize,
    pub absolute_row: usize,
    pub fg: [f32; 4],
    pub bg: [f32; 4],
}

impl LinkHighlight {
    pub fn covers(&self, col: usize, absolute_row: usize) -> bool {
        self.absolute_row == absolute_row && col >= self.start_col && col <= self.end_col
    }
}

/// URI를 기본 브라우저/연결 프로그램으로 연다. 크로스 플랫폼.
/// 성공하면 true, 실패하면 `tracing::warn!`을 남기고 false.
pub fn open_uri(uri: &str) -> bool {
    match webbrowser::open(uri) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("failed to open link {:?}: {}", uri, e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_trailing_punct() {
        assert_eq!(trim_trailing_punct("https://a.com/x."), "https://a.com/x");
        assert_eq!(trim_trailing_punct("https://a.com/x)."), "https://a.com/x");
        assert_eq!(
            trim_trailing_punct("https://a.com/(x)"),
            "https://a.com/(x)"
        );
        assert_eq!(
            trim_trailing_punct("https://a.com/x?q=1!"),
            "https://a.com/x?q=1"
        );
    }
}
