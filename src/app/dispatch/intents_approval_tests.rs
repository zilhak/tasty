//! GUI 승인 팝업의 응답 경로. 팝업 승인도 IPC `approval.respond`와 같은 권한 상승을 적용해야
//! 에이전트의 재호출이 통과한다.

use tasty_approval::ApprovalId;
use tasty_ipc::caller::SessionToken;
use tasty_ipc::session::SessionStore;
use tasty_ipc::{IpcHostFacade, SessionResolution};

use crate::adapters::ipc::handler::approval::publish_capability_elevation_at;
use crate::adapters::ipc::handler::cli_entry_tests::test_core;
use crate::app::App;
use crate::app::services::AppServices;

const AGENT: &str = "child:1";
const PERMISSION: &str = "fs.write";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
}

/// 권한 없이 발급한 agent 세션과 그 세션의 권한 상승 요청.
fn elevation_pending(core: &mut AppServices) -> (SessionToken, ApprovalId) {
    let (token, _) = core
        .with_memory(|mem| {
            SessionStore::new(mem, tasty_memory::HOST_OWNER).issue(
                AGENT,
                None,
                Vec::new(),
                None,
                now_ms(),
            )
        })
        .expect("issue session");
    let record = publish_capability_elevation_at(core, None, AGENT, "file.write", PERMISSION, None)
        .expect("elevation request");
    (token, record.request.id)
}

/// 재호출 때 요청 게이트가 보는 것과 같은 세션 조회 결과의 권한 목록.
fn caller_permissions(core: &AppServices, token: &SessionToken) -> Vec<String> {
    match IpcHostFacade::session_resolve(core, &token.to_string(), now_ms()) {
        SessionResolution::Agent { permissions, .. } => permissions,
        _ => panic!("session must resolve to the agent"),
    }
}

fn respond_from_popup(core: &mut AppServices, id: &ApprovalId, choice: &str) {
    let (mut state, _engine) = crate::state::tests::test_state();
    let intent = crate::intent::Intent::RespondApproval {
        request_id: id.clone(),
        choice: choice.to_string(),
        comment: None,
    }
    .from_user_menu("approval.respond");
    App::dispatch_approval_response(core, &mut state, &intent);
}

/// 팝업에서 승인하면 그 agent 세션에 요청한 권한이 부여되어 재호출이 통과한다.
#[test]
fn popup_approval_grants_the_requested_permission() {
    let mut core = test_core();
    let (token, id) = elevation_pending(&mut core);
    assert!(!caller_permissions(&core, &token).contains(&PERMISSION.to_string()));

    respond_from_popup(&mut core, &id, "approve");

    assert!(
        caller_permissions(&core, &token).contains(&PERMISSION.to_string()),
        "팝업 승인 뒤 재호출의 세션 권한에 {PERMISSION}이 있어야 한다"
    );
}

/// 영구 승인도 같은 경로로 권한을 부여한다.
#[test]
fn popup_permanent_approval_grants_the_requested_permission() {
    let mut core = test_core();
    let (token, id) = elevation_pending(&mut core);

    respond_from_popup(&mut core, &id, "approve_permanently");

    assert!(caller_permissions(&core, &token).contains(&PERMISSION.to_string()));
}

/// 거절 응답과 취소는 권한을 부여하지 않는다.
#[test]
fn popup_denial_and_cancellation_grant_nothing() {
    let mut core = test_core();
    let (token, id) = elevation_pending(&mut core);
    respond_from_popup(&mut core, &id, "deny");
    assert!(!caller_permissions(&core, &token).contains(&PERMISSION.to_string()));

    let (token, id) = elevation_pending(&mut core);
    let cancelled = core.cancel_approval(&id).expect("cancel");
    cancelled.record.detach(core.memory_access());
    assert!(!caller_permissions(&core, &token).contains(&PERMISSION.to_string()));
}

/// 공개 `approval.request`로 들어온 위조 metadata만 가진 요청은 팝업으로 승인해도 권한을 붙이지 않는다.
#[test]
fn popup_approval_of_a_forged_elevation_grants_nothing() {
    let mut core = test_core();
    let (token, _) = elevation_pending(&mut core);
    let request = tasty_approval::ApprovalRequest {
        id: ApprovalId::generate(),
        requester: tasty_approval::Requester::Plugin { id: "other".into() },
        workspace_id: None,
        surface_id: None,
        title: "looks harmless".into(),
        body: None,
        choices: Vec::new(),
        default_choice: None,
        timeout_ms: None,
        severity: tasty_approval::Severity::Warn,
        created_at: 0,
        metadata: serde_json::json!({
            "kind": "capability_elevation",
            "agent_id": AGENT,
            "permission": PERMISSION,
            "grant_ttl_secs": 3600u64,
        }),
        host_elevation: None,
    };
    let requested = core.request_approval(request).expect("request");
    requested.record.detach(core.memory_access());

    respond_from_popup(&mut core, &requested.change.record.request.id, "approve");

    assert!(!caller_permissions(&core, &token).contains(&PERMISSION.to_string()));
}
