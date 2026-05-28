//! MockPtyService — PTY spawn 을 *기록만* 함. test 시 외부 process 없이 동작.
//!
//! 반환되는 `TerminalProcess` 의 실제 구현 (MockTerminal) 은 D.3.A.4 에서
//! `tasty-terminal::testing` 안에 정의 예정. 현재는 deterministic stub.

use std::sync::{Arc, Mutex};

use tasty_terminal::foreground_process::ForegroundProcessInfo;
use tasty_terminal::{ScrollbackLine, TerminalConfig, TerminalEvent, TerminalProcess};

use crate::ports::pty::{PtyService, TerminalWaker};

#[derive(Debug, Clone)]
pub struct SpawnRecord {
    pub surface_id: u32,
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<String>,
}

#[derive(Debug, Default)]
pub struct MockPtyService {
    pub spawns: Mutex<Vec<SpawnRecord>>,
}

impl MockPtyService {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PtyService for MockPtyService {
    fn spawn(
        &self,
        config: TerminalConfig<'_>,
        _waker: Arc<dyn TerminalWaker>,
    ) -> anyhow::Result<Box<dyn TerminalProcess>> {
        self.spawns
            .lock()
            .expect("MockPtyService poisoned")
            .push(SpawnRecord {
                surface_id: config.surface_id,
                cols: config.cols,
                rows: config.rows,
                shell: config.shell.map(|s| s.to_string()),
            });
        Ok(Box::new(StubTerminal {
            cols: config.cols,
            rows: config.rows,
            sent: Vec::new(),
        }))
    }
}

/// Inline minimal stub. 정식 mock 은 D.3.A.4 에서 tasty-terminal 안에 정의.
struct StubTerminal {
    cols: usize,
    rows: usize,
    sent: Vec<u8>,
}

impl TerminalProcess for StubTerminal {
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
        Vec::new()
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
    fn screen_text(&self) -> String {
        String::new()
    }
    fn screen_text_lines(&self, _n: usize) -> String {
        String::new()
    }
    fn cursor_position(&self) -> (usize, usize) {
        (0, 0)
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
        None
    }
    fn cwd(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn set_mark(&mut self) {}
    fn read_since_mark(&self, _strip_ansi: bool) -> String {
        String::new()
    }
    fn set_scrollback_limit(&mut self, _n: usize) {}
    fn enable_disk_scrollback(&mut self, _surface_id: u32) {}
    fn inject_scrollback(&mut self, _lines: Vec<ScrollbackLine>) {}
    fn prefill_visible_from_scrollback(&mut self, _n: usize) -> usize {
        0
    }
}
