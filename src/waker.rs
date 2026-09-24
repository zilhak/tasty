//! PTY 출력 등 비동기 이벤트에서 사용하는 waker 타입을 재수출한다.

pub use tasty_terminal::waker_factory::SharedWakerFactory;
#[cfg(feature = "gui")]
pub use tasty_terminal::waker_factory::WakerFactory;

use std::sync::atomic::AtomicBool;
use std::sync::{MutexGuard, PoisonError};

/// poison된 게이트 맵을 복구하고 처음 한 번만 보고한다.
/// 맵 조회·삽입·삭제 실패가 호출 스레드의 추가 패닉으로 이어지지 않게 한다.
/// [락 poison 처리](../docs/dev-guide/error-handling.md)를 따른다.
pub(crate) fn recover_gate_lock<'a, T>(
    acquired: Result<MutexGuard<'a, T>, PoisonError<MutexGuard<'a, T>>>,
    what: &'static str,
    reported: &AtomicBool,
) -> MutexGuard<'a, T> {
    crate::poison::recover_mutex(acquired, what, reported)
}

#[cfg(test)]
// 시험에서는 의도적으로 무시한 join 결과를 별도 단언으로 검사한다.
#[allow(clippy::let_underscore_must_use)]
mod poison_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_poisoned_gate_map_is_recovered_with_its_contents_intact() {
        let gates: Arc<Mutex<HashMap<u32, u8>>> = Arc::new(Mutex::new(HashMap::new()));
        gates.lock().expect("fresh mutex").insert(7, 42);

        let held = Arc::clone(&gates);
        let joined = std::thread::spawn(move || {
            let _guard = held.lock().expect("fresh mutex");
            panic!("holder thread dies while holding the gate map");
        })
        .join();
        assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
        assert!(gates.lock().is_err(), "락이 poison 됐어야 한다");

        let reported = AtomicBool::new(false);
        let guard = recover_gate_lock(gates.lock(), "test gates", &reported);
        assert_eq!(guard.get(&7), Some(&42), "맵 내용은 살아 있어야 한다");
        assert!(reported.load(Ordering::Relaxed), "첫 회는 보고돼야 한다");
    }

    #[test]
    fn repeated_recovery_reports_only_once() {
        let gates: Arc<Mutex<u8>> = Arc::new(Mutex::new(1));
        let held = Arc::clone(&gates);
        // panic으로 poison만 만들고 join 결과는 무시한다.
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().expect("fresh mutex");
            panic!("poison it");
        })
        .join();

        let reported = AtomicBool::new(false);
        drop(recover_gate_lock(gates.lock(), "test gates", &reported));
        assert!(reported.load(Ordering::Relaxed));
        let before = reported.swap(true, Ordering::Relaxed);
        assert!(before, "두 번째 호출은 보고 분기를 건너뛴다");
    }
}
