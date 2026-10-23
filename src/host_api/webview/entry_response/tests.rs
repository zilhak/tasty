use super::*;
use crate::webview::script_gate::{Response, ScriptGate};
use base64::Engine;
use tasty_ipc::html_transfer::{HtmlMetadata, HtmlReply, HtmlSource, TransferId};
use tasty_model::html_script::HtmlScriptState;
use tasty_remote::html_document::HtmlDocument;
pub(super) fn id() -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 7,
        binding: 9,
        document: 2,
        request: 3,
        transfer: 4,
    }
}
pub(super) fn entry() -> Arc<ReceivedEntry> {
    let id = id();
    let bytes = b"<script>globalThis.received=true</script>";
    let mut document = HtmlDocument::new(id.epoch, id.surface_id, id.binding, id.document);
    document.request(id).unwrap();
    document.receive(HtmlReply::HtmlMetadata {
        id,
        metadata: HtmlMetadata {
            source: HtmlSource::File {
                path: "/server/received.html".into(),
                url: "file:///server/received.html".into(),
            },
            source_generation: 8,
            root: Some("/server".into()),
            cwd: Some("/server".into()),
            size: bytes.len() as u64,
            regular: true,
            file: None,
        },
    });
    document.credit(id).unwrap();
    document.receive(HtmlReply::HtmlChunk {
        id,
        offset: 0,
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    document.receive(HtmlReply::HtmlComplete {
        id,
        size: bytes.len() as u64,
    });
    Arc::new(
        ReceivedEntry::from_complete(id, &document.resources[&id])
            .unwrap()
            .unwrap(),
    )
}
pub(super) fn store(body: Arc<ReceivedEntry>) -> Arc<Mutex<EntryBodyStore>> {
    let mut store = EntryBodyStore::default();
    store.begin(id());
    assert!(store.publish(body));
    Arc::new(Mutex::new(store))
}
#[test]
fn source_captures_view_owner_and_does_not_adopt_a_later_body_or_reused_cache() {
    let body = entry();
    let store = Arc::new(Mutex::new(EntryBodyStore::default()));
    let empty = EntryResponseSource::new(&store);
    {
        let mut store = store.lock().unwrap();
        store.begin(id());
        store.publish(Arc::clone(&body));
    }
    assert!(empty.entry(body.native_uri()).is_none());
    let old = EntryResponseSource::new(&store);
    assert!(old.is_current());
    store.lock().unwrap().retire_native_view();
    assert!(!old.is_current());
    assert!(old.response(body.native_uri()).is_none());
    let new = EntryResponseSource::new(&store);
    assert!(Arc::ptr_eq(&new.entry(body.native_uri()).unwrap(), &body));
    store.lock().unwrap().disconnect();
    assert!(new.is_current());
    drop(store);
    assert!(!new.is_current());
}
#[test]
fn delayed_old_native_callbacks_cannot_mutate_current_script_state() {
    let body = entry();
    let store = store(Arc::clone(&body));
    let state = Arc::new(Mutex::new(HtmlScriptState::new(true)));
    let old_source = EntryResponseSource::new(&store);
    let old = ScriptGate::new(7, Arc::clone(&state)).with_received_source(old_source.clone());
    old_source.retire();
    let current = ScriptGate::new(7, Arc::clone(&state))
        .with_received_source(EntryResponseSource::new(&store));
    assert!(!current.load_started());
    assert!(matches!(
        current.main_response(body.native_uri()),
        Response::Decided(false)
    ));
    assert!(!current.committed(Some(body.native_uri())));
    assert!(current.finished().is_none());
    let before = format!("{:?}", *state.lock().unwrap());
    assert!(!old.effective_js());
    assert!(!old.set_sandbox(false));
    assert!(!old.load_started());
    assert!(matches!(
        old.main_response(body.native_uri()),
        Response::Decided(false)
    ));
    assert!(!old.main_response_scanned("file:///server/other.html", Some(body.scan())));
    assert!(!old.committed(Some("file:///server/other.html")));
    assert!(old.finished().is_none());
    old.failed();
    assert!(!old.is_fragment_move(body.native_uri()));
    assert_eq!(before, format!("{:?}", *state.lock().unwrap()));
    assert_eq!(
        state.lock().unwrap().current().unwrap().url,
        "file:///server/received.html"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn exact_main_resource_intent_retires_before_any_late_scheme_body() {
    let body = entry();
    let store = store(Arc::clone(&body));
    store.lock().unwrap().enable_resources(true);
    let source = EntryResponseSource::new(&store);
    let uri = format!(
        "{}/next.html?q=1#anchor",
        body.native_uri().rsplit_once('/').unwrap().0
    );
    assert!(source.allows_uri(&uri));
    assert!(source.main_resource(&uri));
    assert!(source.response(body.native_uri()).is_none());
    assert!(source.request(&uri).is_none());
    let intent = store.lock().unwrap().take_navigation().unwrap();
    assert_eq!(intent.entry, id());
    assert_eq!(intent.source_generation, 8);
    assert_eq!(intent.target.relative, "next.html");
    assert_eq!(intent.target.query.as_deref(), Some("q=1"));
    assert_eq!(intent.target.fragment.as_deref(), Some("anchor"));
}
#[cfg(target_os = "linux")]
#[test]
fn disconnected_cached_child_is_not_adopted_as_a_new_main_document() {
    let body = entry();
    let store = store(Arc::clone(&body));
    let uri = format!(
        "{}/cached.html",
        body.native_uri().rsplit_once('/').unwrap().0
    );
    let _wait = {
        let mut store = store.lock().unwrap();
        store.enable_resources(true);
        let wait = store.request_resource(&uri);
        let broker = store.resources_mut().unwrap();
        let fetch = broker.take_pending(1).pop().unwrap();
        let child = TransferId {
            request: 5,
            transfer: 6,
            ..id()
        };
        assert!(broker.bind(fetch.ticket, child));
        let cached = Arc::new(
            NativeBodyResponse::new(
                child,
                8,
                "text/html",
                "file:///server/cached.html",
                Arc::new(b"cached".to_vec()),
            )
            .unwrap(),
        );
        assert!(broker.publish(child, cached));
        store.disconnect();
        wait
    };
    let source = EntryResponseSource::new(&store);
    assert!(source.response(&uri).is_some());
    assert!(source.main_resource(&uri));
    assert!(source.response(&uri).is_none());
    assert!(store.lock().unwrap().take_navigation().is_none());
    let current = EntryResponseSource::new(&store);
    assert!(current.is_current());
    assert!(Arc::ptr_eq(
        &current.entry(body.native_uri()).unwrap(),
        &body
    ));
    assert!(current.response(&uri).is_some());
    source.lost_native_view();
    assert!(current.is_current());
}

#[test]
fn native_apartment_poll_wakes_and_rechecks_owner_before_returning_bytes() {
    use std::sync::atomic::AtomicUsize;
    use std::task::Poll;
    let body = entry();
    let store = store(Arc::clone(&body));
    let notifications = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notifications);
    let source = EntryResponseSource::new(&store).with_notify(Arc::new(move || {
        counter.fetch_add(1, Ordering::AcqRel);
    }));
    let (mut wait, target) = ResponseWait::pair();
    assert!(source.poll_response(&mut wait).is_pending());
    let response = Arc::new(
        NativeBodyResponse::new(
            id(),
            8,
            "text/css",
            "file:///server/style.css",
            Arc::new(b"body{}".to_vec()),
        )
        .unwrap(),
    );
    target.finish(Ok(Arc::clone(&response)));
    assert_eq!(notifications.load(Ordering::Acquire), 1);
    let Poll::Ready(Ok(received)) = source.poll_response(&mut wait) else {
        panic!("missing native response")
    };
    assert!(Arc::ptr_eq(&received, &response));
    let (mut late, target) = ResponseWait::pair();
    assert!(source.poll_response(&mut late).is_pending());
    target.finish(Ok(response));
    store.lock().unwrap().retire_native_view();
    assert!(matches!(
        source.poll_response(&mut late),
        Poll::Ready(Err(tasty_remote::html_body::ResourceFailure::Cancelled))
    ));
}

#[test]
fn stopping_one_native_waiter_revokes_only_that_resource_and_view_close_revokes_all() {
    let body = entry();
    let store = store(Arc::clone(&body));
    store.lock().unwrap().enable_resources(true);
    let source = EntryResponseSource::new(&store);
    let prefix = body.native_uri().rsplit_once('/').unwrap().0;
    let wait_a = source.request(&format!("{prefix}/a.css")).unwrap();
    let mut wait_b = source.request(&format!("{prefix}/b.css")).unwrap();
    let (a, b) = {
        let mut store = store.lock().unwrap();
        let broker = store.resources_mut().unwrap();
        let pending = broker.take_pending(2);
        assert_eq!(pending.len(), 2);
        let a = TransferId {
            request: 5,
            transfer: 6,
            ..id()
        };
        let b = TransferId {
            request: 7,
            transfer: 8,
            ..id()
        };
        assert!(broker.bind(pending[0].ticket, a));
        assert!(broker.bind(pending[1].ticket, b));
        (a, b)
    };
    drop(wait_a);
    assert_eq!(
        store.lock().unwrap().resources_mut().unwrap().abandoned(),
        vec![a]
    );
    assert!(source.poll_response(&mut wait_b).is_pending());
    source.retire();
    assert!(matches!(
        source.poll_response(&mut wait_b),
        std::task::Poll::Ready(Err(tasty_remote::html_body::ResourceFailure::Cancelled))
    ));
    drop(wait_b);
    assert_eq!(
        store.lock().unwrap().resources_mut().unwrap().abandoned(),
        vec![b]
    );
}
