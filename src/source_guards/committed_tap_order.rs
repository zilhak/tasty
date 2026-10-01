//! Fixed committed-result and delta producer bodies retain result -> delta -> tap order.
//! This is a lexical check of named calls and Sent arms, not a proof of control flow,
//! delivery, binding validity, or every possible tap producer. Runtime stream tests remain separate.

use super::{fn_body, mask_non_code, repo_root};

fn body(path: &str, function: &str) -> String {
    let source = std::fs::read_to_string(repo_root().join(path)).expect("tap producer source");
    fn_body(&mask_non_code(&source), function)
        .expect("tap producer body")
        .split_whitespace()
        .collect()
}

fn ordered(source: &str, tokens: &[&str]) -> bool {
    let mut rest = source;
    for token in tokens {
        let Some(at) = rest.find(token) else {
            return false;
        };
        rest = &rest[at + token.len()..];
    }
    true
}

const RESULT_ORDER: &[&str] = &[
    "matches_client_binding(",
    "if!reply(hub,remote.client,remote.op_id,true,None)",
    "response.idempotent_replay",
    "workspace_holder(remote.workspace)",
    "StreamControl::StructuralDelta",
    "hub.push(",
    "PushResult::Sent=>{engine.flush_committed_workspace_taps(",
];

#[test]
fn committed_result_precedes_delta_and_sent_delta_precedes_tap() {
    let source = body("src/app/journal/commands/inbound.rs", "fn deliver_result(");
    assert!(ordered(&source, RESULT_ORDER));
    assert_eq!(source.matches("flush_committed_workspace_taps(").count(), 1);
}

#[test]
fn ordinary_structure_delta_opens_taps_only_in_its_sent_arm() {
    let pending = body("src/remote/state.rs", "fn take_structure_changed(");
    assert!(ordered(
        &pending,
        &[
            ".partition(|id|self.structure_reply_pending(*id))",
            "self.structure_changed.extend(held)",
            "ready",
        ]
    ));
    let source = body("src/remote/structure_sync.rs", "fn push_structure_changes(");
    assert!(ordered(
        &source,
        &[
            "workspace_attachment_ready(",
            "StreamControl::StructuralDelta",
            "hub.push(",
            "PushResult::Sent=>self.flush_committed_workspace_taps(",
        ]
    ));
    assert_eq!(source.matches("flush_committed_workspace_taps(").count(), 1);
    let flush = body(
        "src/remote/structure_sync.rs",
        "fn flush_committed_workspace_taps(",
    );
    assert!(ordered(
        &flush,
        &[
            "workspace_attachment_ready(",
            "pending_workspace_taps",
            "workspace_holder(",
            "matches_generation(",
            "find_workspace_index_for_surface(",
            "tap_surface_for_stream(",
        ]
    ));
}

#[test]
fn moving_taps_ahead_of_delta_or_out_of_sent_is_detected() {
    let source = body("src/app/journal/commands/inbound.rs", "fn deliver_result(");
    assert!(
        ordered(&source, RESULT_ORDER),
        "mutation needs a valid starting body"
    );
    let tap = "PushResult::Sent=>{engine.flush_committed_workspace_taps(";
    let reordered = format!(
        "{tap}{}",
        source.replace(tap, "PushResult::Sent=>{removed(")
    );
    assert!(!ordered(&reordered, RESULT_ORDER));
    assert!(!ordered(
        &source.replace("PushResult::Sent=>", "PushResult::Dropped=>"),
        RESULT_ORDER
    ));
    assert!(!ordered(
        &source.replace(
            "if!reply(hub,remote.client,remote.op_id,true,None)",
            "if!removed()"
        ),
        RESULT_ORDER
    ));
}
