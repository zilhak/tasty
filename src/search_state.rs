use tasty_terminal::search::{SearchMatch, SearchOptions};

/// UI-level search state, stored in AppState.
pub struct SearchState {
    /// Current search query.
    pub query: String,
    /// All matches in the terminal buffer (sorted oldest→newest).
    pub matches: Vec<SearchMatch>,
    /// Index of the currently selected match (for navigation).
    pub current_index: usize,
    /// Surface ID being searched.
    pub surface_id: u32,
    /// Whether to ignore case (default: true).
    pub case_insensitive: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_index: 0,
            surface_id: 0,
            case_insensitive: true,
        }
    }

    /// Run search on the given terminal and update matches.
    pub fn execute(&mut self, terminal: &tasty_terminal::Terminal) {
        let options = SearchOptions {
            case_insensitive: self.case_insensitive,
        };
        self.matches = terminal.search(&self.query, &options);
        // Clamp current_index.
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

    /// Clear search state.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_index = 0;
    }

    /// Get the scroll offset needed to show the current match.
    /// Returns None if no matches or current match is on-screen.
    pub fn scroll_to_current(&self, scrollback_len: usize, screen_rows: usize) -> Option<usize> {
        let m = self.matches.get(self.current_index)?;
        let total_rows = scrollback_len + screen_rows;
        // scroll_offset = 0 means showing the bottom (last screen_rows).
        // Visible range: [total_rows - screen_rows - scroll_offset .. total_rows - scroll_offset)
        // To show row `m.row`, we need: total_rows - screen_rows - offset <= m.row
        //   → offset <= total_rows - screen_rows - m.row
        // And: m.row < total_rows - offset
        //   → offset < total_rows - m.row
        // Target: center the match vertically.
        let half = screen_rows / 2;
        if m.row + half >= total_rows {
            // Match is near the bottom — scroll to bottom.
            Some(0)
        } else if m.row < half {
            // Match is near the top — scroll to max.
            Some(scrollback_len)
        } else {
            Some(total_rows - m.row - half)
        }
    }
}
