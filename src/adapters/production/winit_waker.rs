//! WinitWaker — winit `EventLoopProxy` 기반 production TerminalWaker.
//!
//! PTY output 도착 시 `AppEvent::TerminalOutput(surface_id?)` 발행. winit
//! event_loop 가 깨어나 `process_pty_output` 처리.

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::ports::pty::TerminalWaker;

pub struct WinitWaker {
    proxy: EventLoopProxy<AppEvent>,
}

impl WinitWaker {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }
}

impl TerminalWaker for WinitWaker {
    fn wake(&self, surface_id: Option<u32>) {
        crate::shortcuts::send_app_event(&self.proxy, AppEvent::TerminalOutput(surface_id));
    }
}
