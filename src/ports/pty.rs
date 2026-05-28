//! PtyService port — terminal PTY spawn + IO + state.
//!
//! Production adapter 가 `portable-pty` + `tasty-terminal` 조합 wrap. Test mock 은
//! deterministic in-process simulator.

use tasty_terminal::{ScrollbackLine, TerminalConfig, TerminalEvent};

/// Terminal PTY 의 생성 + 관리.
#[allow(dead_code)]
pub trait PtyService: Send + Sync {
    /// 새 PTY 를 fork+exec 하고 VTE 파서로 wrap 한 `TerminalProcess` 반환.
    /// `waker` 는 PTY output 도착 시 호출됨.
    fn spawn(
        &self,
        config: TerminalConfig<'_>,
        waker: std::sync::Arc<dyn TerminalWaker>,
    ) -> anyhow::Result<Box<dyn TerminalProcess>>;
}

/// PTY output 알림. Core 외부 (winit EventLoopProxy / channel) 가 구현.
#[allow(dead_code)]
pub trait TerminalWaker: Send + Sync {
    /// 특정 surface 의 PTY output 도착 알림.
    fn wake(&self, surface_id: Option<u32>);
}

/// 살아 있는 terminal 인스턴스. Drop 시 SIGHUP → child exit.
#[allow(dead_code)]
pub trait TerminalProcess: Send {
    // ─── 입력 ───
    fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()>;
    fn send_key(&mut self, text: &str) -> anyhow::Result<()>;

    // ─── 처리 ───
    /// PTY read + VTE parse. 새 data 있으면 true.
    fn process(&mut self) -> bool;

    /// 누적된 VTE event 들 (OSC, prompt boundary, title, exit 등) 꺼냄.
    fn take_events(&mut self) -> Vec<TerminalEvent>;

    // ─── resize ───
    fn resize(&mut self, cols: usize, rows: usize) -> anyhow::Result<()>;
    fn flush_pty_resize(&mut self);
    fn has_pending_pty_resize(&self) -> bool;
    fn force_flush_pty_resize(&mut self);

    // ─── 화면 read ───
    fn screen_text(&self) -> String;
    fn screen_text_lines(&self, n: usize) -> String;
    fn cursor_position(&self) -> (usize, usize);
    fn cursor_visible(&self) -> bool;
    fn cols(&self) -> usize;
    fn rows(&self) -> usize;
    fn foreground_process_info(&self) -> Option<ForegroundProcess>;
    fn cwd(&self) -> Option<String>;

    // ─── mark (per-terminal internal) ───
    fn set_mark(&mut self);
    fn read_since_mark(&mut self, strip_ansi: bool) -> String;

    // ─── scrollback ───
    fn set_scrollback_limit(&mut self, n: usize);
    fn enable_disk_scrollback(&mut self, surface_id: u32);
    fn inject_scrollback(&mut self, lines: Vec<ScrollbackLine>);
    fn prefill_visible_from_scrollback(&mut self, n: usize);
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ForegroundProcess {
    pub name: String,
    pub pid: u32,
}
