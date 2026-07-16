//! `MockTerminal` — `TerminalProcess` 의 deterministic mock.
//!
//! PTY 없이 in-process simulator. test 시 `Box<dyn TerminalProcess>` 자리에 들어감.

use crate::events::TerminalEvent;
use crate::foreground_process::ForegroundProcessInfo;
use crate::port::TerminalProcess;
use crate::scrollback::ScrollbackLine;

#[derive(Default)]
pub struct MockTerminal {
    /// 외부로 보내려 한 raw 바이트 (test 시 assert).
    pub sent: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
    pub fake_screen: String,
    pub fake_events: Vec<TerminalEvent>,
    pub fake_cursor: (usize, usize),
    pub fake_cwd: Option<std::path::PathBuf>,
    pub fake_foreground: Option<ForegroundProcessInfo>,
    pub mark_text: String,
    pub scrollback_limit: usize,
    pub injected_scrollback: Vec<ScrollbackLine>,
}

impl MockTerminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            ..Self::default()
        }
    }
}

impl TerminalProcess for MockTerminal {
    fn send_bytes(&mut self, bytes: &[u8]) {
        self.sent.extend_from_slice(bytes);
    }
    fn send_key(&mut self, text: &str) {
        self.sent.extend_from_slice(text.as_bytes());
    }
    fn process(&mut self) -> bool {
        false
    }
    fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::take(&mut self.fake_events)
    }
    fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
    }
    fn flush_pty_resize(&mut self) -> bool {
        false
    }
    fn has_pending_pty_resize(&self) -> bool {
        false
    }
    fn force_flush_pty_resize(&mut self) {}
    fn screen_text(&self, _include_dim: bool) -> String {
        self.fake_screen.clone()
    }
    fn screen_text_lines(&self, _n: usize, _include_dim: bool) -> String {
        self.fake_screen.clone()
    }
    fn cursor_position(&self) -> (usize, usize) {
        self.fake_cursor
    }
    fn cursor_visible(&self) -> bool {
        true
    }
    fn cols(&self) -> usize {
        self.cols
    }
    fn rows(&self) -> usize {
        self.rows
    }
    fn foreground_process_info(&self) -> Option<ForegroundProcessInfo> {
        self.fake_foreground.clone()
    }
    fn cwd(&self) -> Option<std::path::PathBuf> {
        self.fake_cwd.clone()
    }
    fn set_mark(&mut self) {
        // mock: 현재 fake_screen 의 길이를 mark 위치로 가정.
        self.mark_text = self.fake_screen.clone();
    }
    fn read_since_mark(&self, _strip_ansi: bool) -> String {
        // mock: mark 이후 추가된 부분 (fake_screen 의 prefix 매칭).
        if let Some(diff) = self.fake_screen.strip_prefix(self.mark_text.as_str()) {
            diff.to_string()
        } else {
            self.fake_screen.clone()
        }
    }
    fn set_scrollback_limit(&mut self, n: usize) {
        self.scrollback_limit = n;
    }
    fn enable_disk_scrollback(&mut self, _surface_id: u32) {}
    fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>) {
        self.injected_scrollback.extend(lines);
    }
    fn prefill_visible_from_scrollback(&mut self, _n: usize) -> usize {
        0
    }
}
