//! Real bridge actions keep response Allow, aggregate Continue and worker publication separate.
use super::{tests::*, *};
use base64::Engine;
use sha2::{Digest, Sha256};
use tasty_ipc::html_http::{BodyIdentity, HttpRequest, ResponseHead};
fn response(bridge: &HttpBridge, number: u64, length: usize) -> ResponseHead {
    let head = ResponseHead {
        id: ResponseId {
            request: id(number),
            source_generation: 9,
            serial: 1,
        },
        logical_url: "http://remote.example/doc".into(),
        status: 200,
        headers: vec![],
        encoded_length: Some(length as u64),
        has_body: true,
    };
    assert!(bridge.receive(HttpReply::Head { head: head.clone() }));
    head
}
fn admitted(
    bridge: &HttpBridge,
    rx: &std::sync::mpsc::Receiver<crate::connection::QueuedFrame>,
    number: &mut u64,
) {
    drive(bridge, number);
    assert!(matches!(command(rx), HttpRequest::Begin { .. }));
    assert!(bridge.receive(HttpReply::UploadReady {
        id: id(*number),
        source_generation: 9,
        offset: 0,
        max_bytes: 1
    }));
    drive(bridge, number);
    assert!(matches!(command(rx), HttpRequest::Commit { .. }));
}
#[tokio::test]
async fn continue_never_grants_response_allow_and_decline_resolves_receiving_without_replay() {
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    response(&bridge, 1, 100_001);
    assert_eq!(bridge.snapshot().notices, vec![id(1)]);
    assert!(bridge.continue_receiving());
    drive(&bridge, &mut number);
    assert!(rx.try_recv().is_err());
    assert_eq!(bridge.snapshot().notices, vec![id(1)]);
    assert!(bridge.decline(id(1)));
    assert!(matches!(command(&rx),HttpRequest::Cancel {id:sent,..}if sent==id(1)));
    assert!(!bridge.snapshot().receiving);
    drive(&bridge, &mut number);
    assert!(matches!(wait.await, Err(BridgeError::Declined)));
    assert!(!bridge.allow(id(1)));
    assert_eq!(number, 1);
    assert!(rx.try_recv().is_err());
    bridge.close();
}
fn chunks_until_pause(
    bridge: &HttpBridge,
    rx: &std::sync::mpsc::Receiver<crate::connection::QueuedFrame>,
    head: &ResponseHead,
    bytes: &[u8],
    number: &mut u64,
    at: &mut usize,
) {
    while *at < bytes.len() {
        drive(bridge, number);
        if bridge.snapshot().paused {
            break;
        }
        let HttpRequest::Credit {
            response,
            offset,
            max_bytes,
            allow_large,
        } = command(rx)
        else {
            panic!("one credit")
        };
        assert_eq!(response, head.id);
        assert_eq!(offset, *at as u64);
        assert!(allow_large);
        let end = (*at + max_bytes as usize).min(bytes.len());
        assert!(bridge.receive(HttpReply::Chunk {
            response,
            offset,
            data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes[*at..end])
        }));
        *at = end;
    }
}
#[tokio::test]
async fn actual_encoded_aggregate_pauses_before_crossing_and_continue_verifies_before_native_body()
{
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let mut wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    let bytes = vec![b'x'; 2_000_001];
    let head = response(&bridge, 1, bytes.len());
    assert!(bridge.allow(id(1)));
    let mut at = 0;
    chunks_until_pause(&bridge, &rx, &head, &bytes, &mut number, &mut at);
    assert!(bridge.snapshot().paused);
    assert!(at <= 2_000_000);
    pending(&mut wait);
    assert!(!bridge.allow(id(1)));
    assert!(bridge.continue_receiving());
    chunks_until_pause(&bridge, &rx, &head, &bytes, &mut number, &mut at);
    assert_eq!(at, bytes.len());
    let identity = BodyIdentity {
        bytes: at as u64,
        sha256: Sha256::digest(&bytes).into(),
    };
    assert!(bridge.receive(HttpReply::Complete {
        response: head.id,
        encoded: identity,
        decoded: identity
    }));
    pending(&mut wait);
    drive(&bridge, &mut number);
    let response = wait.await.unwrap();
    assert_eq!(response.body.encoded(), bytes.as_slice());
    assert_eq!(response.body.decoded_identity(), identity);
    assert_eq!(bridge.snapshot().received, 2_000_001);
    assert_eq!(number, 1);
    drop(response);
    bridge.close();
}
#[tokio::test]
async fn rejected_verifier_dispatch_keeps_owned_job_bounded_and_close_retires_without_publication()
{
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let mut wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    complete(&bridge, id(1));
    let attempts = std::sync::atomic::AtomicUsize::new(0);
    for _ in 0..3 {
        bridge.drive(
            || panic!("no new ID on worker retry"),
            |job| {
                attempts.fetch_add(1, Ordering::AcqRel);
                Err(job)
            },
        );
        pending(&mut wait);
        assert_eq!(bridge.lock().unwrap().jobs.len(), 1);
    }
    assert_eq!(attempts.load(Ordering::Acquire), 3);
    bridge.close();
    assert!(bridge.lock().unwrap().jobs.is_empty());
    assert!(matches!(
        wait.await,
        Err(BridgeError::Closed | BridgeError::Stale)
    ));
    assert!(!bridge.snapshot().receiving);
}

#[tokio::test]
async fn worker_handoff_after_close_drops_the_actual_job_instead_of_repopulating_retired_queue() {
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    complete(&bridge, id(1));
    let mut held = None;
    bridge.drive(
        || panic!("no new ID"),
        |job| {
            held = Some(job);
            Ok(())
        },
    );
    let job = held.expect("actual model verification job");
    // Model the handoff gap: close drains the queue before rejected work is stored again.
    bridge.close();
    assert!(bridge.config.pending.used() > 0);
    assert!(!bridge.retain_pending(vec![job]));
    assert!(bridge.lock().unwrap().jobs.is_empty());
    assert_eq!(bridge.config.pending.used(), 0);
    assert!(matches!(
        wait.await,
        Err(BridgeError::Closed | BridgeError::Stale)
    ));
}

#[tokio::test]
async fn cancelled_unstarted_verifier_is_dropped_without_retrying_the_pool() {
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    complete(&bridge, id(1));
    bridge.drive(|| panic!("no ID"), Err);
    assert_eq!(bridge.lock().unwrap().jobs.len(), 1);
    drop(wait);
    bridge.drive(
        || panic!("no replay"),
        |_| panic!("cancelled job dispatched"),
    );
    assert!(bridge.lock().unwrap().jobs.is_empty());
    assert!(!bridge.snapshot().receiving);
    bridge.close();
}
#[tokio::test]
async fn foreign_response_identity_and_corrupt_complete_never_publish_or_retry() {
    let (bridge, rx) = bridge();
    let (request, _budget) = native(&[]).await;
    let mut wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    let mut wrong = ResponseHead {
        id: ResponseId {
            request: id(1),
            source_generation: 10,
            serial: 1,
        },
        logical_url: "http://remote.example/doc".into(),
        status: 200,
        headers: vec![],
        encoded_length: Some(0),
        has_body: true,
    };
    assert!(!bridge.receive(HttpReply::Head {
        head: wrong.clone()
    }));
    wrong.id.source_generation = 9;
    wrong.id.request = id(2);
    assert!(!bridge.receive(HttpReply::Head {
        head: wrong.clone()
    }));
    pending(&mut wait);
    wrong.id.request = id(1);
    assert!(bridge.receive(HttpReply::Head {
        head: wrong.clone()
    }));
    let bad = BodyIdentity {
        bytes: 0,
        sha256: [0; 32],
    };
    assert!(bridge.receive(HttpReply::Complete {
        response: wrong.id,
        encoded: bad,
        decoded: bad
    }));
    drive(&bridge, &mut number);
    assert!(matches!(wait.await, Err(BridgeError::Failed)));
    assert_eq!(number, 1);
    assert!(rx.try_recv().is_err());
    bridge.close();
}
#[tokio::test]
async fn native_waker_can_reenter_close_and_prevents_late_verified_publication() {
    use std::{
        future::Future,
        pin::Pin,
        sync::{Weak, atomic::AtomicUsize},
        task::{Context, Poll, Wake, Waker},
    };
    struct Close {
        bridge: Weak<HttpBridge>,
        calls: AtomicUsize,
    }
    impl Wake for Close {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.calls.fetch_add(1, Ordering::AcqRel);
            if let Some(bridge) = self.bridge.upgrade() {
                bridge.close();
            }
        }
    }
    let (bridge, rx) = bridge();
    let (request, budget) = native(&[]).await;
    let mut wait = bridge.submit(request).unwrap();
    let mut number = 0;
    admitted(&bridge, &rx, &mut number);
    let callback = Arc::new(Close {
        bridge: Arc::downgrade(&bridge),
        calls: AtomicUsize::new(0),
    });
    let waker = Waker::from(callback.clone());
    assert!(matches!(
        Pin::new(&mut wait).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    complete(&bridge, id(1));
    drive(&bridge, &mut number);
    assert_eq!(callback.calls.load(Ordering::Acquire), 1);
    assert!(matches!(
        wait.await,
        Err(BridgeError::Closed | BridgeError::Stale)
    ));
    assert!(!bridge.snapshot().receiving);
    drop(bridge);
    assert_eq!(budget.used(), 0);
}
