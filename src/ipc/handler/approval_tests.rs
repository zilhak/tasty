//! `approval.*` IPC 단위 테스트.

#![cfg(test)]

use super::*;
use tasty_approval::ApprovalState;

fn elevation_record(extra_metadata: Value) -> ApprovalRecord {
    let mut md = json!({
        "kind": "capability_elevation",
        "agent_id": "child:1",
        "permission": "fs.write",
        "grant_ttl_secs": 3600u64,
    });
    if let (Value::Object(base), Value::Object(extra)) = (&mut md, extra_metadata) {
        for (k, v) in extra {
            base.insert(k, v);
        }
    }
    ApprovalRecord {
        request: ApprovalRequest {
            id: ApprovalId::generate(),
            requester: Requester::Plugin {
                id: "child:1".into(),
            },
            workspace_id: None,
            surface_id: None,
            title: "t".into(),
            body: None,
            choices: vec![],
            default_choice: None,
            timeout_ms: None,
            severity: Severity::Warn,
            created_at: 0,
            metadata: md,
        },
        state: ApprovalState::Pending,
        history: vec![],
    }
}

#[test]
fn approve_yields_finite_ttl_from_metadata() {
    let rec = elevation_record(json!({}));
    let (aid, perm, ttl) = elevation_grant_decision(&rec, "approve").expect("decision");
    assert_eq!(aid, "child:1");
    assert_eq!(perm, "fs.write");
    assert_eq!(ttl, Some(3_600_000));
}

#[test]
fn approve_permanently_yields_no_ttl() {
    let rec = elevation_record(json!({}));
    let (_, _, ttl) =
        elevation_grant_decision(&rec, "approve_permanently").expect("decision");
    assert_eq!(ttl, None);
}

#[test]
fn deny_yields_no_grant() {
    let rec = elevation_record(json!({}));
    assert!(elevation_grant_decision(&rec, "deny").is_none());
}

#[test]
fn non_elevation_record_skipped() {
    let mut rec = elevation_record(json!({}));
    rec.request.metadata = json!({"kind": "other"});
    assert!(elevation_grant_decision(&rec, "approve").is_none());
}

#[test]
fn missing_required_metadata_skipped() {
    let mut rec = elevation_record(json!({}));
    rec.request.metadata = json!({"kind": "capability_elevation"});
    assert!(elevation_grant_decision(&rec, "approve").is_none());
}

#[test]
fn approve_without_grant_ttl_secs_is_indefinite_in_metadata() {
    // grant_ttl_secs 누락 시 approve 는 None (무기한) 으로 fallback.
    let mut rec = elevation_record(json!({}));
    if let Value::Object(m) = &mut rec.request.metadata {
        m.remove("grant_ttl_secs");
    }
    let (_, _, ttl) = elevation_grant_decision(&rec, "approve").expect("decision");
    assert_eq!(ttl, None);
}
