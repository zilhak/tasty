//! `approval.*` IPC 단위 테스트.

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
    let (_, _, ttl) = elevation_grant_decision(&rec, "approve_permanently").expect("decision");
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

/// 공통 권한 요청 오류에 조회 가능한 approval_id와 대상 권한·메서드가 포함된다.
#[test]
fn the_elevation_envelope_carries_what_the_agent_needs_to_recover() {
    let rec = elevation_record(json!({}));
    let data = elevation_error_data(&rec, "fs.write", "file.write");

    assert_eq!(data["kind"], "capability_elevation");
    assert_eq!(data["permission"], "fs.write");
    assert_eq!(data["method"], "file.write");
    assert_eq!(
        data["approval_id"],
        serde_json::to_value(&rec.request.id).unwrap(),
        "승인 요청 기록의 ID가 응답에 포함되어야 한다"
    );
}

fn requested_workspace(params: Value, active: usize) -> (Option<u64>, Vec<u32>) {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    crate::core::apply_create_workspace_inner(
        &mut engine,
        crate::core::WorkspaceCreationParams::terminal(),
    )
    .expect("두 번째 워크스페이스");
    let ids: Vec<u32> = engine.workspaces.iter().map(|w| w.id).collect();
    state.active_workspace = active;
    let mut params = params;
    if params.get("surface_id").and_then(Value::as_str) == Some("ws1") {
        params["surface_id"] = json!(engine.workspaces[1].all_surface_ids()[0]);
    }
    params["title"] = json!("t");
    let res = handle_request(
        &mut core,
        &mut state,
        &mut engine,
        &CallerContext::Local,
        json!(1),
        &params,
    );
    assert!(res.error.is_none(), "성공해야 한다: {:?}", res.error);
    let result = res.result.expect("result");
    (result["record"]["request"]["workspace_id"].as_u64(), ids)
}

/// surface가 지정되면 사용자의 활성 workspace 대신 그 surface 소속을 사용한다(ADR-0017).
#[test]
fn a_named_surface_decides_the_workspace_whatever_the_user_is_viewing() {
    for active in [0, 1] {
        let (ws, ids) = requested_workspace(json!({ "surface_id": "ws1" }), active);
        assert_eq!(
            ws,
            Some(u64::from(ids[1])),
            "활성 {active} 에서 surface 의 워크스페이스가 아닌 곳에 귀속됐다"
        );
    }
}

#[test]
fn an_explicit_workspace_wins_over_the_surface() {
    let (ws, ids) = requested_workspace(json!({ "surface_id": "ws1", "workspace_id": 999 }), 1);
    assert_eq!(ws, Some(999), "명시 workspace_id 를 무시했다 (ids {ids:?})");
}

/// 대상이 없는 요청의 활성 workspace 기본값은 호환을 위해 유지한다.
#[test]
fn without_a_target_the_active_workspace_is_kept_for_compatibility() {
    for active in [0, 1] {
        let (ws, ids) = requested_workspace(json!({}), active);
        assert_eq!(ws, Some(u64::from(ids[active])));
    }
}
