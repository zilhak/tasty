/// Terminal text search engine.
///
/// Searches screen + scrollback buffer for text matches.
/// Row coordinates are absolute: 0 = oldest scrollback line,
/// scrollback_len + screen_row = screen lines.

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
pub struct SearchOptions {
    pub case_insensitive: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_insensitive: true,
        }
    }
}

/// Extract plain text from a line of cells.
fn line_text(cells: &[(String, termwiz::cell::CellAttributes)]) -> String {
    cells.iter().map(|(s, _)| s.as_str()).collect()
}

/// Find all occurrences of `needle` in `haystack`, returning (col_start, col_end) pairs.
/// Handles multi-byte characters correctly by tracking char indices.
fn find_matches_in_line(haystack: &str, needle: &str, case_insensitive: bool) -> Vec<(usize, usize)> {
    if needle.is_empty() || haystack.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();

    if case_insensitive {
        let haystack_lower = haystack.to_lowercase();
        let needle_lower = needle.to_lowercase();
        let mut byte_offset = 0;
        while let Some(pos) = haystack_lower[byte_offset..].find(&needle_lower) {
            let match_byte_start = byte_offset + pos;
            let match_byte_end = match_byte_start + needle_lower.len();

            // Convert byte offsets to cell column indices.
            let col_start = char_col_at_byte(haystack, match_byte_start);
            let col_end = char_col_at_byte(haystack, match_byte_end);

            matches.push((col_start, col_end));
            // Advance past this match (by at least one character to avoid infinite loop).
            byte_offset = match_byte_start + haystack_lower[match_byte_start..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    } else {
        let mut byte_offset = 0;
        while let Some(pos) = haystack[byte_offset..].find(needle) {
            let match_byte_start = byte_offset + pos;
            let match_byte_end = match_byte_start + needle.len();

            let col_start = char_col_at_byte(haystack, match_byte_start);
            let col_end = char_col_at_byte(haystack, match_byte_end);

            matches.push((col_start, col_end));
            byte_offset = match_byte_start + haystack[match_byte_start..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        }
    };

    matches
}

/// Convert a byte offset in a string to the cell column index.
/// Each char maps to 1 cell column (wide chars are handled by termwiz cells,
/// which already split wide chars into separate cell entries).
fn char_col_at_byte(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset].chars().count()
}

impl crate::Terminal {
    /// Search all terminal content (scrollback + screen) for `query`.
    /// Returns matches sorted by position (oldest first).
    pub fn search(&self, query: &str, options: &SearchOptions) -> Vec<SearchMatch> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        let scrollback_len = self.scrollback_len();

        // Search scrollback lines (oldest to newest).
        for i in 0..scrollback_len {
            if let Some(cells) = self.scrollback_line_owned(i) {
                let text = line_text(&cells);
                for (col_start, col_end) in find_matches_in_line(&text, query, options.case_insensitive) {
                    matches.push(SearchMatch {
                        row: i,
                        col_start,
                        col_end,
                    });
                }
            }
        }

        // Search screen lines.
        let screen_lines = self.surface().screen_lines();
        for (row_idx, line) in screen_lines.iter().enumerate() {
            let cells: Vec<(String, termwiz::cell::CellAttributes)> = line
                .visible_cells()
                .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                .collect();
            let text = line_text(&cells);
            for (col_start, col_end) in find_matches_in_line(&text, query, options.case_insensitive) {
                matches.push(SearchMatch {
                    row: scrollback_len + row_idx,
                    col_start,
                    col_end,
                });
            }
        }

        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_basic() {
        let results = find_matches_in_line("hello world hello", "hello", false);
        assert_eq!(results, vec![(0, 5), (12, 17)]);
    }

    #[test]
    fn find_matches_case_insensitive() {
        let results = find_matches_in_line("Hello HELLO hello", "hello", true);
        assert_eq!(results, vec![(0, 5), (6, 11), (12, 17)]);
    }

    #[test]
    fn find_matches_empty() {
        assert!(find_matches_in_line("hello", "", false).is_empty());
        assert!(find_matches_in_line("", "hello", false).is_empty());
    }

    #[test]
    fn find_matches_overlapping() {
        let results = find_matches_in_line("aaa", "aa", false);
        assert_eq!(results, vec![(0, 2), (1, 3)]);
    }

    #[test]
    fn find_matches_multibyte() {
        let results = find_matches_in_line("가나다라", "나다", false);
        assert_eq!(results, vec![(1, 3)]);
    }
}
