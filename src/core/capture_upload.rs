//! (03) attach 서버측 — mirror client 가 스트리밍 채널로 보내는 캡처 파일 바이트
//! 청크를 누적하는 순수 버퍼. 파싱/전송 프로토콜은 `stream_hub.rs`(`CaptureUploadMsg`),
//! holder 검증 + 파일 저장 + 클립보드 기록은 `attach_runtime.rs`
//! (`finalize_capture_upload`) 담당 — 이 파일은 그 사이의 상태만 보관한다.
//!
//! host-IPC-free — 단위 테스트 가능. 시간 의존 연산([`append`](CaptureUploadRegistry::append))은
//! `now: Instant` 를 주입받아 테스트가 TTL 경과를 sleep 없이 재현한다(`PtyRegistry` 와 동형).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 마지막 청크 수신 이후 이 시간이 지나면 방치된 partial 업로드로 간주해 스윕 대상이
/// 된다. 기본 5 분 — `PtyRegistry::DEFAULT_IDLE_TTL` 과 동일 근거(client 가 정상 동작
/// 중이라면 청크/commit 이 초 단위로 오가므로, 5 분 정체는 사실상 죽은 연결).
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(300);

struct PartialUpload {
    bytes: Vec<u8>,
    last_activity: Instant,
}

/// `(client_id, upload_id)` → 누적된 바이트 + 마지막 활동 시각. 여러 mirror client 가
/// 동시에 업로드해도 서로 섞이지 않는다.
///
/// disconnect 시 `clear_client`로 즉시 청소하고, 연결 유지 상태에서 commit이 영원히
/// 안 오는 극단 케이스는 `append` 호출 시점의 lazy sweep(TTL 초과 partial 제거)이 회수한다
/// — `PtyRegistry::sweep_idle`("접근 시 self-heal") 패턴과 동형.
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

    /// 청크 하나를 추가(순서 보존 — TCP 스트림이라 도착 순서 = 전송 순서). 매 호출마다
    /// TTL 초과 partial 을 먼저 스윕한다("다음 업로드 호출 시 정리" — 별도 스레드/타이머
    /// 불필요).
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

    /// TTL 을 초과해 방치된 partial 을 제거한다. 반환값 없음 — 호출자(=`append`)가 그
    /// 결과를 쓰지 않기 때문(commit 이 다시 와도 이미 사라진 upload_id 로 `take` 하면
    /// `None`, `finalize_capture_upload` 가 그 케이스를 이미 처리한다).
    fn sweep_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        let before = self.partials.len();
        self.partials
            .retain(|_, e| now.duration_since(e.last_activity) < ttl);
        let removed = before - self.partials.len();
        if removed > 0 {
            tracing::debug!(
                "capture upload: swept {removed} stale partial upload(s) (idle TTL exceeded)"
            );
        }
    }

    /// 업로드를 커밋(완료)하고 누적 바이트를 꺼낸다. 없으면 `None`(commit 만 오고
    /// chunk 가 하나도 안 왔거나 이미 소비된 경우, 또는 TTL 초과로 스윕된 경우).
    pub(crate) fn take(&mut self, client_id: u32, upload_id: u64) -> Option<Vec<u8>> {
        self.partials
            .remove(&(client_id, upload_id))
            .map(|e| e.bytes)
    }

    /// 한 client의 모든 진행 중 캡처 업로드를 폐기한다(연결 종료 시 partial 청소 —
    /// `BulkTransferRegistry::clear_client`와 동형). commit 없이 끊긴 업로드가 메모리에
    /// 영구 잔존하지 않게 한다.
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
        // 소비 후에는 다시 꺼낼 수 없다.
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

        // TTL 초과 후 (다른 client의) 새 청크가 도착하면 lazy sweep 이 방치된
        // 엔트리를 회수한다.
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

        // TTL 이내에 새 청크가 도착 → last_activity 갱신, 스윕 대상 아님.
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

        // base 로부터 250s 시점에 활동 → last_activity 갱신.
        let touched = base + Duration::from_secs(250);
        reg.append(1, 1, b"b", touched);

        // base 로부터는 301s 지났지만, touched 로부터는 51s 뿐 — 살아남아야 한다.
        let now = base + Duration::from_secs(301);
        reg.append(3, 3, b"trigger-sweep", now);
        assert_eq!(reg.take(1, 1), Some(b"ab".to_vec()));
    }
}
