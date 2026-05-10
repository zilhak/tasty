//! `WakerFactory` 의 winit 구현.
//!
//! `main.rs`에서 EventLoop 준비 후 생성하여 `EngineState`에 주입한다.
//! `tasty-core`는 winit 의존이 0건이므로 어댑터는 본체에만 둔다.

use std::sync::Arc;

use tasty_core::WakerFactory;
use tasty_terminal::Waker;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

pub struct WinitWakerFactory {
    proxy: EventLoopProxy<AppEvent>,
}

impl WinitWakerFactory {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self { proxy }
    }
}

impl WakerFactory for WinitWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        let proxy = self.proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput(Some(surface_id)));
        })
    }

    fn make_default_waker(&self) -> Waker {
        let proxy = self.proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(AppEvent::TerminalOutput(None));
        })
    }
}
