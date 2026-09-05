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

/// 이미 손에 쥔 `PoisonError` 를 **첫 1 회 보고**하고 안쪽 값을 돌려준다.
///
/// `Condvar::wait_timeout_while` 처럼 같은 락을 이 모듈 **밖에서 다시 만나는** 경로가
/// 쓴다. 그 재획득의 `Err` 는 [`recover_mutex`] 를 거치지 않으므로, 진입할 때 아직
/// poison 이 아니었다면 그 회차는 **한 줄도 남기지 않고** 복구된다 — 대기 중에 poison 이
/// 생기는 순서가 정확히 그렇다. 조용한 복구는 조용한 유실과 구분되지 않으므로 여기서
/// 같은 첫-1 회 보고를 태운다.
pub fn recover_poisoned<T>(
    poisoned: std::sync::PoisonError<T>,
    what: &str,
    reported: &AtomicBool,
) -> T {
    report(what, reported);
    poisoned.into_inner()
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

    /// 헬퍼 밖에서 만난 poison 도 첫 1 회 보고되고 값은 보존된다.
    #[test]
    fn a_poison_met_outside_the_helper_is_still_reported_once() {
        let flag = AtomicBool::new(false);
        let m = Mutex::new(7u8);
        let err = std::sync::PoisonError::new(m.lock().expect("아직 성한 락"));
        let guard = recover_poisoned(err, "테스트 락", &flag);
        assert_eq!(*guard, 7, "복구는 지키던 값을 그대로 돌려줘야 한다");
        assert!(flag.load(Ordering::Relaxed), "첫 만남은 보고돼야 한다");
        drop(guard);

        // 두 번째부터는 조용하다 — poison 은 sticky 라 매번 찍으면 그 로그가
        // 다른 진단을 덮는다. 플래그가 이미 true 인 것으로 확인한다.
        let err2 = std::sync::PoisonError::new(m.lock().expect("성한 락"));
        let g2 = recover_poisoned(err2, "테스트 락", &flag);
        drop(g2);
        assert!(flag.load(Ordering::Relaxed));
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
/// **판정은 순수 함수로 뽑아 두었다**([`recovered_forbidden_lines`] ·
/// [`silently_skipped_lock_lines`]). 파일 순회 안에 인라인으로 두면 면제를 겨냥한
/// 변이를 합성 입력으로 찌를 수 없어, 변이가 "레포에 진짜 위반을 심는" 방식으로만
/// 가능해진다 — 느리고 트리를 더럽힌다. 아래 합성 입력 테스트가 그 변이를 영구히
/// 붙박은 것이다.
///
/// **이 테스트는 `tests/` 가 아니라 lib 유닛 테스트다** — `tests/*.rs` 는 컴파일만
/// 자동으로 검사되고 실행 채널이 수동뿐이라, 소스를 런타임에 읽는 가드에게는 그
/// 안전망이 0 이다. 관례(`tests/*_chokepoint.rs`)를 깨는 것이니 되돌리지 마라.
#[cfg(test)]
mod forbidden_lock_guard {
    use std::path::{Path, PathBuf};

    /// 이 헬퍼로 복구하면 안 되는 락 식별자와 그 이유.
    ///
    /// **명부의 술어(무엇의 명부인가): 임계구역이 프레임·레코드 경계를 갖는가.**
    /// 락이 지키는 것이 소켓/스트림에 프레임(한 줄=한 메시지)을 쓰는 자리면, 락을 든 채
    /// 죽은 스레드가 반쪽 프레임을 남길 수 있다. poison 을 `into_inner` 로 복구해 그 위에
    /// 이어 쓰면 프레이밍이 깨져 상대가 쓰레기를 읽는다 — 그래서 복구가 오답이고,
    /// skip(+보고) 또는 전파가 맞다. 자료구조·값 슬롯(경계 없음)은 이 명부의 반대편이라
    /// 복구가 옳다(그쪽은 아래 축 2 가 "조용히 삼키지 말라" 로 지킨다).
    ///
    /// 이 술어에 걸리는 피보호 타입은 [`FORBIDDEN_STREAM_TYPES`] 이고, 그 타입을 감싼
    /// 락의 바인딩 이름이 아래 목록이다 — 이름이 아니라 **타입이 근거**다.
    /// `stream_typed_lock_names_are_all_listed` 가 그 타입의 새 락(다른 이름)이 목록 밖에
    /// 생기면 실패시켜, "이름 둘짜리 명부" 가 조용히 낡는 것을 막는다.
    ///
    /// **이유를 함께 적는 것이 목록의 절반이다.** 근거 없는 금지 목록은 언젠가 통째로
    /// 지워진다. 지금 이 락이 **어느 크레이트에서 무엇을 지키고 있는지**를 함께 남긴다.
    const FORBIDDEN_LOCKS: &[(&str, &str)] = &[
        (
            "writer",
            "임계구역이 소켓/스트림에 프레임을 쓴다(`Mutex<TcpStream>`·`Mutex<HandleStream>`). \
             락을 든 채 죽은 스레드는 줄을 절반만 남겼을 수 있고, 그 위에 이어 쓰면 한 줄 = \
             한 메시지 불변식이 깨져 상대가 쓰레기를 읽는다. 데이터를 신뢰할 수 없는 자리라 \
             복구가 오답이다. 현재 이 락을 잡는 자리와 각자의 선택: \
             `tasty-plugin-sdk` 의 `runtime::send_event`/`send_response` 는 **패닉을 유지**한다 \
             (폭발 반경이 그 plugin 프로세스 하나로 한정되고, 그 범위는 방침이 패닉을 허용하는 \
             범위다). `tasty-plugin-sdk` 의 `HostHandle::notify`/`call` 은 **에러를 반환**한다 \
             (plugin 코드가 호출자라 결과를 받아 처리할 수 있다). `tasty-cli` 의 \
             `local::attach::run_raw_bridge` 는 **복구하지 않고 세션을 접되** \
             `note_writer_poisoned` 로 이유를 남긴다(원인 없이 끊긴 attach 로 보이지 않게). \
             `tasty-host-plugin` 의 `aux_reader_loop`·`with_handle_stream` 은 전송/연산을 \
             **건너뛰되 로그를 남긴다**(상대가 채널을 죽은 것으로 판정하므로 그 이유가 남아야 한다).",
        ),
        (
            "handle_writer",
            "보조 핸들 채널의 프레임 라이터(`Mutex<HandleClient>`)다. `writer` 와 같은 이유 — \
             임계구역이 스트림에 프레임을 쓰므로 반쪽 프레임 위에 이어 쓰면 프레이밍이 깨진다. \
             `tasty-plugin-sdk` 의 `shared_buffer`/`HostHandle`/`runtime` 이 이 이름으로 잡는다.",
        ),
    ];

    /// [`FORBIDDEN_LOCKS`] 의 술어에 걸리는 피보호 타입 — 프레임 I/O 스트림.
    ///
    /// 명부의 근거는 이름이 아니라 이 타입들이다. `fs::File`(줄 단위 로그, 반쪽 줄 허용)·
    /// 자료구조·값 슬롯은 경계가 없어 여기 없다.
    const FORBIDDEN_STREAM_TYPES: &[&str] = &["TcpStream", "HandleStream", "HandleClient"];

    /// 스캔 하한. 경로가 틀리면 대상이 0 개가 되고 가드는 조용히 초록이 된다.
    ///
    /// **하한만으로는 부족하다** — 스캔 대상이 1000개를 넘으므로 수백 개가 빠져도
    /// 이 값을 안 건드리고, 위반이 0 건인 파일이 빠지면 offender 목록도 안 움직여
    /// 아무 신호가 없다. 부분 누락은 아래 `the_scan_reaches_every_crate_and_both_cfg_sides`
    /// 가 집합으로 잡는다. 이 상수는 그 위의 조잡한 안전망일 뿐이다.
    const MIN_FILES_SCANNED: usize = 200;
    const MIN_RECOVER_CALLS: usize = 30;
    const MIN_LOCK_STATEMENTS: usize = 150;

    /// `recover_*(` 를 찾을 때 쓰는 needle. **조각을 붙여 만든다** — 이 파일 안에
    /// 완성된 리터럴이 나타나면 스캐너가 자기 자신을 위반으로 집는다. 파일 통째
    /// 면제(`SELF_PATH`)를 두는 대신 needle 쪽에서 없앤 것이라, 면제가 하나 줄었다.
    fn recover_needles() -> [String; 4] {
        let stem = "recover_";
        [
            format!("{stem}mutex("),
            format!("{stem}read("),
            format!("{stem}write("),
            format!("{stem}try_write("),
        ]
    }

    // ── 순수 판정기 ──────────────────────────────────────────────────────
    //
    // 파일 순회·경로 처리와 분리해 두어 합성 입력으로 찌를 수 있다.

    /// `#[cfg(test)]` 모듈 구간을 빈 줄로 지운다(줄 번호는 보존).
    ///
    /// **왜 파일이 아니라 이 단위로 면제하는가**: 락 방침은 프로덕션 경로의 것이고,
    /// 테스트는 poison 을 **일부러** 만들어 확인하는 자리라 금지 형태가 정당하게
    /// 나타난다. 이 파일 자신의 스캐너 코드와 합성 입력도 같은 이유로 여기에 걸린다 —
    /// 즉 자기 제외를 따로 두지 않아도 된다.
    fn mask_test_modules(src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut masked: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim() != "#[cfg(test)]" {
                i += 1;
                continue;
            }
            // `#[cfg(test)]` 가 붙는 항목은 `mod` 만이 아니다 — `impl` · `fn` · `use` 에도
            // 붙는다. 뒤따르는 속성 줄을 건너뛴 다음 **그 항목 하나**의 범위를 잡는다.
            // (여기서 "다음 `mod`" 를 찾으면 사이에 낀 프로덕션 코드까지 통째로 지워
            // 진짜 위반이 가려진다 — 실제로 그 형태로 변이가 살아남았다.)
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_start().starts_with("#[") {
                j += 1;
            }
            if j >= lines.len() {
                break;
            }
            let end = if lines[j].contains('{') {
                let mut depth = 0i32;
                let mut k = j;
                loop {
                    depth += lines[k].matches('{').count() as i32;
                    depth -= lines[k].matches('}').count() as i32;
                    if depth <= 0 || k + 1 >= lines.len() {
                        break k;
                    }
                    k += 1;
                }
            } else {
                // `mod tests;` · `use ...;` 처럼 한 줄로 끝나는 항목.
                j
            };
            for m in masked.iter_mut().take(end + 1).skip(i) {
                *m = String::new();
            }
            i = end + 1;
        }
        masked.join("\n")
    }

    /// `recover_*(` 호출의 **인자 영역만** 돌려준다(1-based 줄번호, 텍스트).
    ///
    /// 괄호가 닫히는 지점에서 정확히 끊는다 — 안 끊으면 뒤따르는 무관한 코드가 섞여
    /// 거짓 양성이 난다.
    fn recover_call_spans(masked: &str) -> Vec<(usize, String)> {
        let lines: Vec<&str> = masked.lines().collect();
        let needles = recover_needles();
        let mut spans = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let Some(pos) = needles
                .iter()
                .find_map(|k| trimmed.find(k.as_str()).map(|p| p + k.len()))
            else {
                continue;
            };
            let mut depth: i32 = 1;
            let mut text = String::new();
            let mut j = i;
            let mut rest = trimmed[pos..].to_string();
            'outer: loop {
                for c in rest.chars() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break 'outer;
                            }
                        }
                        _ => {}
                    }
                    text.push(c);
                }
                j += 1;
                if j >= lines.len() {
                    break;
                }
                text.push(' ');
                rest = lines[j].trim().to_string();
            }
            spans.push((i + 1, text));
        }
        spans
    }

    /// `<name>.lock()` / `.write()` / `.read()` / `.try_write()` 형태로 그 락을 잠그는가.
    /// 점 주변 공백을 걷어 `self .writer .lock()` 을 `self.writer.lock()` 로 만든다.
    ///
    /// rustfmt 는 한 줄이 100 자를 넘으면 메서드 체인을 **수신자에서** 쪼갠다
    /// (`self` / `.writer` / `.lock()`). 아래 매칭은 이름과 동사가 붙어 있어야 하므로,
    /// 쪼개진 순간 같은 코드가 가드 밖으로 나간다 — 사람의 실수가 아니라 **서식 변경만으로**
    /// 뚫린다. 문자열 리터럴 안의 `" . "` 도 함께 붙지만, 그 방향의 오탐은 가드가 더 많이
    /// 잡는 쪽이라 안전하다.
    fn tighten_dot_chains(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut pending_ws = false;
        for c in text.chars() {
            if c.is_whitespace() {
                pending_ws = true;
                continue;
            }
            if pending_ws && c != '.' && !out.ends_with('.') {
                out.push(' ');
            }
            pending_ws = false;
            out.push(c);
        }
        if pending_ws {
            out.push(' ');
        }
        out
    }

    /// 한 문(statement) 단위로 잇는다 — 축 2 가 줄 단위면 쪼개진 체인을 원리적으로 못 본다.
    ///
    /// 괄호 깊이가 0 으로 돌아오고 `;` · `{` · `}` 로 끝나면 한 문으로 본다. 줄 번호는
    /// **시작 줄**을 쓴다(신고 좌표가 위쪽을 가리켜야 사람이 찾는다).
    fn statement_spans(masked: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut buf = String::new();
        let mut start = 0usize;
        let mut depth: i32 = 0;
        let mut lexer = LineLexer::default();
        for (i, line) in masked.lines().enumerate() {
            // 리터럴·주석 내용을 공백으로 지운 뷰로 괄호를 센다 — `"\x1b[2J"` 의 `[`
            // 처럼 문자열 속 괄호가 깊이를 흔들어 문장을 오조인하는 것을 막는다.
            // 코드 토큰(`.lock()` 등)은 그대로 남아 하위 검사가 그대로 본다.
            let cleaned = lexer.clean_line(line);
            let trimmed = cleaned.trim();
            if trimmed.is_empty() {
                continue;
            }
            if buf.is_empty() {
                start = i + 1;
            } else {
                buf.push(' ');
            }
            buf.push_str(trimmed);
            for c in trimmed.chars() {
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth -= 1,
                    _ => {}
                }
            }
            let ends = trimmed.ends_with(';') || trimmed.ends_with('{') || trimmed.ends_with('}');
            if depth <= 0 && ends {
                out.push((start, std::mem::take(&mut buf)));
                depth = 0;
            }
        }
        if !buf.is_empty() {
            out.push((start, buf));
        }
        out
    }

    /// 줄을 넘어 이어지는 렉서 상태(블록 주석 · 원시 문자열). 일반 문자열·문자 리터럴은
    /// 한 줄 안에서 닫힌다고 보고(러스트에서 개행을 담으려면 원시 문자열을 쓴다) 처리한다.
    #[derive(Default)]
    struct LineLexer {
        in_block_comment: bool,
        in_raw_string: Option<usize>, // 닫는 데 필요한 `#` 개수
    }

    impl LineLexer {
        /// 리터럴·주석 내용을 공백으로 바꾼 줄을 돌려준다. 코드 구조(괄호·`.`·식별자)는
        /// 보존해 괄호 깊이 계산과 하위 부분문자열 검사가 실제 코드만 보게 한다.
        fn clean_line(&mut self, line: &str) -> String {
            let chars: Vec<char> = line.chars().collect();
            let mut out = String::with_capacity(chars.len());
            let mut i = 0;
            while i < chars.len() {
                if self.in_block_comment {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        self.in_block_comment = false;
                        out.push_str("  ");
                        i += 2;
                    } else {
                        out.push(' ');
                        i += 1;
                    }
                    continue;
                }
                if let Some(hashes) = self.in_raw_string {
                    let close: String = std::iter::once('"')
                        .chain(std::iter::repeat_n('#', hashes))
                        .collect();
                    if chars[i] == '"' && line_matches_at(&chars, i, &close) {
                        self.in_raw_string = None;
                        for _ in 0..close.len() {
                            out.push(' ');
                        }
                        i += close.len();
                    } else {
                        out.push(' ');
                        i += 1;
                    }
                    continue;
                }
                let c = chars[i];
                // 줄 주석 — 줄 끝까지 코드가 아니다.
                if c == '/' && chars.get(i + 1) == Some(&'/') {
                    break;
                }
                // 블록 주석 시작.
                if c == '/' && chars.get(i + 1) == Some(&'*') {
                    self.in_block_comment = true;
                    out.push_str("  ");
                    i += 2;
                    continue;
                }
                // 원시 문자열 시작 `r"` · `r#"` · `br#"` 등.
                if (c == 'r' || c == 'b')
                    && let Some(open) = raw_string_open(&chars, i)
                {
                    // open = 접두(`r`=1 · `br`=2) + `#`×hashes + 여는 `"`.
                    let prefix = if c == 'b' { 2 } else { 1 };
                    let hashes = open - prefix - 1;
                    self.in_raw_string = Some(hashes);
                    for _ in 0..open {
                        out.push(' ');
                    }
                    i += open;
                    continue;
                }
                // 일반 문자열.
                if c == '"' {
                    out.push(' ');
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\\' {
                            out.push_str("  ");
                            i += 2;
                            continue;
                        }
                        if chars[i] == '"' {
                            out.push(' ');
                            i += 1;
                            break;
                        }
                        out.push(' ');
                        i += 1;
                    }
                    continue;
                }
                // 문자 리터럴 대 수명(`'a`) 구분 — 닫는 `'` 가 곧 오면 문자 리터럴.
                if c == '\''
                    && let Some(len) = char_literal_len(&chars, i)
                {
                    for _ in 0..len {
                        out.push(' ');
                    }
                    i += len;
                    continue;
                }
                out.push(c);
                i += 1;
            }
            out
        }
    }

    /// `chars[i..]` 가 `needle` 로 시작하는가.
    fn line_matches_at(chars: &[char], i: usize, needle: &str) -> bool {
        needle
            .chars()
            .enumerate()
            .all(|(k, ch)| chars.get(i + k) == Some(&ch))
    }

    /// `chars[i]` 가 원시 문자열의 시작이면 여는 토큰 길이(`r"`=2, `r#"`=3, `br##"`=5…),
    /// 아니면 `None`. `i` 는 `r` 또는 `b` 위치다.
    fn raw_string_open(chars: &[char], i: usize) -> Option<usize> {
        let mut j = i;
        if chars.get(j) == Some(&'b') {
            j += 1;
        }
        if chars.get(j) != Some(&'r') {
            return None;
        }
        j += 1;
        while chars.get(j) == Some(&'#') {
            j += 1;
        }
        if chars.get(j) == Some(&'"') {
            Some(j + 1 - i)
        } else {
            None
        }
    }

    /// `chars[i] == '\''` 일 때 문자 리터럴 전체 길이(닫는 `'` 포함), 수명이면 `None`.
    fn char_literal_len(chars: &[char], i: usize) -> Option<usize> {
        // `'\x'` 형태(이스케이프) — `'` `\` .. `'`
        if chars.get(i + 1) == Some(&'\\') {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            return (chars.get(j) == Some(&'\'')).then_some(j + 1 - i);
        }
        // `'x'` — 한 글자 뒤에 닫는 따옴표.
        if chars.get(i + 2) == Some(&'\'') {
            return Some(3);
        }
        None
    }

    fn locks_named(text: &str, name: &str) -> bool {
        let tightened = tighten_dot_chains(text);
        [".lock()", ".write()", ".read()", ".try_write()"]
            .iter()
            .any(|verb| tightened.contains(&format!("{name}{verb}")))
    }

    /// 축 1 — 금지 락을 이 헬퍼로 복구하는 줄.
    fn recovered_forbidden_lines(masked: &str) -> Vec<usize> {
        recover_call_spans(masked)
            .into_iter()
            .filter(|(_, span)| FORBIDDEN_LOCKS.iter().any(|(n, _)| locks_named(span, n)))
            .map(|(line, _)| line)
            .collect()
    }

    /// std 락 획득 verb. **빈 괄호**만 센다 — `io::Read::read(buf)`/`Write::write(buf)` 는
    /// 버퍼 인자를 받으므로, 빈 괄호 `.read()`/`.write()` 는 `RwLock` 이다(타입 판별점).
    /// 트리에 parking_lot·`tokio::sync`·`.lock().await` 가 0 이라(실측) `.lock()` 은 전부 std.
    const LOCK_VERBS: &[&str] = &[
        ".lock()",
        ".read()",
        ".write()",
        ".try_lock()",
        ".try_read()",
        ".try_write()",
    ];

    /// 의도된 삼킴임을 그 자리에 밝히는 사유 마커.
    ///
    /// ★ **`check-allow-reason` 과 같은 관례가 아니다** — 한때 그렇게 적혀 있었고 그
    /// 문장이 거짓이었다. 실제 차이는 두 방향이다:
    ///
    /// ```text
    /// 셸 게이트   reason: | 이유: | complexity-exempt: | SAFETY
    /// 여기        reason: | 이유: | 사유:
    /// ```
    ///
    /// 즉 `SAFETY`·`complexity-exempt:` 를 쓴 사람은 여기서 거부당하고, `사유:` 를 쓴
    /// 사람은 셸 게이트에서 거부당한다 — **둘 다 "같은 관례" 라는 그 문장을 정확히 따른
    /// 사람이다.** 지키는 것이 없는 동일성 주장은 갈리고, 갈린 뒤에도 계속 같다고 말한다.
    ///
    /// 지금은 두 집합을 **일부러 다르게 둔다**(여기는 poison 삼킴 사유라 `SAFETY` 가
    /// 어울리지 않는다). 합칠 생각이면 셸 쪽 `REASON_PATTERN` 이 정본이고, 그때는 이
    /// 주석이 아니라 **가드**로 묶어라 — 주석은 갈림을 못 막는다.
    const REASON_MARKERS: &[&str] = &["이유:", "reason:", "사유:"];

    /// 축 2(넓힘) — **어떤** std 락이든 poison 을 조용히 지나치는 문.
    ///
    /// 첫 sweep 은 [`FORBIDDEN_LOCKS`] 만 봤다. 이 판정기는 그 밖의 락도 본다 — 조용한
    /// 삼킴은 poison 을 버리고 임계구역을 건너뛰는데 아무것도 안 깨지는 **조용한** 결함이라
    /// (R424) 명부는 시끄러운 쪽이어야 한다: 삼킴은 복구/전파거나, 의도면 그 자리에 사유.
    ///
    /// 삼킴 형태: 락 verb 직후 `.ok()` · 체인 끝 `.unwrap_or(_default)` · `if/while/&& let Ok(`
    /// (else 없음). poison 을 다루는 형태 — 복구(`into_inner`·`recover_*`)·전파(`unwrap`·
    /// `expect`·`?`·`map_err`)·`let Ok..else`·`match` — 는 삼킴이 아니다. `.ok()` 는 **락 verb
    /// 바로 뒤**만 본다(`x.lock().unwrap().foo().ok()` 를 오탐하지 않게).
    fn silently_skipped_lock_lines(masked: &str) -> Vec<usize> {
        let lines: Vec<&str> = masked.lines().collect();
        let mut hits = Vec::new();
        for (line_no, stmt) in statement_spans(masked) {
            let tight = tighten_dot_chains(&stmt);
            if !LOCK_VERBS.iter().any(|v| tight.contains(v)) {
                continue;
            }
            // poison 을 다루는 형태면 삼킴이 아니다.
            let handled = tight.contains("into_inner()")
                || tight.contains("recover_mutex(")
                || tight.contains("recover_read(")
                || tight.contains("recover_write(")
                || tight.contains("recover_poisoned(");
            if handled {
                continue;
            }
            let silent_ok = LOCK_VERBS
                .iter()
                .any(|v| tight.contains(&format!("{v}.ok()")));
            let silent_unwrap_or =
                tight.contains(".unwrap_or(") || tight.contains(".unwrap_or_default()");
            let has_let_ok = stmt.contains("if let Ok(")
                || stmt.contains("while let Ok(")
                || tight.contains("&&let Ok(");
            let silent_iflet = has_let_ok && !stmt.contains("else");
            if !(silent_ok || silent_unwrap_or || silent_iflet) {
                continue;
            }
            // 의도된 삼킴: 그 문 또는 위에 붙은 주석 블록에 사유 마커.
            if REASON_MARKERS.iter().any(|m| stmt.contains(m))
                || reason_in_attached_comment(&lines, line_no)
            {
                continue;
            }
            hits.push(line_no);
        }
        hits
    }

    /// `start_line`(1 기반) 위로 **연속된 주석 줄**에서 사유 마커를 찾는다.
    fn reason_in_attached_comment(lines: &[&str], start_line: usize) -> bool {
        let mut i = start_line; // lines[i-1] 이 시작 줄(0 기반 i-1).
        while i >= 2 {
            let above = lines[i - 2].trim_start();
            if !above.starts_with("//") {
                break;
            }
            if REASON_MARKERS.iter().any(|m| above.contains(m)) {
                return true;
            }
            i -= 1;
        }
        false
    }

    /// 축 2 가 **본** std 락 문의 수. 판정기가 죽어 아무 락도 못 보면(verb 목록이
    /// 망가지면) 위반이 0 이라 게이트가 조용히 통과한다 — 그 거짓 초록을 막는 모수다.
    fn lock_statements_seen(masked: &str) -> usize {
        statement_spans(masked)
            .into_iter()
            .filter(|(_, stmt)| {
                let tight = tighten_dot_chains(stmt);
                LOCK_VERBS.iter().any(|v| tight.contains(v))
            })
            .count()
    }

    // ── 파일 순회 ────────────────────────────────────────────────────────

    fn repo_root() -> PathBuf {
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

    /// Windows 잡에서도 도는 테스트라 CRLF 를 먼저 벗긴다.
    fn normalized(path: &Path) -> Option<String> {
        std::fs::read_to_string(path)
            .ok()
            .map(|t| mask_test_modules(&t.replace('\r', "")))
    }

    // ── 합성 입력 판정기 테스트 (면제를 겨냥한 변이가 여기 붙박여 있다) ──

    #[test]
    fn detects_a_forbidden_recovery_on_one_line_and_across_lines() {
        let one = r#"let mut w = poison::recover_mutex(self.writer.lock(), W, &P);"#;
        assert_eq!(recovered_forbidden_lines(one), vec![1]);

        let many =
            "let mut w = poison::recover_mutex(\n    self.writer.lock(),\n    W,\n    &P,\n);";
        assert_eq!(
            recovered_forbidden_lines(many),
            vec![1],
            "인자가 여러 줄에 걸쳐도 잡아야 한다"
        );
    }

    /// 면제 ①(주석 줄)을 겨냥한 변이 — 면제 창 **안쪽**과 **바깥쪽**을 함께 고정한다.
    #[test]
    fn the_comment_exemption_does_not_swallow_real_code() {
        let commented = "// poison::recover_mutex(self.writer.lock(), W, &P);";
        assert!(
            recovered_forbidden_lines(commented).is_empty(),
            "주석은 코드가 아니다(의도된 false negative)"
        );

        // 주석처럼 **보이지만** 코드인 줄은 잡혀야 한다 — 면제가 줄 단위라 여기서 갈린다.
        let looks_commented =
            r#"let s = "// nope"; let w = poison::recover_mutex(self.writer.lock(), W, &P);"#;
        assert_eq!(
            recovered_forbidden_lines(looks_commented),
            vec![1],
            "면제는 '`//` 로 시작하는 줄' 이지 '`//` 를 포함한 줄' 이 아니다"
        );
    }

    /// 면제 ②(`#[cfg(test)]` 모듈)를 겨냥한 변이.
    #[test]
    fn the_test_module_exemption_covers_only_the_test_module() {
        let src = "fn prod() {\n    let w = poison::recover_mutex(self.writer.lock(), W, &P);\n}\n\
                   #[cfg(test)]\nmod t {\n    fn helper() {\n        \
                   let w = poison::recover_mutex(self.writer.lock(), W, &P);\n    }\n}\n";
        let masked = mask_test_modules(src);
        assert_eq!(
            recovered_forbidden_lines(&masked),
            vec![2],
            "프로덕션 줄만 잡고 테스트 모듈 안은 면제한다"
        );
    }

    /// 면제 ②의 **범위**를 겨냥한 변이 — `#[cfg(test)]` 는 `mod` 에만 붙지 않는다.
    ///
    /// 이 케이스가 실제로 위반 하나를 가렸다: 판정기가 "`#[cfg(test)]` 다음의 `mod`"
    /// 를 찾는 바람에 `#[cfg(test)] impl` 과 한참 뒤의 `mod tests` **사이의 프로덕션
    /// 코드 전체**를 마스킹했고, 그 안의 진짜 위반이 사라져 변이가 살아남았다.
    #[test]
    fn the_test_module_exemption_does_not_swallow_code_between_items() {
        let src = "#[cfg(test)]\nimpl Foo {\n    fn helper() {}\n}\n\
                   fn prod() {\n    if let Ok(mut w) = writer.lock() {\n        w.send();\n    }\n}\n\
                   #[cfg(test)]\nmod tests {\n    fn t() {\n        \
                   if let Ok(mut w) = writer.lock() { w.send(); }\n    }\n}\n";
        let masked = mask_test_modules(src);
        assert_eq!(
            silently_skipped_lock_lines(&masked),
            vec![6],
            "테스트 항목 둘 사이에 낀 프로덕션 줄은 살아 있어야 한다"
        );
    }

    /// 세미콜론으로 끝나는 테스트 항목(`#[cfg(test)] mod tests;`)도 한 줄만 먹는다.
    #[test]
    fn a_semicolon_test_item_masks_only_its_own_line() {
        let src = "#[cfg(test)]\nmod tests;\n\
                   fn prod() {\n    if let Ok(mut w) = writer.lock() { w.send(); }\n}\n";
        let masked = mask_test_modules(src);
        assert_eq!(silently_skipped_lock_lines(&masked), vec![4]);
    }

    #[test]
    fn detects_and_spares_the_three_forms_of_silent_skip() {
        assert_eq!(
            silently_skipped_lock_lines("if let Ok(mut w) = writer.lock() {"),
            vec![1]
        );
        assert_eq!(
            silently_skipped_lock_lines("let tx = writer.lock().ok().and_then(|w| w.take());"),
            vec![1]
        );
        // 허용 형태 셋 — 못 잡는 것이 의도다. 나중에 판정기를 넓히면 여기서 드러난다.
        for allowed in [
            r#"let mut w = writer.lock().expect("writer lock");"#,
            r#"let mut w = writer.lock().map_err(|_| Poisoned)?;"#,
            "match writer.lock() {",
            "let Ok(mut w) = writer.lock() else { return; };",
        ] {
            assert!(
                silently_skipped_lock_lines(allowed).is_empty(),
                "poison 을 다루는 형태다: {allowed}"
            );
        }
    }

    /// 금지 목록에 없는 락은 두 축 모두 건드리지 않는다(의도된 false negative).
    /// rustfmt 가 체인을 **수신자에서** 쪼개도 두 축이 그대로 본다.
    ///
    /// 여섯 입력을 한 실행에 넣는다 — 셋은 사각이었던 형태(D·C·F), 셋은 대조군(A·E·B)이다.
    /// 대조군이 없으면 "추출이 통째로 망가져 전부 빈 것" 과 구분되지 않는다.
    #[test]
    fn a_receiver_split_across_lines_is_still_seen() {
        // A — 한 줄 복구 (대조군, 축 1)
        let a = "fn p() {\n    let w = recover_mutex(writer.lock(), W, &R);\n}\n";
        // E — 인자만 쪼갬 (대조군, 축 1)
        let e = "fn p() {\n    let w = recover_mutex(\n        writer.lock(),\n        W,\n        &R,\n    );\n}\n";
        // D — 수신자 쪼갬 (사각이었다, 축 1)
        let d = "fn p() {\n    let w = recover_mutex(\n        self\n            .writer\n            .lock(),\n        W,\n        &R,\n    );\n}\n";
        // B — 한 줄 무음 지나침 (대조군, 축 2)
        let b = "fn p() {\n    if let Ok(mut w) = writer.lock() {\n        w.send();\n    }\n}\n";
        // C — 수신자 쪼갠 무음 지나침 (사각이었다, 축 2)
        let c = "fn p() {\n    if let Ok(mut w) = self\n        .writer\n        .lock()\n    {\n        w.send();\n    }\n}\n";
        // F — 수신자 쪼갠 `.ok()` (사각이었다, 축 2)
        let f =
            "fn p() {\n    let w = self\n        .writer\n        .lock()\n        .ok()?;\n}\n";

        assert!(
            !recovered_forbidden_lines(a).is_empty(),
            "A 대조군이 안 발화하면 이 회차의 다른 0 은 아무것도 뜻하지 않는다"
        );
        assert!(!recovered_forbidden_lines(e).is_empty(), "E 대조군");
        assert!(
            !recovered_forbidden_lines(d).is_empty(),
            "D: 수신자가 쪼개져도 축 1 이 봐야 한다"
        );
        assert!(
            !silently_skipped_lock_lines(b).is_empty(),
            "B 대조군이 안 발화하면 아래 둘의 판정이 성립하지 않는다"
        );
        assert!(
            !silently_skipped_lock_lines(c).is_empty(),
            "C: 쪼개진 `if let Ok(` 도 축 2 가 봐야 한다"
        );
        assert!(
            !silently_skipped_lock_lines(f).is_empty(),
            "F: 쪼개진 `.ok()` 도 축 2 가 봐야 한다"
        );
    }

    /// 두 축의 범위가 다르다. 축 1(복구 금지)은 **명부에 있는 락만** 본다 — 명부 밖
    /// 락의 복구는 정당하다. 축 2(무음 지나침)는 **명부와 무관하게** 어떤 std 락이든
    /// 본다: 조용한 삼킴은 그 자체가 결함이라 시끄러운 쪽이어야 한다(R424). `pending`
    /// 은 명부 밖이지만 poison 을 아무 말 없이 버리므로 축 2 는 잡는다.
    #[test]
    fn axis1_is_list_scoped_but_axis2_sees_every_lock() {
        let recovered = "let g = poison::recover_mutex(self.pending.lock(), W, &P);";
        assert!(recovered_forbidden_lines(recovered).is_empty());
        assert!(!silently_skipped_lock_lines("if let Ok(mut p) = pending.lock() {").is_empty());
    }

    /// 사유 마커가 붙으면 의도된 삼킴이라 축 2 가 면제한다 — 문 안에서든, 바로 위
    /// 주석 블록에서든. 마커가 없으면 같은 삼킴이 걸린다(대조군).
    #[test]
    fn a_reasoned_silent_skip_is_spared() {
        // 위 주석에 사유.
        let above = "    // 이유: 종료 경로라 poison 이면 그냥 버린다\n    \
                     let _ = pending.lock().ok();\n";
        assert!(
            silently_skipped_lock_lines(above).is_empty(),
            "위 주석의 사유가 면제해야 한다"
        );
        // 같은 삼킴, 사유 없음 — 걸린다.
        assert!(
            !silently_skipped_lock_lines("    let _ = pending.lock().ok();\n").is_empty(),
            "사유 없는 대조군은 걸려야 한다"
        );
    }

    /// 빈 괄호 `.read()`/`.write()` 는 `RwLock` 이라 축 2 가 본다. io `Read::read(buf)`/
    /// `Write::write(buf)` 는 버퍼 인자가 있어 락이 아니다 — 삼켜도 걸리지 않는다.
    #[test]
    fn empty_parens_read_write_is_a_lock_but_buffered_io_is_not() {
        assert!(
            !silently_skipped_lock_lines("if let Ok(g) = cfg.read() { use_it(&g); }").is_empty(),
            "빈 괄호 .read() 는 RwLock"
        );
        assert!(
            silently_skipped_lock_lines("if let Ok(n) = sock.read(&mut buf) { emit(n); }")
                .is_empty(),
            "버퍼 인자를 받는 io read 는 락이 아니다"
        );
    }

    // ── 트리 스캔 테스트 ─────────────────────────────────────────────────

    /// 스캔 **모집단**을 집합으로 못박는다.
    ///
    /// 기대 집합을 스캐너와 **다른 순회 방법**으로 만든다 — `rust_sources` 는 재귀
    /// 스택 순회인데 여기서는 `crates/` 를 한 겹만 나열한다. 같은 방법으로 기대값을
    /// 만들면 스캐너의 버그가 기대값에도 그대로 들어가 항상 통과한다.
    ///
    /// ②의 앵커 둘은 **cfg 양방향**을 하나씩 집는다. 이 가드는 컴파일된 심볼이 아니라
    /// 디스크의 텍스트를 읽으므로 `--no-default-features` 조합에서도 같은 파일을 봐야
    /// 하는데, 그 사실이 하한으로는 안 드러난다(빠져도 하한 위다). 앵커가 그것을
    /// 조합마다 직접 증명한다.
    #[test]
    fn the_scan_reaches_every_crate_and_both_cfg_sides() {
        let root = repo_root();
        let scanned: std::collections::BTreeSet<PathBuf> =
            rust_sources(&root).into_iter().collect();

        let mut missing = Vec::new();
        for entry in std::fs::read_dir(root.join("crates"))
            .expect("crates/ 가 있어야 한다")
            .flatten()
        {
            let src = entry.path().join("src");
            if !src.is_dir() {
                continue;
            }
            if !scanned.iter().any(|p| p.starts_with(&src)) {
                missing.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "스캔이 이 크레이트들의 src 를 하나도 안 읽었다: {}",
            missing.join(", ")
        );

        for anchor in [
            // `src/adapters/mod.rs` 의 `#[cfg(feature = "gui")] pub mod ui;` 안쪽.
            "src/adapters/ui/popup/remote_attach.rs",
            // `src/adapters/production/mod.rs` 의 `#[cfg(not(feature = "gui"))]` 안쪽.
            "src/adapters/production/headless_waker.rs",
        ] {
            let path = root.join(anchor);
            assert!(
                path.is_file(),
                "앵커 파일이 옮겨졌다: {anchor} — 같은 cfg 쪽의 다른 파일로 갱신해라"
            );
            assert!(
                scanned.contains(&path),
                "cfg 로 갈리는 자리를 스캔이 놓쳤다: {anchor}"
            );
        }
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
            let Some(masked) = normalized(path) else {
                continue;
            };
            recover_calls += recover_call_spans(&masked).len();
            for line in recovered_forbidden_lines(&masked) {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                violations.push(format!("{}:{line}", rel.display()));
            }
        }

        assert!(
            recover_calls >= MIN_RECOVER_CALLS,
            "recover 호출을 {recover_calls}개만 찾았다 — 스캐너가 형태를 놓쳤을 가능성이 \
             크다(하한 {MIN_RECOVER_CALLS})"
        );
        assert!(
            violations.is_empty(),
            "복구가 오답인 락을 이 헬퍼로 복구한다 — 이유는 `FORBIDDEN_LOCKS` 에 있다:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn no_lock_is_silently_skipped_without_a_reason() {
        let root = repo_root();
        let files = rust_sources(&root);
        assert!(files.len() >= MIN_FILES_SCANNED, "스캔 대상이 너무 적다");

        let mut violations = Vec::new();
        let mut seen = 0usize;
        for path in &files {
            let Some(masked) = normalized(path) else {
                continue;
            };
            seen += lock_statements_seen(&masked);
            for line in silently_skipped_lock_lines(&masked) {
                let rel = path.strip_prefix(&root).unwrap_or(path);
                violations.push(format!("{}:{line}", rel.display()));
            }
        }
        assert!(
            seen >= MIN_LOCK_STATEMENTS,
            "본 락 문이 너무 적다({seen}) — verb 판정이 죽었을 수 있다(하한 {MIN_LOCK_STATEMENTS})"
        );
        assert!(
            violations.is_empty(),
            "std 락 poison 을 무음으로 지나친다 — 복구(into_inner·recover_*)·전파(expect·\
             map_err·?·match)로 바꾸거나, 의도된 삼킴이면 그 자리에 사유(`이유:`/`reason:`)를 \
             남겨라:\n{}",
            violations.join("\n")
        );
    }

    /// 금지 목록이 낡지 않았는가 — 각 락이 **여전히** 트리에 있어야 한다. 사라졌다면
    /// 목록에서 빼야 하고, 그대로 두면 아무것도 지키지 않는 항목이 남는다.
    #[test]
    fn every_forbidden_lock_still_exists_in_the_tree() {
        let root = repo_root();
        let files = rust_sources(&root);
        for (name, _) in FORBIDDEN_LOCKS {
            let found = files.iter().any(|path| {
                normalized(path).is_some_and(|masked| {
                    // 문 단위로 잇는다 — `self\n.handle_writer\n.lock()` 처럼 수신자가
                    // 쪼개진 락은 줄 단위로는 안 보인다.
                    statement_spans(&masked)
                        .iter()
                        .any(|(_, stmt)| locks_named(stmt, name))
                })
            });
            assert!(
                found,
                "금지 목록의 `{name}` 락이 트리에 없다 — 목록이 낡았다"
            );
        }
    }

    /// 명부의 술어를 **타입으로** 못박는다 — 프레임 스트림 타입([`FORBIDDEN_STREAM_TYPES`])을
    /// 감싼 락이 새로 생기면, 그 이름이 무엇이든 [`FORBIDDEN_LOCKS`] 에 있어야 한다.
    ///
    /// 이름 목록만 두면 "셋째(다음 스트림 락)는 안 걸린다"(R426). 이 테스트가 선언을
    /// 타입으로 훑어, 명부에 없는 이름이 프레임 타입을 감싸는 순간 실패한다 — 명부가
    /// 조용히 낡는 것을 막는다. 익명 자리(enum variant 등 `이름:` 없는 선언)는 바인딩
    /// 이름이 없어 사용처(`writer`/skip-report)로 덮이므로 여기서 세지 않는다.
    #[test]
    fn stream_typed_lock_names_are_all_listed() {
        let root = repo_root();
        let files = rust_sources(&root);
        let listed: std::collections::BTreeSet<&str> =
            FORBIDDEN_LOCKS.iter().map(|(n, _)| *n).collect();

        let mut found_any = false;
        let mut missing = std::collections::BTreeSet::new();
        for path in &files {
            let Some(masked) = normalized(path) else {
                continue;
            };
            for line in masked.lines() {
                let t = line.trim_start();
                if t.starts_with("//") {
                    continue;
                }
                let Some(at) = FORBIDDEN_STREAM_TYPES
                    .iter()
                    .find_map(|ty| line.find(&format!("Mutex<{ty}>")))
                else {
                    continue;
                };
                // 바인딩 이름은 `이름: <…Mutex<stream>…>` 형태(필드·파라미터)의 그 콜론
                // **앞** 식별자다. `Mutex<stream>` 앞부분에서 바인딩 콜론(경로 `::` 아닌
                // 단일 `:`)을 찾는다 — 반환 타입(`-> Result<…Mutex<…>, …mpsc::…>`)에는
                // 앞쪽에 바인딩 콜론이 없어 익명으로 걸러진다.
                let prefix = &line[..at];
                let bytes = prefix.as_bytes();
                let mut colon = None;
                for i in 0..bytes.len() {
                    if bytes[i] == b':'
                        && (i == 0 || bytes[i - 1] != b':')
                        && (i + 1 >= bytes.len() || bytes[i + 1] != b':')
                    {
                        colon = Some(i);
                    }
                }
                let Some(ci) = colon else {
                    continue;
                };
                let name: String = prefix[..ci]
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                if name.is_empty() {
                    continue;
                }
                found_any = true;
                if !listed.contains(name.as_str()) {
                    let rel = path.strip_prefix(&root).unwrap_or(path);
                    missing.insert(format!("{}: {name}", rel.display()));
                }
            }
        }
        assert!(
            found_any,
            "프레임 스트림 타입을 감싼 락 선언을 하나도 못 찾았다 — 스캔이 죽었거나 \
             FORBIDDEN_STREAM_TYPES 의 타입명이 트리와 어긋났다"
        );
        assert!(
            missing.is_empty(),
            "프레임 스트림 타입({:?})을 감싼 락인데 FORBIDDEN_LOCKS 에 이름이 없다 — \
             복구가 오답인 자리가 명부 밖에 생겼다. 이름을 명부에 넣어라:\n{}",
            FORBIDDEN_STREAM_TYPES,
            missing.into_iter().collect::<Vec<_>>().join("\n")
        );
    }
}
