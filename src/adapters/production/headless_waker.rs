//! 헤드리스에서 AppEvent 채널로 IPC·PTY 처리를 깨운다.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use tasty_terminal::Waker;
use tasty_terminal::waker_factory::{SharedWakerFactory, WakerFactory};

use crate::AppEvent;
use crate::ipc::server::IpcWaker;

pub(crate) struct HeadlessWaker {
    tx: Sender<AppEvent>,
    /// 처리 전인 IpcReady는 채널에 하나만 둔다. 모든 IPC waker가 같은 플래그를 공유한다.
    /// 반복 알림이 PTY·플러그인 이벤트를 밀어내지 않게 하며, 남은 명령은 wake_ipc로 다시 알린다.
    ipc_gate: Arc<AtomicBool>,
}

impl HeadlessWaker {
    pub(crate) fn new(tx: Sender<AppEvent>) -> Self {
        Self {
            tx,
            ipc_gate: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn ipc_waker(&self) -> IpcWaker {
        let tx = self.tx.clone();
        let gate = self.ipc_gate.clone();
        Arc::new(move || send_ipc_ready(&tx, &gate))
    }

    /// 회차를 시작하기 전에 플래그를 풀어 새 요청이 다음 wake를 만들 수 있게 한다.
    /// swap으로 생략된 wake의 쓰기를 획득해 먼저 들어온 명령도 다음 회차에서 보이게 한다.
    pub(crate) fn note_ipc_drained(&self) {
        self.ipc_gate.swap(false, Ordering::AcqRel);
    }

    /// 예산 때문에 남은 명령을 처리하도록 채널 끝에 다시 알린다. 앞선 다른 wake가 먼저 처리된다.
    pub(crate) fn wake_ipc(&self) {
        send_ipc_ready(&self.tx, &self.ipc_gate);
    }

    pub(crate) fn stream_waker(&self) -> IpcWaker {
        let tx = self.tx.clone();
        Arc::new(move || {
            let _ = tx.send(AppEvent::StreamReady); // shutdown race — drop quietly.
        })
    }

    pub(crate) fn waker_factory(&self) -> SharedWakerFactory {
        Arc::new(HeadlessWakerFactory {
            tx: self.tx.clone(),
            default_gate: Arc::new(AtomicBool::new(false)),
            targeted_gates: Mutex::new(HashMap::new()),
            poison_reported: AtomicBool::new(false),
        })
    }
}

fn send_ipc_ready(tx: &Sender<AppEvent>, gate: &AtomicBool) {
    if gate.swap(true, Ordering::AcqRel) {
        return;
    }
    // headless receiver 가 종료된 후의 race 는 무시 (정상 shutdown 시퀀스).
    let _ = tx.send(AppEvent::IpcReady); // receiver dropped during shutdown — drop quietly.
}

pub(crate) struct HeadlessWakerFactory {
    tx: Sender<AppEvent>,
    /// 전체 drain은 공용 플래그, surface별 drain은 개별 플래그를 쓴다.
    default_gate: Arc<AtomicBool>,
    targeted_gates: Mutex<HashMap<u32, Arc<AtomicBool>>>,
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

    // poison 이후에도 waker를 사용할 수 있어야 한다. 이 시험은 헤드리스 빌드에서만 실행된다.
    #[test]
    fn a_poisoned_gate_map_does_not_take_the_factory_down() {
        let (tx, _rx) = mpsc::channel();
        let factory = Arc::new(HeadlessWakerFactory {
            tx,
            default_gate: Arc::new(AtomicBool::new(false)),
            targeted_gates: Mutex::new(HashMap::new()),
            poison_reported: AtomicBool::new(false),
        });
        let _ = factory.make_targeted_waker(11);
        let held = Arc::clone(&factory);
        let joined = std::thread::spawn(move || {
            let _guard = held.targeted_gates.lock().expect("fresh mutex");
            panic!("a thread dies while holding the gate map");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(factory.targeted_gates.lock().is_err(), "poison 됐어야 한다");

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

    fn ipc_ready_count(rx: &mpsc::Receiver<AppEvent>) -> usize {
        rx.try_iter()
            .filter(|ev| matches!(ev, AppEvent::IpcReady))
            .count()
    }

    #[test]
    fn ipc_wakes_coalesce_until_the_loop_takes_one() {
        let (tx, rx) = mpsc::channel();
        let hw = HeadlessWaker::new(tx);
        let a = hw.ipc_waker();
        let b = hw.ipc_waker();
        a();
        b();
        a();
        assert_eq!(
            ipc_ready_count(&rx),
            1,
            "사본 여럿이 부른 wake 가 하나로 접혀야 한다"
        );

        hw.note_ipc_drained();
        b();
        b();
        assert_eq!(ipc_ready_count(&rx), 1, "꺼낸 뒤에는 다시 하나가 선다");
    }

    #[test]
    fn a_rewake_obeys_the_same_gate() {
        let (tx, rx) = mpsc::channel();
        let hw = HeadlessWaker::new(tx);
        hw.ipc_waker()();
        hw.wake_ipc();
        assert_eq!(ipc_ready_count(&rx), 1);
        hw.note_ipc_drained();
        hw.wake_ipc();
        assert_eq!(ipc_ready_count(&rx), 1, "꺼낸 뒤의 재깨움은 서야 한다");
    }

    #[test]
    fn default_waker_coalesces_until_drained() {
        let (tx, rx) = mpsc::channel();
        let factory = HeadlessWaker::new(tx).waker_factory();
        let waker = factory.make_default_waker();

        waker();
        waker();
        waker();
        assert_eq!(drain_counts(&rx), (1, 0), "coalesced to a single None");

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

        waker_a();
        waker_a();
        waker_b();
        waker_b();
        assert_eq!(drain_counts(&rx), (0, 2), "each surface queues once");

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

        factory.forget_surface(1);
        let waker2 = factory.make_targeted_waker(1);
        waker2();
        assert_eq!(drain_counts(&rx), (0, 1), "gate was removed → re-armed");
    }
}
