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
pub(crate) struct OutputSink {
    tx: mpsc::Sender<Vec<u8>>,
    /// local Pty writer의 완료 카운터. 외부 채널은 완료를 보고하지 않으므로 증가하지 않는다.
    progress: WriteProgress,
}

impl OutputSink {
    /// 완료 카운터를 가진 writer(local Pty)와 연결한다.
    pub(crate) fn with_progress(tx: mpsc::Sender<Vec<u8>>, progress: WriteProgress) -> Self {
        Self { tx, progress }
    }

    /// 완료를 보고하지 않는 외부 채널(attach 입력 등)과 연결한다.
    pub(crate) fn external(tx: mpsc::Sender<Vec<u8>>) -> Self {
        Self::with_progress(tx, new_write_progress())
    }

    pub(crate) fn send(&self, bytes: Vec<u8>) -> Result<(), mpsc::SendError<Vec<u8>>> {
        self.tx.send(bytes)
    }

    pub(crate) fn progress(&self) -> &WriteProgress {
        &self.progress
    }
}
