//! 원격 캡처 회신은 기다리던 업로드를 요청한 View에만 한 번 알린다.

use super::*;
use crate::app::attach_client::tests::{supply_ids, test_session};
use crate::app::attach_client::wire::mirror_event_from_control;
use std::collections::HashMap;
use std::sync::Arc;

fn capture_result(upload_id: u64) -> MirrorEvent {
    capture_result_at(upload_id, &format!("/srv/shot-{upload_id}.png"))
}

fn capture_result_at(upload_id: u64, path: &str) -> MirrorEvent {
    let payload = serde_json::to_vec(&serde_json::json!({
        "event": "capture_result",
        "upload_id": upload_id,
        "ok": true,
        "path": path,
        "image": true,
    }))
    .unwrap();
    mirror_event_from_control(&payload).expect("캡처 회신은 이벤트가 된다")
}

/// `view` 창에서 회신들을 적용한 뒤 쌓인 토스트 수.
fn toasts_after(sess: &mut AttachClientSession, view: &Arc<()>, events: Vec<MirrorEvent>) -> usize {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    supply_ids(&engine);
    let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
    {
        let mut host = MirrorHost::windowed(&mut state, &mut engine).for_view(Arc::downgrade(view));
        apply_mirror_events(sess, &mut host, &mut plugin_manager, events);
    }
    state.toasts.len()
}

#[test]
fn a_reply_reaches_the_requesting_view_once() {
    let view = Arc::new(());
    let mut sess = test_session(9_000, HashMap::new());
    sess.state.capture_uploads.insert(7, Arc::downgrade(&view));
    assert_eq!(
        toasts_after(
            &mut sess,
            &view,
            vec![capture_result(7), capture_result_at(7, "/srv/late.png")]
        ),
        1,
        "같은 업로드의 두 번째 회신은 내용이 달라도 다시 알리지 않는다"
    );
    assert!(sess.state.capture_uploads.is_empty());
}

#[test]
fn a_reply_nobody_waits_for_is_not_shown() {
    let view = Arc::new(());
    let mut sess = test_session(9_000, HashMap::new());
    sess.state.capture_uploads.insert(7, Arc::downgrade(&view));
    assert_eq!(
        toasts_after(&mut sess, &view, vec![capture_result(8)]),
        0,
        "다른 요청의 회신을 이 요청의 결과로 보이면 안 된다"
    );
    assert!(sess.state.capture_uploads.contains_key(&7));
}

#[test]
fn a_reply_for_another_view_is_not_shown_here() {
    let requester = Arc::new(());
    let other = Arc::new(());
    let mut sess = test_session(9_000, HashMap::new());
    sess.state
        .capture_uploads
        .insert(7, Arc::downgrade(&requester));
    assert_eq!(
        toasts_after(&mut sess, &other, vec![capture_result(7)]),
        0,
        "요청한 View 가 아닌 창(교체된 View 포함)에 안내하지 않는다"
    );
}

#[test]
fn concurrent_uploads_are_answered_out_of_order() {
    let view = Arc::new(());
    let mut sess = test_session(9_000, HashMap::new());
    for id in [1, 2] {
        sess.state.capture_uploads.insert(id, Arc::downgrade(&view));
    }
    assert_eq!(
        toasts_after(&mut sess, &view, vec![capture_result(2), capture_result(1)]),
        2
    );
    assert!(sess.state.capture_uploads.is_empty());
}
