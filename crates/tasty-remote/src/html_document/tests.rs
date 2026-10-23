use super::*;
use tasty_ipc::html_transfer::HtmlSource;
fn id() -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 7,
        binding: 9,
        document: 1,
        request: 1,
        transfer: 1,
    }
}
fn metadata(id: TransferId, size: u64) -> HtmlReply {
    HtmlReply::HtmlMetadata {
        id,
        metadata: HtmlMetadata {
            source: HtmlSource::File {
                path: "/remote/a.html".into(),
                url: "file:///remote/a.html".into(),
            },
            source_generation: 1,
            root: Some("/remote".into()),
            cwd: Some("/remote".into()),
            size,
            regular: true,
            file: None,
        },
    }
}
fn document(size: u64) -> HtmlDocument {
    let mut d = HtmlDocument::new(1, 7, 9, 1);
    d.request(id()).unwrap();
    assert!(d.receive(metadata(id(), size)));
    d
}
fn chunk(id: TransferId, offset: u64, bytes: &[u8]) -> HtmlReply {
    HtmlReply::HtmlChunk {
        id,
        offset,
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}
#[test]
fn boundaries_and_decline_finish_without_receiving() {
    for size in [99_999, 100_000] {
        let mut d = document(size);
        assert!(d.notices.is_empty());
        assert!(d.credit(id()).is_some());
        assert!(d.credit(id()).is_none());
    }
    let mut d = document(100_001);
    assert_eq!(d.notices.len(), 1);
    assert!(d.credit(id()).is_none());
    d.decline(id()).unwrap();
    assert_eq!(d.skipped, 1);
    assert!(!d.receiving());
    assert!(d.decline(id()).is_none());
}
#[test]
fn large_document_hash_and_script_body_reuse() {
    let bytes: Vec<_> = (0..1_500_000).map(|i| (i % 251) as u8).collect();
    let mut d = document(bytes.len() as u64);
    assert_eq!(d.allow_next(), Some(id()));
    for (n, bytes) in bytes.chunks(CHUNK_BYTES).enumerate() {
        assert!(d.credit(id()).is_some());
        assert!(d.receive(chunk(id(), (n * CHUNK_BYTES) as u64, bytes)));
    }
    d.receive(HtmlReply::HtmlComplete {
        id: id(),
        size: bytes.len() as u64,
    });
    assert_eq!(d.resources[&id()].body(), Some(bytes.as_slice()));
    assert!(d.credit(id()).is_none());
    assert_eq!(d.received, 1_500_000);
    assert!(d.reset(1, 9, 2).is_empty());
    assert_eq!(d.received, 0);
    assert!(d.resources.is_empty());
}
#[test]
fn duplicate_missing_and_foreign_id_cannot_complete() {
    let mut d = document(6);
    d.credit(id()).unwrap();
    d.receive(chunk(id(), 0, b"abc"));
    assert!(d.receive(chunk(id(), 0, b"abc")));
    assert_eq!(d.resources[&id()].state, ReceiveState::Failed);
    assert!(d.resources[&id()].body().is_none());
    let mut d = document(6);
    d.credit(id()).unwrap();
    d.receive(chunk(id(), 3, b"abc"));
    assert_eq!(d.resources[&id()].state, ReceiveState::Failed);
    for field in 0..6 {
        let mut d = document(3);
        d.credit(id()).unwrap();
        let mut wrong = id();
        match field {
            0 => wrong.epoch += 1,
            1 => wrong.surface_id += 1,
            2 => wrong.binding += 1,
            3 => wrong.document += 1,
            4 => wrong.request += 1,
            _ => wrong.transfer += 1,
        };
        assert!(!d.receive(chunk(wrong, 0, b"abc")));
        assert_eq!(d.received, 0);
    }
}
#[test]
fn cumulative_reserves_outstanding_credit_and_stop_cancels() {
    let mut d = HtmlDocument::new(1, 7, 9, 1);
    d.received = DOCUMENT_BYTES - CHUNK_BYTES as u64;
    let one = id();
    let two = TransferId {
        request: 2,
        transfer: 2,
        ..one
    };
    for i in [one, two] {
        d.request(i).unwrap();
        d.receive(metadata(i, CHUNK_BYTES as u64));
    }
    assert!(d.credit(one).is_some());
    assert!(d.credit(two).is_none());
    assert!(d.paused);
    d.continue_receiving();
    assert!(d.credit(two).is_some());
    assert_eq!(d.stop().len(), 2);
    assert!(!d.receiving());
    assert!(d.resources.values().all(|r| r.body.is_empty()));
}
#[test]
fn disconnect_reset_discards_late_complete_and_old_capabilities() {
    let mut d = document(0);
    d.reset(2, 10, 1);
    assert!(!d.receive(HtmlReply::HtmlComplete { id: id(), size: 0 }));
    assert!(!HtmlSession::from_descriptor(&serde_json::json!({})).server_transfer);
}

#[test]
fn timeout_during_size_question_retires_allowance_until_a_fresh_request() {
    let mut d = document(300_000);
    assert_eq!(d.resources[&id()].state, ReceiveState::SizeNotice);
    assert!(d.receive(HtmlReply::HtmlFailure {
        id: id(),
        reason: "credit timeout".into()
    }));
    assert!(!d.receiving());
    assert!(d.notices.is_empty());
    assert!(d.allow_next().is_none());
    assert!(d.credit(id()).is_none());
    assert!(d.request(id()).is_none());
    d.reset(1, 9, 2);
    let retry = TransferId {
        document: 2,
        request: 2,
        transfer: 2,
        ..id()
    };
    assert!(d.request(retry).is_some());
    d.receive(metadata(retry, 300_000));
    assert_eq!(d.allow_next(), Some(retry));
    assert!(d.credit(retry).is_some());
    assert!(!d.receive(HtmlReply::HtmlCancelled { id: id() }));
}
