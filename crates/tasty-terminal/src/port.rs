//! TerminalProcess lets the host use a Terminal or a test implementation through one interface.

use crate::events::TerminalEvent;
use crate::foreground_process::ForegroundProcessInfo;
use crate::scrollback::ScrollbackLine;

/// Terminal PTY 의 *동작 인터페이스*. `Terminal` struct 가 impl.
pub trait TerminalProcess: Send {
    fn send_bytes(&mut self, bytes: &[u8]);
    fn send_key(&mut self, text: &str);

    /// Process pending resize and exit checks; report newly ingested data. Parsing runs separately.
    fn process(&mut self) -> bool;

    /// 누적된 VTE event 들 (OSC, prompt boundary, title, exit 등) 꺼냄.
    fn take_events(&mut self) -> Vec<TerminalEvent>;

    fn resize(&mut self, cols: usize, rows: usize);
    fn flush_pty_resize(&mut self) -> bool;
    fn has_pending_pty_resize(&self) -> bool;
    fn force_flush_pty_resize(&mut self);

    /// `include_dim=false` 면 dim(ghost-suggestion) 셀을 제외한다.
    fn screen_text(&self, include_dim: bool) -> String;
    fn screen_text_lines(&self, n: usize, include_dim: bool) -> String;
    fn cursor_position(&self) -> (usize, usize);
    fn cursor_visible(&self) -> bool;
    fn cols(&self) -> usize;
    fn rows(&self) -> usize;
    fn foreground_process_info(&self) -> Option<ForegroundProcessInfo>;
    fn cwd(&self) -> Option<std::path::PathBuf>;

    fn set_mark(&mut self);
    fn read_since_mark(&self, strip_ansi: bool) -> String;

    fn set_scrollback_limit(&mut self, n: usize);
    fn enable_disk_scrollback(&mut self, surface_id: u32);
    fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>);
    fn prefill_visible_from_scrollback(&mut self, n: usize) -> usize;
}
