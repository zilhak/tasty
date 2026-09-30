use tasty_terminal::search::{SearchError, SearchMatch, SearchOptions};

pub struct SearchState {
    pub epoch: tasty_terminal::ContentEpoch,
    pub query: String,
    /// All matches in the terminal buffer (sorted oldest→newest).
    pub matches: Vec<SearchMatch>,
    pub current_index: usize,
    pub surface_id: u32,
    /// Whether to ignore case (default: true).
    pub case_insensitive: bool,
    /// Whether the query is interpreted as a regular expression.
    pub regex: bool,
    /// Whether to match whole words only.
    pub whole_word: bool,
    /// Last regex compilation error, if any. Cleared on successful runs.
    pub last_error: Option<String>,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            epoch: Default::default(),
            query: String::new(),
            matches: Vec::new(),
            current_index: 0,
            surface_id: 0,
            case_insensitive: true,
            regex: false,
            whole_word: false,
            last_error: None,
        }
    }

    fn options(&self) -> SearchOptions {
        SearchOptions {
            case_insensitive: self.case_insensitive,
            regex: self.regex,
            whole_word: self.whole_word,
        }
    }

    pub fn execute(&mut self, terminal: &tasty_terminal::Terminal) {
        let options = self.options();
        let result = terminal.with_content(|view| {
            self.epoch = view.cut().epoch;
            view.search(&self.query, &options)
        });
        match result {
            Ok(matches) => {
                self.matches = matches;
                self.last_error = None;
            }
            Err(SearchError::InvalidRegex(msg)) => {
                self.matches.clear();
                self.last_error = Some(msg);
            }
        }
        if self.matches.is_empty() {
            self.current_index = 0;
        } else if self.current_index >= self.matches.len() {
            self.current_index = self.matches.len() - 1;
        }
    }

    /// Move to the next match. Wraps around.
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_index = (self.current_index + 1) % self.matches.len();
        }
    }

    /// Move to the previous match. Wraps around.
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.current_index = if self.current_index == 0 {
                self.matches.len() - 1
            } else {
                self.current_index - 1
            };
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_index = 0;
        self.last_error = None;
    }

    /// 현재 결과를 화면 중앙에 놓을 스크롤 오프셋을 계산한다. 현재 결과가 없으면 None이다.
    pub fn scroll_to_current(&self, cut: tasty_terminal::ContentCut) -> Option<usize> {
        if self.epoch != cut.epoch {
            return None;
        }
        let scrollback_len = cut.history_len();
        let screen_rows = cut.rows;
        let m = self.matches.get(self.current_index)?;
        let row = m.row.checked_sub(cut.first_row)?;
        let total_rows = scrollback_len + screen_rows;
        // 화면 경계에서는 맨 위·아래로 제한하고 나머지는 결과 행을 중앙에 둔다.
        let half = screen_rows / 2;
        if row + half >= total_rows {
            Some(0)
        } else if row < half {
            Some(scrollback_len)
        } else {
            Some(total_rows - row - half)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_rows_survive_trim_but_not_history_reset() {
        let mut terminal = tasty_terminal::Terminal::new_detached(20, 3);
        terminal.set_scrollback_limit(3);
        terminal.feed_bytes(b"zero\r\nneedle\r\ntwo\r\nthree\r\nfour");
        let mut search = SearchState::new();
        search.query = "needle".into();
        search.execute(&terminal);
        assert_eq!(search.matches[0].row, 1);
        terminal.feed_bytes(b"\r\nfive\r\nsix");
        let cut = terminal.content_cut();
        assert_eq!(cut.first_row, 1);
        assert!(search.scroll_to_current(cut).is_some());
        terminal.feed_bytes(b"\r\nseven");
        assert!(search.scroll_to_current(terminal.content_cut()).is_none());
        terminal.feed_bytes(b"\x1b[3J\r\nneedle");
        assert!(search.scroll_to_current(terminal.content_cut()).is_none());
        search.execute(&terminal);
        assert!(search.scroll_to_current(terminal.content_cut()).is_some());
    }
}
