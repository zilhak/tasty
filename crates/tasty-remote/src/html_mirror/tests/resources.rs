use super::*;
use crate::html_body::{NativeBodyResponse, ResourceFailure, ResponseWait};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use tasty_ipc::html_transfer::{CHUNK_BYTES, DOCUMENT_BYTES, HtmlFileMetadata};
fn next(n: u64) -> TransferId {
    TransferId {
        request: n,
        transfer: n,
        ..id()
    }
}
fn css_metadata(id: TransferId, size: u64) -> HtmlReply {
    let mut reply = metadata(id, size);
    if let HtmlReply::HtmlMetadata { metadata, .. } = &mut reply {
        metadata.file = Some(HtmlFileMetadata {
            relative: "style.css".into(),
            mime: "text/css".into(),
        });
        metadata.source = HtmlSource::File {
            path: "/server/style.css".into(),
            url: "file:///server/style.css".into(),
        };
    }
    reply
}
fn poll(wait: &mut ResponseWait) -> Poll<Result<Arc<NativeBodyResponse>, ResourceFailure>> {
    Pin::new(wait).poll(&mut Context::from_waker(Waker::noop()))
}
fn ready() -> HtmlMirror {
    let mut mirror = completed(b"entry");
    scan(&mut mirror);
    mirror.enable_resources(true);
    mirror
}
fn uri(m: &HtmlMirror, path: &str) -> String {
    url::Url::parse(m.body_store().current().unwrap().native_uri())
        .unwrap()
        .join(path)
        .unwrap()
        .into()
}
fn request(m: &HtmlMirror, path: &str) -> ResponseWait {
    let target = uri(m, path);
    m.body_store().request_resource(&target)
}
fn start(m: &mut HtmlMirror, path: &str, n: u64) -> ResponseWait {
    let wait = request(m, path);
    let fetch = m.take_resource_fetches().pop().unwrap();
    assert!(m.bind_resource(fetch, next(n)).is_some());
    wait
}
#[test]
fn child_completion_reuses_exact_allocation_and_has_no_extra_credit_or_request() {
    let mut m = ready();
    let mut wait = start(&mut m, "style.css?q=1", 2);
    assert!(m.receive(css_metadata(next(2), 3)));
    assert_eq!(m.credit().unwrap().id(), Some(next(2)));
    m.receive(HtmlReply::HtmlChunk {
        id: next(2),
        offset: 0,
        data_b64: base64::engine::general_purpose::STANDARD.encode(b"css"),
    });
    assert!(poll(&mut wait).is_pending());
    m.receive(HtmlReply::HtmlComplete {
        id: next(2),
        size: 3,
    });
    let original = m.document.as_ref().unwrap().resources[&next(2)]
        .shared_body()
        .unwrap();
    assert!(m.poll_resources());
    let Poll::Ready(Ok(body)) = poll(&mut wait) else {
        panic!("resource not completed")
    };
    assert!(Arc::ptr_eq(&original, &body.shared_bytes()));
    assert_eq!(body.mime(), "text/css");
    let mut reused = request(&m, "style.css?q=1#anchor");
    let Poll::Ready(Ok(cached)) = poll(&mut reused) else {
        panic!("cache miss")
    };
    assert!(Arc::ptr_eq(&body, &cached));
    assert!(m.take_resource_fetches().is_empty());
    assert!(m.credit().is_none());
    assert_eq!(m.document.as_ref().unwrap().received, 8);
    assert!(m.failure.is_none());
}
#[test]
fn child_notice_fifo_decline_and_timeout_leave_entry_and_end_receiving() {
    let mut m = ready();
    let entry = Arc::clone(m.body_store().current().unwrap());
    let mut a = start(&mut m, "a.css", 2);
    let mut b = start(&mut m, "b.css", 3);
    m.receive(css_metadata(next(2), 300_000));
    m.receive(css_metadata(next(3), 300_000));
    assert!(!m.allow(next(3)));
    assert_eq!(m.decline(next(2)).unwrap().id(), Some(next(2)));
    assert!(matches!(
        poll(&mut a),
        Poll::Ready(Err(ResourceFailure::Declined))
    ));
    m.receive(HtmlReply::HtmlFailure {
        id: next(3),
        reason: "credit timeout".into(),
    });
    m.poll_resources();
    assert!(matches!(
        poll(&mut b),
        Poll::Ready(Err(ResourceFailure::Failed))
    ));
    assert!(!m.allow(next(3)));
    let document = m.document.as_ref().unwrap();
    assert_eq!(document.skipped, 2);
    assert!(!document.receiving());
    assert!(m.failure.is_none());
    assert!(Arc::ptr_eq(&entry, m.body_store().current().unwrap()));
}
#[test]
fn invalid_child_binding_or_source_generation_finishes_the_waiter() {
    let mut m = ready();
    let mut wait = request(&m, "bad.css");
    let fetch = m.take_resource_fetches().pop().unwrap();
    assert!(m.bind_resource(fetch, id()).is_none());
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Denied))
    ));
    let mut wait = start(&mut m, "stale.css", 2);
    let mut metadata = css_metadata(next(2), 3);
    if let HtmlReply::HtmlMetadata { metadata, .. } = &mut metadata {
        metadata.source_generation += 1;
    }
    m.receive(metadata);
    m.poll_resources();
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Failed))
    ));
    assert!(m.document.as_ref().unwrap().notices.is_empty());
    assert!(m.failure.is_none());
}
#[test]
fn main_navigation_keeps_original_root_handoff_and_explicit_reload_uses_fresh_root() {
    let mut m = ready();
    let target = uri(&m, "sub/next.html?q=1#anchor");
    let retired_uri = uri(&m, "retired.css");
    assert!(m.body_store().navigate(&target));
    assert!(m.body_store().current().is_none());
    let mut retired = m.body_store().request_resource(&retired_uri);
    assert!(matches!(
        poll(&mut retired),
        Poll::Ready(Err(ResourceFailure::Cancelled))
    ));
    assert!(m.poll_navigation());
    assert!(!m.receive(metadata(id(), 3)));
    let new = TransferId {
        document: 2,
        ..next(2)
    };
    let requests = m.begin(new);
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(&requests[0],HtmlRequest::HtmlNavigateRequest { id:actual,entry,relative,query,fragment,.. }
        if *actual==new && *entry==id() && relative=="sub/next.html" && query.as_deref()==Some("q=1") && fragment.as_deref()==Some("anchor"))
    );
    assert!(m.user_reload());
    let reload = TransferId {
        document: 3,
        ..next(3)
    };
    let requests = m.begin(reload);
    assert!(
        matches!(requests.last().unwrap(),HtmlRequest::HtmlFileEntryRequest { relative,.. } if relative=="sub/next.html")
    );
    assert_eq!(m.document.as_ref().unwrap().received, 0);
}
#[test]
fn round_robin_credit_reserves_document_limit_and_continue_does_not_approve_large_files() {
    let mut m = ready();
    let mut waits = Vec::new();
    let mut sequence = 2;
    let mut offsets = std::collections::HashMap::new();
    for n in 0..21 {
        waits.push(request(&m, &format!("{n}.bin")));
    }
    let mut continued = false;
    loop {
        for fetch in m.take_resource_fetches() {
            let id = next(sequence);
            sequence += 1;
            assert!(m.bind_resource(fetch, id).is_some());
            m.receive(css_metadata(id, 100_000));
            offsets.insert(id, 0usize);
        }
        if let Some(HtmlRequest::HtmlRead { id, offset, .. }) = m.credit() {
            assert_eq!(offset, offsets[&id] as u64);
            let count = (100_000 - offset as usize).min(CHUNK_BYTES);
            m.receive(HtmlReply::HtmlChunk {
                id,
                offset,
                data_b64: base64::engine::general_purpose::STANDARD.encode(vec![b'x'; count]),
            });
            *offsets.get_mut(&id).unwrap() += count;
            if offsets[&id] == 100_000 {
                m.receive(HtmlReply::HtmlComplete { id, size: 100_000 });
                m.poll_resources();
            }
        } else if m.document.as_ref().unwrap().paused && !continued {
            assert!(m.document.as_ref().unwrap().received <= DOCUMENT_BYTES);
            assert!(m.continue_receiving(id()));
            continued = true;
        } else {
            break;
        }
    }
    assert!(continued);
    assert_eq!(m.document.as_ref().unwrap().received, 2_100_005);
    // An oversized resource still requires its separate FIFO allowance after cumulative Continue.
    let mut large = start(&mut m, "large.css", sequence);
    m.receive(css_metadata(next(sequence), 300_000));
    assert!(!m.document.as_ref().unwrap().notices.is_empty());
    assert!(poll(&mut large).is_pending());
    let cancels = m.stop(id());
    assert!(!cancels.is_empty());
    m.poll_resources();
    assert!(matches!(
        poll(&mut large),
        Poll::Ready(Err(ResourceFailure::Stopped))
    ));
    assert!(m.body_store().current().is_some());
    assert!(!m.document.as_ref().unwrap().receiving());
}

#[test]
fn cumulative_pause_does_not_admit_followup_file_requests_until_continue() {
    let mut m = ready();
    let _active = start(&mut m, "limit.css", 2);
    m.receive(css_metadata(next(2), 10));
    m.document.as_mut().unwrap().received = DOCUMENT_BYTES;
    assert!(m.credit().is_none());
    assert!(m.document.as_ref().unwrap().paused);
    let _delayed = request(&m, "after-pause.css");
    assert!(m.take_resource_fetches().is_empty());
    assert!(m.continue_receiving(id()));
    let jobs = m.take_resource_fetches();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].relative, "after-pause.css");
}

#[test]
fn stop_keeps_completed_document_navigable_while_disconnect_refuses_new_documents() {
    let mut mirror = ready();
    let target = uri(&mirror, "next.html");
    assert!(mirror.stop(id()).is_empty());
    assert!(mirror.body_store().navigate(&target));
    assert!(mirror.poll_navigation());
    let new = TransferId {
        document: 2,
        ..next(2)
    };
    assert!(
        matches!(mirror.begin(new).last(),Some(HtmlRequest::HtmlNavigateRequest { entry,.. }) if *entry==id())
    );
    let mut mirror = ready();
    let target = uri(&mirror, "next.html");
    mirror.disconnect();
    assert!(!mirror.body_store().navigate(&target));
    assert!(!mirror.poll_navigation());
    assert!(mirror.body_store().current().is_some());
}
