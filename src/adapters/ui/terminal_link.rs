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
    // 3) 스키마 없는 경로 (CWD 기준 exists 검증).
    append_path_matches(&text, &col_of_byte, absolute_row, cwd, &mut result);
    result
}

/// screen 라인(termwiz Line)에서 링크를 검출한다.
pub fn detect_screen_line(
    line: &termwiz::surface::line::Line,
    absolute_row: usize,
    cwd: Option<&Path>,
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
    append_path_matches(&text, &col_of_byte, absolute_row, cwd, &mut result);
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
            s.absolute_row == absolute_row && !(end_col < s.start_col || start_col > s.end_col)
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
    let cwd = terminal.get_cwd();
    let cwd_ref = cwd.as_deref();
    let spans = if absolute_row < scrollback_len {
        let line = terminal.scrollback_line_owned(absolute_row)?;
        detect_scrollback_line(&line, absolute_row, cwd_ref)
    } else {
        let screen_row = absolute_row - scrollback_len;
        let surface = terminal.surface();
        let lines = surface.screen_lines();
        let line = lines.get(screen_row)?;
        detect_screen_line(line, absolute_row, cwd_ref)
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
    pub fg: tasty_type_appearance::color::GpuRgba,
    pub bg: tasty_type_appearance::color::GpuRgba,
}

impl LinkHighlight {
    pub fn covers(&self, col: usize, absolute_row: usize) -> bool {
        self.absolute_row == absolute_row && col >= self.start_col && col <= self.end_col
    }
}

/// 경로 후보가 실제로 존재하는 파일/디렉토리인지 확인하고, 존재하면
/// `file://` URI 형식의 String으로 변환해서 반환.
/// - 절대경로면 그대로 사용.
/// - 상대경로면 `cwd` 기준으로 해석.
fn resolve_path(candidate: &str, cwd: Option<&Path>) -> Option<String> {
    let p = Path::new(candidate);
    let abs: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd?.join(p)
    };
    if !abs.exists() {
        return None;
    }
    // 정규화 (심볼릭/`.` `..`): canonicalize는 실패 가능하고 Windows에서 UNC 접두사를
    // 붙일 수 있어, 존재 확인만 하고 원본 abs 경로를 file:// URI로 변환.
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
        let Some(uri) = resolve_path(trimmed, cwd) else {
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
            s.absolute_row == absolute_row && !(end_col < s.start_col || start_col > s.end_col)
        });
        if overlap {
            continue;
        }
        out.push(LinkSpan {
            start_col,
            end_col,
            uri,
            absolute_row,
        });
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
        assert!(resolve_path("and/or", Some(std::path::Path::new("."))).is_none());
        assert!(resolve_path("TCP/IP", Some(std::path::Path::new("."))).is_none());
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
        assert!(resolve_path("/definitely/not/a/real/path/xyz123", None).is_none());
        assert!(
            resolve_path("./nope_does_not_exist_xyz", Some(std::path::Path::new("."))).is_none()
        );
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
}
