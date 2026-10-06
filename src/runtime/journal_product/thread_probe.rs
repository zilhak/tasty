//! 시험이 저널 worker 스레드가 지금 일하는 중인지 읽는다.
//!
//! 정체 감지는 worker가 잠들어 있는 시간만 세야 한다. worker가 실행 중이거나 디스크 I/O를
//! 기다리는 시간까지 세면 같은 디스크를 쓰는 다른 프로세스의 fsync 지연이 교착처럼 보인다.
//! 스레드 상태는 Linux의 `/proc/<pid>/task/<tid>/stat`에서만 읽는다. 다른 OS에서는 항상
//! 모름으로 답하고, 호출자는 쉰 시간을 모두 정체로 센다.
//!
//! join이 돌아온 직후에도 커널은 그 스레드의 종료 경로를 아직 실행하고 있을 수 있다. 그동안
//! 상태는 R로 읽히지만 flags에 PF_EXITING이 서 있으므로 일하는 중으로 보지 않는다. 같은 TID가
//! 다른 스레드에 다시 쓰이면 시작 시각이 달라지므로 그 스레드도 일하는 중으로 보지 않는다.
//!
//! 정체로 판정한 시험은 worker를 버린다고 표시한다. 멈춘 worker를 Drop에서 join하면 시험이
//! 실패로 끝나지 않고 그 자리에서 멈추기 때문이다.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// 커널 `PF_EXITING`: 스레드가 종료 경로에 들어갔다.
const PF_EXITING: u64 = 0x4;

/// worker 스레드의 상태 파일 경로와 버림 표시. worker가 시작하며 경로와 시작 시각을 한 번 기록한다.
#[derive(Clone, Default)]
pub(crate) struct ThreadProbe {
    stat: Arc<OnceLock<Bound>>,
    abandoned: Arc<AtomicBool>,
}

impl ThreadProbe {
    /// 지금 스레드를 이 probe가 볼 스레드로 기록한다. worker 스레드 첫머리에서 부른다.
    pub(crate) fn bind_current(&self) {
        #[cfg(target_os = "linux")]
        match std::fs::read_link("/proc/thread-self") {
            Ok(link) => {
                let path = PathBuf::from("/proc").join(link).join("stat");
                let Some(start) = start_of(&path) else {
                    return;
                };
                if self.stat.set(Bound { path, start }).is_err() {
                    tracing::warn!("journal worker thread probe was bound twice");
                }
            }
            Err(error) => tracing::warn!("cannot locate the journal worker thread: {error}"),
        }
    }

    /// worker가 실행 중(R)이거나 끊을 수 없는 대기(D, 대개 디스크 I/O)에 있으면 true.
    /// 잠든 상태, 끝났거나 끝나는 중인 스레드, TID를 물려받은 다른 스레드, 상태를 읽을 수 없는
    /// OS는 false다.
    pub(crate) fn is_working(&self) -> bool {
        let Some(bound) = self.stat.get() else {
            return false;
        };
        match std::fs::read_to_string(&bound.path) {
            Ok(stat) => works(&stat, &bound.start),
            Err(_) => false,
        }
    }
}

impl ThreadProbe {
    /// 시험이 이 worker를 정체로 판정했다. 이후 Drop은 worker를 join하지 않는다.
    pub(crate) fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
    }

    pub(crate) fn is_abandoned(&self) -> bool {
        self.abandoned.load(Ordering::Acquire)
    }
}

/// 기록한 스레드의 상태 파일과 그 스레드의 시작 시각(`stat` 22번째 필드).
struct Bound {
    path: PathBuf,
    start: String,
}

/// `stat`에서 읽는 필드. 3번째 state, 9번째 flags, 22번째 starttime.
struct Fields {
    state: char,
    flags: u64,
    start: String,
}

/// 둘째 필드(comm)는 괄호와 공백을 품을 수 있어 마지막 `)` 뒤에서 읽는다.
fn fields_of(stat: &str) -> Option<Fields> {
    let (_, rest) = stat.rsplit_once(')')?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(Fields {
        state: fields.first()?.chars().next()?,
        flags: fields.get(6)?.parse().ok()?,
        start: (*fields.get(19)?).to_owned(),
    })
}

/// `path`의 스레드 시작 시각. 읽거나 해석하지 못하면 경고를 남기고 None.
#[cfg(target_os = "linux")]
fn start_of(path: &std::path::Path) -> Option<String> {
    match std::fs::read_to_string(path).map(|stat| fields_of(&stat)) {
        Ok(Some(fields)) => Some(fields.start),
        Ok(None) => {
            tracing::warn!("cannot parse the journal worker thread stat");
            None
        }
        Err(error) => {
            tracing::warn!("cannot read the journal worker thread stat: {error}");
            None
        }
    }
}

/// `start`에 시작한 스레드가 이 `stat`에서 일하는 중인지.
fn works(stat: &str, start: &str) -> bool {
    fields_of(stat).is_some_and(|f| {
        f.start == start && f.flags & PF_EXITING == 0 && matches!(f.state, 'R' | 'D')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 `stat` 한 줄의 꼴. state, flags(9번째), starttime(22번째)만 바꿔 넣는다.
    fn stat(comm: &str, state: char, flags: u64, start: u64) -> String {
        format!("12 ({comm}) {state} 1 12 12 0 -1 {flags} 0 0 0 0 0 0 0 0 20 0 1 0 {start} 0 0")
    }

    #[test]
    fn the_fields_are_read_after_a_comm_that_holds_parentheses() {
        let f = fields_of(&stat("a) b", 'D', 64, 777)).expect("parsed");
        assert_eq!((f.state, f.flags, f.start.as_str()), ('D', 64, "777"));
        assert!(fields_of(&stat("structure-journ", 'S', 0, 1)).is_some());
        assert!(fields_of("garbage").is_none());
        assert!(fields_of("12 (x) R 1").is_none());
    }

    #[test]
    fn only_the_bound_live_thread_running_or_in_io_is_working() {
        assert!(works(&stat("w", 'R', 64, 5), "5"));
        assert!(works(&stat("w", 'D', 64, 5), "5"));
        assert!(!works(&stat("w", 'S', 64, 5), "5"));
        // join 직후 종료 경로를 실행 중인 같은 스레드.
        assert!(!works(&stat("w", 'R', 64 | PF_EXITING, 5), "5"));
        // 같은 TID를 물려받은 다른 스레드.
        assert!(!works(&stat("w", 'R', 64, 9), "5"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_thread_spinning_reads_as_working_and_a_parked_one_does_not() {
        let probe = ThreadProbe::default();
        let bound = probe.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let spin = stop.clone();
        let (park_tx, park_rx) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            bound.bind_current();
            while !spin.load(std::sync::atomic::Ordering::Relaxed) {
                std::hint::spin_loop();
            }
            // 잠든 상태(S)로 머문다.
            park_rx.recv().ok();
        });
        while probe.stat.get().is_none() {
            std::thread::yield_now();
        }
        assert!(probe.is_working(), "spinning thread is R");
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while probe.is_working() {
            assert!(
                std::time::Instant::now() < deadline,
                "parked thread never read as sleeping"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        drop(park_tx);
        handle.join().unwrap();
        assert!(!probe.is_working(), "an exited thread is not working");
    }
}
