//! Actual codec quota owners, ordered sender/model/verifier; no Backend/App/native acceptance.
use super::*;
use crate::{
    connection::{QueuedFrame, channel},
    html_http_native::{self, Destination, NativeBudget, NativeLimits},
};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    pin::Pin,
    sync::atomic::AtomicUsize,
    task::{Context, Poll, Waker},
    time::Duration,
};
use tasty_ipc::html_http::{BodyIdentity, CommandKind, FailureCode, HttpRequest, ResponseHead};
use tokio::io::AsyncWriteExt;
pub(super) fn bridge() -> (Arc<HttpBridge>, std::sync::mpsc::Receiver<QueuedFrame>) {
    let (sender, rx) = channel();
    let bridge = HttpBridge::new(
        BridgeConfig {
            scope: BridgeScope {
                epoch: 1,
                surface_id: 42,
                binding: 7,
                document: 2,
                source_generation: 9,
                entry_url: "http://remote.example/doc".into(),
                allow_remote_content: false,
            },
            limits: ClientLimits {
                requests: 256,
                upload_bytes: 1_000_000,
                encoded_bytes: 4_000_000,
                decoded_bytes: 4_000_000,
                verify_jobs: 2,
            },
            pending: PendingBudget::new(8_000_000),
            bodies: BodyBudget::new(8_000_000),
        },
        sender,
        Arc::new(|| true),
        Arc::new(|| {}),
    )
    .unwrap_or_else(|_| panic!("bridge"));
    (bridge, rx)
}
pub(super) async fn native(bytes: &[u8]) -> (Arc<NativeRequest>, NativeBudget) {
    let budget = NativeBudget::new(4_000_000);
    let (mut browser, mut socket) = tokio::io::duplex(100_000);
    browser.write_all(format!("POST /doc HTTP/1.1\r\nHost: remote.example\r\nCookie: native=kept\r\nContent-Length: {}\r\n\r\n",bytes.len()).as_bytes()).await.unwrap();
    browser.write_all(bytes).await.unwrap();
    let request = html_http_native::read_request(
        &mut socket,
        &Destination::new("http", "remote.example", 80).unwrap(),
        NativeLimits {
            upload_bytes: 1_000_000,
            io_timeout: Duration::from_secs(1),
        },
        &budget,
        &|| true,
    )
    .await
    .unwrap();
    (Arc::new(request), budget)
}
pub(super) fn id(counter: u64) -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 42,
        binding: 7,
        document: 2,
        request: counter,
        transfer: counter,
    }
}
pub(super) fn command(rx: &std::sync::mpsc::Receiver<QueuedFrame>) -> HttpRequest {
    serde_json::from_slice(&rx.try_recv().unwrap().frame.payload).unwrap()
}
pub(super) fn pending(wait: &mut NativeWait) {
    assert!(matches!(
        Pin::new(wait).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}
pub(super) fn empty() -> BodyIdentity {
    BodyIdentity {
        bytes: 0,
        sha256: Sha256::digest([]).into(),
    }
}
pub(super) fn complete(bridge: &HttpBridge, id: TransferId) -> ResponseHead {
    let head = ResponseHead {
        id: ResponseId {
            request: id,
            source_generation: 9,
            serial: 1,
        },
        logical_url: "http://remote.example/doc".into(),
        status: 200,
        headers: vec![],
        encoded_length: Some(0),
        has_body: true,
    };
    assert!(bridge.receive(HttpReply::Head { head: head.clone() }));
    assert!(bridge.receive(HttpReply::Complete {
        response: head.id,
        encoded: empty(),
        decoded: empty()
    }));
    head
}
pub(super) fn drive(bridge: &HttpBridge, number: &mut u64) -> bool {
    bridge.drive(
        || {
            *number += 1;
            Some(id(*number))
        },
        |job| {
            job.run();
            Ok(())
        },
    )
}
#[tokio::test]
async fn actual_native_post_zero_one_and_window_plus_one_allocates_at_ordered_send_and_never_replays()
 {
    for bytes in [vec![], vec![255], vec![7; 49_153]] {
        let (bridge, rx) = bridge();
        let (request, budget) = native(&bytes).await;
        let mut wait = bridge.submit(request.clone()).unwrap();
        assert!(bridge.document().is_none());
        assert!(rx.try_recv().is_err());
        pending(&mut wait);
        let mut number = 8;
        assert!(drive(&bridge, &mut number));
        assert_eq!(number, 9);
        assert_eq!(bridge.document().unwrap().entry(), id(9));
        let HttpRequest::Begin {
            id: request_id,
            upload,
            headers,
            ..
        } = command(&rx)
        else {
            panic!("one begin")
        };
        assert_eq!(request_id, id(9));
        assert_eq!(upload.bytes, bytes.len() as u64);
        assert!(
            headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("cookie") && h.value == b"native=kept")
        );
        let mut offset = 0;
        loop {
            assert!(bridge.receive(HttpReply::UploadReady {
                id: request_id,
                source_generation: 9,
                offset,
                max_bytes: 49_152
            }));
            drive(&bridge, &mut number);
            if let Ok(frame) = rx.try_recv() {
                let sent: HttpRequest = serde_json::from_slice(&frame.frame.payload).unwrap();
                match sent {
                    HttpRequest::Upload {
                        offset: at,
                        data_b64,
                        ..
                    } => {
                        assert_eq!(at, offset);
                        offset +=
                            tasty_ipc::html_http::decode_chunk(&data_b64).unwrap().len() as u64;
                        if offset == bytes.len() as u64 {
                            assert!(
                                matches!(command(&rx),HttpRequest::Commit {id,..}if id==request_id)
                            );
                            break;
                        }
                    }
                    HttpRequest::Commit { id, .. } => {
                        assert!(bytes.is_empty());
                        assert_eq!(id, request_id);
                        break;
                    }
                    _ => panic!("upload or commit"),
                }
            } else {
                panic!("ordered upload command")
            }
        }
        let head = complete(&bridge, request_id);
        pending(&mut wait);
        drive(&bridge, &mut number);
        let response = wait.await.unwrap();
        assert_eq!(response.body.head().id, head.id);
        assert!(Arc::ptr_eq(&response.request, &request));
        assert_eq!(number, 9);
        assert!(rx.try_recv().is_err());
        drop(request);
        assert!(budget.used() > 0);
        drop(response);
        drive(&bridge, &mut number);
        bridge.close();
        drop(bridge);
        assert_eq!(budget.used(), 0);
    }
}
#[tokio::test]
async fn nonterminal_command_rejection_preserves_native_waiter_and_exact_reuse_returns_same_body() {
    let (bridge, rx) = bridge();
    let (request, budget) = native(&[]).await;
    let mut wait = bridge.submit(request.clone()).unwrap();
    let mut number = 0;
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx), HttpRequest::Begin { .. }));
    assert!(bridge.receive(HttpReply::UploadReady {
        id: id(1),
        source_generation: 9,
        offset: 0,
        max_bytes: 1
    }));
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx), HttpRequest::Commit { .. }));
    assert!(!bridge.receive(HttpReply::CommandRejected {
        id: id(1),
        source_generation: 9,
        command: CommandKind::Commit,
        response: None,
        code: FailureCode::InProgress,
        reason: "InProgress".into()
    }));
    drive(&bridge, &mut number);
    pending(&mut wait);
    assert!(rx.try_recv().is_err());
    let head = complete(&bridge, id(1));
    drive(&bridge, &mut number);
    let response = wait.await.unwrap();
    let original = response.body.clone();
    drop(response);
    drive(&bridge, &mut number);
    let mut reuse = bridge.reuse(head.id, request.clone()).unwrap();
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx),HttpRequest::Reuse {response}if response==head.id));
    pending(&mut reuse);
    assert!(bridge.receive(HttpReply::Head { head: head.clone() }));
    assert!(bridge.receive(HttpReply::Complete {
        response: head.id,
        encoded: empty(),
        decoded: empty()
    }));
    drive(&bridge, &mut number);
    let response = reuse.await.unwrap();
    assert!(Arc::ptr_eq(&response.body, &original));
    assert_eq!(number, 1);
    assert!(rx.try_recv().is_err());
    assert!(bridge.release(head.id));
    assert!(matches!(command(&rx), HttpRequest::Release { .. }));
    assert!(matches!(
        bridge.reuse(head.id, request.clone()),
        Err(BridgeError::Invalid)
    ));
    drop(response);
    drop(original);
    drop(request);
    bridge.close();
    drop(bridge);
    assert_eq!(budget.used(), 0);
}
#[tokio::test]
async fn dropped_native_waiter_cancels_exact_attempt_and_pre_drive_drop_consumes_no_id() {
    let (bridge, rx) = bridge();
    let (request, budget) = native(b"binary").await;
    let wait = bridge.submit(request.clone()).unwrap();
    drop(wait);
    let mut number = 0;
    drive(&bridge, &mut number);
    assert_eq!(number, 0);
    assert!(rx.try_recv().is_err());
    let wait = bridge.submit(request.clone()).unwrap();
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx),HttpRequest::Begin {id:sent,..}if sent==id(1)));
    drop(wait);
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx),HttpRequest::Cancel {id:sent,..}if sent==id(1)));
    drive(&bridge, &mut number);
    assert!(rx.try_recv().is_err());
    bridge.close();
    assert!(matches!(command(&rx),HttpRequest::Close {entry,..}if entry==id(1)));
    bridge.close();
    assert!(rx.try_recv().is_err());
    drop(request);
    drop(bridge);
    assert_eq!(budget.used(), 0);
}
#[tokio::test]
async fn waiter_capacity_counts_ready_native_response_until_actual_write_owner_drop() {
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let wait = bridge.submit(request.clone()).unwrap();
    let mut number = 0;
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx), HttpRequest::Begin { .. }));
    assert!(bridge.receive(HttpReply::UploadReady {
        id: id(1),
        source_generation: 9,
        offset: 0,
        max_bytes: 1
    }));
    drive(&bridge, &mut number);
    assert!(matches!(command(&rx), HttpRequest::Commit { .. }));
    complete(&bridge, id(1));
    drive(&bridge, &mut number);
    let response = wait.await.unwrap();
    let mut pending = Vec::new();
    for _ in 0..7 {
        pending.push(bridge.submit(request.clone()).unwrap());
    }
    assert!(matches!(
        bridge.submit(request.clone()),
        Err(BridgeError::Capacity)
    ));
    drop(response);
    drop(pending);
    drive(&bridge, &mut number);
    assert!(bridge.submit(request).is_ok());
    bridge.close();
}
#[tokio::test]
async fn first_foreign_url_and_stale_source_do_not_consume_protocol_ids_or_publish() {
    let (bridge, rx) = bridge();
    let (mut request, _budget) = native(&[]).await;
    Arc::get_mut(&mut request).unwrap().logical_url = "http://other.example/doc".into();
    let wait = bridge.submit(request).unwrap();
    let allocations = AtomicUsize::new(0);
    bridge.drive(
        || {
            allocations.fetch_add(1, Ordering::AcqRel);
            Some(id(1))
        },
        Err,
    );
    assert!(matches!(wait.await, Err(BridgeError::Invalid)));
    assert_eq!(allocations.load(Ordering::Acquire), 0);
    assert!(rx.try_recv().is_err());
    bridge.close();
    assert!(!bridge.snapshot().receiving);
}
