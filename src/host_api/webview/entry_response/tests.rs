use super::*;
use crate::webview::script_gate::{Response, ScriptGate};
use base64::Engine;
use tasty_ipc::html_transfer::{HtmlMetadata, HtmlReply, HtmlSource, TransferId};
use tasty_model::html_script::HtmlScriptState;
use tasty_remote::html_document::HtmlDocument;
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
fn entry() -> Arc<ReceivedEntry> {
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
fn store(body: Arc<ReceivedEntry>) -> Arc<Mutex<EntryBodyStore>> {
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
