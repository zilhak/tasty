//! 권한 상승 승인 응답의 권한 부여. 응답 진입점(IPC `approval.respond`·GUI 승인 팝업)과 관계없이
//! [`AppServices::respond_approval`]이 전이 직후, 기록 제출 전에 이 경로로 권한을 부여한다.

use tasty_approval::ApprovalRecord;

use crate::app::services::AppServices;

/// 승인 응답에서 agent·권한·유효 시간을 추출한다. host 가 발행한 권한 상승 요청만 대상이며,
/// 호출자가 채울 수 있는 metadata 는 보지 않는다. 거절 응답에는 권한을 부여하지 않는다.
pub(crate) fn elevation_grant_decision(
    record: &ApprovalRecord,
    choice: &str,
) -> Option<(String, String, Option<u64>)> {
    let elevation = record.request.host_elevation.as_ref()?;
    let ttl_secs = match choice {
        "approve" => elevation.grant_ttl_secs,
        "approve_permanently" => None,
        _ => return None, // deny / 그 외는 grant 없음.
    };
    let ttl_ms = ttl_secs.map(|s| s.saturating_mul(1000));
    Some((
        elevation.agent_id.clone(),
        elevation.permission.clone(),
        ttl_ms,
    ))
}

/// `elevation_grant_decision` 결과를 SessionStore 에 적용하는 writer 요청을 기다리지 않고 넣는다.
/// 권한 부여에 실패해도 승인 응답은 보존한다. agent 재시도 때 다시 권한을 요청한다.
///
/// 호출자가 이 요청을 기록 요청보다 먼저 넣으므로, 둘 뒤에 접수된 요청(그 agent의 다음 요청 게이트
/// 포함)은 부여가 확정됐다면 그 권한을 본다. 두 요청은 같은 commit으로 합쳐질 수 있어 같은 묶음의
/// 저장 실패에 함께 되돌려질 수 있다([저장 계약](../../docs/design/systems/storage-contract.md)).
pub(crate) fn apply_elevation_grant_if_any(
    core: &AppServices,
    record: &ApprovalRecord,
    choice: &str,
) {
    let Some((agent_id, permission, ttl_ms)) = elevation_grant_decision(record, choice) else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let bytes = agent_id.len() + permission.len();
    let (body_agent, body_permission) = (agent_id.clone(), permission.clone());
    core.memory_access().submit_detached(
        tasty_memory::writer::MemoryRequest::unit(bytes, move |mem| {
            grant_in(mem, &body_agent, &body_permission, ttl_ms, now_ms)
        }),
        "capability elevation grant",
        Some(Box::new(move |output| {
            report_grant(
                &agent_id,
                &permission,
                ttl_ms,
                output.into_value::<Result<bool, String>>(),
            )
        })),
    );
}

/// 확정된 부여 결과를 로그로 남긴다. 저장 실패는 writer가 따로 경고한다.
fn report_grant(
    agent_id: &str,
    permission: &str,
    ttl_ms: Option<u64>,
    result: Option<Result<bool, String>>,
) {
    let reason = match result {
        Some(Ok(added)) => {
            tracing::info!(
                agent_id,
                permission,
                ttl_ms,
                added,
                "capability_elevation grant applied"
            );
            return;
        }
        Some(Err(e)) => e,
        None => "no result".to_string(),
    };
    tracing::warn!(
        agent_id,
        permission,
        "capability_elevation grant failed: {reason}"
    );
}

/// 세션을 찾아 권한을 붙인다. 다시 실행해도 결과가 같다. 저장 실패는 writer 요청의 실패로 돌려
/// 묶음과 함께 되돌린다. 세션이 없는 것처럼 다시 해도 같은 오류는 확정된 결과로 돌려준다.
fn grant_in(
    mem: &mut dyn tasty_memory::MemoryStorage,
    agent_id: &str,
    permission: &str,
    ttl_ms: Option<u64>,
    now_ms: u64,
) -> tasty_memory::Result<Result<bool, String>> {
    use tasty_ipc::session::SessionError;
    let mut store = crate::ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
    let granted = match store.find_by_agent_id(agent_id, now_ms) {
        Ok(Some((token, _))) => store.grant_permission(&token, permission, ttl_ms, now_ms),
        Ok(None) => Err(SessionError::InvalidArgument(format!(
            "no active session for agent_id '{agent_id}'"
        ))),
        Err(e) => Err(e),
    };
    match granted {
        Ok(added) => Ok(Ok(added)),
        Err(SessionError::Memory(e)) if e.storage_failure().is_some() => Err(e),
        Err(e) => Ok(Err(e.to_string())),
    }
}
