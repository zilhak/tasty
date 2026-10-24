use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use tasty_ipc::html_http::{BodyIdentity, CommandKind, FailureCode};
pub(super) fn id(n: u64) -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 1,
        binding: 1,
        document: 1,
        request: n,
        transfer: n,
    }
}
pub(super) fn identity(b: &[u8]) -> BodyIdentity {
    BodyIdentity {
        bytes: b.len() as u64,
        sha256: Sha256::digest(b).into(),
    }
}
pub(super) fn begin(n: u64, b: &[u8]) -> HttpRequest {
    HttpRequest::Begin {
        id: id(n),
        source_generation: 1,
        method: "POST".into(),
        logical_url: "http://example.test/".into(),
        headers: vec![],
        upload: identity(b),
        allow_remote_content: false,
    }
}
pub(super) fn model() -> ClientDocument {
    model_with(PendingBudget::new(32_000_000), BodyBudget::new(32_000_000))
}
pub(super) fn model_with(p: PendingBudget, b: BodyBudget) -> ClientDocument {
    let (sender, _queue) = crate::connection::channel();
    model_on(sender.epoch(), p, b)
}
pub(super) fn model_on(epoch: ConnectionEpoch, p: PendingBudget, b: BodyBudget) -> ClientDocument {
    ClientDocument::new(
        DocumentKey {
            entry: id(1),
            source_generation: 1,
            entry_url: "http://example.test/".into(),
            allow_remote_content: false,
        },
        epoch,
        ClientLimits {
            requests: 16,
            upload_bytes: 100_000,
            encoded_bytes: 3_000_000,
            decoded_bytes: 3_000_000,
            verify_jobs: 2,
        },
        p,
        b,
        Arc::new(|| {}),
    )
    .unwrap()
}
pub(super) fn ready(d: &mut ClientDocument, n: u64, offset: u64, max_bytes: u32) {
    assert!(d.receive(HttpReply::UploadReady {
        id: id(n),
        source_generation: 1,
        offset,
        max_bytes
    }));
}
pub(super) fn admitted(d: &mut ClientDocument, n: u64) {
    assert!(matches!(
        d.admit(begin(n, b""), Arc::new(vec![])),
        Ok(HttpRequest::Begin { .. })
    ));
    ready(d, n, 0, 49_152);
    assert!(matches!(d.next_command(), Some(HttpRequest::Commit { .. })));
}
pub(super) fn response(n: u64) -> ResponseId {
    ResponseId {
        request: id(n),
        source_generation: 1,
        serial: n,
    }
}
pub(super) fn head(n: u64, len: Option<u64>) -> ResponseHead {
    ResponseHead {
        id: response(n),
        logical_url: "http://example.test/".into(),
        status: 200,
        headers: vec![],
        encoded_length: len,
        has_body: true,
    }
}
pub(super) fn start(d: &mut ClientDocument, n: u64, len: Option<u64>) {
    admitted(d, n);
    assert!(d.receive(HttpReply::Head { head: head(n, len) }));
}
pub(super) fn credit(d: &mut ClientDocument) -> (ResponseId, u64, u32, bool) {
    let Some(HttpRequest::Credit {
        response,
        offset,
        max_bytes,
        allow_large,
    }) = d.next_command()
    else {
        panic!("credit missing: {:?}", d.snapshot());
    };
    (response, offset, max_bytes, allow_large)
}
pub(super) fn chunk(d: &mut ClientDocument, r: ResponseId, offset: u64, b: &[u8]) {
    assert!(d.receive(HttpReply::Chunk {
        response: r,
        offset,
        data_b64: STANDARD.encode(b)
    }));
}
pub(super) fn complete(d: &mut ClientDocument, n: u64, b: &[u8]) {
    assert!(d.receive(HttpReply::Complete {
        response: response(n),
        encoded: identity(b),
        decoded: identity(b)
    }));
}
pub(super) fn verified_body(d: &mut ClientDocument, n: u64, b: &[u8]) {
    start(d, n, Some(b.len() as u64));
    if b.len() > 100_000 {
        assert!(d.allow(id(n)));
    }
    let mut offset = 0;
    while offset < b.len() {
        let (r, o, max, _) = credit(d);
        assert_eq!(o, offset as u64);
        let end = (offset + max as usize).min(b.len());
        chunk(d, r, o, &b[offset..end]);
        offset = end;
    }
    complete(d, n, b);
    d.take_verify_job().unwrap().run();
    assert!(d.poll_verification());
    assert!(d.verified(id(n)).is_some());
}
#[test]
pub(super) fn upload_windows_hash_empty_and_one_commit() {
    let mut d = model();
    let b = b"abcdef";
    assert_eq!(
        d.admit(begin(1, b"bad"), Arc::new(b.to_vec())).err(),
        Some(ClientError::Invalid)
    );
    d.admit(begin(1, b), Arc::new(b.to_vec()))
        .unwrap_or_else(|_| panic!("admit"));
    assert!(d.next_command().is_none());
    ready(&mut d, 1, 0, 3);
    for off in [0, 3] {
        let Some(HttpRequest::Upload {
            offset, data_b64, ..
        }) = d.next_command()
        else {
            panic!()
        };
        assert_eq!(offset, off);
        assert_eq!(
            STANDARD.decode(data_b64).unwrap(),
            b[off as usize..off as usize + 3]
        );
        assert!(d.next_command().is_none());
        ready(&mut d, 1, off + 3, 3);
    }
    assert!(matches!(d.next_command(), Some(HttpRequest::Commit { .. })));
    assert!(d.next_command().is_none());
    admitted(&mut d, 2);
    assert!(d.next_command().is_none());
}
#[test]
pub(super) fn failed_admission_advances_neither_counter_and_never_allocates() {
    let mut d = model();
    admitted(&mut d, 1);
    let mut req = begin(2, b"");
    if let HttpRequest::Begin { id, .. } = &mut req {
        id.transfer = 1;
    }
    assert_eq!(
        d.admit(req, Arc::new(vec![])).err(),
        Some(ClientError::AlreadyUsed)
    );
    admitted(&mut d, 2);
    assert_eq!(
        d.admit(begin(2, b""), Arc::new(vec![])).err(),
        Some(ClientError::AlreadyUsed)
    );
}
#[test]
pub(super) fn full_identity_source_serial_epoch_and_tombstones() {
    let mut d = model();
    start(&mut d, 1, Some(1));
    let (r, o, _, _) = credit(&mut d);
    for field in 0..8 {
        let mut wrong = r;
        match field {
            0 => wrong.request.epoch += 1,
            1 => wrong.request.surface_id += 1,
            2 => wrong.request.binding += 1,
            3 => wrong.request.document += 1,
            4 => wrong.request.request += 1,
            5 => wrong.request.transfer += 1,
            6 => wrong.source_generation += 1,
            _ => wrong.serial += 1,
        };
        assert!(!d.receive(HttpReply::Chunk {
            response: wrong,
            offset: o,
            data_b64: STANDARD.encode(b"x")
        }));
    }
    assert_eq!(d.snapshot().reserved, 1);
    chunk(&mut d, r, o, b"x");
    complete(&mut d, 1, b"x");
    assert!(!d.receive(HttpReply::Complete {
        response: r,
        encoded: identity(b"x"),
        decoded: identity(b"x")
    }));
    d.take_verify_job().unwrap().run();
    d.poll_verification();
    assert!(!d.receive(HttpReply::Complete {
        response: r,
        encoded: identity(b"x"),
        decoded: identity(b"x")
    }));
    d.epoch.retire();
    assert!(d.verified(id(1)).is_none());
    assert!(!d.receive(HttpReply::Head {
        head: head(1, Some(1))
    }));
    d.poll_verification();
    assert_eq!(d.snapshot().reserved, 0);
}
#[test]
pub(super) fn short_chunk_releases_entire_window_and_one_credit_only() {
    let mut d = model();
    start(&mut d, 1, None);
    let (r, o, max, _) = credit(&mut d);
    assert_eq!(max, 49_152);
    assert!(d.next_command().is_none());
    chunk(&mut d, r, o, b"x");
    assert_eq!((d.snapshot().received, d.snapshot().reserved), (1, 0));
    complete(&mut d, 1, b"x");
    assert!(!d.receive(HttpReply::Complete {
        response: r,
        encoded: identity(b"x"),
        decoded: identity(b"x")
    }));
    d.take_verify_job().unwrap().run();
    assert!(d.poll_verification());
}
#[test]
pub(super) fn command_rejections_preserve_credit_body_and_post_then_failure_is_terminal() {
    let mut d = model();
    start(&mut d, 1, Some(3));
    let (r, o, _, _) = credit(&mut d);
    for command in [CommandKind::Begin, CommandKind::Credit, CommandKind::Commit] {
        assert!(!d.receive(HttpReply::CommandRejected {
            id: id(1),
            source_generation: 1,
            command,
            response: Some(r),
            code: FailureCode::Invalid,
            reason: "invalid".into()
        }));
        assert_eq!(d.snapshot().reserved, 3);
        assert!(d.next_command().is_none());
    }
    chunk(&mut d, r, o, b"abc");
    complete(&mut d, 1, b"abc");
    d.take_verify_job().unwrap().run();
    d.poll_verification();
    assert_eq!(d.verified(id(1)).unwrap().encoded(), b"abc");
    admitted(&mut d, 2);
    assert!(!d.receive(HttpReply::CommandRejected {
        id: id(2),
        source_generation: 1,
        command: CommandKind::Commit,
        response: None,
        code: FailureCode::Invalid,
        reason: "invalid".into()
    }));
    assert!(d.receive(HttpReply::Failure {
        id: id(2),
        source_generation: 1,
        response: None,
        code: FailureCode::Invalid,
        reason: "invalid".into()
    }));
    assert_eq!(d.state(id(2)), Some(ReceiveState::Failed));
    assert!(!d.receive(HttpReply::Head {
        head: head(2, None)
    }));
}
#[test]
pub(super) fn declined_inflight_chunk_is_counted_once_and_not_retained() {
    let mut d = model();
    start(&mut d, 1, Some(3));
    let (r, o, _, _) = credit(&mut d);
    assert!(d.decline(id(1)).is_some());
    assert_eq!(d.snapshot().reserved, 3);
    assert!(!d.snapshot().receiving);
    chunk(&mut d, r, o, b"abc");
    assert_eq!(
        (
            d.snapshot().received,
            d.snapshot().reserved,
            d.snapshot().skipped
        ),
        (3, 0, 1)
    );
    assert!(d.next_command().is_none());
    assert!(!d.receive(HttpReply::Cancelled {
        id: id(1),
        source_generation: 1,
        response: Some(r)
    }));
    assert!(!d.receive(HttpReply::Chunk {
        response: r,
        offset: 0,
        data_b64: STANDARD.encode(b"abc")
    }));
    assert_eq!(d.snapshot().received, 3);
}
