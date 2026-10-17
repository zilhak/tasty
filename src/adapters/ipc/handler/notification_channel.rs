//! Notification acknowledgment uses the same source attention policy as the panel.
use crate::notification::UnreadCounts;
use crate::runtime::engine_access::EngineMut;
use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;

pub(crate) struct Selection {
    pub id: Option<u64>,
    pub workspace: Option<u32>,
}

pub(crate) fn selection(params: &Value, read: bool) -> Result<Selection, String> {
    let workspace = super::params::read_int::<u32>(params, "workspace_id")?;
    let id = super::params::read_int::<u64>(params, "notification_id")?;
    let all = match params.get("all") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("all must be a boolean")?,
    };
    if read && (id.is_some() == all) {
        return Err("provide notification_id or all=true, exclusively".to_owned());
    }
    Ok(Selection { id, workspace })
}

pub(crate) fn counts_response(id: Value, counts: UnreadCounts) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({"error": counts.error, "warning": counts.warning, "info": counts.info, "total": counts.total()}),
    )
}

pub(crate) fn read_response(id: Value, found: bool, changed: usize) -> JsonRpcResponse {
    if !found {
        return JsonRpcResponse::invalid_params(
            id,
            "notification_id not found in the requested scope",
        );
    }
    JsonRpcResponse::success(id, json!({"read": true, "changed": changed}))
}

pub(crate) fn acknowledge(engine: &mut EngineMut<'_>, selection: &Selection) -> (bool, usize) {
    let found = selection.id.is_none_or(|id| {
        engine.live.notifications.all().any(|n| {
            n.id == id
                && selection
                    .workspace
                    .is_none_or(|ws| n.source_workspace == ws)
        })
    });
    let ids: Vec<_> = engine
        .live
        .notifications
        .all()
        .filter(|n| {
            !n.read
                && selection.id.is_none_or(|id| n.id == id)
                && selection
                    .workspace
                    .is_none_or(|ws| n.source_workspace == ws)
        })
        .map(|n| n.id)
        .collect();
    let changed = engine
        .live
        .notifications
        .acknowledge(selection.id, selection.workspace);
    for id in ids {
        engine.mark_notification_read(id);
    }
    (found, changed)
}

pub(crate) fn handle(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
    read: bool,
) -> JsonRpcResponse {
    let selection = match selection(params, read) {
        Ok(value) => value,
        Err(message) => return JsonRpcResponse::invalid_params(id, message),
    };
    if read {
        let (found, changed) = acknowledge(engine, &selection);
        read_response(id, found, changed)
    } else {
        counts_response(
            id,
            engine.live.notifications.unread_counts(selection.workspace),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::attention::AttentionKind;
    use crate::notification::NotificationLevel;

    #[test]
    fn read_requires_an_explicit_non_conflicting_selection() {
        for params in [
            json!({}),
            json!({"notification_id": -1}),
            json!({"notification_id": 1, "all": true}),
            json!({"all":"true"}),
            json!({"all":true,"workspace_id":4294967296u64}),
        ] {
            assert!(selection(&params, true).is_err(), "{params}");
        }
        assert!(selection(&json!({"notification_id": 1}), true).is_ok());
        assert!(selection(&json!({"all": true}), true).is_ok());
    }

    #[test]
    fn acknowledgment_is_scoped_and_preserves_hard_occupied_attention() {
        let (_state, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let id = engine
            .live
            .notifications
            .add_with_level(
                9,
                42,
                "Failure".into(),
                String::new(),
                NotificationLevel::Error,
            )
            .unwrap();
        engine.raise_attention(42, AttentionKind::NeedsInput);
        engine.live.occupancy.acquire(42, 7).unwrap();
        let wrong = handle(
            &mut engine,
            json!(1),
            &json!({"notification_id":id,"workspace_id":10}),
            true,
        );
        assert!(wrong.error.is_some());
        assert_eq!(engine.live.notifications.unread_counts(None).error, 1);
        let response = handle(&mut engine, json!(2), &json!({"notification_id":id}), true);
        assert_eq!(response.result.unwrap()["changed"], 1);
        assert_eq!(engine.attention_kind(42), Some(AttentionKind::NeedsInput));
        engine.live.occupancy.release(42, 7).unwrap();
        let next = engine
            .live
            .notifications
            .add_with_level(9, 43, "next".into(), String::new(), NotificationLevel::Info)
            .unwrap();
        engine.raise_attention(43, AttentionKind::Completion);
        handle(
            &mut engine,
            json!(5),
            &json!({"notification_id":next}),
            true,
        );
        assert_eq!(engine.attention_kind(43), None);
        assert_eq!(
            handle(&mut engine, json!(3), &json!({"notification_id":id}), true)
                .result
                .unwrap()["changed"],
            0
        );
        assert_eq!(
            handle(&mut engine, json!(4), &json!({}), false)
                .result
                .unwrap()["total"],
            0
        );
    }
}
