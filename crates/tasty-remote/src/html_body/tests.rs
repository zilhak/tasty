use super::*;
use crate::html_document::HtmlDocument;
use base64::Engine;
use tasty_ipc::html_transfer::{CHUNK_BYTES, HtmlMetadata, HtmlReply};
use tasty_model::html_script::ScriptDetection;

fn id() -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 7,
        binding: 9,
        document: 2,
        request: 3,
        transfer: 4,
    }
}
fn receiving(id: TransferId, url: &str, bytes: &[u8]) -> HtmlDocument {
    let mut d = HtmlDocument::new(id.epoch, id.surface_id, id.binding, id.document);
    d.request(id).unwrap();
    assert!(d.receive(HtmlReply::HtmlMetadata {
        id,
        metadata: HtmlMetadata {
            source: HtmlSource::File {
                path: "/server/entry.html".into(),
                url: url.into()
            },
            source_generation: 8,
            root: Some("/server".into()),
            cwd: Some("/server".into()),
            size: bytes.len() as u64,
            regular: true,
        }
    }));
    if !d.notices.is_empty() {
        d.allow_next().unwrap();
    }
    d
}
fn complete(id: TransferId, url: &str, bytes: &[u8]) -> HtmlDocument {
    let mut d = receiving(id, url, bytes);
    for (n, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        if d.paused {
            d.continue_receiving();
        }
        // Crossing the cumulative boundary pauses before issuing this chunk's credit.
        if d.credit(id).is_none() {
            d.continue_receiving();
            d.credit(id).unwrap();
        }
        assert!(d.receive(HtmlReply::HtmlChunk {
            id,
            offset: (n * CHUNK_BYTES) as u64,
            data_b64: base64::engine::general_purpose::STANDARD.encode(chunk)
        }));
    }
    if bytes.is_empty() {
        d.credit(id).unwrap();
    }
    assert!(d.receive(HtmlReply::HtmlComplete {
        id,
        size: bytes.len() as u64
    }));
    d
}
fn entry(id: TransferId) -> Arc<ReceivedEntry> {
    let d = complete(id, "file:///server/entry.html", b"<p>remote</p>");
    Arc::new(
        ReceivedEntry::from_complete(id, &d.resources[&id])
            .unwrap()
            .unwrap(),
    )
}
#[test]
fn only_complete_bytes_are_scanned_and_shared_without_reencoding() {
    let bytes = b"\xff<p>remote</p><script>globalThis.remote=1</script>\x00";
    let mut d = receiving(id(), "file:///server/entry.html?q=1#section", bytes);
    assert!(
        ReceivedEntry::from_complete(id(), &d.resources[&id()])
            .unwrap()
            .is_none()
    );
    d.credit(id()).unwrap();
    d.receive(HtmlReply::HtmlChunk {
        id: id(),
        offset: 0,
        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    });
    assert!(d.resources[&id()].shared_body().is_none());
    d.receive(HtmlReply::HtmlComplete {
        id: id(),
        size: bytes.len() as u64,
    });
    let shared = d.resources[&id()].shared_body().unwrap();
    let e = ReceivedEntry::from_complete(id(), &d.resources[&id()])
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&shared, &e.response().shared_bytes()));
    assert_eq!(e.response().bytes(), bytes);
    assert_eq!(e.scan().detection, ScriptDetection::Scripts);
    assert_eq!(
        e.scan().fingerprint,
        scan_reader(bytes.as_slice(), SCAN_LIMIT_BYTES)
            .unwrap()
            .fingerprint
    );
    assert_eq!(
        e.response().source_url(),
        "file:///server/entry.html?q=1#section"
    );
    assert!(e.native_uri().ends_with("/r3/t4/index.html#section"));
    d.reset(2, 10, 3);
    assert_eq!(e.response().bytes(), bytes);
}
#[test]
fn full_body_fingerprint_changes_even_beyond_detection_limit() {
    let mut bytes = vec![b' '; SCAN_LIMIT_BYTES + 1];
    let d = complete(id(), "file:///server/entry.html", &bytes);
    let a = ReceivedEntry::from_complete(id(), &d.resources[&id()])
        .unwrap()
        .unwrap();
    *bytes.last_mut().unwrap() = b'x';
    let d = complete(id(), "file:///server/entry.html", &bytes);
    let b = ReceivedEntry::from_complete(id(), &d.resources[&id()])
        .unwrap()
        .unwrap();
    assert_ne!(a.scan().fingerprint, b.scan().fingerprint);
    assert_eq!(a.scan().detection, ScriptDetection::None);
}
#[test]
fn svg_mime_and_unsupported_source_are_explicit() {
    let mut d = complete(id(), "file:///server/drawing.SVG", b"<svg/>");
    let e = ReceivedEntry::from_complete(id(), &d.resources[&id()])
        .unwrap()
        .unwrap();
    assert_eq!(e.response().mime(), "image/svg+xml");
    d.resources
        .get_mut(&id())
        .unwrap()
        .metadata
        .as_mut()
        .unwrap()
        .source = HtmlSource::Http {
        url: "https://example.invalid/".into(),
    };
    assert_eq!(
        ReceivedEntry::from_complete(id(), &d.resources[&id()])
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    assert!(
        NativeBodyResponse::new(
            id(),
            8,
            "text/html\r\nInjected: yes",
            "file:///server/a",
            Arc::new(vec![])
        )
        .is_err()
    );
}
#[test]
fn entry_route_has_no_relative_file_or_network_fallback() {
    let e = entry(id());
    let mut store = EntryBodyStore::default();
    store.begin(id());
    assert!(store.publish(Arc::clone(&e)));
    let route = e.native_uri();
    assert!(Arc::ptr_eq(
        &store.response_for_uri(route).unwrap(),
        e.response()
    ));
    assert!(store.response_for_uri(&format!("{route}#anchor")).is_some());
    for uri in [
        "file:///server/entry.html",
        "file:///tmp/entry.html",
        "https://example.invalid/a",
        "http://127.0.0.1/a",
        "image.png",
        "data:text/html,test",
        "about:blank",
    ] {
        assert!(store.response_for_uri(uri).is_none(), "{uri}");
    }
    let base = Url::parse(route).unwrap();
    for relative in ["image.png", "../entry.html", "?query=1", "./other.html"] {
        assert!(
            store
                .response_for_uri(base.join(relative).unwrap().as_str())
                .is_none()
        );
    }
}
#[test]
fn every_identity_component_rejects_late_scan_publication() {
    for field in 0..6 {
        let old = id();
        let mut new = old;
        match field {
            0 => new.epoch += 1,
            1 => new.surface_id += 1,
            2 => new.binding += 1,
            3 => new.document += 1,
            4 => new.request += 1,
            _ => new.transfer += 1,
        }
        let mut store = EntryBodyStore::default();
        store.begin(new);
        assert!(!store.publish(entry(old)));
        assert!(store.publish(entry(new)));
        assert!(!store.publish(entry(new)));
        assert!(!store.cancel(old));
        assert!(store.current().is_some());
        assert!(store.cancel(new));
        assert!(store.current().is_none());
    }
}
#[test]
fn reload_disconnect_and_close_preserve_only_current_completed_body() {
    let first = entry(id());
    let mut store = EntryBodyStore::default();
    store.begin(id());
    store.disconnect();
    assert!(!store.publish(Arc::clone(&first)));
    store.begin(id());
    assert!(store.publish(Arc::clone(&first)));
    assert!(!store.begin(id()));
    store.disconnect();
    assert!(Arc::ptr_eq(store.current().unwrap(), &first));
    let next = TransferId {
        document: 3,
        request: 5,
        transfer: 6,
        ..id()
    };
    assert!(store.begin(next));
    assert!(store.response_for_uri(first.native_uri()).is_none());
    assert!(!store.publish(first));
    assert!(store.publish(entry(next)));
    store.clear();
    assert!(store.current().is_none());
    assert!(!store.publish(entry(next)));
}

#[test]
fn completed_resource_cannot_be_relabelled_as_a_new_transfer() {
    let d = complete(id(), "file:///server/entry.html", b"remote");
    let wrong = TransferId { request: 5, ..id() };
    assert_eq!(
        ReceivedEntry::from_complete(wrong, &d.resources[&id()])
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn scan_cancellation_ends_before_publication_and_keeps_source_generation() {
    let document = complete(
        id(),
        "file:///server/entry.html",
        b"<script>remote()</script>",
    );
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        ReceivedEntry::from_complete_cancellable(id(), &document.resources[&id()], &cancelled)
            .unwrap_err()
            .kind(),
        io::ErrorKind::ConnectionAborted
    );
    let entry = ReceivedEntry::from_complete(id(), &document.resources[&id()])
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.response().source_generation(),
        document.resources[&id()]
            .metadata
            .as_ref()
            .unwrap()
            .source_generation
    );
}
