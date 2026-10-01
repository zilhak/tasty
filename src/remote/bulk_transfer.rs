//! 파일 전송 청크를 (client_id, transfer_id)별로 누적한다.
//! 프레임 해석은 stream_hub, 권한 확인·저장은 attach_runtime이 맡는다.
//! 연결 종료 시 호출자가 clear_client로 미완료 버퍼를 회수해야 한다.

use std::collections::HashMap;

struct BulkPartial {
    filename: String,
    /// begin이 알린 크기. 이 버퍼는 누적 바이트 수가 이 값과 맞는지 검사하지 않는다.
    #[allow(dead_code)]
    total_size: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct BulkTransferRegistry {
    transfers: HashMap<(u32, u64), BulkPartial>,
}

impl BulkTransferRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 같은 키로 다시 시작하면 이전 버퍼를 버린다.
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

    /// 호출 순서대로 추가하며 seq로 재정렬하거나 중복을 제거하지 않는다.
    /// begin이 없는 전송은 버리고 false를 반환한다.
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

    /// 항목을 제거하고 파일명·바이트를 반환한다. 이미 소비됐거나 등록되지 않았으면 None이다.
    pub(crate) fn take(&mut self, client_id: u32, transfer_id: u64) -> Option<(String, Vec<u8>)> {
        self.transfers
            .remove(&(client_id, transfer_id))
            .map(|p| (p.filename, p.bytes))
    }

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
        assert_eq!(reg.take(1, 100), None);
    }

    #[test]
    fn append_without_begin_is_dropped() {
        let mut reg = BulkTransferRegistry::new();
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
        assert_eq!(reg.take(1, 1), None);
        assert_eq!(reg.take(1, 2), None);
        assert_eq!(reg.take(2, 1), Some(("c".to_string(), Vec::new())));
    }

    #[test]
    fn begin_overwrites_stale_entry() {
        let mut reg = BulkTransferRegistry::new();
        reg.begin(1, 1, "old".to_string(), 0);
        reg.append(1, 1, 0, b"stale");
        reg.begin(1, 1, "new".to_string(), 0);
        reg.append(1, 1, 0, b"fresh");
        assert_eq!(reg.take(1, 1), Some(("new".to_string(), b"fresh".to_vec())));
    }

    #[test]
    fn ordered_batch_routes_to_intact_bytes() {
        // 한 pump 배치의 begin·chunk·commit 순서를 실제 허브와 버퍼를 연결해 확인한다.
        // 아래 루프는 호출부의 라우팅을 재현하며 GUI·헤드리스 메인 루프 자체를 실행하지는 않는다.
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
