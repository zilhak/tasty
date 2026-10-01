//! Terminal이 밖으로 내보내는 byte의 전달 계약.
//!
//! 사용자 입력과 VT 응답(DSR/DA/OSC 조회)은 같은 sink로 나간다. sink는 local Pty의
//! writer이거나 detached mirror의 attach 입력 채널이다. TerminalState는 sink의
//! 구체 소유자(Pty)를 모르고 채널과 완료 카운터만 가진다.

use std::sync::{Arc, Condvar, Mutex, mpsc};

/// writer가 지금까지 flush 완료한 write 개수(`Mutex<u64>`)와 대기자 깨우기용 condvar.
/// [`crate::WriteAck`]가 폴링 없이 "이 write가 실제로 flush 됐는지"를 확인하는 데 쓴다.
pub(crate) type WriteProgress = Arc<(Mutex<u64>, Condvar)>;

pub(crate) const WRITE_PROGRESS_WHAT: &str = "the PTY write-progress counter";
pub(crate) static WRITE_PROGRESS_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// writer와 WriteAck가 함께 쓰는 완료 횟수 락. poison을 복구하고 처음 한 번 보고한다.
/// 복구 없이 갱신을 건너뛰면 PTY 쓰기가 끝나도 대기자는 완료를 확인하지 못한다.
pub(crate) fn lock_write_progress(count: &Mutex<u64>) -> std::sync::MutexGuard<'_, u64> {
    tasty_utils::poison::recover_mutex(
        count.lock(),
        WRITE_PROGRESS_WHAT,
        &WRITE_PROGRESS_POISON_REPORTED,
    )
}

pub(crate) fn new_write_progress() -> WriteProgress {
    Arc::new((Mutex::new(0), Condvar::new()))
}

/// Terminal이 byte를 내보내는 현재 연결. 보낸 순서대로 전달된다.
pub type ExternalInput = Arc<dyn Fn(Vec<u8>) -> Result<(), mpsc::SendError<Vec<u8>>> + Send + Sync>;
enum Target {
    Channel(mpsc::Sender<Vec<u8>>),
    External(ExternalInput),
}
pub(crate) struct OutputSink {
    target: Target,
    /// local Pty writer의 완료 카운터. 외부 채널은 완료를 보고하지 않으므로 증가하지 않는다.
    progress: WriteProgress,
    lease: Option<crate::binding::ConnectionLease>,
}

impl OutputSink {
    /// 완료 카운터를 가진 writer(local Pty)와 연결한다.
    pub(crate) fn with_progress(tx: mpsc::Sender<Vec<u8>>, progress: WriteProgress) -> Self {
        Self {
            target: Target::Channel(tx),
            progress,
            lease: None,
        }
    }

    /// 완료를 보고하지 않는 외부 채널(attach 입력 등)과 연결한다.
    pub(crate) fn external(tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self::with_progress(tx, new_write_progress())
    }

    pub(crate) fn callback(send: ExternalInput) -> Self {
        Self {
            target: Target::External(send),
            progress: new_write_progress(),
            lease: None,
        }
    }

    pub(crate) fn send(&self, bytes: Vec<u8>) -> Result<(), mpsc::SendError<Vec<u8>>> {
        if self.lease.as_ref().is_some_and(|lease| !lease.is_active()) {
            return Err(mpsc::SendError(bytes));
        }
        match &self.target {
            Target::Channel(tx) => tx.send(bytes),
            Target::External(send) => send(bytes),
        }
    }

    pub(crate) fn bind(mut self, lease: crate::binding::ConnectionLease) -> Self {
        self.lease = Some(lease);
        self
    }

    pub(crate) fn progress(&self) -> &WriteProgress {
        &self.progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Write};
    use std::time::Duration;

    struct HeldWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
        entered: Option<mpsc::Sender<()>>,
        release: mpsc::Receiver<()>,
    }
    impl Write for HeldWriter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            self.bytes.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// An already-started OS write may finish, but the retired queue must not start the next write.
    #[test]
    fn retirement_fences_queued_writes_after_the_in_flight_write() {
        let connection = crate::binding::ConnectionLease::new();
        let progress = new_write_progress();
        let (tx, rx) = mpsc::channel();
        let sink = OutputSink::with_progress(tx, Arc::clone(&progress)).bind(connection.clone());
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer = HeldWriter {
            bytes: Arc::clone(&bytes),
            entered: Some(entered_tx),
            release: release_rx,
        };
        let worker_connection = connection.clone();
        let worker_progress = Arc::clone(&progress);
        let worker = std::thread::spawn(move || {
            crate::pty::run_writer_loop(
                Box::new(writer),
                rx,
                worker_progress,
                Some(worker_connection),
            )
        });
        sink.send(b"started".to_vec()).unwrap();
        sink.send(b"queued-old".to_vec()).unwrap();
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("actual writer entered write");
        connection.revoke();
        assert!(sink.send(b"late-response".to_vec()).is_err());
        release_tx.send(()).unwrap();
        drop(sink);
        worker.join().unwrap();
        assert_eq!(*bytes.lock().unwrap(), b"started");
        assert_eq!(*lock_write_progress(&progress.0), 1);
        assert!(!connection.is_active());
    }
}
