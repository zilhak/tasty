//! `ShmError` 정의.

/// 공유 메모리 작업 중 발생할 수 있는 에러.
#[derive(Debug, thiserror::Error)]
pub enum ShmError {
    /// OS syscall 실패.
    #[error("os error: {0}")]
    Os(#[from] std::io::Error),

    /// 요청 크기가 플랫폼 한도를 초과 (현재 8 GB 상한).
    #[error("size {0} exceeds platform limit (8 GB)")]
    TooLarge(usize),

    /// `create(0)` 호출.
    #[error("zero-size shared memory is not supported")]
    ZeroSize,

    /// 이미 소비된 핸들을 재사용 시도.
    #[error("handle already consumed")]
    HandleConsumed,

    /// Windows에서 `OpenProcess(peer_pid)` 실패 (PID가 죽었거나 권한 없음).
    #[error("peer process not accessible (PID {0})")]
    PeerUnreachable(u32),
}
