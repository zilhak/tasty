//! 승인 기록의 영속 저장. 상태 전이를 만든 App 기능이 그 전이의 기록을 같은 key·scope·직렬화로
//! memory writer에 제출하고, 진입점(IPC·GUI·대기 워커)은 받은 접수 결과의 완료를 어떻게 관측할지만 정한다.
//!
//! workspace가 있으면 그 scope에, 없으면 global에 `tasty.approval.<소문자 id>`로 저장한다. 저장 실패는
//! 경고로만 남기고 이미 끝난 메모리 상태 전이는 되돌리지 않는다.

use std::time::Duration;

use tasty_approval::{
    ApprovalError, ApprovalId, ApprovalRecord, ApprovalStore, StateChange, WaitOutcome,
};
use tasty_memory::writer::{MemoryAccess, MemoryRequest, MemoryTicket, Waited};
use tasty_memory::{MemoryValue, PutOpts, Scope};
use tasty_utils::batch_writer::SubmitError;

/// 승인 기록 key의 접두사. 기록 조회도 이 접두사로 목록을 읽는다.
pub(crate) const APPROVAL_KEY_PREFIX: &str = "tasty.approval.";

/// workspace별 세션 요약 key. 개별 승인 기록과 같은 접두사를 쓰므로 기록 조회에서 뺀다.
pub(crate) const SUMMARY_KEY: &str = "tasty.approval.summary";

fn scope_for(record: &ApprovalRecord) -> Scope {
    match record.request.workspace_id {
        Some(wid) => Scope::Workspace(wid),
        None => Scope::Global,
    }
}

/// 승인 기록의 저장 key. memory key는 소문자만 허용하므로 생성 ID의 소문자 표기를 쓰고,
/// 원래 ID는 기록 값에 남는다. 생성 형식이 아닌 ID는 다른 ID와 같은 key가 될 수 있어 None이다.
/// 생성 ID는 `req_`로 시작해 세션 요약 key와 겹치지 않는다.
pub(crate) fn record_key(id: &ApprovalId) -> Option<String> {
    id.lowercase_token()
        .map(|token| format!("{APPROVAL_KEY_PREFIX}{token}"))
}

/// 승인 기록 하나를 쓰는 writer 요청. key를 만들지 못하거나 직렬화하지 못하면 경고하고 None이다.
pub(crate) fn record_write(record: &ApprovalRecord) -> Option<MemoryRequest> {
    let Some(key) = record_key(&record.request.id) else {
        tracing::warn!(
            "approval: {} is not a generated id; the record is not stored",
            record.request.id
        );
        return None;
    };
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
        key,
        value,
        opts: PutOpts {
            expires_at: None,
            cas: None,
        },
    }))
}

/// 승인 상태 전이와 그 기록의 제출 결과. 기록 제출은 전이를 만든 App 기능이 이미 끝냈다.
#[derive(Debug)]
#[must_use = "the record receipt decides how its completion is observed"]
pub(crate) struct ApprovalTransition {
    pub(crate) change: StateChange,
    pub(crate) record: RecordReceipt,
}

/// 기록 제출 결과. 호출자는 완료를 어떻게 관측할지만 정하고 같은 전이를 다시 제출하지 않는다.
#[derive(Debug)]
#[must_use = "detach or wait on the receipt so a failed write is logged"]
pub(crate) enum RecordReceipt {
    /// writer가 접수했다. 표를 버려도 쓰기는 계속된다.
    Accepted(MemoryTicket),
    /// writer가 접수하지 않았다.
    Refused(SubmitError),
    /// key나 직렬화를 만들지 못해 제출하지 않았다. 경고는 이미 남겼다.
    Skipped,
}

impl RecordReceipt {
    /// 메인 루프 경로. 결과는 메인 루프가 거두고 실패는 경고로만 남는다. View 수명과 무관하다.
    pub(crate) fn detach(self, memory: &MemoryAccess) {
        match self {
            Self::Accepted(ticket) => memory.detach(ticket, "approval record", None),
            Self::Refused(error) => {
                tracing::warn!(%error, "approval: memory writer did not accept the record")
            }
            Self::Skipped => {}
        }
    }

    /// 메인 루프 밖 워커 경로. 결과를 `timeout`까지 기다리고 실패는 경고만 남긴다.
    fn wait(self, id: &ApprovalId, timeout: Duration) {
        let failure = match self {
            Self::Accepted(mut ticket) => match ticket.wait_timeout(timeout) {
                Some(completion) => tasty_memory::writer::settle(completion)
                    .err()
                    .map(Waited::Failed),
                None => Some(Waited::TimedOut),
            },
            Self::Refused(error) => Some(Waited::Refused(error)),
            Self::Skipped => None,
        };
        if let Some(why) = failure {
            tracing::warn!("approval: memory put failed for {id}: {why:?}");
        }
    }
}

/// 전이 하나의 기록을 writer에 제출한다. 상태 전이는 이미 끝났고 제출 실패로 되돌리지 않는다.
pub(crate) fn submit(memory: &MemoryAccess, change: StateChange) -> ApprovalTransition {
    let record = match record_write(&change.record) {
        Some(request) => match memory.submit(request) {
            Ok(ticket) => RecordReceipt::Accepted(ticket),
            Err(error) => RecordReceipt::Refused(error),
        },
        None => RecordReceipt::Skipped,
    };
    ApprovalTransition { change, record }
}

/// 대기 워커 경로. 이 대기가 만료 전이를 적용했을 때만 그 기록을 제출하고 결과를 기다린다.
/// 응답·취소로 끝난 대기는 그 전이를 만든 쪽이 이미 제출했으므로 다시 쓰지 않는다.
pub(crate) fn await_approval(
    store: &ApprovalStore,
    memory: &MemoryAccess,
    id: &ApprovalId,
    timeout_ms: Option<u64>,
) -> Result<WaitOutcome, ApprovalError> {
    let awaited = store.await_transition(id, timeout_ms)?;
    if let Some(change) = awaited.change {
        submit(memory, change).record.wait(id, AWAIT_RECORD_TIMEOUT);
    }
    Ok(awaited.outcome)
}

/// 대기 워커가 만료 기록의 결과를 기다리는 시한.
const AWAIT_RECORD_TIMEOUT: Duration = Duration::from_secs(30);

/// 저장된 승인 기록. scope 목록을 읽지 못하면 오류이고, scope 하나를 읽지 못하면 건너뛴다.
pub(crate) fn stored_records(
    s: &mut dyn tasty_memory::MemoryStorage,
    workspace_filter: Option<u32>,
    since: Option<i64>,
    until: Option<i64>,
) -> tasty_memory::Result<Vec<ApprovalRecord>> {
    let mut collected: Vec<ApprovalRecord> = Vec::new();
    for scope_str in s.scopes()? {
        let Ok(scope) = Scope::parse(&scope_str) else {
            continue;
        };
        if let Some(wid) = workspace_filter
            && !matches!(scope, Scope::Workspace(s) if s == wid)
        {
            continue;
        }
        let list_opts = tasty_memory::ListOpts {
            prefix: Some(APPROVAL_KEY_PREFIX.to_string()),
            limit: None,
            since,
            until,
            offset: None,
        };
        let Ok(entries) = s.list(&scope, &list_opts) else {
            continue;
        };
        for entry in entries {
            if entry.key == SUMMARY_KEY {
                continue;
            }
            let MemoryValue::Json(v) = entry.value else {
                continue;
            };
            let Ok(record) = serde_json::from_value::<ApprovalRecord>(v) else {
                continue;
            };
            collected.push(record);
        }
    }
    Ok(collected)
}

#[cfg(test)]
#[path = "approval_records_store_tests.rs"]
mod store_tests;

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
            host_elevation: None,
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
            .get(scope, &record_key(id).expect("generated id"))
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
            let requested = submit(&access, store.request(request(workspace, None)).unwrap());
            let id = requested.change.record.request.id.clone();
            requested.record.detach(&access);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Pending
            ));
            let change = store
                .respond(&id, "approve".into(), Responder::User, None)
                .unwrap();
            submit(&access, change).record.detach(&access);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Responded { .. }
            ));

            let cancelled = store.request(request(workspace, None)).unwrap();
            let id = cancelled.record.request.id.clone();
            submit(&access, store.cancel(&id).unwrap())
                .record
                .detach(&access);
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::Cancelled { .. }
            ));

            // 대기 워커가 만료를 적용하고 기다리는 쓰기로 같은 자리에 남긴다.
            let expiring = store.request(request(workspace, Some(1))).unwrap();
            let id = expiring.record.request.id.clone();
            assert!(matches!(
                await_approval(&store, &access, &id, Some(1)),
                Ok(tasty_approval::WaitOutcome::TimedOut { .. })
            ));
            assert!(matches!(
                stored(&memory, &scope, &id).state,
                ApprovalState::TimedOut { .. }
            ));
        }
    }
}
