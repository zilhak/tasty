//! writer 누계 계측. 원자값을 Relaxed로 읽으므로 스냅샷 전체가 원자적이지는 않다.

use std::sync::atomic::{AtomicU64, Ordering};

/// 묶음 크기 분포의 구간 상한(포함). 마지막 구간은 그 위 전부다.
pub const BATCH_SIZE_BUCKETS: [u64; 5] = [1, 4, 16, 64, 256];

#[derive(Debug, Default)]
pub(crate) struct WriterStats {
    admitted: AtomicU64,
    rejected_saturated: AtomicU64,
    rejected_closed: AtomicU64,
    batches: AtomicU64,
    batch_requests: AtomicU64,
    batch_size_max: AtomicU64,
    batch_sizes: [AtomicU64; BATCH_SIZE_BUCKETS.len() + 1],
    superseded: AtomicU64,
    retries: AtomicU64,
    flushes: AtomicU64,
    peak_items: AtomicU64,
    peak_bytes: AtomicU64,
}

fn as_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

impl WriterStats {
    pub(crate) fn record_admitted(&self, held_items: usize, held_bytes: usize) {
        self.admitted.fetch_add(1, Ordering::Relaxed);
        self.peak_items
            .fetch_max(as_u64(held_items), Ordering::Relaxed);
        self.peak_bytes
            .fetch_max(as_u64(held_bytes), Ordering::Relaxed);
    }

    pub(crate) fn record_rejected_saturated(&self) {
        self.rejected_saturated.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_rejected_closed(&self) {
        self.rejected_closed.fetch_add(1, Ordering::Relaxed);
    }

    /// 실행한 묶음 하나. 재시도는 같은 묶음이라 다시 세지 않는다.
    pub(crate) fn record_batch(&self, requests: usize, superseded: usize) {
        let n = as_u64(requests);
        self.batches.fetch_add(1, Ordering::Relaxed);
        self.batch_requests.fetch_add(n, Ordering::Relaxed);
        self.batch_size_max.fetch_max(n, Ordering::Relaxed);
        let bucket = BATCH_SIZE_BUCKETS
            .iter()
            .position(|&upper| n <= upper)
            .unwrap_or(BATCH_SIZE_BUCKETS.len());
        self.batch_sizes[bucket].fetch_add(1, Ordering::Relaxed);
        self.superseded
            .fetch_add(as_u64(superseded), Ordering::Relaxed);
    }

    pub(crate) fn record_retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_flush(&self) {
        self.flushes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self, held_items: usize, held_bytes: usize) -> WriterStatsSnapshot {
        let load = |a: &AtomicU64| a.load(Ordering::Relaxed);
        WriterStatsSnapshot {
            admitted: load(&self.admitted),
            rejected_saturated: load(&self.rejected_saturated),
            rejected_closed: load(&self.rejected_closed),
            batches: load(&self.batches),
            batch_requests: load(&self.batch_requests),
            batch_size_max: load(&self.batch_size_max),
            batch_sizes: std::array::from_fn(|i| load(&self.batch_sizes[i])),
            superseded: load(&self.superseded),
            retries: load(&self.retries),
            flushes: load(&self.flushes),
            held_items: as_u64(held_items),
            held_bytes: as_u64(held_bytes),
            peak_items: load(&self.peak_items),
            peak_bytes: load(&self.peak_bytes),
        }
    }
}

/// writer 계측의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriterStatsSnapshot {
    /// 접수한 요청 수(flush 제외).
    pub admitted: u64,
    /// 예산이 차서 거절한 수.
    pub rejected_saturated: u64,
    /// 접수를 멈춘 뒤 거절한 수.
    pub rejected_closed: u64,
    /// 실행한 묶음 수. barrier 요청 하나도 묶음 하나다.
    pub batches: u64,
    /// 묶음에 들어간 요청 수 합(대체된 요청 포함).
    pub batch_requests: u64,
    pub batch_size_max: u64,
    /// 묶음 크기 분포. i번째는 `BATCH_SIZE_BUCKETS[i-1]` 초과 `BATCH_SIZE_BUCKETS[i]` 이하, 마지막은 그 위.
    pub batch_sizes: [u64; BATCH_SIZE_BUCKETS.len() + 1],
    /// 같은 snapshot 키의 뒤 요청에 대체된 요청 수.
    pub superseded: u64,
    /// busy 재실행 횟수.
    pub retries: u64,
    pub flushes: u64,
    /// 지금 잡혀 있는 항목 수와 바이트.
    pub held_items: u64,
    pub held_bytes: u64,
    /// 접수 직후 기준 최댓값.
    pub peak_items: u64,
    pub peak_bytes: u64,
}
