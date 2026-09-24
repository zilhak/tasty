//! 캡처 업로드를 (client_id, upload_id)별로 누적한다.
//! 프레임 해석은 stream_hub, 권한 확인·파일 저장·클립보드는 attach_runtime이 맡는다.
//! 시각을 인자로 받아 실제 대기 없이 만료를 검사할 수 있다.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 마지막 청크 이후 이 시간 이상 지난 버퍼를 회수한다. 연결 종료를 판정하는 값은 아니다.
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(300);

struct PartialUpload {
    bytes: Vec<u8>,
    last_activity: Instant,
}

/// append가 만료 버퍼를 회수하고, 연결 종료 시 호출자가 clear_client를 호출한다.
/// GUI는 주기 타이머로도 정리한다. 헤드리스에는 주기 회수가 없어 새 청크나 연결 종료를 기다린다.
pub(crate) struct CaptureUploadRegistry {
    partials: HashMap<(u32, u64), PartialUpload>,
    ttl: Duration,
}

impl Default for CaptureUploadRegistry {
    fn default() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }
}

impl CaptureUploadRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            partials: HashMap::new(),
            ttl,
        }
    }

    /// 만료 버퍼를 먼저 지운 뒤 청크를 추가한다. 만료된 같은 ID가 다시 오면 새 버퍼가 된다.
    pub(crate) fn append(&mut self, client_id: u32, upload_id: u64, data: &[u8], now: Instant) {
        self.sweep_expired(now);
        let entry = self
            .partials
            .entry((client_id, upload_id))
            .or_insert_with(|| PartialUpload {
                bytes: Vec::new(),
                last_activity: now,
            });
        entry.bytes.extend_from_slice(data);
        entry.last_activity = now;
    }

    /// 마지막 활동에서 TTL 이상 지난 버퍼를 지운다.
    pub(crate) fn sweep_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        let before = self.partials.len();
        self.partials
            .retain(|_, e| now.duration_since(e.last_activity) < ttl);
        let removed = before - self.partials.len();
        if removed > 0 {
            tracing::debug!(
                "capture upload: removed {removed} incomplete upload(s) idle for at least the TTL"
            );
        }
    }

    /// 버퍼를 제거하고 바이트를 반환한다. 여기서는 만료 시간을 다시 검사하지 않는다.
    pub(crate) fn take(&mut self, client_id: u32, upload_id: u64) -> Option<Vec<u8>> {
        self.partials
            .remove(&(client_id, upload_id))
            .map(|e| e.bytes)
    }

    pub(crate) fn clear_client(&mut self, client_id: u32) {
        self.partials.retain(|(cid, _), _| *cid != client_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_in_order_and_takes_once() {
        let mut reg = CaptureUploadRegistry::new();
        let now = Instant::now();
        reg.append(1, 100, b"hello ", now);
        reg.append(1, 100, b"world", now);
        assert_eq!(reg.take(1, 100), Some(b"hello world".to_vec()));
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn distinct_uploads_and_clients_are_isolated() {
        let mut reg = CaptureUploadRegistry::new();
        let now = Instant::now();
        reg.append(1, 1, b"a", now);
        reg.append(2, 1, b"b", now);
        reg.append(1, 2, b"c", now);
        assert_eq!(reg.take(1, 1), Some(b"a".to_vec()));
        assert_eq!(reg.take(2, 1), Some(b"b".to_vec()));
        assert_eq!(reg.take(1, 2), Some(b"c".to_vec()));
    }

    /// sweep을 직접 호출해 새 청크 없이 회수되는지 확인한다. 타이머 연결 자체를 검사하지는 않는다.
    #[test]
    fn periodic_sweep_reaps_a_stalled_upload_without_a_new_chunk() {
        let mut reg = CaptureUploadRegistry::new();
        let t0 = Instant::now();
        reg.append(1, 100, b"partial", t0);
        assert_eq!(reg.partials.len(), 1);

        reg.sweep_expired(t0 + DEFAULT_TTL - Duration::from_secs(1));
        assert_eq!(reg.partials.len(), 1, "TTL 안의 partial 은 보존");

        reg.sweep_expired(t0 + DEFAULT_TTL);
        assert!(reg.partials.is_empty(), "TTL에 도달한 미완료 버퍼 회수");
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn take_without_chunks_is_none() {
        let mut reg = CaptureUploadRegistry::new();
        assert_eq!(reg.take(9, 9), None);
    }

    #[test]
    fn clear_client_drops_only_that_clients_partials() {
        let mut reg = CaptureUploadRegistry::new();
        let now = Instant::now();
        reg.append(1, 1, b"a", now);
        reg.append(1, 2, b"b", now);
        reg.append(2, 1, b"c", now);
        reg.clear_client(1);
        assert_eq!(reg.take(1, 1), None);
        assert_eq!(reg.take(1, 2), None);
        assert_eq!(reg.take(2, 1), Some(b"c".to_vec()));
    }

    #[test]
    fn stale_partial_is_swept_on_next_append() {
        let ttl = Duration::from_secs(300);
        let mut reg = CaptureUploadRegistry::with_ttl(ttl);
        let base = Instant::now();
        reg.append(1, 1, b"orphaned", base);

        let beyond = base + Duration::from_secs(301);
        reg.append(2, 2, b"fresh", beyond);

        assert_eq!(reg.take(1, 1), None, "TTL 초과 partial 은 스윕되어야 한다");
        assert_eq!(reg.take(2, 2), Some(b"fresh".to_vec()));
    }

    #[test]
    fn active_partial_within_ttl_survives_sweep() {
        let ttl = Duration::from_secs(300);
        let mut reg = CaptureUploadRegistry::with_ttl(ttl);
        let base = Instant::now();
        reg.append(1, 1, b"still ", base);

        let within = base + Duration::from_secs(299);
        reg.append(1, 1, b"going", within);
        assert_eq!(reg.take(1, 1), Some(b"still going".to_vec()));
    }

    #[test]
    fn activity_resets_ttl_countdown() {
        let ttl = Duration::from_secs(300);
        let mut reg = CaptureUploadRegistry::with_ttl(ttl);
        let base = Instant::now();
        reg.append(1, 1, b"a", base);

        let touched = base + Duration::from_secs(250);
        reg.append(1, 1, b"b", touched);

        let now = base + Duration::from_secs(301);
        reg.append(3, 3, b"trigger-sweep", now);
        assert_eq!(reg.take(1, 1), Some(b"ab".to_vec()));
    }
}
