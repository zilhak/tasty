//! PTY 출력 도착 등 비동기 이벤트가 발생했을 때 메인 루프를 깨우는 메커니즘.
//!
//! Trait/Noop 정의는 `tasty_terminal::waker_factory` 로 이동. 본 모듈은 호환용
//! thin re-export 만 유지. `WinitWakerFactory` (boot/waker.rs) 가 winit
//! `EventLoopProxy` 로 production impl 을 제공한다.

pub use tasty_terminal::waker_factory::SharedWakerFactory;
#[cfg(feature = "gui")]
pub use tasty_terminal::waker_factory::WakerFactory;

use std::sync::atomic::AtomicBool;
use std::sync::{MutexGuard, PoisonError};

/// Poison 된 waker 게이트 맵을 복구한다.
///
/// 게이트 맵은 `HashMap<u32, Arc<AtomicBool>>` 하나뿐이고, 임계구역은 entry 삽입·조회·
/// 제거만 한다 — 그 안에서 패닉이 나도 맵 자체의 불변식은 깨지지 않으므로 데이터는 그대로
/// 쓸 수 있다. 반면 여기서 패닉하면 **메인 루프 스레드**가 죽어 실행 중인 모든 창의 모든
/// 터미널 세션이 사라진다. 사망 범위가 압도적으로 크므로 복구가 맞다
/// ([`docs/dev-guide/error-handling.md`](../docs/dev-guide/error-handling.md) "락 poison").
///
/// poison 은 sticky 라 한 번 걸리면 이후 **모든** 호출이 이 경로를 탄다. 게이트는 PTY 출력
/// 도착마다 만져지는 hot path 이므로 매번 로그를 내면 폭주한다 — 첫 1 회만 남긴다.
pub(crate) fn recover_gate_lock<'a, T>(
    acquired: Result<MutexGuard<'a, T>, PoisonError<MutexGuard<'a, T>>>,
    what: &'static str,
    reported: &AtomicBool,
) -> MutexGuard<'a, T> {
    crate::poison::recover_mutex(acquired, what, reported)
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod poison_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    /// 게이트 맵이 poison 돼도 **패닉하지 않고** 데이터를 그대로 돌려준다.
    ///
    /// 이 성질이 깨지면 메인 루프 스레드가 죽어 모든 창의 터미널 세션이 사라진다 —
    /// 이 lane 이 `.expect()` 를 걷어낸 이유가 그것이다.
    #[test]
    fn a_poisoned_gate_map_is_recovered_with_its_contents_intact() {
        let gates: Arc<Mutex<HashMap<u32, u8>>> = Arc::new(Mutex::new(HashMap::new()));
        gates.lock().expect("fresh mutex").insert(7, 42);

        // 락을 든 채 패닉시켜 poison 을 만든다.
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

    /// 두 번째부터는 보고하지 않는다 — poison 은 sticky 라 hot path 에서 매 호출마다
    /// 로그를 내면 폭주한다.
    #[test]
    fn repeated_recovery_reports_only_once() {
        let gates: Arc<Mutex<u8>> = Arc::new(Mutex::new(1));
        let held = Arc::clone(&gates);
        // 이 join 은 반드시 Err 다(패닉시켰으니) — 목적은 poison 을 만드는 것뿐이라
        // 결과를 쓰지 않는다. 실제 검사는 아래 `reported` 두 줄이 한다.
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().expect("fresh mutex");
            panic!("poison it");
        })
        .join();

        let reported = AtomicBool::new(false);
        drop(recover_gate_lock(gates.lock(), "test gates", &reported));
        assert!(reported.load(Ordering::Relaxed));
        // 이미 true 이므로 `swap` 이 true 를 돌려주고 로그 분기를 타지 않는다.
        let before = reported.swap(true, Ordering::Relaxed);
        assert!(before, "두 번째 호출은 보고 분기를 건너뛴다");
    }
}
