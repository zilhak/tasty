/// Terminal text search engine.
///
/// Searches screen + scrollback buffer for text matches.
/// Row coordinates are absolute: 0 = oldest scrollback line,
/// scrollback_len + screen_row = screen lines.
use regex::Regex;

/// A single search match in the terminal buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Absolute row (0 = oldest scrollback, scrollback_len..scrollback_len+rows = screen).
    pub row: usize,
    /// Start column (inclusive, 0-based).
    pub col_start: usize,
    /// End column (exclusive).
    pub col_end: usize,
}

/// Search options.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_insensitive: true,
            regex: false,
            whole_word: false,
        }
    }
}

/// Errors produced while preparing or running a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// Regex compilation failed (invalid syntax). Carries the displayable error.
    InvalidRegex(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::InvalidRegex(msg) => write!(f, "invalid regex: {msg}"),
        }
    }
}

impl std::error::Error for SearchError {}

/// Prepared matcher reused across all lines.
enum Matcher {
    Literal {
        needle: String,
        case_insensitive: bool,
        whole_word: bool,
    },
    Regex(Regex),
}

impl Matcher {
    fn build(query: &str, opts: &SearchOptions) -> Result<Self, SearchError> {
        if opts.regex {
            let mut pattern = query.to_string();
            if opts.whole_word {
                pattern = format!(r"\b(?:{pattern})\b");
            }
            if opts.case_insensitive {
                pattern = format!("(?i){pattern}");
            }
            Regex::new(&pattern)
                .map(Matcher::Regex)
                .map_err(|e| SearchError::InvalidRegex(e.to_string()))
        } else {
            Ok(Matcher::Literal {
                needle: query.to_string(),
                case_insensitive: opts.case_insensitive,
                whole_word: opts.whole_word,
            })
        }
    }

    /// Find all matches in `haystack`, returning (col_start, col_end) cell coordinates.
    fn find_in(&self, haystack: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Literal {
                needle,
                case_insensitive,
                whole_word,
            } => find_literal(haystack, needle, *case_insensitive, *whole_word),
            Matcher::Regex(re) => {
                let mut out = Vec::new();
                for m in re.find_iter(haystack) {
                    if m.start() == m.end() {
                        // Skip zero-width matches (e.g. `a*` against empty positions).
                        continue;
                    }
                    let col_start = cell_col_at_byte(haystack, m.start());
                    let col_end = cell_col_at_byte(haystack, m.end());
                    out.push((col_start, col_end));
                }
                out
            }
        }
    }
}

/// Literal substring search with optional case-insensitivity and whole-word constraint.
fn find_literal(
    haystack: &str,
    needle: &str,
    case_insensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || haystack.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();

    let (search_in, search_for): (std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>) =
        if case_insensitive {
            (
                std::borrow::Cow::Owned(haystack.to_lowercase()),
                std::borrow::Cow::Owned(needle.to_lowercase()),
            )
        } else {
            (
                std::borrow::Cow::Borrowed(haystack),
                std::borrow::Cow::Borrowed(needle),
            )
        };

    let mut byte_offset = 0;
    while let Some(pos) = search_in[byte_offset..].find(search_for.as_ref()) {
        let match_byte_start = byte_offset + pos;
        let match_byte_end = match_byte_start + search_for.len();

        if !whole_word || is_whole_word_boundary(&search_in, match_byte_start, match_byte_end) {
            let col_start = cell_col_at_byte(haystack, match_byte_start);
            let col_end = cell_col_at_byte(haystack, match_byte_end);
            matches.push((col_start, col_end));
        }

        // Advance by one character to allow overlapping matches.
        byte_offset = match_byte_start
            + search_in[match_byte_start..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
    }

    matches
}

/// Word boundary check (Unicode-aware via `char::is_alphanumeric` + underscore).
fn is_whole_word_boundary(haystack: &str, byte_start: usize, byte_end: usize) -> bool {
    let before_ok = byte_start == 0
        || haystack[..byte_start]
            .chars()
            .next_back()
            .map(|c| !is_word_char(c))
            .unwrap_or(true);
    let after_ok = byte_end >= haystack.len()
        || haystack[byte_end..]
            .chars()
            .next()
            .map(|c| !is_word_char(c))
            .unwrap_or(true);
    before_ok && after_ok
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Extract plain text from a line of cells.
fn line_text(cells: &[(String, termwiz::cell::CellAttributes)]) -> String {
    cells.iter().map(|(s, _)| s.as_str()).collect()
}

/// Convert a byte offset in the line's text to its cell column index.
///
/// Cell columns account for wide characters (CJK, fullwidth, …) occupying 2
/// columns, matching the renderer's column layout (`cell_index()` /
/// `unicode_width`). Counting chars instead would under-count every preceding
/// wide character and shift the highlight left.
fn cell_col_at_byte(s: &str, byte_offset: usize) -> usize {
    termwiz::cell::unicode_column_width(&s[..byte_offset], None)
}

impl crate::TerminalState {
    /// Search all terminal content (scrollback + screen) for `query`.
    /// Returns matches sorted by position (oldest first), or an error if the
    /// query options are invalid (e.g. malformed regex).
    pub fn search(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchMatch>, SearchError> {
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let matcher = Matcher::build(query, options)?;
        let mut matches = Vec::new();
        let scrollback_len = self.scrollback_len();

        for i in 0..scrollback_len {
            if let Some(cells) = self.scrollback_line_owned(i) {
                let text = line_text(&cells);
                for (col_start, col_end) in matcher.find_in(&text) {
                    matches.push(SearchMatch {
                        row: i,
                        col_start,
                        col_end,
                    });
                }
            }
        }

        let screen_lines = self.surface().screen_lines();
        for (row_idx, line) in screen_lines.iter().enumerate() {
            let cells: Vec<(String, termwiz::cell::CellAttributes)> = line
                .visible_cells()
                .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                .collect();
            let text = line_text(&cells);
            for (col_start, col_end) in matcher.find_in(&text) {
                matches.push(SearchMatch {
                    row: scrollback_len + row_idx,
                    col_start,
                    col_end,
                });
            }
        }

        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_literal_basic() {
        let results = find_literal("hello world hello", "hello", false, false);
        assert_eq!(results, vec![(0, 5), (12, 17)]);
    }

    #[test]
    fn find_literal_case_insensitive() {
        let results = find_literal("Hello HELLO hello", "hello", true, false);
        assert_eq!(results, vec![(0, 5), (6, 11), (12, 17)]);
    }

    #[test]
    fn find_literal_empty() {
        assert!(find_literal("hello", "", false, false).is_empty());
        assert!(find_literal("", "hello", false, false).is_empty());
    }

    #[test]
    fn find_literal_overlapping() {
        let results = find_literal("aaa", "aa", false, false);
        assert_eq!(results, vec![(0, 2), (1, 3)]);
    }

    #[test]
    fn find_literal_multibyte() {
        // Columns are cell columns: each wide (CJK) char occupies 2 columns, so
        // "나다" inside "가나다라" starts at column 2 (after the 2-wide "가") and
        // ends at column 6. Char-index counting would wrongly yield (1, 3) and
        // shift the highlight left.
        let results = find_literal("가나다라", "나다", false, false);
        assert_eq!(results, vec![(2, 6)]);
    }

    #[test]
    fn find_literal_wide_then_ascii() {
        // Mixed wide + ASCII: "한글code" → "code" starts after two 2-wide chars
        // (columns 0..4), so it spans columns 4..8.
        let results = find_literal("한글code", "code", false, false);
        assert_eq!(results, vec![(4, 8)]);
    }

    #[test]
    fn find_whole_word_filters_partial() {
        // "cat" appears inside "category" — should not match with whole_word.
        let results = find_literal("cat category catastrophe cat!", "cat", false, true);
        assert_eq!(results, vec![(0, 3), (25, 28)]);
    }

    #[test]
    fn find_whole_word_with_underscore() {
        // Underscores are word chars; `foo` inside `foo_bar` should not match.
        let results = find_literal("foo foo_bar foo", "foo", false, true);
        assert_eq!(results, vec![(0, 3), (12, 15)]);
    }

    #[test]
    fn regex_matcher_basic() {
        let opts = SearchOptions {
            case_insensitive: false,
            regex: true,
            whole_word: false,
        };
        let m = Matcher::build(r"\d+", &opts).unwrap();
        assert_eq!(m.find_in("abc 123 def 4567"), vec![(4, 7), (12, 16)]);
    }

    #[test]
    fn regex_matcher_case_insensitive() {
        let opts = SearchOptions {
            case_insensitive: true,
            regex: true,
            whole_word: false,
        };
        let m = Matcher::build("hello", &opts).unwrap();
        assert_eq!(m.find_in("Hello HELLO"), vec![(0, 5), (6, 11)]);
    }

    #[test]
    fn regex_matcher_whole_word_wraps_pattern() {
        let opts = SearchOptions {
            case_insensitive: false,
            regex: true,
            whole_word: true,
        };
        let m = Matcher::build("cat", &opts).unwrap();
        assert_eq!(m.find_in("cat category cats cat."), vec![(0, 3), (18, 21)]);
    }

    #[test]
    fn regex_matcher_invalid_pattern() {
        let opts = SearchOptions {
            case_insensitive: false,
            regex: true,
            whole_word: false,
        };
        let err = match Matcher::build("[invalid", &opts) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(matches!(err, SearchError::InvalidRegex(_)));
    }

    #[test]
    fn regex_matcher_skips_zero_width() {
        let opts = SearchOptions {
            case_insensitive: false,
            regex: true,
            whole_word: false,
        };
        let m = Matcher::build("a*", &opts).unwrap();
        // `a*` would match empty positions; we skip zero-width matches but keep `aa`.
        let results = m.find_in("baab");
        assert_eq!(results, vec![(1, 3)]);
    }
}
