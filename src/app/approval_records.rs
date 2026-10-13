//! 승인 기록의 영속 저장. GUI 응답·IPC 요청·대기 워커가 같은 key·scope·직렬화로 memory writer에 쓴다.
//!
//! workspace가 있으면 그 scope에, 없으면 global에 `tasty.approval.<id>`로 저장한다. 저장 실패는
//! 경고로만 남기고 이미 끝난 메모리 상태 전이는 되돌리지 않는다.

use tasty_approval::ApprovalRecord;
use tasty_memory::writer::{MemoryAccess, MemoryRequest};
use tasty_memory::{MemoryValue, PutOpts, Scope};

/// 승인 기록 key의 접두사. 기록 조회도 이 접두사로 목록을 읽는다.
pub(crate) const APPROVAL_KEY_PREFIX: &str = "tasty.approval.";

fn scope_for(record: &ApprovalRecord) -> Scope {
    match record.request.workspace_id {
        Some(wid) => Scope::Workspace(wid),
        None => Scope::Global,
    }
}

/// 승인 기록 하나를 쓰는 writer 요청. 직렬화하지 못하면 경고하고 None이다.
pub(crate) fn record_write(record: &ApprovalRecord) -> Option<MemoryRequest> {
    let value = match serde_json::to_value(record) {
        Ok(v) => MemoryValue::Json(v),
        Err(e) => {
            tracing::warn!("approval: serialize failed for {}: {e}", record.request.id);
            return None;
        }
    };
    Some(MemoryRequest::Write(tasty_memory::WriteOp::Put {
        owner: tasty_memory::HOST_OWNER.to_string(),
        scope: scope_for(record),
        key: format!("{}{}", APPROVAL_KEY_PREFIX, record.request.id),
        value,
        opts: PutOpts {
            expires_at: None,
            cas: None,
        },
    }))
}

/// 응답과 무관한 경로의 기록. 결과를 기다리지 않고 실패는 writer가 경고로 남긴다.
pub(crate) fn persist(memory: &MemoryAccess, record: &ApprovalRecord) {
    if let Some(request) = record_write(record) {
        memory.submit_detached(request, "approval record", None);
    }
}

/// 메인 루프 밖의 대기 워커가 쓰는 기록. 워커 스레드라 결과를 기다리고 실패는 경고만 남긴다.
pub(crate) fn persist_and_wait(memory: &MemoryAccess, record: &ApprovalRecord) {
    let Some(request) = record_write(record) else {
        return;
    };
    if let Err(why) = memory.run_and_wait(request, std::time::Duration::from_secs(30)) {
        tracing::warn!(
            "approval: memory put failed for {}: {why:?}",
            record.request.id
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tasty_approval::{
        ApprovalChoice, ApprovalId, ApprovalRequest, ApprovalState, ApprovalStore, Requester,
        Responder, Severity,
    };
    use tasty_memory::MemoryStorage;
    use tasty_memory::testing::InMemoryStorage;

    use super::*;

    fn request(workspace_id: Option<u32>, timeout_ms: Option<u64>) -> ApprovalRequest {
        ApprovalRequest {
            id: ApprovalId::generate(),
            requester: Requester::Plugin { id: "p".into() },
            workspace_id,
            surface_id: None,
            title: "t".into(),
            body: None,
            choices: vec![ApprovalChoice::approve(), ApprovalChoice::deny()],
            default_choice: None,
            timeout_ms,
            severity: Severity::Info,
            created_at: 0,
            metadata: serde_json::Value::Null,
        }
    }

    fn stored(
        memory: &Arc<Mutex<InMemoryStorage>>,
        scope: &Scope,
        id: &ApprovalId,
    ) -> ApprovalRecord {
        let entry = memory
            .lock()
            .unwrap()
            .get(scope, &format!("{APPROVAL_KEY_PREFIX}{id}"))
            .unwrap()
            .expect("the record is stored");
        assert_eq!(entry.owner.as_deref(), Some(tasty_memory::HOST_OWNER));
        match entry.value {
            MemoryValue::Json(v) => serde_json::from_value(v).unwrap(),
            other => panic!("{other:?}"),
        }
    }

    /// request·respond·cancel·만료가 모두 같은 key·scope·owner에 마지막 상태를 쓴다.
    #[test]
    fn every_transition_writes_the_same_key_in_the_request_scope() {
        let memory = Arc::new(Mutex::new(InMemoryStorage::new()));
        let access = MemoryAccess::inline(memory.clone());
        let store = ApprovalStore::new();

        for (workspace, scope) in [(Some(7), Scope::Workspace(7)), (None, Scope::Global)] {
            let responded = store.request(request(workspace, None)).unwrap();
            let id = responded.record.request.id.clone();
            persist(&access, &responded.record);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Pending
            ));
            let change = store
                .respond(&id, "approve".into(), Responder::User, None)
                .unwrap();
            persist(&access, &change.record);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Responded { .. }
            ));

            let cancelled = store.request(request(workspace, None)).unwrap();
            let id = cancelled.record.request.id.clone();
            let change = store.cancel(&id).unwrap();
            persist(&access, &change.record);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Cancelled { .. }
            ));

            // 대기 워커가 만료를 관측하고 기다리는 쓰기로 같은 자리에 남긴다.
            let expiring = store.request(request(workspace, Some(1))).unwrap();
            let id = expiring.record.request.id.clone();
            assert!(matches!(
                store.await_response(&id, Some(1)),
                Ok(tasty_approval::WaitOutcome::TimedOut { .. })
            ));
            persist_and_wait(&access, &store.get(&id).unwrap());
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::TimedOut { .. }
            ));
        }
    }
}
