//! 터미널 셀 내용에서 클릭 가능한 링크(URL)를 검출한다.
//!
//! 두 가지 경로:
//! 1. **OSC 8 hyperlink**: termwiz `CellAttributes::hyperlink()`로 얻는다.
//!    셀에 이미 URI가 붙어 있으므로 그대로 사용.
//! 2. **Plain text URL**: 라인을 문자열로 이어 붙인 뒤 regex로 검출한다.
//!
//! 링크 좌표는 표시 컬럼(display column) 단위이며 와이드 문자는 2칸을 차지한다.
//! scrollback 라인과 screen 라인 양쪽을 지원한다.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use termwiz::cell::CellAttributes;

use crate::renderer::unicode_width;

/// 링크가 걸쳐 있는 화면상 한 행의 컬럼 범위.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkSegment {
    /// 절대 row (scrollback 0 = 가장 오래된 라인).
    pub absolute_row: usize,
    /// 시작 컬럼(포함).
    pub start_col: usize,
    /// 끝 컬럼(포함).
    pub end_col: usize,
}

/// 검출된 링크 1건. 단일 행이면 `segments`가 원소 1개, wrap 으로 여러 화면
/// 행에 걸치면 화면 순서(위→아래)대로 여러 개 — 각 세그먼트는 그 행 안에서의
/// 정확한 컬럼 범위를 담는다.
#[derive(Debug, Clone)]
pub struct LinkSpan {
    pub segments: Vec<LinkSegment>,
    /// 링크 대상 URI.
    pub uri: String,
}

impl LinkSpan {
    /// 단일 행짜리 span 생성 (기존 `detect_scrollback_line`/`detect_screen_line`
    /// 등 한 행만 보는 검출 경로용).
    fn single(absolute_row: usize, start_col: usize, end_col: usize, uri: String) -> Self {
        Self {
            segments: vec![LinkSegment {
                absolute_row,
                start_col,
                end_col,
            }],
            uri,
        }
    }

    /// (col, absolute_row)가 이 링크 범위 안에 있는지 — 세그먼트 중 하나라도 덮으면 true.
    pub fn contains(&self, col: usize, absolute_row: usize) -> bool {
        self.segments
            .iter()
            .any(|s| s.absolute_row == absolute_row && col >= s.start_col && col <= s.end_col)
    }

    /// `absolute_row` 행에서의 컬럼 범위(있으면).
    fn range_at(&self, absolute_row: usize) -> Option<(usize, usize)> {
        self.segments
            .iter()
            .find(|s| s.absolute_row == absolute_row)
            .map(|s| (s.start_col, s.end_col))
    }
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // 스킴 + 호스트/경로. URL 끝에 붙을 수 있는 구두점(. , ; : ) ] } " ' !) 은 후처리에서 제거.
        Regex::new(r"(?i)\b(?:https?|ftp|file)://[^\s<>\[\]\{\}\\^`|]+").expect("URL regex compile")
    })
}

/// 스키마 없는 경로 후보를 찾는 regex.
///
/// - Unix 절대: `/foo/bar`
/// - Windows 절대: `C:\...` 또는 `C:/...`
/// - 상대(접두사 있음): `./foo`, `../foo`
/// - 상대(접두사 없음): `src/main.rs`, `crates/x/Cargo.toml` — 구분자(`/`, Windows 는 `\` 포함)
///   가 1개 이상 포함된 토큰만. 단어 하나(`Makefile`)는 제외.
///
/// 후보일 뿐이며 실제 파일 존재 여부는 별도로 검증한다. 접두사 없는 상대경로는
/// 슬래시 prefilter(이 regex) + cwd 기준 `exists()`(resolve_path) 2단으로 오탐을 억제한다.
fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // 접두사 없는 상대경로(rel_bare)의 경로 구분자: Windows 는 `/` 와 `\` 모두,
        // 그 외 플랫폼은 `/` 만(리눅스/macOS 에서 `\` 는 정상 파일명 문자라 산문 오탐 방지).
        #[cfg(windows)]
        let rel_bare = r"(?P<rel_bare>[A-Za-z0-9._\-]+(?:[\\/][A-Za-z0-9._\-]+)+)";
        #[cfg(not(windows))]
        let rel_bare = r"(?P<rel_bare>[A-Za-z0-9._\-]+(?:/[A-Za-z0-9._\-]+)+)";

        // 경로 문자: 영숫자, -, _, ., /, \, :, ~, (, ), 공백 제외. 한글 등 non-ASCII는 제외.
        // 단어 경계(앞)에서 시작해 공백/괄호/따옴표까지.
        // rel/unix/win 을 먼저 두어(leftmost-first) 절대경로·`./`·`../` 가 rel_bare 에
        // 흡수되지 않게 한다.
        let pattern = format!(
            r#"(?x)
            (?:
              (?:^|[\s"'(\[<])                              # 앞 경계(시작/공백/구두점)
              (?P<rel>\.{{1,2}}/[A-Za-z0-9._\-/]+)          # ./foo, ../foo/bar
            )
            |
            (?:
              (?:^|[\s"'(\[<])
              (?P<unix>/[A-Za-z0-9._\-/]+)                  # /foo/bar
            )
            |
            (?:
              \b(?P<win>[A-Za-z]:[\\/][A-Za-z0-9._\-\\/]+)  # C:\foo, C:/foo
            )
            |
            (?:
              (?:^|[\s"'(\[<])
              {rel_bare}                                    # src/main.rs, crates/x/Cargo.toml
            )
            "#
        );
        Regex::new(&pattern).expect("path regex compile")
    })
}

/// URL 뒤에 딸려 붙기 쉬운 구두점을 잘라낸다.
/// 예: "see https://a.com/x." → "https://a.com/x"
fn trim_trailing_punct(s: &str) -> &str {
    let trimmed = s.trim_end_matches(['.', ',', ';', ':', '!', '?']);
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
    cwd: Option<&Path>,
    mirror: bool,
) -> Vec<LinkSpan> {
    // 1) OSC 8 hyperlink 수집 (연속된 동일 uri 셀을 하나로 묶음).
    let mut result = Vec::new();
    let mut col_of_byte: Vec<usize> = Vec::with_capacity(line.len());
    let mut text = String::new();
    let mut col: usize = 0;
    let mut current: Option<(String, usize, usize)> = None; // (uri, start_col, end_col)
    for (cell_text, attrs) in line {
        col_of_byte.extend(std::iter::repeat_n(col, cell_text.len()));
        text.push_str(cell_text);
        let ch = cell_text.chars().next().unwrap_or(' ');
        let width = unicode_width(ch);
        let end_col = col + width.saturating_sub(1);
        let uri = attrs.hyperlink().map(|h| h.uri().to_string());
        match (&mut current, uri) {
            (Some(cur), Some(u)) if cur.0 == u => cur.2 = end_col,
            (Some(cur), Some(u)) => {
                result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0.clone()));
                *cur = (u, col, end_col);
            }
            (Some(cur), None) => {
                result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0.clone()));
                current = None;
            }
            (None, Some(u)) => current = Some((u, col, end_col)),
            (None, None) => {}
        }
        col += width;
    }
    if let Some(cur) = current {
        result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0));
    }

    // 2) 일반 텍스트에서 URL regex 검출. OSC8 범위와 겹치지 않는 경우만 추가.
    append_regex_matches(&text, &col_of_byte, absolute_row, &mut result);
    // 3) 스키마 없는 경로 (CWD 기준 exists 검증; mirror 면 검증 건너뜀).
    append_path_matches(&text, &col_of_byte, absolute_row, cwd, mirror, &mut result);
    result
}

/// screen 라인(termwiz Line)에서 링크를 검출한다.
pub fn detect_screen_line(
    line: &termwiz::surface::line::Line,
    absolute_row: usize,
    cwd: Option<&Path>,
    mirror: bool,
) -> Vec<LinkSpan> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut col_of_byte: Vec<usize> = Vec::new();
    let mut current: Option<(String, usize, usize)> = None;
    for cell_ref in line.visible_cells() {
        let col = cell_ref.cell_index();
        let cell_text = cell_ref.str();
        col_of_byte.extend(std::iter::repeat_n(col, cell_text.len()));
        text.push_str(cell_text);
        let ch = cell_text.chars().next().unwrap_or(' ');
        let width = unicode_width(ch);
        let end_col = col + width.saturating_sub(1);
        let uri = cell_ref.attrs().hyperlink().map(|h| h.uri().to_string());
        match (&mut current, uri) {
            (Some(cur), Some(u)) if cur.0 == u => cur.2 = end_col,
            (Some(cur), Some(u)) => {
                result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0.clone()));
                *cur = (u, col, end_col);
            }
            (Some(cur), None) => {
                result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0.clone()));
                current = None;
            }
            (None, Some(u)) => current = Some((u, col, end_col)),
            (None, None) => {}
        }
    }
    if let Some(cur) = current {
        result.push(LinkSpan::single(absolute_row, cur.1, cur.2, cur.0));
    }

    append_regex_matches(&text, &col_of_byte, absolute_row, &mut result);
    append_path_matches(&text, &col_of_byte, absolute_row, cwd, mirror, &mut result);
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
        let end_ch = text[..end_byte].chars().next_back().unwrap_or(' ');
        let end_col = last_start_col + unicode_width(end_ch).saturating_sub(1);

        // 이미 OSC8이 덮고 있으면 스킵.
        let overlap = out.iter().any(|s| {
            s.range_at(absolute_row)
                .is_some_and(|(s_start, s_end)| !(end_col < s_start || start_col > s_end))
        });
        if overlap {
            continue;
        }
        out.push(LinkSpan::single(
            absolute_row,
            start_col,
            end_col,
            trimmed.to_string(),
        ));
    }
}

/// 주어진 절대 row 하나에서 링크를 검출한다 — scrollback/screen 판별 포함.
fn detect_row_links(
    terminal: &tasty_terminal::Terminal,
    absolute_row: usize,
    scrollback_len: usize,
    cwd: Option<&Path>,
    mirror: bool,
) -> Option<Vec<LinkSpan>> {
    if absolute_row < scrollback_len {
        let line = terminal.scrollback_line_owned(absolute_row)?;
        Some(detect_scrollback_line(&line, absolute_row, cwd, mirror))
    } else {
        let screen_row = absolute_row - scrollback_len;
        let lines = terminal.screen_lines();
        let line = lines.get(screen_row)?;
        Some(detect_screen_line(line, absolute_row, cwd, mirror))
    }
}

/// `absolute_row` 가 소프트 wrap 됐는지(다음 행이 논리적 연속인지) — scrollback 은
/// 캡처 시점에 기록된 `ScrollbackLine.wrapped` 플래그를 그대로 쓰고, screen(live)
/// 행은 그 플래그가 없어 동일 휴리스틱(맨 오른쪽 컬럼이 공백 아닌 grapheme 로
/// 채워져 있으면 wrap)을 여기서 재현한다 — `tasty_terminal::TerminalState::
/// line_was_soft_wrapped` 와 같은 판정이지만 그 API는 크레이트 밖에 노출되지 않는다.
fn row_wrapped(
    terminal: &tasty_terminal::Terminal,
    absolute_row: usize,
    scrollback_len: usize,
) -> Option<bool> {
    if absolute_row < scrollback_len {
        terminal.scrollback_line_wrapped(absolute_row)
    } else {
        let screen_row = absolute_row - scrollback_len;
        let lines = terminal.screen_lines();
        let line = lines.get(screen_row)?;
        Some(screen_line_soft_wrapped(line, line.len()))
    }
}

/// `absolute_row` 행의 컬럼 수(그 행 자체의 셀 개수 — 과거 리사이즈로 현재
/// 터미널 폭과 다를 수 있는 scrollback 행도 정확히 반영).
fn row_cols(
    terminal: &tasty_terminal::Terminal,
    absolute_row: usize,
    scrollback_len: usize,
) -> Option<usize> {
    if absolute_row < scrollback_len {
        terminal
            .scrollback_line_owned(absolute_row)
            .map(|l| l.len())
    } else {
        let screen_row = absolute_row - scrollback_len;
        terminal.screen_lines().get(screen_row).map(|l| l.len())
    }
}

/// screen 라인 소프트 wrap 휴리스틱: 맨 오른쪽 컬럼이 공백 아닌 grapheme 로
/// 채워져 있으면 wrap. `tasty-terminal` 의 scrollback 캡처용 휴리스틱과 동일 기준.
fn screen_line_soft_wrapped(line: &termwiz::surface::line::Line, cols: usize) -> bool {
    if cols == 0 {
        return false;
    }
    for cell in line.visible_cells() {
        let idx = cell.cell_index();
        let width = cell.width().max(1);
        if idx + width == cols {
            let s = cell.str();
            return !s.is_empty() && s.trim() != "";
        }
    }
    false
}

/// OSC8 wrap 체인을 아래 방향으로 병합한다. `span`의 마지막 행이 wrapped 이고
/// 다음 행이 col 0 부터 동일 `uri`의 OSC8 링크로 시작하면 그 행의 세그먼트를
/// 이어붙이고, 그 행도 wrapped 면 계속 내려간다(3행 이상 체인 대응).
fn merge_wrap_chain_down(
    terminal: &tasty_terminal::Terminal,
    scrollback_len: usize,
    cwd: Option<&Path>,
    mirror: bool,
    span: &mut LinkSpan,
) {
    while let Some(last) = span.segments.last() {
        let last_row = last.absolute_row;
        if row_wrapped(terminal, last_row, scrollback_len) != Some(true) {
            break;
        }
        let next_row = last_row + 1;
        let Some(next_spans) = detect_row_links(terminal, next_row, scrollback_len, cwd, mirror)
        else {
            break;
        };
        let next = next_spans.into_iter().find(|s| {
            s.uri == span.uri
                && s.segments
                    .first()
                    .is_some_and(|seg| seg.absolute_row == next_row && seg.start_col == 0)
        });
        let Some(next) = next else { break };
        span.segments.extend(next.segments);
    }
}

/// OSC8 wrap 체인을 위 방향으로 병합한다 — `merge_wrap_chain_down`과 대칭.
/// `span`의 첫 행 바로 위 행이 wrapped(그 행이 현재 행으로 이어짐)이고 동일
/// `uri`의 OSC8 링크로 끝나면 그 행의 세그먼트를 앞에 붙이고 계속 올라간다.
fn merge_wrap_chain_up(
    terminal: &tasty_terminal::Terminal,
    scrollback_len: usize,
    cwd: Option<&Path>,
    mirror: bool,
    span: &mut LinkSpan,
) {
    while let Some(first) = span.segments.first() {
        let first_row = first.absolute_row;
        let Some(prev_row) = first_row.checked_sub(1) else {
            break;
        };
        if row_wrapped(terminal, prev_row, scrollback_len) != Some(true) {
            break;
        }
        let Some(prev_spans) = detect_row_links(terminal, prev_row, scrollback_len, cwd, mirror)
        else {
            break;
        };
        let Some(prev_cols) = row_cols(terminal, prev_row, scrollback_len) else {
            break;
        };
        let prev = prev_spans.into_iter().find(|s| {
            s.uri == span.uri
                && s.segments
                    .iter()
                    .any(|seg| seg.absolute_row == prev_row && seg.end_col + 1 == prev_cols)
        });
        let Some(prev) = prev else { break };
        let mut merged = prev.segments;
        merged.append(&mut span.segments);
        span.segments = merged;
    }
}

/// 주어진 (col, absolute_row)에 있는 링크를 터미널에서 찾는다. scrollback과
/// screen 양쪽을 처리하고, OSC8 하이퍼링크가 소프트 wrap 으로 여러 화면 행에
/// 걸쳐 있으면(`ScrollbackLine.wrapped`/동등 휴리스틱 + 동일 uri) 그 체인 전체를
/// 위/아래 양방향으로 병합해 반환한다. plain-text URL/경로(regex 검출)는 wrap
/// continuation 행에 스킴 프리픽스가 없어 그 행 자체에서 애초에 매치가 나지
/// 않으므로(따라서 uri 도 일치하지 않으므로) 별도 처리 없이도 병합 대상이 되지
/// 않는다 — merge 조건(`uri` 일치)이 자연히 걸러낸다.
pub fn link_at(
    terminal: &tasty_terminal::Terminal,
    col: usize,
    absolute_row: usize,
) -> Option<LinkSpan> {
    let scrollback_len = terminal.scrollback_len();
    let cwd = terminal.get_cwd();
    let cwd_ref = cwd.as_deref();
    // 원격(mirror) surface 판별: detached mirror 는 자식 PTY 가 없어 process_id() 가
    // None. 화면 경로는 원격 호스트 경로라 로컬 exists() 검증을 건너뛰어야 한다.
    // (`Terminal::new_detached` 호출처는 여럿이지만 — attach_readonly/attach CLI 등 —
    //  그것들은 GUI terminals store 밖이다. `link_at` 에 들어오는 terminal 은
    //  find_terminal_by_id 가 보는 GUI store 기준이고, 그 store 안에서 detached 인 것은
    //  attach_client::start_gui_attach 의 mirror 뿐이라 process_id().is_none() ⟺ mirror.)
    let mirror = terminal.process_id().is_none();
    let spans = detect_row_links(terminal, absolute_row, scrollback_len, cwd_ref, mirror)?;
    let mut found = spans.into_iter().find(|s| s.contains(col, absolute_row))?;
    merge_wrap_chain_down(terminal, scrollback_len, cwd_ref, mirror, &mut found);
    merge_wrap_chain_up(terminal, scrollback_len, cwd_ref, mirror, &mut found);
    Some(found)
}

/// 렌더러에 전달하는 링크 하이라이트 정보. hovered된 단일 링크를(wrap 으로
/// 여러 화면 행에 걸치면 그 모든 행을) 해당 셀 범위에 대해 fg/bg 색으로
/// 오버라이드한다.
#[derive(Debug, Clone)]
pub struct LinkHighlight {
    pub segments: Vec<LinkSegment>,
    pub fg: tasty_type_appearance::color::GpuRgba,
    pub bg: tasty_type_appearance::color::GpuRgba,
}

impl LinkHighlight {
    pub fn covers(&self, col: usize, absolute_row: usize) -> bool {
        self.segments
            .iter()
            .any(|s| s.absolute_row == absolute_row && col >= s.start_col && col <= s.end_col)
    }
}

/// 경로 후보를 `file://` URI 형식의 String 으로 변환해서 반환.
/// - 절대경로면 그대로 사용.
/// - 상대경로면 `cwd` 기준으로 해석.
/// - `mirror` 가 false(로컬 surface)면 실제 존재하는 경로만 반환(오탐 방지).
/// - `mirror` 가 true(원격 mirror surface)면 화면 경로가 원격 호스트 경로라
///   로컬 `exists()` 검증을 건너뛰고 (원격 cwd 기준 결합한) 경로를 그대로 emit.
fn resolve_link_target(candidate: &str, cwd: Option<&Path>, mirror: bool) -> Option<String> {
    let p = Path::new(candidate);
    // mirror surface 의 화면 경로는 원격 호스트(유닉스) 규약을 따른다 — Windows
    // 의 `Path::is_absolute()` 는 `/remote/...` 를 (드라이브/UNC 접두사가 없어)
    // 비절대로 판정하므로 로컬 규약만 쓰면 원격 절대경로가 상대경로로 오판돼
    // cwd 없이는 링크화되지 않는다. mirror 면 유닉스식 루트를 절대로 인정한다.
    let is_abs = p.is_absolute() || (mirror && candidate.starts_with('/'));
    let abs: PathBuf = if is_abs {
        p.to_path_buf()
    } else {
        cwd?.join(p)
    };
    // 정규화 (심볼릭/`.` `..`): canonicalize는 실패 가능하고 Windows에서 UNC 접두사를
    // 붙일 수 있어, 존재 확인만 하고 원본 abs 경로를 file:// URI로 변환.
    if !mirror && !abs.exists() {
        return None;
    }
    Some(path_to_file_uri(&abs))
}

/// 절대 경로를 `file://` URI로 변환. Windows 백슬래시는 `/`로 치환.
fn path_to_file_uri(abs: &Path) -> String {
    let s = abs.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        // Windows 드라이브 문자 경로: `C:/...` → `file:///C:/...`
        format!("file:///{s}")
    }
}

/// 경로 regex 매치를 돌면서 존재하는 경로만 LinkSpan으로 추가.
fn append_path_matches(
    text: &str,
    col_of_byte: &[usize],
    absolute_row: usize,
    cwd: Option<&Path>,
    mirror: bool,
    out: &mut Vec<LinkSpan>,
) {
    for caps in path_regex().captures_iter(text) {
        let m = caps
            .name("rel")
            .or_else(|| caps.name("unix"))
            .or_else(|| caps.name("win"))
            .or_else(|| caps.name("rel_bare"));
        let Some(m) = m else { continue };
        let raw = m.as_str();
        let trimmed = trim_trailing_punct(raw);
        if trimmed.is_empty() {
            continue;
        }
        let Some(uri) = resolve_link_target(trimmed, cwd, mirror) else {
            continue;
        };
        let start_byte = m.start();
        let end_byte = start_byte + trimmed.len();
        let Some(&start_col) = col_of_byte.get(start_byte) else {
            continue;
        };
        let last_byte = end_byte.saturating_sub(1);
        let Some(&last_start_col) = col_of_byte.get(last_byte) else {
            continue;
        };
        let end_ch = text[..end_byte].chars().next_back().unwrap_or(' ');
        let end_col = last_start_col + unicode_width(end_ch).saturating_sub(1);
        let overlap = out.iter().any(|s| {
            s.range_at(absolute_row)
                .is_some_and(|(s_start, s_end)| !(end_col < s_start || start_col > s_end))
        });
        if overlap {
            continue;
        }
        out.push(LinkSpan::single(absolute_row, start_col, end_col, uri));
    }
}

/// 터미널에서 드래그/더블클릭으로 확정 선택한 텍스트가 실재하는 파일/폴더 경로(또는
/// 그 접두사)인지 판별한다. `path_regex()`로 재매칭하지 않고 선택 문자열 전체를 그대로
/// 1차 후보로 쓴다 — regex 로 다시 걸러내면 비-ASCII 후행 문자(예: 슬래시 바로 뒤에
/// 붙는 한글 조사)가 매치 단계에서 이미 잘려나가, 아래 "`/` 단위로 축약 재검사"가 애초에
/// 무의미해진다.
///
/// 1차 후보가 존재하지 않으면 마지막 `/` 앞까지 잘라 재검사하고, 그래도 없으면 그 앞의
/// `/`로 계속 반복해 실재하는 가장 긴 접두사를 찾는다. 선택 모드(문자/단어/줄/블록)는
/// 구분하지 않는다 — 하나의 연결된 드래그 블록이면 `extract_selected_text`가 뽑아준
/// 문자열을 그대로 쓴다.
pub fn longest_existing_selection_path(
    raw_selected_text: &str,
    cwd: Option<&Path>,
    mirror: bool,
) -> Option<PathBuf> {
    let mut candidate = trim_trailing_punct(raw_selected_text.trim());
    loop {
        if candidate.is_empty() {
            return None;
        }
        if let Some(path) = resolve_selection_path_candidate(candidate, cwd, mirror) {
            return Some(path);
        }
        candidate = &candidate[..candidate.rfind('/')?];
    }
}

/// `longest_existing_selection_path` 전용 저수준 후보 해석. 절대경로 판별/cwd
/// join/mirror 시 exists() 스킵 규칙은 `resolve_link_target`과 동일하되, 반환 타입이
/// 다르고(file:// URI 문자열이 아니라 `PathBuf`) `resolve_link_target` 자체는 기존
/// hover-link 회귀를 피하기 위해 건드리지 않으므로 별도로 둔다.
fn resolve_selection_path_candidate(
    candidate: &str,
    cwd: Option<&Path>,
    mirror: bool,
) -> Option<PathBuf> {
    let p = Path::new(candidate);
    let is_abs = p.is_absolute() || (mirror && candidate.starts_with('/'));
    let abs: PathBuf = if is_abs {
        p.to_path_buf()
    } else {
        cwd?.join(p)
    };
    if !mirror && !abs.exists() {
        return None;
    }
    Some(abs)
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
    fn path_regex_matches_common_forms() {
        let re = path_regex();
        let cases = [
            ("open ./src/main.rs now", "./src/main.rs"),
            ("see /etc/passwd", "/etc/passwd"),
            ("at ../foo/bar", "../foo/bar"),
            ("C:\\Users\\a.txt file", "C:\\Users\\a.txt"),
            ("C:/Users/a.txt yay", "C:/Users/a.txt"),
        ];
        for (input, expected) in cases {
            let caps = re
                .captures(input)
                .unwrap_or_else(|| panic!("no match: {input}"));
            let m = caps
                .name("rel")
                .or_else(|| caps.name("unix"))
                .or_else(|| caps.name("win"))
                .unwrap();
            assert_eq!(m.as_str(), expected, "input: {input}");
        }
    }

    #[test]
    fn detects_bare_relative_path() {
        // path_regex 가 "open src/main.rs now" 에서 "src/main.rs" 를 rel_bare 로 잡아야 함.
        let re = path_regex();
        let caps = re
            .captures("open src/main.rs now")
            .expect("bare relative path should match");
        assert_eq!(caps.name("rel_bare").unwrap().as_str(), "src/main.rs");

        let caps = re
            .captures("see crates/x/Cargo.toml here")
            .expect("nested bare relative path should match");
        assert_eq!(
            caps.name("rel_bare").unwrap().as_str(),
            "crates/x/Cargo.toml"
        );
    }

    #[test]
    fn rejects_single_word_without_separator() {
        // "see Makefile here" 에서 "Makefile" 은 구분자가 없어 후보가 아님.
        let re = path_regex();
        assert!(
            re.captures("see Makefile here").is_none(),
            "single word without separator must not match"
        );
    }

    #[test]
    fn bare_relative_does_not_steal_prefixed_or_absolute() {
        // rel/unix/win 이 우선해 rel_bare 가 절대경로·`./`·`../` 를 흡수하지 않아야 함.
        let re = path_regex();
        let prefixed = re.captures("open ./src/main.rs now").unwrap();
        assert!(prefixed.name("rel").is_some());
        assert!(prefixed.name("rel_bare").is_none());

        let unix = re.captures("see /etc/passwd").unwrap();
        assert!(unix.name("unix").is_some());
        assert!(unix.name("rel_bare").is_none());
    }

    #[test]
    fn slash_non_path_rejected_by_exists() {
        // 슬래시는 있지만 경로가 아닌 토큰은 regex 후보가 되어도 exists() 에서 배제된다.
        assert!(resolve_link_target("and/or", Some(std::path::Path::new(".")), false).is_none());
        assert!(resolve_link_target("TCP/IP", Some(std::path::Path::new(".")), false).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn detects_bare_relative_path_with_backslash() {
        let re = path_regex();
        let caps = re
            .captures("open src\\main.rs now")
            .expect("windows bare relative path should match");
        assert_eq!(caps.name("rel_bare").unwrap().as_str(), "src\\main.rs");
    }

    #[test]
    fn path_to_file_uri_forms() {
        assert_eq!(
            path_to_file_uri(std::path::Path::new("/home/user/a.txt")),
            "file:///home/user/a.txt"
        );
        assert_eq!(
            path_to_file_uri(std::path::Path::new("C:\\Users\\a.txt")),
            "file:///C:/Users/a.txt"
        );
    }

    #[test]
    fn resolve_path_rejects_nonexistent() {
        assert!(resolve_link_target("/definitely/not/a/real/path/xyz123", None, false).is_none());
        assert!(
            resolve_link_target(
                "./nope_does_not_exist_xyz",
                Some(std::path::Path::new(".")),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn mirror_emits_path_without_exists_check() {
        let cwd = std::path::Path::new("/remote/project");
        // 로컬에 없는 경로라도 mirror(원격 surface)면 file:// URI 를 emit한다.
        let uri = resolve_link_target("src/main.rs", Some(cwd), true);
        assert_eq!(uri.as_deref(), Some("file:///remote/project/src/main.rs"));
        // 같은 입력이라도 비-mirror(로컬)면 exists() 실패로 None.
        assert!(resolve_link_target("src/main.rs", Some(cwd), false).is_none());
    }

    #[test]
    fn mirror_absolute_path_without_cwd() {
        // 원격 절대경로는 cwd 없이도 그대로 emit.
        let uri = resolve_link_target("/remote/abs/file.rs", None, true);
        assert_eq!(uri.as_deref(), Some("file:///remote/abs/file.rs"));
    }

    #[test]
    fn link_span_contains_checks_all_segments() {
        let span = LinkSpan {
            uri: "https://example.com/a/b".into(),
            segments: vec![
                LinkSegment {
                    absolute_row: 5,
                    start_col: 10,
                    end_col: 19,
                },
                LinkSegment {
                    absolute_row: 6,
                    start_col: 0,
                    end_col: 7,
                },
            ],
        };
        assert!(span.contains(10, 5));
        assert!(span.contains(19, 5));
        assert!(span.contains(0, 6));
        assert!(!span.contains(9, 5), "5행 시작 컬럼 이전은 범위 밖");
        assert!(
            !span.contains(0, 5),
            "다른 행 컬럼은 그 행에 매치되면 안 됨"
        );
        assert!(!span.contains(8, 6), "6행 끝 컬럼 다음은 범위 밖");
    }

    #[test]
    fn link_highlight_covers_checks_all_segments() {
        let highlight = LinkHighlight {
            segments: vec![
                LinkSegment {
                    absolute_row: 5,
                    start_col: 10,
                    end_col: 19,
                },
                LinkSegment {
                    absolute_row: 6,
                    start_col: 0,
                    end_col: 7,
                },
            ],
            fg: tasty_type_appearance::color::GpuRgba::dangerously_force_from_array([
                0.0, 0.0, 0.0, 1.0,
            ]),
            bg: tasty_type_appearance::color::GpuRgba::dangerously_force_from_array([
                0.0, 0.0, 0.0, 1.0,
            ]),
        };
        assert!(highlight.covers(15, 5));
        assert!(highlight.covers(3, 6));
        assert!(!highlight.covers(15, 6), "행이 다르면 매치되면 안 됨");
        assert!(!highlight.covers(20, 5));
    }

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

    #[test]
    fn selection_path_full_candidate_used_when_it_exists() {
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
        let result = longest_existing_selection_path("Cargo.toml", Some(cwd), false);
        assert_eq!(result, Some(cwd.join("Cargo.toml")));
    }

    #[test]
    fn selection_path_trims_back_to_last_slash_when_full_candidate_missing() {
        // 실재하는 디렉토리 뒤에 `/` 와 실재하지 않는 비-ASCII 세그먼트(예: 한글 조사)가
        // 붙은 경우 — 마지막 `/` 앞까지 잘라 실재 접두사를 돌려준다. 픽스처는 테스트가
        // 임시 디렉토리에 직접 만든다: 레포 밖·gitignored 경로의 실존에 기대면 clone
        // 직후나 CI 러너에서 결과가 달라진다(`docs/dev-guide/unit-test-isolation.md`).
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tmp.path().join("notes")).expect("fixture dir");
        let cwd = tmp.path();
        let result = longest_existing_selection_path("notes/에", Some(cwd), false);
        assert_eq!(result, Some(cwd.join("notes")));
    }

    #[test]
    fn selection_path_trims_repeatedly_across_multiple_slash_boundaries() {
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
        // "src/adapters/ui" 까지는 실재, 그 뒤 세그먼트가 통째로 가짜.
        let result = longest_existing_selection_path(
            "src/adapters/ui/totally_bogus_file_xyz",
            Some(cwd),
            false,
        );
        assert_eq!(result, Some(cwd.join("src/adapters/ui")));
    }

    #[test]
    fn selection_path_none_when_no_prefix_exists() {
        assert!(
            longest_existing_selection_path(
                "totally/bogus/path/xyz123",
                Some(Path::new(".")),
                false
            )
            .is_none()
        );
    }

    #[test]
    fn selection_path_mirror_skips_exists_check_like_resolve_link_target() {
        let cwd = Path::new("/remote/project");
        let result = longest_existing_selection_path("src/main.rs", Some(cwd), true);
        assert_eq!(result, Some(cwd.join("src/main.rs")));
    }
}
