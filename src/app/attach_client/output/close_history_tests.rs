//! A close warning belongs to the original request View, connection and operation.
use super::*;
use crate::app::attach_client::tests::{supply_ids, test_session};
use crate::app::attach_client::wire::mirror_event_from_control;
use crate::ipc::stream::{StreamControl, StructuralOp};
use std::collections::HashMap;
use std::sync::{Arc, Weak};

fn result(op_id: u64, history_omitted: Option<u32>) -> MirrorEvent {
    let payload = serde_json::to_vec(&StreamControl::StructuralResult {
        created_surface_id: None,
        op_id,
        ok: true,
        reason: None,
        history_omitted,
    })
    .unwrap();
    mirror_event_from_control(&payload).unwrap()
}

fn toasts_after(
    requester: Option<Weak<()>>,
    host_view: Option<Weak<()>>,
    different_epoch: bool,
    events: Vec<MirrorEvent>,
) -> Vec<String> {
    let (mut state, mut session) = crate::state::tests::test_mirror_state();
    let anchor = state.focused_surface_id(&session.core_state).unwrap();
    let workspace = session.core_state.workspace_at(0).unwrap().id;
    let mut mirror = test_session(workspace, HashMap::new());
    let (other, _queue) = tasty_remote::connection::channel();
    if let Some(view) = requester {
        state
            .attach_pending
            .begin(
                &session.core_state,
                &state.navigation,
                &StructuralOp::CloseSurface { surface_id: anchor },
                if different_epoch {
                    other.epoch()
                } else {
                    mirror.transport.frame_tx.epoch()
                },
                view,
                std::time::Instant::now(),
            )
            .unwrap()
            .bind(3);
    }
    let mut engine = session.borrow_mut();
    supply_ids(&engine);
    let mut host = match host_view {
        Some(view) => MirrorHost::windowed(&mut state, &mut engine).for_view(view),
        None => MirrorHost::parked(&mut state, &mut engine),
    };
    apply_mirror_events(&mut mirror, &mut host, &mut None, events);
    state
        .toasts
        .messages()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_remote_close_without_history_warns_the_requesting_window_once() {
    let view = Arc::new(());
    let identity = Arc::downgrade(&view);
    assert_eq!(
        toasts_after(
            Some(identity.clone()),
            Some(identity),
            false,
            // Different bodies prevent toast coalescing from hiding duplicate delivery.
            vec![result(3, Some(1)), result(3, Some(2))],
        ),
        vec![crate::app::journal::notices::close_history_omitted_body(1)]
    );
}

#[test]
fn a_close_warning_does_not_follow_the_engine_to_another_view() {
    let requester = Arc::new(());
    let restored_view = Arc::new(());
    assert!(
        toasts_after(
            Some(Arc::downgrade(&requester)),
            Some(Arc::downgrade(&restored_view)),
            false,
            vec![result(3, Some(1))],
        )
        .is_empty()
    );
}

#[test]
fn a_close_warning_for_a_destroyed_view_is_discarded() {
    let requester = Arc::new(());
    let identity = Arc::downgrade(&requester);
    drop(requester);
    assert!(
        toasts_after(
            Some(identity.clone()),
            Some(identity),
            false,
            vec![result(3, Some(1))],
        )
        .is_empty()
    );
}

#[test]
fn a_close_warning_requires_the_request_connection_epoch() {
    let view = Arc::new(());
    let identity = Arc::downgrade(&view);
    assert!(
        toasts_after(
            Some(identity.clone()),
            Some(identity),
            true,
            vec![result(3, Some(1))],
        )
        .is_empty()
    );
}

#[test]
fn a_close_warning_requires_the_request_operation_id() {
    let view = Arc::new(());
    let identity = Arc::downgrade(&view);
    assert!(
        toasts_after(
            Some(identity.clone()),
            Some(identity),
            false,
            vec![result(4, Some(1))],
        )
        .is_empty()
    );
}

#[test]
fn a_close_warning_without_a_user_request_is_discarded() {
    let view = Arc::new(());
    assert!(
        toasts_after(
            None,
            Some(Arc::downgrade(&view)),
            false,
            vec![result(3, Some(1))],
        )
        .is_empty()
    );
}

#[test]
fn a_close_warning_is_not_replayed_after_a_failed_result() {
    let view = Arc::new(());
    let identity = Arc::downgrade(&view);
    let messages = toasts_after(
        Some(identity.clone()),
        Some(identity),
        false,
        vec![
            MirrorEvent::StructuralFailed(3, Some("close denied".into())),
            result(3, Some(1)),
        ],
    );
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("close denied"));
}

#[test]
fn a_close_warning_is_absent_for_omitted_counts_and_parked_hosts() {
    let view = Arc::new(());
    let identity = Arc::downgrade(&view);
    assert!(
        toasts_after(
            Some(identity.clone()),
            Some(identity.clone()),
            false,
            vec![result(3, None)],
        )
        .is_empty()
    );
    assert!(toasts_after(Some(identity), None, false, vec![result(3, Some(1))],).is_empty());
}
