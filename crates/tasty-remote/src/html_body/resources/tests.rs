use super::*;
use crate::html_body::EntryBodyStore;
use crate::html_document::HtmlDocument;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use tasty_ipc::html_transfer::{HtmlMetadata, HtmlReply, HtmlSource};
fn poll(wait: &mut ResponseWait) -> Poll<Result<Arc<NativeBodyResponse>, ResourceFailure>> {
    Pin::new(wait).poll(&mut Context::from_waker(Waker::noop()))
}
fn entry() -> Arc<ReceivedEntry> {
    let id = TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: 5,
        transfer: 6,
    };
    let mut doc = HtmlDocument::new(id.epoch, id.surface_id, id.binding, id.document);
    doc.request(id).unwrap();
    doc.receive(HtmlReply::HtmlMetadata {
        id,
        metadata: HtmlMetadata {
            source: HtmlSource::File {
                path: "/remote/index.html".into(),
                url: "file:///remote/index.html".into(),
            },
            source_generation: 7,
            root: Some("/remote".into()),
            cwd: None,
            size: 0,
            regular: true,
            file: None,
        },
    });
    doc.credit(id).unwrap();
    doc.receive(HtmlReply::HtmlComplete { id, size: 0 });
    Arc::new(
        ReceivedEntry::from_complete(id, &doc.resources[&id])
            .unwrap()
            .unwrap(),
    )
}
fn uri(entry: &ReceivedEntry, suffix: &str) -> String {
    entry
        .native_uri()
        .strip_suffix("index.html")
        .unwrap()
        .to_owned()
        + suffix
}
#[test]
fn completed_response_is_shared_and_replayed_without_another_fetch() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    assert!(!broker.has_pending());
    let url = uri(&entry, "theme.css");
    let mut first = broker.request(&url);
    let mut second = broker.request(&url);
    assert!(broker.has_pending());
    assert!(poll(&mut first).is_pending());
    let jobs = broker.take_pending(4);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].relative, "theme.css");
    assert_eq!(jobs[0].source_generation, 7);
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..jobs[0].entry
    };
    assert!(broker.bind(jobs[0].ticket, id));
    assert!(broker.has_pending());
    let response = Arc::new(
        NativeBodyResponse::new(
            id,
            7,
            "text/css",
            "file:///remote/theme.css",
            Arc::new(b"body{}".to_vec()),
        )
        .unwrap(),
    );
    assert!(broker.publish(id, Arc::clone(&response)));
    assert!(!broker.has_pending());
    for wait in [&mut first, &mut second] {
        let Poll::Ready(Ok(body)) = poll(wait) else {
            panic!("missing completion");
        };
        assert!(Arc::ptr_eq(&body, &response));
    }
    let mut replay = broker.request(&(url.clone() + "#fragment"));
    let Poll::Ready(Ok(body)) = poll(&mut replay) else {
        panic!("missing replay");
    };
    assert!(Arc::ptr_eq(&body, &response));
    assert!(broker.take_pending(4).is_empty());
    assert!(Arc::ptr_eq(&broker.cached(&url).unwrap(), &response));
}
#[test]
fn routes_reject_other_owners_traversal_schemes_and_encoded_separators() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    for suffix in [
        "../outside",
        "%2e%2e/outside",
        "%2Fetc/passwd",
        "a%5cb.css",
        "a%00b",
        "a%GG",
        "a%ff",
    ] {
        let mut wait = broker.request(&uri(&entry, suffix));
        assert!(
            matches!(poll(&mut wait), Poll::Ready(Err(ResourceFailure::Denied))),
            "{suffix}"
        );
    }
    for url in [
        "file:///etc/passwd",
        "https://example.invalid/a",
        "tasty-html-entry://other/r5/t6/a",
    ] {
        let mut wait = broker.request(url);
        assert!(matches!(
            poll(&mut wait),
            Poll::Ready(Err(ResourceFailure::Denied))
        ));
    }
    assert!(broker.take_pending(4).is_empty());
    let _wait = broker.request(&uri(&entry, "a+b.css?v=1"));
    let job = broker.take_pending(1).pop().unwrap();
    assert_eq!(job.relative, "a+b.css");
    assert_eq!(job.query.as_deref(), Some("v=1"));
}
#[test]
fn native_cancellation_abandons_only_its_original_request() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    let wait = broker.request(&uri(&entry, "unused.css"));
    drop(wait);
    assert!(broker.take_pending(4).is_empty());
    let wait = broker.request(&uri(&entry, "active.css"));
    let job = broker.take_pending(1).pop().unwrap();
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..job.entry
    };
    assert!(broker.bind(job.ticket, id));
    drop(wait);
    assert_eq!(broker.abandoned(), vec![id]);
    assert!(broker.abandoned().is_empty());
}
#[test]
fn disconnect_finishes_waiters_and_rejects_new_or_late_work() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    let mut wait = broker.request(&uri(&entry, "active.css"));
    let job = broker.take_pending(1).pop().unwrap();
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..job.entry
    };
    assert!(broker.bind(job.ticket, id));
    broker.disconnect();
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Disconnected))
    ));
    let mut fresh = broker.request(&uri(&entry, "unseen.css"));
    assert!(matches!(
        poll(&mut fresh),
        Poll::Ready(Err(ResourceFailure::Denied))
    ));
    assert!(broker.take_pending(4).is_empty());
    let response = Arc::new(
        NativeBodyResponse::new(
            id,
            7,
            "text/css",
            "file:///remote/active.css",
            Arc::new(vec![]),
        )
        .unwrap(),
    );
    assert!(!broker.publish(id, response));
}
#[test]
fn new_store_generation_drops_old_native_completion_rights() {
    let entry = entry();
    let mut store = EntryBodyStore::default();
    store.enable_resources(true);
    store.begin(entry.response().generation());
    assert!(store.publish(Arc::clone(&entry)));
    let mut wait = store.request_resource(&uri(&entry, "pending.css"));
    assert!(poll(&mut wait).is_pending());
    let id = entry.response().generation();
    store.begin(TransferId {
        document: id.document + 1,
        request: id.request + 1,
        transfer: id.transfer + 1,
        ..id
    });
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Cancelled))
    ));
    assert!(store.response_for_uri(entry.native_uri()).is_none());
}
#[test]
fn disabled_capability_and_wrong_binding_never_start_a_resource() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), false);
    let mut wait = broker.request(&uri(&entry, "a.css"));
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Denied))
    ));
    broker.set_enabled(true);
    let _wait = broker.request(&uri(&entry, "a.css"));
    let job = broker.take_pending(1).pop().unwrap();
    assert!(!broker.bind(
        job.ticket,
        TransferId {
            epoch: 9,
            ..job.entry
        }
    ));
    broker.reject(job.ticket, ResourceFailure::Denied);
    assert!(broker.take_pending(4).is_empty());
}

#[test]
fn failed_resource_identity_cannot_be_rebound_to_another_native_request() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    let mut first = broker.request(&uri(&entry, "first.css"));
    let _second = broker.request(&uri(&entry, "second.css"));
    let jobs = broker.take_pending(2);
    assert_eq!(jobs.len(), 2);
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..jobs[0].entry
    };
    assert!(!broker.bind(jobs[0].ticket, jobs[0].entry));
    assert!(broker.bind(jobs[0].ticket, id));
    assert!(!broker.bind(jobs[1].ticket, id));
    assert!(broker.fail(id, ResourceFailure::Declined));
    assert!(matches!(
        poll(&mut first),
        Poll::Ready(Err(ResourceFailure::Declined))
    ));
    assert!(!broker.bind(jobs[1].ticket, id));
}
#[test]
fn mismatched_body_generation_ends_the_native_wait_without_publication() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    let mut wait = broker.request(&uri(&entry, "first.css"));
    let job = broker.take_pending(1).pop().unwrap();
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..job.entry
    };
    assert!(broker.bind(job.ticket, id));
    let bad = Arc::new(
        NativeBodyResponse::new(
            id,
            8,
            "text/css",
            "file:///remote/first.css",
            Arc::new(vec![]),
        )
        .unwrap(),
    );
    assert!(!broker.publish(id, bad));
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Denied))
    ));
    assert!(broker.cached(&uri(&entry, "first.css")).is_none());
}

#[test]
fn queued_abandonment_retires_without_admission_and_preserves_shared_live_sibling() {
    let entry = entry();
    let mut broker = NativeResources::new(Arc::clone(&entry), true);
    let abandoned_uri = uri(&entry, "never-admitted.css");
    let abandoned = broker.request(&abandoned_uri);
    let live_uri = uri(&entry, "live.css");
    let live = broker.request(&live_uri);
    let shared = broker.request(&live_uri);
    drop(abandoned);
    drop(shared);
    // Reaping must also work while the document pause prevents take_pending.
    // An unbound request has no protocol ID and must not invent a transport cancel.
    assert!(broker.abandoned().is_empty());
    let mut replay = broker.request(&abandoned_uri);
    assert!(matches!(
        poll(&mut replay),
        Poll::Ready(Err(ResourceFailure::Cancelled))
    ));
    let jobs = broker.take_pending(4);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].relative, "live.css");
    assert!(broker.has_pending());
    let id = TransferId {
        request: 8,
        transfer: 8,
        ..jobs[0].entry
    };
    assert!(broker.bind(jobs[0].ticket, id));
    drop(live);
    assert_eq!(broker.abandoned(), vec![id]);
    assert!(!broker.has_pending());
    assert!(broker.take_pending(4).is_empty());
}
