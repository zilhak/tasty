//! HTTP replies use the real attach parser, ordered output application and captured model.
use super::*;
use crate::app::attach_client::{tests::test_session, wire::mirror_event_from_control};
use std::{collections::HashMap, sync::Arc};
use tasty_ipc::{html_http::*, html_transfer::TransferId};
use tasty_remote::{
    html_http_body::BodyBudget, html_http_client::*, html_http_session::CapturedDocument,
};
fn id() -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 42,
        binding: 1,
        document: 1,
        request: 1,
        transfer: 1,
    }
}
fn document(sess: &AttachClientSession) -> Arc<CapturedDocument> {
    CapturedDocument::new(
        DocumentKey {
            entry: id(),
            source_generation: 7,
            entry_url: "http://example.test/".into(),
            allow_remote_content: false,
        },
        sess.transport.frame_tx.epoch(),
        ClientLimits {
            requests: 8,
            upload_bytes: 1000,
            encoded_bytes: 1000,
            decoded_bytes: 1000,
            verify_jobs: 1,
        },
        PendingBudget::new(1_000_000),
        BodyBudget::new(1_000_000),
        Arc::new(|| true),
        Arc::new(|| {}),
    )
    .unwrap_or_else(|_| panic!("captured document"))
}
fn event(reply: HttpReply) -> MirrorEvent {
    mirror_event_from_control(&reply.frame().unwrap().payload).expect("HTTP reply parsed")
}
#[test]
fn http_session_actual_dispatch_preserves_command_rejection_then_terminal_failure() {
    let mut sess = test_session(9000, HashMap::new());
    let document = document(&sess);
    sess.transport
        .html
        .http_documents
        .insert(document.clone())
        .unwrap();
    let begin = HttpRequest::Begin {
        id: id(),
        source_generation: 7,
        method: "POST".into(),
        logical_url: "http://example.test/".into(),
        headers: vec![],
        upload: BodyIdentity {
            bytes: 0,
            sha256: [0; 32],
        },
        allow_remote_content: false,
    };
    // Compute the actual empty upload identity rather than bypass the model's admission checks.
    use sha2::{Digest, Sha256};
    let mut begin = begin;
    if let HttpRequest::Begin { upload, .. } = &mut begin {
        upload.sha256 = Sha256::digest([]).into();
    }
    assert!(
        document
            .with_current(|model| model.admit(begin, Arc::new(vec![])).is_ok())
            .unwrap()
    );
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut host = MirrorHost::parked(&mut state, &mut engine);
    apply_mirror_events(
        &mut sess,
        &mut host,
        &mut None,
        vec![event(HttpReply::UploadReady {
            id: id(),
            source_generation: 7,
            offset: 0,
            max_bytes: 1,
        })],
    );
    assert!(
        document
            .with_current(|model| matches!(model.next_command(), Some(HttpRequest::Commit { .. })))
            .unwrap()
    );
    apply_mirror_events(
        &mut sess,
        &mut host,
        &mut None,
        vec![event(HttpReply::CommandRejected {
            id: id(),
            source_generation: 7,
            response: None,
            command: CommandKind::Begin,
            code: FailureCode::InProgress,
            reason: "Pending".into(),
        })],
    );
    assert_eq!(
        document.with_current(|model| model.state(id())).flatten(),
        Some(ReceiveState::WaitingHead)
    );
    assert!(
        document
            .with_current(|model| model.next_command().is_none())
            .unwrap()
    );
    apply_mirror_events(
        &mut sess,
        &mut host,
        &mut None,
        vec![event(HttpReply::Failure {
            id: id(),
            source_generation: 7,
            response: None,
            code: FailureCode::Network,
            reason: "Network".into(),
        })],
    );
    assert_eq!(
        document.with_current(|model| model.state(id())).flatten(),
        Some(ReceiveState::Failed)
    );
}
#[test]
fn http_session_desync_and_role_removal_revoke_native_clones_on_actual_dispatch() {
    let (mut sess, frames) =
        crate::app::attach_client::tests::test_session_with_frames(9000, HashMap::new());
    let document = document(&sess);
    sess.transport
        .html
        .http_documents
        .insert(document.clone())
        .unwrap();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut host = MirrorHost::parked(&mut state, &mut engine);
    apply_mirror_events(
        &mut sess,
        &mut host,
        &mut None,
        vec![MirrorEvent::Desynced { frames: 1 }],
    );
    assert!(!document.is_current());
    assert!(sess.transport.html.http_documents.snapshot().is_empty());
    assert!(sess.transport.frame_tx.epoch().is_active());
    assert!(
        matches!(serde_json::from_slice::<HttpRequest>(&frames.try_recv().unwrap().frame.payload).unwrap(), HttpRequest::Close { entry, source_generation: 7 } if entry == id())
    );
    let next = super::http_tests::document(&sess);
    sess.transport
        .html
        .http_documents
        .insert(next.clone())
        .unwrap();
    super::super::html::retain(
        &mut sess,
        &[serde_json::json!({"remote_id":42,"role":"terminal"})],
    );
    assert!(!next.is_current());
    assert!(
        matches!(serde_json::from_slice::<HttpRequest>(&frames.try_recv().unwrap().frame.payload).unwrap(), HttpRequest::Close { entry, source_generation: 7 } if entry == id())
    );
    assert!(frames.try_recv().is_err());
    assert!(sess.transport.html.http_documents.snapshot().is_empty());
}
