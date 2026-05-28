//! PtyService port — terminal PTY spawn 의 외부 자원 추상화.
//!
//! Production adapter 가 `portable-pty` + `tasty-terminal::Terminal::new` 조합 wrap.
//! Test mock 은 deterministic in-process simulator (tasty-terminal::testing 의 mock 사용).
//!
//! ## 관련 trait 위치
//!
//! - `PtyService` (외부 자원 spawn 추상화) — 본 module
//! - `TerminalWaker` (winit proxy 의존) — 본 module
//! - `TerminalProcess` (Terminal 객체의 동작 trait) — **`tasty_terminal::TerminalProcess`**
//!   (internal crate 자체 정의)

use std::sync::Arc;

use tasty_terminal::{TerminalConfig, TerminalProcess};

/// Terminal PTY 의 생성. `Box<dyn TerminalProcess>` 반환 — trait 통해 다형성.
#[allow(dead_code)]
pub trait PtyService: Send + Sync {
    fn spawn(
        &self,
        config: TerminalConfig<'_>,
        waker: Arc<dyn TerminalWaker>,
    ) -> anyhow::Result<Box<dyn TerminalProcess>>;
}

/// PTY output 알림. Core 외부 (winit EventLoopProxy / channel) 가 구현.
#[allow(dead_code)]
pub trait TerminalWaker: Send + Sync {
    /// 특정 surface 의 PTY output 도착 알림.
    fn wake(&self, surface_id: Option<u32>);
}
