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
            poison_reported: AtomicBool::new(false),
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
    /// `targeted_gates` poison 을 이미 보고했는가 — 로그 폭주 방지용 1 회 게이트.
    poison_reported: AtomicBool,
}

impl WakerFactory for HeadlessWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        let tx = self.tx.clone();
        let gate = crate::waker::recover_gate_lock(
            self.targeted_gates.lock(),
            "HeadlessWakerFactory targeted_gates",
            &self.poison_reported,
        )
        .entry(surface_id)
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone();
        Arc::new(move || {
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            let _ = tx.send(AppEvent::TerminalOutput(Some(surface_id))); // headless receiver shutdown race — send 실패 무시(정상 종료 시퀀스).
        })
    }

    fn make_default_waker(&self) -> Waker {
        let tx = self.tx.clone();
        let gate = self.default_gate.clone();
        Arc::new(move || {
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            let _ = tx.send(AppEvent::TerminalOutput(None)); // headless receiver shutdown race — send 실패 무시(정상 종료 시퀀스).
        })
    }

    fn note_drained(&self, surface_id: Option<u32>) {
        match surface_id {
            Some(sid) => {
                if let Some(gate) = crate::waker::recover_gate_lock(
                    self.targeted_gates.lock(),
                    "HeadlessWakerFactory targeted_gates",
                    &self.poison_reported,
                )
                .get(&sid)
                {
                    gate.store(false, Ordering::Release);
                }
            }
            None => self.default_gate.store(false, Ordering::Release),
        }
    }

    fn forget_surface(&self, surface_id: u32) {
        // surface 닫힘 — 게이트 제거(미제거 시 surface 마다 영구 누적).
        crate::waker::recover_gate_lock(
            self.targeted_gates.lock(),
            "HeadlessWakerFactory targeted_gates",
            &self.poison_reported,
        )
        .remove(&surface_id);
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

    /// 게이트 맵이 poison 돼도 factory 는 계속 동작한다.
    ///
    /// `.expect()` 이던 시절에는 이 세 호출이 전부 패닉했다. headless 호스트에서는
    /// 그 패닉이 **메인 루프 스레드**에서 나 프로세스 전체가 죽는다 — 사망 범위가
    /// 게이트 맵 하나의 정합성보다 압도적으로 크다는 것이 복구를 고른 이유다
    /// (`docs/dev-guide/error-handling.md` "락 poison").
    ///
    /// **주의**: 이 모듈은 `#[cfg(not(feature = "gui"))]` 이고 기본 빌드는 gui 라,
    /// `cargo test` 는 이 파일의 테스트를 컴파일하지 않는다. 반대로 `--no-default-features`
    /// 로는 테스트 타깃 자체가 안 서므로(바이너리의 다른 테스트들이 egui 를 쓴다) 현재
    /// 이 테스트를 실행할 수 있는 조합이 없다 — 같은 파일의 기존 테스트도 마찬가지다.
    /// 실제로 도는 등가 커버리지는 `crate::waker::poison_tests` 에 있다.
    #[test]
    fn a_poisoned_gate_map_does_not_take_the_factory_down() {
        let (tx, _rx) = mpsc::channel();
        let factory = Arc::new(HeadlessWakerFactory {
            tx,
            default_gate: Arc::new(AtomicBool::new(false)),
            targeted_gates: Mutex::new(HashMap::new()),
            poison_reported: AtomicBool::new(false),
        });
        // 게이트를 하나 만들어 둔 뒤 락을 든 채 패닉시켜 poison 을 만든다.
        let _ = factory.make_targeted_waker(11);
        let held = Arc::clone(&factory);
        let joined = std::thread::spawn(move || {
            let _guard = held.targeted_gates.lock().expect("fresh mutex");
            panic!("a thread dies while holding the gate map");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(factory.targeted_gates.lock().is_err(), "poison 됐어야 한다");

        // 세 진입점 모두 패닉하지 않는다.
        let _ = factory.make_targeted_waker(12);
        factory.note_drained(Some(11));
        factory.forget_surface(11);

        let gates = factory
            .targeted_gates
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        assert!(
            gates.contains_key(&12),
            "poison 이후에도 게이트 등록이 된다"
        );
        assert!(
            !gates.contains_key(&11),
            "poison 이후에도 게이트 정리가 된다"
        );
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

    #[test]
    fn forget_surface_removes_gate() {
        let (tx, rx) = mpsc::channel();
        let factory = HeadlessWaker::new(tx).waker_factory();
        let waker = factory.make_targeted_waker(1);

        waker(); // 큐잉(게이트 true)
        assert_eq!(drain_counts(&rx), (0, 1));
        waker(); // drain 안 했으니 게이트 여전히 true → coalesce
        assert_eq!(drain_counts(&rx), (0, 0));

        // surface 닫힘 → 게이트 제거. 새 waker 는 fresh 게이트(false)를 만든다.
        factory.forget_surface(1);
        let waker2 = factory.make_targeted_waker(1);
        waker2();
        // 게이트가 제거됐으므로 다시 큐잉됨(미제거였다면 옛 true 게이트 재사용 → 0).
        assert_eq!(drain_counts(&rx), (0, 1), "gate was removed → re-armed");
    }
}
