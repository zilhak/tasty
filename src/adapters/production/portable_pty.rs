//! PortablePtyService — `tasty_terminal::Terminal::new` 기반 production PtyService.
//!
//! PtyService trait (port) 의 `Arc<dyn TerminalWaker>` 를 `tasty_terminal::Waker`
//! (Arc closure) 로 변환해 `Terminal::new` 에 전달. 결과 `Terminal` 이
//! `TerminalProcess` trait 구현 (tasty-terminal 안 정의) — 그대로 `Box<dyn>` 으로
//! 반환.

use std::sync::Arc;

use tasty_terminal::{Terminal, TerminalConfig, TerminalProcess};

use crate::ports::pty::{PtyService, TerminalWaker};

#[derive(Debug, Default)]
pub struct PortablePtyService;

impl PtyService for PortablePtyService {
    fn spawn(
        &self,
        config: TerminalConfig<'_>,
        waker: Arc<dyn TerminalWaker>,
    ) -> anyhow::Result<Box<dyn TerminalProcess>> {
        let surface_id = config.surface_id;
        let waker_for_closure = Arc::clone(&waker);
        // PtyService 가 자기 spawn 의 surface_id 를 capture 해 *targeted waker* 의
        // 의미로 호출. (untargeted 가 필요하면 호출자가 None 전달해 만들어도 OK)
        let closure: tasty_terminal::Waker = Arc::new(move || {
            waker_for_closure.wake(Some(surface_id));
        });
        let terminal = Terminal::new(config, closure)?;
        Ok(Box::new(terminal))
    }
}
