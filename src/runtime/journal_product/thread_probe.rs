//! 시험이 저널 worker 스레드가 지금 일하는 중인지 읽는다.
//!
//! 정체 감지는 worker가 잠들어 있는 시간만 세야 한다. worker가 실행 중이거나 디스크 I/O를
//! 기다리는 시간까지 세면 같은 디스크를 쓰는 다른 프로세스의 fsync 지연이 교착처럼 보인다.
//! 스레드 상태는 Linux의 `/proc/<pid>/task/<tid>/stat`에서만 읽는다. 다른 OS에서는 항상
//! 모름으로 답하고, 호출자는 쉰 시간을 모두 정체로 센다.
//!
//! 정체로 판정한 시험은 worker를 버린다고 표시한다. 멈춘 worker를 Drop에서 join하면 시험이
//! 실패로 끝나지 않고 그 자리에서 멈추기 때문이다.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// worker 스레드의 상태 파일 경로와 버림 표시. worker가 시작하며 경로를 한 번 기록한다.
#[derive(Clone, Default)]
pub(crate) struct ThreadProbe {
    stat: Arc<OnceLock<PathBuf>>,
    abandoned: Arc<AtomicBool>,
}

impl ThreadProbe {
    /// 지금 스레드를 이 probe가 볼 스레드로 기록한다. worker 스레드 첫머리에서 부른다.
    pub(crate) fn bind_current(&self) {
        #[cfg(target_os = "linux")]
        match std::fs::read_link("/proc/thread-self") {
            Ok(link) => {
                let path = PathBuf::from("/proc").join(link).join("stat");
                if self.stat.set(path).is_err() {
                    tracing::warn!("journal worker thread probe was bound twice");
                }
            }
            Err(error) => tracing::warn!("cannot locate the journal worker thread: {error}"),
        }
    }

    /// worker가 실행 중(R)이거나 끊을 수 없는 대기(D, 대개 디스크 I/O)에 있으면 true.
    /// 잠든 상태, 끝난 스레드, 상태를 읽을 수 없는 OS는 false다.
    pub(crate) fn is_working(&self) -> bool {
        let Some(path) = self.stat.get() else {
            return false;
        };
        match std::fs::read_to_string(path) {
            Ok(stat) => matches!(state_of(&stat), Some('R' | 'D')),
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

/// `stat`의 세 번째 필드. 둘째 필드(comm)는 괄호와 공백을 품을 수 있어 마지막 `)` 뒤에서 읽는다.
fn state_of(stat: &str) -> Option<char> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.trim_start().chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_is_read_after_a_comm_that_holds_parentheses() {
        assert_eq!(state_of("12 (a) b) D 1 2"), Some('D'));
        assert_eq!(state_of("12 (structure-journ) S 1"), Some('S'));
        assert_eq!(state_of("garbage"), None);
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
