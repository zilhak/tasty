use super::*;

use crate::ipc::stream::StreamControl;
use tasty_remote::client_session::MirrorEvent;

#[test]
fn a_loss_notice_becomes_a_desync_event() {
    let payload = serde_json::to_vec(&StreamControl::Loss { frames: 7 }).unwrap();
    assert!(
        matches!(
            mirror_event_from_control(&payload),
            Some(MirrorEvent::Desynced { frames: 7 })
        ),
        "Loss 가 재동기화 이벤트로 옮겨지지 않았다"
    );
}

#[test]
fn parse_markdown_content_result_reads_both_shapes_and_ignores_other_events() {
    let ok = serde_json::json!({
        "event": "markdown_content_result", "request_id": 4, "surface_id": 30,
        "ok": true, "file": "/r/a.md", "source": "# hi", "truncated": true,
    });
    match parse_markdown_content_result(&serde_json::to_vec(&ok).unwrap()) {
        Some(MirrorEvent::MarkdownContentResult {
            request_id: 4,
            surface_id: 30,
            ok: true,
            file,
            source,
            truncated: true,
            reason: None,
        }) => {
            assert_eq!(file.as_deref(), Some("/r/a.md"));
            assert_eq!(source.as_deref(), Some("# hi"));
        }
        _ => panic!("success shape not parsed"),
    }
    let failed = serde_json::json!({
        "event": "markdown_content_result", "request_id": 5, "surface_id": 30,
        "ok": false, "reason": "permission denied",
    });
    match parse_markdown_content_result(&serde_json::to_vec(&failed).unwrap()) {
        Some(MirrorEvent::MarkdownContentResult {
            ok: false, reason, ..
        }) => assert_eq!(reason.as_deref(), Some("permission denied")),
        _ => panic!("failure shape not parsed"),
    }
    let other = serde_json::json!({ "event": "git_query_result", "request_id": 1 });
    assert!(parse_markdown_content_result(&serde_json::to_vec(&other).unwrap()).is_none());
}

#[test]
fn parse_markdown_changed_reads_the_remote_id_and_ignores_other_events() {
    let changed = serde_json::json!({ "event": "markdown_changed", "surface_id": 30 });
    assert!(matches!(
        parse_markdown_changed(&serde_json::to_vec(&changed).unwrap()),
        Some(MirrorEvent::MarkdownChanged { surface_id: 30 })
    ));
    let result = serde_json::json!({
        "event": "markdown_content_result", "request_id": 4, "surface_id": 30, "ok": true,
    });
    assert!(parse_markdown_changed(&serde_json::to_vec(&result).unwrap()).is_none());
    assert!(parse_markdown_content_result(&serde_json::to_vec(&changed).unwrap()).is_none());
}
