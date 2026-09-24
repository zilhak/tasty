use tasty_terminal::search::{SearchError, SearchMatch, SearchOptions};

pub struct SearchState {
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
        match terminal.search(&self.query, &options) {
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
    pub fn scroll_to_current(&self, scrollback_len: usize, screen_rows: usize) -> Option<usize> {
        let m = self.matches.get(self.current_index)?;
        let total_rows = scrollback_len + screen_rows;
        // 화면 경계에서는 맨 위·아래로 제한하고 나머지는 결과 행을 중앙에 둔다.
        let half = screen_rows / 2;
        if m.row + half >= total_rows {
            Some(0)
        } else if m.row < half {
            Some(scrollback_len)
        } else {
            Some(total_rows - m.row - half)
        }
    }
}
