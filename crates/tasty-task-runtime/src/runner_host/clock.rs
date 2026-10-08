//! runner 가 쓰는 현재 시각(Unix epoch 밀리초).

pub(crate) fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// TTL 점유를 얻고 갱신할 때 쓰는 시각. 기본은 [`now_ms`]다. 시험은 [`HoldingClock::manual`]로 시각을
/// 직접 진행시켜, tick 이 늦게 와도 갱신 주기와 만료 판정이 같은 시각을 보게 한다.
#[derive(Clone)]
pub(crate) struct HoldingClock(std::sync::Arc<dyn Fn() -> u64 + Send + Sync>);

impl HoldingClock {
    pub(crate) fn system() -> Self {
        Self(std::sync::Arc::new(now_ms))
    }

    pub(crate) fn now_ms(&self) -> u64 {
        (self.0)()
    }

    /// `start_ms` 에서 멈춰 있는 시계와 그 시각. 시각은 돌려준 값을 바꿀 때만 움직인다.
    // 사용처인 TTL 갱신 시험이 unix 전용이다.
    #[cfg(all(test, unix))]
    pub(crate) fn manual(start_ms: u64) -> (Self, std::sync::Arc<std::sync::atomic::AtomicU64>) {
        use std::sync::atomic::{AtomicU64, Ordering};
        let now = std::sync::Arc::new(AtomicU64::new(start_ms));
        let read = now.clone();
        (
            Self(std::sync::Arc::new(move || read.load(Ordering::SeqCst))),
            now,
        )
    }
}
