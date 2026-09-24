//! native bulk 파일 전송 서버측 — 전용 bulk 연결이 나른 파일 청크를
//! `(client_id, transfer_id)` 단위로 누적하는 버퍼(ADR-0022).
//!
//! 스크린샷 캡처 업로드([`CaptureUploadRegistry`](crate::core::capture_upload))의 일반화
//! 버전이며 **병렬 신설**이다(캡처 경로는 그대로 유지). 차이:
//! - 키가 `(client_id, transfer_id)` 이고, **메타데이터(파일명·총 크기)를 begin 에서**
//!   먼저 받아 보관한다(캡처는 commit 에서 파일명을 받음). 청크는 이후 append.
//! - 대용량이라 disconnect 시 남은 partial 을 [`clear_client`](BulkTransferRegistry::clear_client)
//!   로 일괄 청소한다(캡처 레지스트리의 알려진 partial 잔존 결함을 여기선 회피).
//!
//! 파싱/분류는 `tasty_ipc::stream_hub`(`BulkEvent`)·`tasty_ipc::stream`(`decode_bulk_chunk`), 인가·저장·경로
//! 회신은 `attach_runtime.rs`(`finalize_bulk_transfer`) 담당 — 이 파일은 그 사이의
//! 상태만 보관한다.

use std::collections::HashMap;

/// 진행 중인 한 전송의 누적 상태. `begin` 이 메타데이터로 생성, 청크가 `bytes` 에
/// append, `commit` 이 통째로 take.
struct BulkPartial {
    filename: String,
    /// begin 이 통지한 총 크기(진단·용량 사전 승인 입력). 저장 자체엔 쓰지 않는다.
    #[allow(dead_code)]
    total_size: u64,
    bytes: Vec<u8>,
}

/// `(client_id, transfer_id)` → 진행 중 전송. 여러 client 가 동시에 전송해도, 한
/// client 가 여러 transfer 를 병행해도 서로 섞이지 않는다.
#[derive(Default)]
pub(crate) struct BulkTransferRegistry {
    transfers: HashMap<(u32, u64), BulkPartial>,
}

impl BulkTransferRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 전송 시작 — 파일명·총 크기를 등록한다. 같은 키의 stale 엔트리가 있으면
    /// 덮어써 새 전송으로 취급한다(재시작한 transfer_id).
    pub(crate) fn begin(
        &mut self,
        client_id: u32,
        transfer_id: u64,
        filename: String,
        total_size: u64,
    ) {
        self.transfers.insert(
            (client_id, transfer_id),
            BulkPartial {
                filename,
                total_size,
                bytes: Vec::new(),
            },
        );
    }

    /// 청크 하나를 append(순서 보존 — TCP 는 연결당 도착 순서 = 전송 순서라 `seq` 로
    /// 재정렬하지 않는다). begin 이 선행하지 않은 transfer 의 청크는 파일명을 알 수
    /// 없어 저장할 수 없으므로 조용히 버린다(begin→chunk 순서는 같은 연결의 TCP 순서
    /// 보장; 위반은 비정상 클라). 버린 경우 `false`.
    pub(crate) fn append(
        &mut self,
        client_id: u32,
        transfer_id: u64,
        _seq: u32,
        data: &[u8],
    ) -> bool {
        match self.transfers.get_mut(&(client_id, transfer_id)) {
            Some(p) => {
                p.bytes.extend_from_slice(data);
                true
            }
            None => false,
        }
    }

    /// 전송을 완료(take)하고 `(파일명, 누적 바이트)` 를 꺼낸다. begin 이 없었거나 이미
    /// 소비된 경우 `None`.
    pub(crate) fn take(&mut self, client_id: u32, transfer_id: u64) -> Option<(String, Vec<u8>)> {
        self.transfers
            .remove(&(client_id, transfer_id))
            .map(|p| (p.filename, p.bytes))
    }

    /// 한 client 의 모든 진행 중 전송을 폐기한다(연결 종료 시 partial 청소 — 대용량이라
    /// 커밋 없이 끊긴 전송이 메모리에 잔존하지 않게).
    pub(crate) fn clear_client(&mut self, client_id: u32) {
        self.transfers.retain(|(cid, _), _| *cid != client_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_append_take_roundtrip() {
        let mut reg = BulkTransferRegistry::new();
        reg.begin(1, 100, "img.png".to_string(), 11);
        assert!(reg.append(1, 100, 0, b"hello "));
        assert!(reg.append(1, 100, 1, b"world"));
        assert_eq!(
            reg.take(1, 100),
            Some(("img.png".to_string(), b"hello world".to_vec()))
        );
        // 소비 후에는 다시 꺼낼 수 없다.
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn append_without_begin_is_dropped() {
        let mut reg = BulkTransferRegistry::new();
        // begin 없이 온 청크는 파일명 미상이라 버려진다.
        assert!(!reg.append(1, 100, 0, b"orphan"));
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn distinct_transfers_and_clients_are_isolated() {
        let mut reg = BulkTransferRegistry::new();
        reg.begin(1, 1, "a.bin".to_string(), 1);
        reg.begin(2, 1, "b.bin".to_string(), 1);
        reg.begin(1, 2, "c.bin".to_string(), 1);
        reg.append(1, 1, 0, b"a");
        reg.append(2, 1, 0, b"b");
        reg.append(1, 2, 0, b"c");
        assert_eq!(reg.take(1, 1), Some(("a.bin".to_string(), b"a".to_vec())));
        assert_eq!(reg.take(2, 1), Some(("b.bin".to_string(), b"b".to_vec())));
        assert_eq!(reg.take(1, 2), Some(("c.bin".to_string(), b"c".to_vec())));
    }

    #[test]
    fn clear_client_drops_only_that_clients_partials() {
        let mut reg = BulkTransferRegistry::new();
        reg.begin(1, 1, "a".to_string(), 0);
        reg.begin(1, 2, "b".to_string(), 0);
        reg.begin(2, 1, "c".to_string(), 0);
        reg.clear_client(1);
        // client 1 의 전송은 모두 사라지고, client 2 는 남는다.
        assert_eq!(reg.take(1, 1), None);
        assert_eq!(reg.take(1, 2), None);
        assert_eq!(reg.take(2, 1), Some(("c".to_string(), Vec::new())));
    }

    #[test]
    fn begin_overwrites_stale_entry() {
        let mut reg = BulkTransferRegistry::new();
        reg.begin(1, 1, "old".to_string(), 0);
        reg.append(1, 1, 0, b"stale");
        // 같은 키로 다시 begin 하면 새 전송으로 리셋된다.
        reg.begin(1, 1, "new".to_string(), 0);
        reg.append(1, 1, 0, b"fresh");
        assert_eq!(reg.take(1, 1), Some(("new".to_string(), b"fresh".to_vec())));
    }

    #[test]
    fn ordered_batch_routes_to_intact_bytes() {
        // 회귀(Gate4) end-to-end: begin+chunk0+chunk1+commit 이 **한 pump 배치**에
        // 함께 도착 → 라우팅이 bulk_events 를 순서대로 레지스트리에 반영하면 최종
        // take 된 bytes 가 전량 온전해야 한다. (분리 벡터 시절엔 chunk pass 가 begin
        // pass 보다 먼저 돌아 청크가 미등록 transfer 로 폐기 → 빈 파일이 저장됐다.)
        //
        // 허브(`tasty_ipc::stream_hub`)와 이 레지스트리를 **함께** 재는 시험이라 두 계층 중
        // 위쪽인 여기에 둔다 — 허브 크레이트가 본체 core 를 역참조할 수는 없다
        // (docs/adr/0001-crate-dependency-boundaries.md).
        use std::sync::mpsc;

        use tasty_ipc::stream::{StreamControl, StreamFrame, StreamTag, encode_bulk_chunk};
        use tasty_ipc::stream_hub::{BulkEvent, StreamHub, StreamInbound};

        let frame = |tag: StreamTag, p: &[u8]| StreamFrame::new(tag, p.to_vec());

        let hub = StreamHub::new();
        hub.register_bulk(5, 2);
        let (tx, inbound_rx) = mpsc::channel();
        let begin = serde_json::to_vec(&StreamControl::BulkBegin {
            transfer_id: 7,
            filename: "f.bin".to_string(),
            total_size: 6,
        })
        .unwrap();
        let commit = serde_json::to_vec(&StreamControl::BulkCommit { transfer_id: 7 }).unwrap();
        for f in [
            frame(StreamTag::Control, &begin),
            frame(StreamTag::Data, &encode_bulk_chunk(7, 0, b"abc")),
            frame(StreamTag::Data, &encode_bulk_chunk(7, 1, b"def")),
            frame(StreamTag::Control, &commit),
        ] {
            tx.send(StreamInbound::Frame {
                client_id: 5,
                frame: f,
            })
            .unwrap();
        }
        let out = hub.pump_inbound(&inbound_rx);

        // 라우팅(boot.rs/event_handler.rs)이 하는 것과 동형: 단일 벡터를 순서대로 처리.
        let mut reg = BulkTransferRegistry::new();
        let mut committed: Option<(String, Vec<u8>)> = None;
        for (client_id, event) in out.bulk_events {
            match event {
                BulkEvent::Begin {
                    transfer_id,
                    filename,
                    total_size,
                } => reg.begin(client_id, transfer_id, filename, total_size),
                BulkEvent::Chunk {
                    transfer_id,
                    seq,
                    bytes,
                } => {
                    assert!(
                        reg.append(client_id, transfer_id, seq, &bytes),
                        "chunk must land on a registered transfer (begin already processed)"
                    );
                }
                BulkEvent::Commit { transfer_id } => {
                    committed = reg.take(client_id, transfer_id);
                }
            }
        }
        assert_eq!(
            committed,
            Some(("f.bin".to_string(), b"abcdef".to_vec())),
            "commit 시 누적 bytes 가 전량 온전해야 한다"
        );
    }
}
