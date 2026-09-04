//! 락 poison 복구 헬퍼.
//!
//! 방침 전문은 저장소의 `docs/dev-guide/error-handling.md` "락 poison" 절이다. 요약하면 판단은 두 질문으로 갈린다 — 임계구역이 불변식을
//! 깨진 채 남길 수 있는가(→ 아니면 복구), 그리고 여기서 패닉하면 무엇이 죽는가
//! (→ 프로세스 전체면 패닉 금지).
//!
//! 본 모듈은 **첫 번째 질문의 답이 "아니오" 인 지점**(자료구조 조작만 하는 임계구역)이
//! 쓰는 복구 경로만 제공한다. 데이터를 신뢰할 수 없는 지점은 복구하면 안 되므로 여기를
//! 쓰지 않고 각자 에러를 반환한다(예: `tasty-approval` 의 `StorePoisoned`).
//!
//! 본체 크레이트에만 있던 것을 여기로 올린 이유는 **두 번째·세 번째 소비자가 생겼기
//! 때문**이다 — `tasty-host-plugin` 의 handshake 대기 맵과 `tasty-telemetry` 의 탐지
//! 창이 같은 모양(자료구조 임계구역 + 첫 1 회 보고)을 각자 다시 만들 자리에 있었다.
//! 소비자가 하나뿐일 때 미리 올리지 않은 것은 모양을 잘못 잡기 쉬워서였다.
//!
//! 반대로 `tasty-plugin-agent-stream` 은 **일부러 여기를 쓰지 않는다**: 그 크레이트의
//! 락 지점들은 지점마다 답이 다르고(에러 반환 · 복구 후 기록 · 포기), 이미 인라인으로
//! 그 판단을 각각 적어 두었다. 남은 한 곳만 헬퍼로 바꾸면 한 파일에 형태가 둘이 된다.
//!
//! 보호 대상 타입은 `?Sized` 다 — `Mutex<dyn MemoryStorage>` 처럼 trait object 를
//! 감싼 락도 같은 헬퍼를 지나야 관측이 한 곳에 모인다.
//!
//! 여기 있는 함수는 모두 **첫 1 회만** 로그를 남긴다. poison 은 sticky 라 한 번 걸리면 이후 모든
//! 호출이 이 경로를 타는데, 여기 오는 지점은 프레임·PTY 출력 단위로 도는 hot path 가
//! 많아 매번 남기면 정작 그 로그를 묻어 버린다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    LockResult, MutexGuard, PoisonError, RwLockReadGuard, RwLockWriteGuard, TryLockError,
};

fn report(what: &str, reported: &AtomicBool) {
    if !reported.swap(true, Ordering::Relaxed) {
        tracing::error!(
            "{what} lock poisoned — a thread panicked while holding it. Recovering (the guarded \
             data keeps its invariants); later occurrences are not logged."
        );
    }
}

/// Poison 된 `Mutex` 를 복구해 guard 를 돌려준다.
pub fn recover_mutex<'a, T: ?Sized>(
    acquired: LockResult<MutexGuard<'a, T>>,
    what: &str,
    reported: &AtomicBool,
) -> MutexGuard<'a, T> {
    acquired.unwrap_or_else(|poisoned: PoisonError<MutexGuard<'a, T>>| {
        report(what, reported);
        poisoned.into_inner()
    })
}

/// Poison 된 `RwLock` 의 read guard 를 복구한다.
pub fn recover_read<'a, T: ?Sized>(
    acquired: LockResult<RwLockReadGuard<'a, T>>,
    what: &str,
    reported: &AtomicBool,
) -> RwLockReadGuard<'a, T> {
    acquired.unwrap_or_else(|poisoned| {
        report(what, reported);
        poisoned.into_inner()
    })
}

/// Poison 된 `RwLock` 의 write guard 를 복구한다.
pub fn recover_write<'a, T: ?Sized>(
    acquired: LockResult<RwLockWriteGuard<'a, T>>,
    what: &str,
    reported: &AtomicBool,
) -> RwLockWriteGuard<'a, T> {
    acquired.unwrap_or_else(|poisoned| {
        report(what, reported);
        poisoned.into_inner()
    })
}

/// `try_write` 처럼 "지금 못 잡음" 과 "poison" 을 함께 돌려주는 자리용.
/// poison 은 복구하고, 경합(`WouldBlock`)은 `None` 으로 넘긴다.
#[allow(dead_code)]
pub fn recover_try_write<'a, T: ?Sized>(
    attempted: Result<RwLockWriteGuard<'a, T>, TryLockError<RwLockWriteGuard<'a, T>>>,
    what: &str,
    reported: &AtomicBool,
) -> Option<RwLockWriteGuard<'a, T>> {
    match attempted {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(poisoned)) => {
            report(what, reported);
            Some(poisoned.into_inner())
        }
        Err(TryLockError::WouldBlock) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, RwLock};

    fn poison_mutex<T: Send + Sync + 'static>(m: &Arc<Mutex<T>>) {
        let held = Arc::clone(m);
        // 패닉시키는 것이 목적 — join 결과는 아래 assert 로 확인한다.
        let joined = std::thread::spawn(move || {
            let _guard = held.lock().expect("fresh mutex");
            panic!("poison the mutex");
        })
        .join();
        assert!(joined.is_err());
    }

    fn poison_rwlock<T: Send + Sync + 'static>(l: &Arc<RwLock<T>>) {
        let held = Arc::clone(l);
        let joined = std::thread::spawn(move || {
            let _guard = held.write().expect("fresh rwlock");
            panic!("poison the rwlock");
        })
        .join();
        assert!(joined.is_err());
    }

    #[test]
    fn recovery_keeps_the_guarded_value_and_reports_once() {
        let m = Arc::new(Mutex::new(7u8));
        poison_mutex(&m);
        let reported = AtomicBool::new(false);
        assert_eq!(*recover_mutex(m.lock(), "test mutex", &reported), 7);
        assert!(reported.load(Ordering::Relaxed), "첫 회는 보고한다");
        // 두 번째부터는 보고 분기를 건너뛴다 — `swap` 이 이미 true 를 돌려준다.
        assert!(reported.swap(true, Ordering::Relaxed));
    }

    #[test]
    fn both_rwlock_directions_recover() {
        let l = Arc::new(RwLock::new(vec![1u8, 2]));
        poison_rwlock(&l);
        let reported = AtomicBool::new(false);
        assert_eq!(recover_read(l.read(), "test rwlock", &reported).len(), 2);
        recover_write(l.write(), "test rwlock", &reported).push(3);
        assert_eq!(recover_read(l.read(), "test rwlock", &reported).len(), 3);
    }

    #[test]
    fn try_write_separates_contention_from_poison() {
        let l = Arc::new(RwLock::new(0u8));
        poison_rwlock(&l);
        let reported = AtomicBool::new(false);
        assert!(
            recover_try_write(l.try_write(), "test rwlock", &reported).is_some(),
            "poison 은 복구한다"
        );
        let _held = recover_write(l.write(), "test rwlock", &reported);
        assert!(
            recover_try_write(l.try_write(), "test rwlock", &reported).is_none(),
            "경합은 None 이다"
        );
    }
}
