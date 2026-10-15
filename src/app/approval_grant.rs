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

/// I/O wrapper — `elevation_grant_decision` 결과를 SessionStore 에 적용.
/// 권한 부여에 실패해도 승인 응답은 보존한다. agent 재시도 때 다시 권한을 요청한다.
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
    let result = core.with_memory(|mem| {
        let mut store = crate::ipc::session::SessionStore::new(mem, tasty_memory::HOST_OWNER);
        let token = match store.find_by_agent_id(&agent_id, now_ms)? {
            Some((t, _)) => t,
            None => {
                return Err(tasty_ipc::session::SessionError::InvalidArgument(format!(
                    "no active session for agent_id '{agent_id}'"
                )));
            }
        };
        store.grant_permission(&token, &permission, ttl_ms, now_ms)
    });
    match result {
        Ok(added) => {
            tracing::info!(
                agent_id,
                permission,
                ttl_ms,
                added,
                "capability_elevation grant applied"
            );
        }
        Err(e) => {
            tracing::warn!(
                agent_id,
                permission,
                "capability_elevation grant failed: {e}"
            );
        }
    }
}
