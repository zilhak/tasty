//! Headless 빌드 / `--headless` 모드용 IpcWaker + WakerFactory.
//!
//! gui 빌드의 winit `EventLoopProxy` 대신 `mpsc::Sender` 를 통해
//! [`crate::AppEvent`] 를 push — `boot::run_headless` 의 receiver loop 가
//! 깨어난다.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use tasty_terminal::Waker;
use tasty_terminal::waker_factory::{SharedWakerFactory, WakerFactory};

use crate::AppEvent;
use crate::ipc::server::IpcWaker;

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

    /// `CoreState` 에 주입할 WakerFactory. targeted/default 양쪽 waker 가 같은
    /// `mpsc::Sender` 로 `TerminalOutput` 을 push 한다.
    pub(crate) fn waker_factory(&self) -> SharedWakerFactory {
        Arc::new(HeadlessWakerFactory {
            tx: self.tx.clone(),
            default_gate: Arc::new(AtomicBool::new(false)),
            targeted_gates: Mutex::new(HashMap::new()),
        })
    }
}

/// `WakerFactory` 의 headless 구현 — winit `WinitWakerFactory` 의 mpsc 미러.
/// `CoreState::make_waker` 가 surface 별 targeted waker 를 발급할 때 사용한다.
pub(crate) struct HeadlessWakerFactory {
    tx: Sender<AppEvent>,
    /// `WinitWakerFactory` 와 동일한 dual gate (research §5). None 은 전체 drain
    /// 이라 글로벌 1 개, Some 은 surface 단위 drain 이라 surface 별 게이트.
    default_gate: Arc<AtomicBool>,
    targeted_gates: Mutex<HashMap<u32, Arc<AtomicBool>>>,
}

impl WakerFactory for HeadlessWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        let tx = self.tx.clone();
        let gate = self
            .targeted_gates
            .lock()
            .expect("HeadlessWakerFactory targeted_gates poisoned")
            .entry(surface_id)
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone();
        Arc::new(move || {
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            // headless receiver shutdown race 는 무시 (정상 shutdown 시퀀스).
            let _ = tx.send(AppEvent::TerminalOutput(Some(surface_id)));
        })
    }

    fn make_default_waker(&self) -> Waker {
        let tx = self.tx.clone();
        let gate = self.default_gate.clone();
        Arc::new(move || {
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            // headless receiver shutdown race 는 무시 (정상 shutdown 시퀀스).
            let _ = tx.send(AppEvent::TerminalOutput(None));
        })
    }

    fn note_drained(&self, surface_id: Option<u32>) {
        match surface_id {
            Some(sid) => {
                if let Some(gate) = self
                    .targeted_gates
                    .lock()
                    .expect("HeadlessWakerFactory targeted_gates poisoned")
                    .get(&sid)
                {
                    gate.store(false, Ordering::Release);
                }
            }
            None => self.default_gate.store(false, Ordering::Release),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// 큐에 쌓인 `TerminalOutput` 을 (None 개수, Some 개수) 로 집계하며 비운다.
    fn drain_counts(rx: &mpsc::Receiver<AppEvent>) -> (usize, usize) {
        let mut none = 0;
        let mut some = 0;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                AppEvent::TerminalOutput(None) => none += 1,
                AppEvent::TerminalOutput(Some(_)) => some += 1,
                _ => {}
            }
        }
        (none, some)
    }

    #[test]
    fn default_waker_coalesces_until_drained() {
        let (tx, rx) = mpsc::channel();
        let factory = HeadlessWaker::new(tx).waker_factory();
        let waker = factory.make_default_waker();

        // 연속 호출 → 직전 wake 가 소화되기 전까지 1 회만 큐잉.
        waker();
        waker();
        waker();
        assert_eq!(drain_counts(&rx), (1, 0), "coalesced to a single None");

        // 핸들러가 drain 후 게이트 리셋 → 다시 큐잉 가능.
        factory.note_drained(None);
        waker();
        waker();
        assert_eq!(
            drain_counts(&rx),
            (1, 0),
            "re-armed after note_drained(None)"
        );
    }

    #[test]
    fn targeted_wakers_are_independent_per_surface() {
        let (tx, rx) = mpsc::channel();
        let factory = HeadlessWaker::new(tx).waker_factory();
        let waker_a = factory.make_targeted_waker(1);
        let waker_b = factory.make_targeted_waker(2);

        // surface 1 게이트가 닫혀도 surface 2 는 독립적으로 큐잉된다.
        waker_a();
        waker_a();
        waker_b();
        waker_b();
        assert_eq!(drain_counts(&rx), (0, 2), "each surface queues once");

        // surface 1 만 리셋 → surface 1 만 재무장, surface 2 는 여전히 닫힘.
        factory.note_drained(Some(1));
        waker_a();
        waker_b();
        assert_eq!(drain_counts(&rx), (0, 1), "only surface 1 re-armed");
    }
}
