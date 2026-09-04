//! 락 poison 복구 헬퍼.
//!
//! 방침 전문은 저장소의 `docs/dev-guide/error-handling.md` "락 poison" 절이다. 요약하면
//! 판단은 두 질문으로 갈린다 — 임계구역이 불변식을 깨진 채 남길 수 있는가(→ 아니면
//! 복구), 그리고 여기서 패닉하면 무엇이 죽는가(→ 프로세스 전체면 패닉 금지).
//!
//! 본 모듈은 **첫 번째 질문의 답이 "아니오" 인 지점**(자료구조 조작만 하는 임계구역)이
//! 쓰는 복구 경로만 제공한다. 데이터를 신뢰할 수 없는 지점은 복구하면 안 되므로 여기를
//! 쓰지 않고 각자 에러를 반환한다(예: `tasty-approval` 의 `StorePoisoned`).
//!
//! ## 왜 본체가 아니라 여기 있나 (앞선 결정을 무엇이 바꿨나)
//!
//! 이 헬퍼를 처음 만든 작업은 **leaf 크레이트로 올리지 않기로 명시적으로 결정**했고
//! 근거가 둘이었다: ① 소비자가 하나뿐인 추상은 모양을 잘못 잡기 쉽다 ② 복구가 오답인
//! 크레이트(`tasty-approval` 같은)에 복구 헬퍼를 노출하면 잘못된 기본값을 심는다.
//!
//! **①은 전제가 무너졌다.** 지금 소비자는 넷이다 — 본체(`crate::poison` 재수출),
//! `tasty-host-plugin` 의 handshake 대기 맵, `tasty-telemetry` 의 탐지 창,
//! `tasty-plugin-sdk` 의 host call pending 맵. 셋이 같은 모양(자료구조 임계구역 +
//! 첫 1 회 보고)을 각자 다시 만들 자리에 있었으므로 "하나뿐" 이 더는 참이 아니다.
//!
//! **②는 그대로 살아 있다.** 다만 그 예시로 든 `tasty-approval` 은 `tasty-utils` 를
//! 의존하지 않아 헬퍼가 보이지 않는다. 대신 **복구가 오답인 지점을 가진 다른 두
//! 크레이트가 의존한다** — `tasty-plugin-sdk`(runtime 의 writer 는 패닉 유지)와
//! `tasty-cli`(attach 의 writer 는 복구하지 않는다). 그래서 우려 자체는 유효하고,
//! 사람이 조심하는 것으로 닫지 않는다: 아래 `forbidden_lock_guard` 가 **복구하면 안
//! 되는 락을 소스 스캔으로 고정**한다.
//!
//! 반대로 헬퍼를 **일부러 쓰지 않는** 크레이트도 있다(`tasty-plugin-agent-stream` ·
//! `tasty-cli`). 한 파일 안에서 지점마다 답이 갈리고 그 판단이 이미 인라인으로 적혀
//! 있으면, 그중 한 곳만 헬퍼로 바꾸는 것은 형태를 둘로 늘릴 뿐이다.
//!
//! ## 쓰는 법
//!
//! 보호 대상 타입은 `?Sized` 다 — `Mutex<dyn MemoryStorage>` 처럼 trait object 를
//! 감싼 락도 같은 헬퍼를 지나야 관측이 한 곳에 모인다.
//!
//! 여기 있는 함수는 모두 **첫 1 회만** 로그를 남긴다. poison 은 sticky 라 한 번 걸리면
//! 이후 모든 호출이 이 경로를 타는데, 여기 오는 지점은 프레임·PTY 출력 단위로 도는 hot path 가
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

/// 이 헬퍼를 **써서는 안 되는 락**을 소스 스캔으로 고정한다.
///
/// 헬퍼를 leaf 크레이트로 올리면서 생긴 위험을 닫는다: 복구가 오답인 지점을 가진
/// 크레이트도 이제 `tasty-utils` 를 의존하므로(`tasty-plugin-sdk` · `tasty-cli`),
/// 헬퍼가 그 파일들에서 **보인다.** 사람이 조심해서 막을 종류가 아니라 가드로 막는다.
///
/// **왜 크레이트 목록이 아니라 락 이름인가**: 복구가 오답인 것은 크레이트도 파일도
/// 함수도 아니라 **그 락**이다. 실제로 `tasty-plugin-sdk` 의 `HostHandle::call` 은
/// 한 함수 안에서 writer 락(에러 반환)과 pending 맵(복구)을 함께 잡는다 — 크레이트나
/// 함수 단위로 금지하면 정당한 복구까지 걸린다.
///
/// **이 테스트는 `tests/` 가 아니라 lib 유닛 테스트다** — `tests/*.rs` 는 컴파일만
/// 자동으로 검사되고 실행 채널이 수동뿐이라, 소스를 런타임에 읽는 가드에게는 그
/// 안전망이 0 이다. 관례(`tests/*_chokepoint.rs`)를 깨는 것이니 되돌리지 마라.
#[cfg(test)]
mod forbidden_lock_guard {
    use std::path::{Path, PathBuf};

    /// 이 헬퍼로 복구하면 안 되는 락 식별자와 그 이유.
    const FORBIDDEN_LOCKS: &[(&str, &str)] = &[(
        "writer",
        "임계구역이 소켓/스트림에 프레임을 쓴다. 락을 든 채 죽은 스레드는 줄을 절반만 \
         남겼을 수 있고, 그 위에 이어 쓰면 한 줄 = 한 메시지 불변식이 깨져 상대가 \
         쓰레기를 읽는다. 데이터를 신뢰할 수 없는 자리라 복구가 오답이다.",
    )];

    /// 스캔 하한. 경로가 틀리면 대상이 0 개가 되고 가드는 조용히 초록이 된다 —
    /// 그것을 막는 유일한 장치다.
    const MIN_FILES_SCANNED: usize = 200;
    const MIN_RECOVER_CALLS: usize = 30;

    fn repo_root() -> PathBuf {
        // crates/tasty-utils → 레포 루트
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crates/<name> 아래에 있어야 한다")
            .to_path_buf()
    }

    fn rust_sources(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.join("src")];
        if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
            for e in entries.flatten() {
                stack.push(e.path().join("src"));
            }
        }
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        out
    }

    /// `recover_*(` 호출과 그 인자 영역(괄호가 닫힐 때까지)을 돌려준다.
    fn recover_call_spans(lines: &[&str]) -> Vec<(usize, String)> {
        let mut spans = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let Some(pos) = [
                "recover_mutex(",
                "recover_read(",
                "recover_write(",
                "recover_try_write(",
            ]
            .iter()
            .find_map(|k| trimmed.find(k).map(|p| p + k.len())) else {
                continue;
            };
            // 괄호 균형이 맞을 때까지 이어 붙인다(호출이 여러 줄에 걸친다).
            let mut depth: i32 = 1;
            let mut text = trimmed[pos..].to_string();
            let mut j = i;
            while depth > 0 && j + 1 < lines.len() {
                for c in text.chars() {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                if depth <= 0 {
                    break;
                }
                j += 1;
                text.push(' ');
                text.push_str(lines[j].trim());
            }
            spans.push((i + 1, text));
        }
        spans
    }

    /// `<name>.lock()` / `<name>.write()` / `<name>.read()` 형태로 그 락을 잠그는가.
    fn locks_named(span: &str, name: &str) -> bool {
        [".lock()", ".write()", ".read()", ".try_write()"]
            .iter()
            .any(|verb| span.contains(&format!("{name}{verb}")))
    }

    #[test]
    fn forbidden_locks_are_never_recovered() {
        let root = repo_root();
        let files = rust_sources(&root);
        assert!(
            files.len() >= MIN_FILES_SCANNED,
            "스캔 대상이 {}개뿐이다 — 레포 루트를 잘못 잡았을 가능성이 크다(하한 {})",
            files.len(),
            MIN_FILES_SCANNED
        );

        let mut recover_calls = 0usize;
        let mut violations = Vec::new();
        for path in &files {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            // Windows 잡에서도 도는 테스트라 CRLF 를 먼저 벗긴다.
            let normalized = text.replace('\r', "");
            let lines: Vec<&str> = normalized.lines().collect();
            for (lineno, span) in recover_call_spans(&lines) {
                recover_calls += 1;
                for (name, reason) in FORBIDDEN_LOCKS {
                    if locks_named(&span, name) {
                        let rel = path.strip_prefix(&root).unwrap_or(path);
                        violations.push(format!(
                            "{}:{lineno} — `{name}` 락: {reason}",
                            rel.display()
                        ));
                    }
                }
            }
        }

        assert!(
            recover_calls >= MIN_RECOVER_CALLS,
            "recover 호출을 {recover_calls}개만 찾았다 — 스캐너가 형태를 놓쳤을 가능성이 \
             크다(하한 {MIN_RECOVER_CALLS})"
        );
        assert!(
            violations.is_empty(),
            "복구가 오답인 락을 이 헬퍼로 복구한다:\n{}",
            violations.join("\n")
        );
    }

    /// 금지 목록이 낡지 않았는가 — 각 락이 **여전히** 복구 아닌 형태로 잠기는 자리가
    /// 남아 있어야 한다. 그 자리가 사라졌다면 목록에서 빼야 하고, 그대로 두면 아무것도
    /// 지키지 않는 항목이 남는다.
    #[test]
    fn every_forbidden_lock_still_exists_in_the_tree() {
        let root = repo_root();
        let files = rust_sources(&root);
        for (name, _) in FORBIDDEN_LOCKS {
            let found = files.iter().any(|path| {
                let Ok(text) = std::fs::read_to_string(path) else {
                    return false;
                };
                text.replace('\r', "")
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .any(|l| locks_named(l, name))
            });
            assert!(
                found,
                "금지 목록의 `{name}` 락이 트리에 없다 — 목록이 낡았다"
            );
        }
    }
}
