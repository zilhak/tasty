//! Questions outlive the server's credit timeout; consent issues a fresh request.
use super::resources::{css_metadata, next, poll, ready, request, start};
use super::*;
use crate::html_body::ResourceFailure;
use std::task::Poll;
use tasty_ipc::html_transfer::{AUTO_BYTES, CHUNK_BYTES};

fn expire(m: &mut HtmlMirror, id: TransferId) {
    assert!(m.receive(HtmlReply::HtmlFailure {
        id,
        reason: "HTML credit timeout".into(),
    }));
}
fn chunk(id: TransferId, bytes: &[u8]) -> HtmlReply {
    HtmlReply::HtmlChunk {
        id,
        offset: 0,
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}
fn expired_entry() -> HtmlMirror {
    let mut m = mirror();
    m.begin(id());
    m.receive(metadata(id(), 300_000));
    expire(&mut m, id());
    m
}

#[test]
fn expired_entry_question_stays_and_allow_requests_again_with_consent() {
    let mut m = expired_entry();
    let d = m.document.as_ref().unwrap();
    assert_eq!(d.notices.front(), Some(&id()));
    assert!(d.receiving());
    assert_eq!(m.state(), Some(ReceiveState::SizeNotice));
    assert!(m.failure.is_none());
    assert!(m.credit().is_none());
    assert!(m.take_retries().is_empty());
    assert!(m.allow(id()));
    assert!(m.receiving());
    assert_eq!(m.take_retries(), vec![id()]);
    let fresh = next(2);
    let Some(HtmlRequest::HtmlEntryRequest { id: sent }) = m.bind_retry(id(), fresh) else {
        panic!("entry retry must reuse the entry request");
    };
    assert_eq!(sent, fresh);
    assert_eq!(m.id, Some(fresh));
    assert!(m.body_store().expects(fresh));
    assert!(m.take_retries().is_empty());
    assert!(m.receive(metadata(fresh, 300_000)));
    assert!(m.document.as_ref().unwrap().notices.is_empty());
    let Some(HtmlRequest::HtmlRead { id, allow, .. }) = m.credit() else {
        panic!("allowed retry must receive without asking again");
    };
    assert_eq!((id, allow), (fresh, true));
    assert!(!m.receive(HtmlReply::HtmlCancelled { id: super::id() }));
}

#[test]
fn changed_size_after_expiry_asks_again() {
    let mut m = expired_entry();
    assert!(m.allow(id()));
    m.bind_retry(id(), next(2)).unwrap();
    assert!(m.receive(metadata(next(2), 300_001)));
    assert_eq!(m.document.as_ref().unwrap().notices.front(), Some(&next(2)));
    assert!(m.credit().is_none());
    assert!(m.allow(next(2)));
    let Some(HtmlRequest::HtmlRead { allow, .. }) = m.credit() else {
        panic!("second consent must credit the fresh transfer");
    };
    assert!(allow);
}

#[test]
fn shrunk_size_after_expiry_receives_without_consent_flag() {
    let mut m = expired_entry();
    assert!(m.allow(id()));
    m.bind_retry(id(), next(2)).unwrap();
    assert!(m.receive(metadata(next(2), AUTO_BYTES)));
    let Some(HtmlRequest::HtmlRead { allow, .. }) = m.credit() else {
        panic!("small entry must be credited");
    };
    assert!(!allow);
}

#[test]
fn decline_after_expiry_skips_without_cancelling_a_finished_transfer() {
    let mut m = expired_entry();
    m.queue_decline(id());
    assert!(m.take_pending().is_empty());
    let d = m.document.as_ref().unwrap();
    assert_eq!(d.skipped, 1);
    assert!(!d.receiving());
    assert_eq!(m.state(), Some(ReceiveState::Skipped));
    assert!(m.take_retries().is_empty());
}

#[test]
fn nested_entry_retry_authorizes_the_same_target_again() {
    let mut m = ready();
    let target = url::Url::parse(m.body_store().current().unwrap().native_uri())
        .unwrap()
        .join("sub/next.html?q=1#frag")
        .unwrap();
    assert!(m.body_store().navigate(target.as_str()));
    assert!(m.poll_navigation());
    let nav = TransferId {
        document: 2,
        request: 2,
        transfer: 2,
        ..id()
    };
    assert!(matches!(
        m.begin(nav).last(),
        Some(HtmlRequest::HtmlNavigateRequest { .. })
    ));
    m.receive(metadata(nav, 300_000));
    expire(&mut m, nav);
    assert!(m.allow(nav));
    let fresh = TransferId {
        request: 3,
        transfer: 3,
        ..nav
    };
    let Some(HtmlRequest::HtmlFileEntryRequest {
        id,
        relative,
        query,
        fragment,
    }) = m.bind_retry(nav, fresh)
    else {
        panic!("nested retry must authorize the target from the current source");
    };
    assert_eq!(id, fresh);
    assert_eq!(relative, "sub/next.html");
    assert_eq!(query.as_deref(), Some("q=1"));
    assert_eq!(fragment.as_deref(), Some("frag"));
}

#[test]
fn expired_child_question_keeps_its_native_waiter_and_allow_delivers_the_body() {
    let mut m = ready();
    let mut wait = start(&mut m, "big.css?v=2", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    assert!(!m.poll_resources());
    assert!(poll(&mut wait).is_pending());
    assert_eq!(m.document.as_ref().unwrap().skipped, 0);
    assert!(m.receiving());
    assert!(m.allow(next(2)));
    assert_eq!(m.take_retries(), vec![next(2)]);
    let Some(HtmlRequest::HtmlResourceRequest {
        id,
        entry,
        source_generation,
        relative,
        query,
    }) = m.bind_retry(next(2), next(3))
    else {
        panic!("child retry must repeat its resource request");
    };
    assert_eq!((id, entry, source_generation), (next(3), super::id(), 1));
    assert_eq!(relative, "big.css");
    assert_eq!(query.as_deref(), Some("v=2"));
    assert!(m.receive(css_metadata(next(3), 300_000)));
    let Some(HtmlRequest::HtmlRead { allow, .. }) = m.credit() else {
        panic!("child retry must carry the earlier consent");
    };
    assert!(allow);
    let body = vec![b'a'; CHUNK_BYTES];
    assert!(m.receive(chunk(next(3), &body)));
    assert!(poll(&mut wait).is_pending());
    let mut offset = CHUNK_BYTES as u64;
    while offset < 300_000 {
        m.credit().unwrap();
        let len = (300_000 - offset).min(CHUNK_BYTES as u64) as usize;
        assert!(m.receive(HtmlReply::HtmlChunk {
            id: next(3),
            offset,
            data_b64: base64::engine::general_purpose::STANDARD.encode(vec![b'a'; len]),
        }));
        offset += len as u64;
    }
    assert!(m.receive(HtmlReply::HtmlComplete {
        id: next(3),
        size: 300_000
    }));
    assert!(m.poll_resources());
    let Poll::Ready(Ok(body)) = poll(&mut wait) else {
        panic!("the original waiter must receive the retried body");
    };
    assert_eq!(body.bytes().len(), 300_000);
    assert_eq!(m.document.as_ref().unwrap().skipped, 0);
}

#[test]
fn expired_child_decline_finishes_the_waiter_without_cancel() {
    let mut m = ready();
    let mut wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    m.queue_decline(next(2));
    assert!(m.take_pending().is_empty());
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Declined))
    ));
    assert_eq!(m.document.as_ref().unwrap().skipped, 1);
    assert!(!m.receiving());
}

#[test]
fn abandoned_waiter_ends_an_answered_retry_as_skipped() {
    let mut m = ready();
    let wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    assert!(m.allow(next(2)));
    drop(wait);
    assert!(m.bind_retry(next(2), next(3)).is_none());
    assert!(m.take_retries().is_empty());
    assert_eq!(m.document.as_ref().unwrap().skipped, 1);
    assert!(!m.receiving());
}

#[test]
fn files_expired_during_pause_are_requested_again_on_continue() {
    let mut m = ready();
    let mut wait = start(&mut m, "small.css", 2);
    m.document.as_mut().unwrap().paused = true;
    assert!(m.receive(css_metadata(next(2), 3)));
    assert!(m.credit().is_none());
    expire(&mut m, next(2));
    assert!(!m.poll_resources());
    assert!(poll(&mut wait).is_pending());
    assert!(m.take_retries().is_empty());
    assert!(m.continue_receiving(super::id()));
    assert_eq!(m.take_retries(), vec![next(2)]);
    assert!(matches!(
        m.bind_retry(next(2), next(3)),
        Some(HtmlRequest::HtmlResourceRequest { .. })
    ));
    assert!(m.receive(css_metadata(next(3), 3)));
    m.credit().unwrap();
    assert!(m.receive(chunk(next(3), b"css")));
    assert!(m.receive(HtmlReply::HtmlComplete {
        id: next(3),
        size: 3
    }));
    assert!(m.poll_resources());
    assert!(matches!(poll(&mut wait), Poll::Ready(Ok(_))));
}

#[test]
fn partial_body_expired_during_pause_is_not_counted_twice() {
    let mut m = ready();
    let _wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    assert!(m.allow(next(2)));
    m.credit().unwrap();
    assert!(m.receive(chunk(next(2), &vec![b'a'; CHUNK_BYTES])));
    let before = m.document.as_ref().unwrap().received;
    m.document.as_mut().unwrap().paused = true;
    expire(&mut m, next(2));
    assert_eq!(
        m.document.as_ref().unwrap().received,
        before - CHUNK_BYTES as u64
    );
    assert!(m.continue_receiving(super::id()));
    m.bind_retry(next(2), next(3)).unwrap();
    assert!(m.receive(css_metadata(next(3), 300_000)));
    let Some(HtmlRequest::HtmlRead { offset, allow, .. }) = m.credit() else {
        panic!("consented file must restart without asking");
    };
    assert_eq!((offset, allow), (0, true));
}

#[test]
fn reload_and_stop_end_expired_waiters_instead_of_leaking_them() {
    let mut m = ready();
    let mut a = start(&mut m, "a.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    let d = m.document.as_mut().unwrap();
    d.paused = true;
    m.queue_stop(super::id());
    assert!(m.take_pending().is_empty());
    assert!(matches!(poll(&mut a), Poll::Ready(Err(_))));
    assert!(m.take_retries().is_empty());

    let mut m = ready();
    let mut b = start(&mut m, "b.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    assert!(m.allow(next(2)));
    assert!(m.user_reload());
    assert!(matches!(poll(&mut b), Poll::Ready(Err(_))));
    assert!(m.take_retries().is_empty());
}

#[test]
fn waiter_caps_still_apply_while_a_question_is_expired() {
    let mut m = ready();
    let mut waits = vec![start(&mut m, "big.css", 2)];
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    for _ in 1..32 {
        waits.push(request(&m, "big.css"));
    }
    assert!(waits.iter_mut().all(|w| poll(w).is_pending()));
    let mut over = request(&m, "big.css");
    assert!(matches!(
        poll(&mut over),
        Poll::Ready(Err(ResourceFailure::Denied))
    ));
}

#[test]
fn failure_before_metadata_is_not_retried_even_while_paused() {
    let mut m = ready();
    let mut wait = start(&mut m, "outside.css", 2);
    m.document.as_mut().unwrap().paused = true;
    assert!(m.receive(HtmlReply::HtmlFailure {
        id: next(2),
        reason: "HTML resource outside root".into(),
    }));
    assert!(m.continue_receiving(super::id()));
    assert!(m.take_retries().is_empty());
    m.poll_resources();
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Failed))
    ));
}

#[test]
fn failed_retry_after_a_source_change_ends_without_asking_again() {
    let mut m = ready();
    let mut wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    assert!(m.allow(next(2)));
    m.bind_retry(next(2), next(3)).unwrap();
    assert!(m.receive(HtmlReply::HtmlFailure {
        id: next(3),
        reason: "stale HTML document resource".into(),
    }));
    m.poll_resources();
    assert!(matches!(
        poll(&mut wait),
        Poll::Ready(Err(ResourceFailure::Failed))
    ));
    assert!(m.take_retries().is_empty());
    assert!(m.document.as_ref().unwrap().notices.is_empty());
    assert!(!m.receiving());

    let mut m = ready();
    let mut wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    assert!(m.allow(next(2)));
    m.bind_retry(next(2), next(3)).unwrap();
    let mut stale = css_metadata(next(3), 300_000);
    if let HtmlReply::HtmlMetadata { metadata, .. } = &mut stale {
        metadata.source_generation += 1;
    }
    m.receive(stale);
    m.poll_resources();
    assert!(matches!(poll(&mut wait), Poll::Ready(Err(_))));
    assert!(m.take_retries().is_empty());
    assert!(m.document.as_ref().unwrap().notices.is_empty());
}

#[test]
fn disconnect_and_navigation_end_expired_questions_and_waiters() {
    let mut m = ready();
    let mut wait = start(&mut m, "big.css", 2);
    m.receive(css_metadata(next(2), 300_000));
    expire(&mut m, next(2));
    m.disconnect();
    assert!(matches!(poll(&mut wait), Poll::Ready(Err(_))));
    assert!(m.take_retries().is_empty());
    assert!(!m.allow(next(2)));

    let mut m = expired_entry();
    assert!(m.allow(id()));
    m.disconnect();
    assert!(m.take_retries().is_empty());
    assert!(m.bind_retry(id(), next(2)).is_none());
}
