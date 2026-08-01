//! (03) attach 서버측 — mirror client 가 스트리밍 채널로 보내는 캡처 파일 바이트
//! 청크를 누적하는 순수 버퍼. 파싱/전송 프로토콜은 `stream_hub.rs`(`CaptureUploadMsg`),
//! holder 검증 + 파일 저장 + 클립보드 기록은 `attach_runtime.rs`
//! (`finalize_capture_upload`) 담당 — 이 파일은 그 사이의 상태만 보관한다.

use std::collections::HashMap;

/// `(client_id, upload_id)` → 누적된 바이트. 여러 mirror client 가 동시에 업로드해도
/// 서로 섞이지 않는다.
///
/// disconnect 시 `clear_client`로 청소한다(연결 유지 상태에서 commit이 영원히 안 오는
/// 극단 케이스는 TTL 없음 — 필요 시 후속 과제, `PtyRegistry::sweep_idle` 패턴 참고).
#[derive(Default)]
pub(crate) struct CaptureUploadRegistry {
    partials: HashMap<(u32, u64), Vec<u8>>,
}

impl CaptureUploadRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 청크 하나를 추가(순서 보존 — TCP 스트림이라 도착 순서 = 전송 순서).
    pub(crate) fn append(&mut self, client_id: u32, upload_id: u64, data: &[u8]) {
        self.partials
            .entry((client_id, upload_id))
            .or_default()
            .extend_from_slice(data);
    }

    /// 업로드를 커밋(완료)하고 누적 바이트를 꺼낸다. 없으면 `None`(commit 만 오고
    /// chunk 가 하나도 안 왔거나 이미 소비된 경우).
    pub(crate) fn take(&mut self, client_id: u32, upload_id: u64) -> Option<Vec<u8>> {
        self.partials.remove(&(client_id, upload_id))
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
        reg.append(1, 100, b"hello ");
        reg.append(1, 100, b"world");
        assert_eq!(reg.take(1, 100), Some(b"hello world".to_vec()));
        // 소비 후에는 다시 꺼낼 수 없다.
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn distinct_uploads_and_clients_are_isolated() {
        let mut reg = CaptureUploadRegistry::new();
        reg.append(1, 1, b"a");
        reg.append(2, 1, b"b");
        reg.append(1, 2, b"c");
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
        reg.append(1, 1, b"a");
        reg.append(1, 2, b"b");
        reg.append(2, 1, b"c");
        reg.clear_client(1);
        assert_eq!(reg.take(1, 1), None);
        assert_eq!(reg.take(1, 2), None);
        assert_eq!(reg.take(2, 1), Some(b"c".to_vec()));
    }
}
