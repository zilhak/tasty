//! 락 poison을 복구하고 지정된 플래그마다 처음 한 번 보고하는 헬퍼.
//! 보호한 데이터의 불변식이 유지되는 곳에서만 사용한다. 부분 프레임이 남을 수 있는
//! 소켓 writer나 신뢰할 수 없는 승인 상태에는 적용하지 않는다.
//! 각 호출자는 복구·오류 전파·보고 후 건너뛰기 중 적절한 처리를 선택해야 한다.
//! 자세한 규칙은 docs/dev-guide/error-handling.md의 락 poison 절을 따른다.
//!
//! 여러 크레이트가 공유하며 ?Sized도 지원한다. 아래 소스 검사는 복구 금지 스트림과
//! 사유 없이 건너뛴 락 실패를 검사한다. 타입 검사기를 대신하는 것은 아니다.

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

/// 이미 받은 PoisonError를 복구하고 처음 한 번 보고한다.
/// Condvar 대기 중 poison이 생기면 최초 lock 검사에서 발견하지 못하므로 재획득도 처리해야 한다.
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

/// 복구 금지 스트림과 사유 없는 락 실패 생략을 소스에서 검사한다.
/// 한 함수가 writer와 pending 맵을 함께 잡을 수 있어 크레이트 전체를 금지하지 않는다.
/// 파일 순회와 판정을 분리해 같은 함수에 합성 입력도 전달한다.
/// lib 단위 시험에 두어 lib를 실행하는 CI에서도 실제 소스 검사가 수행되게 한다.
#[cfg(test)]
mod forbidden_lock_guard {
    use std::path::{Path, PathBuf};

    /// 부분 프레임 뒤에 이어 쓰면 안 되는 락 이름과 처리 이유.
    /// 타입 목록으로 새 스트림 락 이름의 누락도 검사한다. 파일 로그의 부분 줄 허용과
    /// 자료구조 복구는 별도이며, 모든 파일·값 슬롯에 복구가 안전하다는 뜻은 아니다.
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

    /// 프레임 스트림 타입. fs::File 로그와 자료구조는 이 금지 목록에 포함하지 않는다.
    const FORBIDDEN_STREAM_TYPES: &[&str] = &["TcpStream", "HandleStream", "HandleClient"];

    /// 잘못된 루트나 빈 스캔을 잡는 최소 파일 수. 부분 누락은 아래 크레이트·cfg 집합 검사로 확인한다.
    const MIN_FILES_SCANNED: usize = 200;
    const MIN_RECOVER_CALLS: usize = 30;
    const MIN_LOCK_STATEMENTS: usize = 150;

    /// 검사 코드 자체가 실제 호출로 잡히지 않도록 검색 문자열을 나눠 만든다.
    fn recover_needles() -> [String; 4] {
        let stem = "recover_";
        [
            format!("{stem}mutex("),
            format!("{stem}read("),
            format!("{stem}write("),
            format!("{stem}try_write("),
        ]
    }

    /// cfg(test) 항목을 빈 줄로 바꾸되 원래 줄 번호를 유지한다.
    /// 시험에서 의도적으로 만드는 poison 경로와 합성 입력은 제품 코드 검사에서 제외한다.
    fn mask_test_modules(src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut masked: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim() != "#[cfg(test)]" {
                i += 1;
                continue;
            }
            // 다음 mod까지 지우지 않고 cfg(test)가 붙은 항목 하나만 제거한다.
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

    /// 락 호출의 점 앞뒤 공백을 없애 여러 줄로 쓴 메서드 체인도 찾는다.
    /// 텍스트 기반 처리이며 Rust의 실제 수신자 타입을 확인하지는 않는다.
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

    /// 괄호 깊이가 0이고 ;/{/}로 끝나는 줄까지 한 문으로 모은다. 시작 줄 번호를 보관한다.
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

    /// 블록 주석과 raw 문자열 상태만 줄 사이에 보관한다.
    /// 여러 줄의 일반 문자열은 처리하지 못하는 스캐너 한계다.
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

    /// 인자가 없는 락 메서드 표기. 버퍼 인자를 받는 IO read/write와 구분한다.
    /// 같은 이름의 자체 헬퍼가 guard를 직접 반환할 수도 있으므로 실제 타입의 증거는 아니다.
    const LOCK_VERBS: &[&str] = &[
        ".lock()",
        ".read()",
        ".write()",
        ".try_lock()",
        ".try_read()",
        ".try_write()",
    ];

    /// 의도적으로 생략한 실패 처리의 사유 마커. 이 검사는 마커 존재만 확인한다.
    /// check-allow-reason과 다르다: 여기서는 사유:를 허용하고 SAFETY:/complexity-exempt:는
    /// 허용하지 않으며, 셸 검사는 마커 뒤 설명까지 요구한다.
    const REASON_MARKERS: &[&str] = &["이유:", "reason:", "사유:"];

    /// 락 결과를 유지한다고 간주하는 메서드 체인 단계.
    /// 이 단계만 거쳐 unwrap_or/unwrap_or_default에 도달하면 실패를 생략한 것으로 본다.
    /// map 뒤의 unwrap_or_default도 검사하지만, unwrap으로 guard를 얻은 뒤의 Option 처리는 제외한다.
    ///
    /// 문법만 보는 검사이므로 guard를 직접 반환하는 자체 lock 헬퍼 뒤에 as_ref/map이
    /// 붙으면 오탐할 수 있다. 지원하지 않는 메서드 체인은 놓칠 수 있으며 타입 추론은 하지 않는다.
    /// 합성 입력은 실제로 컴파일 가능한 락 실패 생략과 일반 값 처리를 함께 대조한다.
    const RESULT_PRESERVING_STEPS: &[&str] = &[
        ".map(",
        ".map_err(",
        ".and_then(",
        ".or_else(",
        ".inspect(",
        ".inspect_err(",
        ".as_ref()",
        ".as_mut()",
        ".cloned()",
        ".copied()",
    ];

    /// `tight[open]` 이 `(` 일 때 짝이 되는 `)` **다음** 바이트 위치. 입력은 마스킹된
    /// 텍스트라 문자열·문자 리터럴 안의 괄호는 이미 중화돼 있다.
    fn after_balanced_paren(tight: &str, open: usize) -> Option<usize> {
        let mut depth = 0i32;
        for (i, c) in tight[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + i + 1);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// 락 호출 뒤 허용된 체인만 거쳐 unwrap_or/unwrap_or_default에 도달하는지 검사한다.
    /// unwrap_or_else는 이 검사에 포함하지 않는다. 복구 금지 락은 별도 검사도 적용한다.
    fn unwrap_or_rides_the_lock_result(tight: &str) -> bool {
        for verb in LOCK_VERBS {
            let mut search = 0usize;
            while let Some(rel) = tight[search..].find(verb) {
                let mut i = search + rel + verb.len();
                loop {
                    let rest = &tight[i..];
                    if rest.starts_with(".unwrap_or(") || rest.starts_with(".unwrap_or_default()") {
                        return true;
                    }
                    let Some(step) = RESULT_PRESERVING_STEPS
                        .iter()
                        .find(|s| rest.starts_with(**s))
                    else {
                        break;
                    };
                    i = if step.ends_with("()") {
                        i + step.len()
                    } else {
                        // `.map(` 류 — 닫는 괄호까지 건너뛴다(중첩 클로저 포함).
                        match after_balanced_paren(tight, i + step.len() - 1) {
                            Some(next) => next,
                            None => break,
                        }
                    };
                }
                search += rel + verb.len();
            }
        }
        false
    }

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
            let silent_unwrap_or = unwrap_or_rides_the_lock_result(&tight);
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

    /// 스캐너가 찾은 락 문 수. 인식 실패로 빈 위반 목록이 나와 통과하는 일을 막는다.
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

    /// 주석 행만 제외하고 실제 코드 행은 검사하는지 확인한다.
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

    /// cfg(test) 항목 사이에 있는 제품 코드를 잘못 지우지 않는지 확인한다.
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

    /// 실제 락 결과의 실패 생략 네 경우와 일반 값 처리 두 경우를 대조한다.
    /// MutexGuard에는 Default가 없으므로 lock().unwrap_or_default() 자체는 양성 예제로 쓰지 않는다.
    #[test]
    fn unwrap_or_is_judged_by_what_it_unwraps_not_by_distance() {
        // 잡아야 한다 — `.unwrap_or*` 가 아직 `LockResult` 에 걸려 있다.
        for swallow in [
            // 락 verb 바로 뒤. guard 값을 다른 락에서 가져오는 형태라 실제로 컴파일된다.
            "let g = a.lock().unwrap_or(b.lock().unwrap());",
            // `.map(` 은 Result 를 보존한다 — 바로 뒤가 아니어도 삼키는 것은 poison 이다.
            "let n = m.lock().map(|g| g.len()).unwrap_or_default();",
            "let n = cfg.read().map(|g| g.len()).unwrap_or(0);",
            "let n = m.try_lock().map(|g| g.len()).unwrap_or_default();",
        ] {
            assert!(
                !silently_skipped_lock_lines(swallow).is_empty(),
                "락 결과를 삼키는데 안 걸렸다: {swallow}"
            );
        }

        // 봐줘야 한다 — `.unwrap_or*` 가 걸린 값이 이미 락 결과가 아니다.
        for spared in [
            // 자체 lock 헬퍼 뒤 HashMap 조회의 Option을 처리하는 경우.
            "let v = self.lock().get(kind).cloned().unwrap_or_default();",
            // 진짜 std 락이지만 poison 은 `unwrap()` 이 패닉으로 전파한다.
            "let n = m.lock().unwrap().get(k).copied().unwrap_or_default();",
        ] {
            assert!(
                silently_skipped_lock_lines(spared).is_empty(),
                "락 결과가 아닌 값의 `.unwrap_or*` 인데 걸렸다: {spared}"
            );
        }
    }

    /// 한 줄·인자 줄바꿈·수신자 줄바꿈 모두 같은 위반을 찾는지 대조한다.
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

    /// 복구 금지는 목록의 락만 검사한다. 실패를 조용히 생략하는 형태는 목록 밖 락도 검사한다.
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

    /// 재귀 스캔과 별도로 크레이트 src 목록을 만들고 누락을 검사한다.
    /// GUI와 headless 파일을 각각 확인해 현재 컴파일 cfg 밖의 파일도 읽는지 검사한다.
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

    /// 금지 목록의 락이 여전히 소스에 존재하는지 확인한다.
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

    /// 프레임 스트림 타입을 감싼 이름 있는 락 선언이 모두 금지 목록에 있는지 확인한다.
    /// 익명 반환 타입 등 이름: 형태가 없는 선언은 이 검사에서 세지 않는다.
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
