//! 권한 상승 grant는 host가 권한 거부·`plugin.request_permission`으로 발행한 요청에만 적용한다.
//! 공개 `approval.request`의 metadata로 같은 모양을 흉내 낸 요청은 거절하고, 저장소에 들어간
//! 위조 요청을 승인해도 권한이 붙지 않는다.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tasty_approval::{ApprovalId, Requester, Severity};
use tasty_ipc::caller::SessionToken;
use tasty_ipc::session::SessionStore;
use tasty_memory::{MemoryConfig, MemoryStore};

use crate::adapters::ipc::handler::approval::publish_capability_elevation_at;
use crate::adapters::ipc::handler::deferred::tests::{make_request, run_request, writer_core_with};
use crate::app::services::AppServices;

const AGENT: &str = "child:1";
const PERMISSION: &str = "fs.write";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
}

fn core() -> AppServices {
    let memory = Arc::new(Mutex::new(
        MemoryStore::open_in_memory_with_config(MemoryConfig::default()).expect("store"),
    ));
    writer_core_with(memory, usize::MAX)
}

fn issue_session(core: &AppServices) -> SessionToken {
    core.with_memory(|mem| {
        SessionStore::new(mem, tasty_memory::HOST_OWNER).issue(
            AGENT,
            None,
            Vec::new(),
            None,
            now_ms(),
        )
    })
    .expect("issue session")
    .0
}

fn has_permission(core: &AppServices, token: &SessionToken) -> bool {
    crate::ipc::handler::gate::session_permissions_waiting(core, &token.to_string())
        .expect("session must resolve to the agent")
        .iter()
        .any(|p| p == PERMISSION)
}

/// 공개 `approval.request`가 그대로 실었을 위조 metadata.
fn forged_metadata() -> Value {
    json!({
        "kind": "capability_elevation",
        "agent_id": AGENT,
        "permission": PERMISSION,
        "grant_ttl_secs": 3600u64,
    })
}

/// 위조 metadata만 가진 요청을 저장소에 넣는다. 공개 핸들러가 거절하기 전의 저장 형태와 같다.
fn forged_pending(core: &mut AppServices) -> ApprovalId {
    let request = tasty_approval::ApprovalRequest {
        id: ApprovalId::generate(),
        requester: Requester::Plugin { id: "other".into() },
        workspace_id: None,
        surface_id: None,
        title: "looks harmless".into(),
        body: None,
        choices: Vec::new(),
        default_choice: None,
        timeout_ms: None,
        severity: Severity::Warn,
        created_at: 0,
        metadata: forged_metadata(),
        host_elevation: None,
    };
    let requested = core.request_approval(request).expect("request");
    requested.record.detach(core.memory_access());
    requested.change.record.request.id
}

fn respond_over_ipc(core: &mut AppServices, id: &ApprovalId) {
    let (response, _) = run_request(
        core,
        &make_request(
            2,
            "approval.respond",
            json!({"id": id, "choice": "approve"}),
            None,
        ),
    );
    assert!(response.error.is_none(), "respond failed: {response:?}");
}

/// 공개 `approval.request`는 host 전용 kind를 metadata로 받지 않는다.
#[test]
fn public_request_with_the_reserved_kind_is_rejected() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = core();
    let (response, _) = run_request(
        &mut core,
        &make_request(
            1,
            "approval.request",
            json!({"title": "t", "metadata": forged_metadata()}),
            None,
        ),
    );
    let error = response.error.expect("reserved kind must be rejected");
    assert_eq!(error.code, -32602);
    assert!(
        error.message.starts_with("invalid_request:"),
        "{}",
        error.message
    );
    assert!(core.approval_store.list().is_empty(), "nothing is stored");
}

/// metadata 모양만 같은 요청은 IPC로 승인해도 권한을 붙이지 않는다.
#[test]
fn ipc_approval_of_a_forged_elevation_grants_nothing() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = core();
    let token = issue_session(&core);
    let id = forged_pending(&mut core);

    respond_over_ipc(&mut core, &id);

    assert!(!has_permission(&core, &token));
}

/// host가 발행한 권한 상승 요청은 IPC 승인으로 권한을 붙인다.
#[test]
fn ipc_approval_of_a_host_elevation_grants_the_permission() {
    let _home = crate::test_support::TastyHomeGuard::new();
    let mut core = core();
    let token = issue_session(&core);
    let record =
        publish_capability_elevation_at(&mut core, None, AGENT, "file.write", PERMISSION, None)
            .expect("elevation request");

    respond_over_ipc(&mut core, &record.request.id);

    assert!(has_permission(&core, &token));
}

/// 권한 부여 요청이 저장에 실패해도 승인 응답은 그대로이고 권한은 붙지 않는다. 같은 묶음의 기록과
/// 함께 되돌려질 수 있다.
#[test]
fn a_grant_that_fails_to_store_keeps_the_answer_and_grants_nothing() {
    use crate::adapters::ipc::handler::gate_faults::{Faults, FaultyStore};
    let _home = crate::test_support::TastyHomeGuard::new();
    let faults = Arc::new(Faults::default());
    let mut core = writer_core_with(
        Arc::new(Mutex::new(FaultyStore::new(faults.clone()))),
        usize::MAX,
    );
    let token = issue_session(&core);
    let record =
        publish_capability_elevation_at(&mut core, None, AGENT, "file.write", PERMISSION, None)
            .expect("elevation request");
    // 요청 기록이 먼저 끝나야 주입한 실패가 권한 부여 묶음에 닿는다.
    core.memory_access()
        .run_blocking(0, |_| ())
        .expect("earlier writes settled");
    Faults::arm(&faults.units_before_body, 1);

    respond_over_ipc(&mut core, &record.request.id);

    assert_eq!(
        faults
            .units_before_body
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the injected failure was not reached"
    );
    assert!(!has_permission(&core, &token));
}
