//! `TerminalProcess` trait 의 `Terminal` impl — 기존 inherent 메서드들 delegation.

use crate::Terminal;
use crate::events::TerminalEvent;
use crate::foreground_process::ForegroundProcessInfo;
use crate::port::TerminalProcess;
use crate::scrollback::ScrollbackLine;

impl TerminalProcess for Terminal {
    fn send_bytes(&mut self, bytes: &[u8]) {
        Terminal::send_bytes(self, bytes);
    }

    fn send_key(&mut self, text: &str) {
        Terminal::send_key(self, text);
    }

    fn process(&mut self) -> bool {
        Terminal::process(self)
    }

    fn take_events(&mut self) -> Vec<TerminalEvent> {
        Terminal::take_events(self)
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        Terminal::resize(self, cols, rows);
    }

    fn flush_pty_resize(&mut self) -> bool {
        Terminal::flush_pty_resize(self)
    }

    fn has_pending_pty_resize(&self) -> bool {
        Terminal::has_pending_pty_resize(self)
    }

    fn force_flush_pty_resize(&mut self) {
        Terminal::force_flush_pty_resize(self);
    }

    fn screen_text(&self) -> String {
        Terminal::screen_text(self)
    }

    fn screen_text_lines(&self, n: usize) -> String {
        Terminal::screen_text_lines(self, n)
    }

    fn cursor_position(&self) -> (usize, usize) {
        Terminal::cursor_position(self)
    }

    fn cursor_visible(&self) -> bool {
        Terminal::cursor_visible(self)
    }

    fn cols(&self) -> usize {
        Terminal::cols(self)
    }

    fn rows(&self) -> usize {
        Terminal::rows(self)
    }

    fn foreground_process_info(&self) -> Option<ForegroundProcessInfo> {
        Terminal::foreground_process_info(self)
    }

    fn cwd(&self) -> Option<std::path::PathBuf> {
        Terminal::get_cwd(self)
    }

    fn set_mark(&mut self) {
        Terminal::set_mark(self);
    }

    fn read_since_mark(&self, strip_ansi: bool) -> String {
        Terminal::read_since_mark(self, strip_ansi)
    }

    fn set_scrollback_limit(&mut self, n: usize) {
        Terminal::set_scrollback_limit(self, n);
    }

    fn enable_disk_scrollback(&mut self, surface_id: u32) {
        Terminal::enable_disk_scrollback(self, surface_id);
    }

    fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>) {
        Terminal::inject_scrollback(self, lines);
    }

    fn prefill_visible_from_scrollback(&mut self, n: usize) -> usize {
        Terminal::prefill_visible_from_scrollback(self, n)
    }
}
