//! `TerminalProcess` trait — Hexagonal architecture 의 *internal port*.
//!
//! 본 crate 의 `Terminal` struct 가 자체 impl. 외부 (bin) 는 `Box<dyn TerminalProcess>`
//! 받아 *동적 swap* 가능 — test 시 deterministic mock, 미래 다른 backend.
//!
//! 위치 결정: `tasty-terminal` 이 internal crate (워크스페이스) 라 *trait 정의도 crate
//! 안*. bin 의 wrap layer 회피 (1:1 passthrough adapter 없음).

use crate::events::TerminalEvent;
use crate::foreground_process::ForegroundProcessInfo;
use crate::scrollback::ScrollbackLine;

/// Terminal PTY 의 *동작 인터페이스*. `Terminal` struct 가 impl.
pub trait TerminalProcess: Send {
    // ─── 입력 ───
    fn send_bytes(&mut self, bytes: &[u8]);
    fn send_key(&mut self, text: &str);

    // ─── 처리 ───
    /// PTY read + VTE parse. 새 data 있으면 true.
    fn process(&mut self) -> bool;

    /// 누적된 VTE event 들 (OSC, prompt boundary, title, exit 등) 꺼냄.
    fn take_events(&mut self) -> Vec<TerminalEvent>;

    // ─── resize ───
    fn resize(&mut self, cols: usize, rows: usize);
    fn flush_pty_resize(&mut self) -> bool;
    fn has_pending_pty_resize(&self) -> bool;
    fn force_flush_pty_resize(&mut self);

    // ─── 화면 read ───
    fn screen_text(&self) -> String;
    fn screen_text_lines(&self, n: usize) -> String;
    fn cursor_position(&self) -> (usize, usize);
    fn cursor_visible(&self) -> bool;
    fn cols(&self) -> usize;
    fn rows(&self) -> usize;
    fn foreground_process_info(&self) -> Option<ForegroundProcessInfo>;
    fn cwd(&self) -> Option<std::path::PathBuf>;

    // ─── mark (per-terminal internal) ───
    fn set_mark(&mut self);
    fn read_since_mark(&self, strip_ansi: bool) -> String;

    // ─── scrollback ───
    fn set_scrollback_limit(&mut self, n: usize);
    fn enable_disk_scrollback(&mut self, surface_id: u32);
    fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>);
    fn prefill_visible_from_scrollback(&mut self, n: usize) -> usize;
}
