//! Headless 빌드 / `--headless` 모드용 IpcWaker + TerminalWaker.
//!
//! `winit_waker.rs` 와 동등 시그니처. winit `EventLoopProxy` 대신 `mpsc::Sender`
//! 를 통해 [`crate::AppEvent`] 를 push — `boot::run_headless` 의 receiver loop 가
//! 깨어난다.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use tasty_terminal::Waker;
use tasty_terminal::waker_factory::{SharedWakerFactory, WakerFactory};

use crate::AppEvent;
use crate::ipc::server::IpcWaker;
use crate::ports::pty::TerminalWaker;

/// Headless 모드 waker factory. `mpsc::Sender<AppEvent>` 한 개를 clone 해서
/// IPC / PTY waker 두 가지로 fan-out 한다.
pub(crate) struct HeadlessWaker {
    tx: Sender<AppEvent>,
}

impl HeadlessWaker {
    pub(crate) fn new(tx: Sender<AppEvent>) -> Self {
        Self { tx }
    }

    /// IPC accept 스레드가 호출하는 waker. `IpcReady` 발화.
    pub(crate) fn ipc_waker(&self) -> IpcWaker {
        let tx = self.tx.clone();
        Arc::new(move || {
            // headless receiver 가 종료된 후의 race 는 무시 (정상 shutdown 시퀀스).
            let _ = tx.send(AppEvent::IpcReady); // receiver dropped during shutdown — drop quietly.
        })
    }

    /// 스트림 연결의 read 스레드가 inbound 프레임 수신 시 호출하는 waker.
    /// `StreamReady` 발화 → 메인 루프가 `StreamHub::pump_inbound` 로 drain.
    pub(crate) fn stream_waker(&self) -> IpcWaker {
        let tx = self.tx.clone();
        Arc::new(move || {
            let _ = tx.send(AppEvent::StreamReady); // shutdown race — drop quietly.
        })
    }

    /// PTY reader 스레드가 호출하는 waker. `TerminalOutput(surface_id?)` 발화.
    pub(crate) fn terminal_waker(&self) -> Arc<dyn TerminalWaker> {
        Arc::new(HeadlessTerminalWaker {
            tx: self.tx.clone(),
        })
    }

    /// `CoreState` 에 주입할 WakerFactory. targeted/default 양쪽 waker 가 같은
    /// `mpsc::Sender` 로 `TerminalOutput` 을 push 한다.
    pub(crate) fn waker_factory(&self) -> SharedWakerFactory {
        Arc::new(HeadlessWakerFactory {
            tx: self.tx.clone(),
        })
    }
}

/// `WakerFactory` 의 headless 구현 — winit `WinitWakerFactory` 의 mpsc 미러.
/// `CoreState::make_waker` 가 surface 별 targeted waker 를 발급할 때 사용한다.
pub(crate) struct HeadlessWakerFactory {
    tx: Sender<AppEvent>,
}

impl WakerFactory for HeadlessWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        let tx = self.tx.clone();
        Arc::new(move || {
            // headless receiver shutdown race 는 무시 (정상 shutdown 시퀀스).
            let _ = tx.send(AppEvent::TerminalOutput(Some(surface_id)));
        })
    }

    fn make_default_waker(&self) -> Waker {
        let tx = self.tx.clone();
        Arc::new(move || {
            let _ = tx.send(AppEvent::TerminalOutput(None));
        })
    }
}

struct HeadlessTerminalWaker {
    tx: Sender<AppEvent>,
}

impl TerminalWaker for HeadlessTerminalWaker {
    fn wake(&self, surface_id: Option<u32>) {
        // receiver shutdown race 는 무시 (정상 shutdown 시퀀스).
        let _ = self.tx.send(AppEvent::TerminalOutput(surface_id)); // shutdown race — drop quietly.
    }
}
