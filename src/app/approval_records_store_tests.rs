//! 실제 SQLite 저장소로 승인 기록의 쓰기·조회·재시작 보존을 확인한다. 키 검증이 없는
//! 시험 대역으로는 저장소가 거절하는 키를 잡지 못한다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tasty_approval::{
    ApprovalChoice, ApprovalId, ApprovalRequest, ApprovalState, ApprovalStore, Requester,
    Responder, Severity, WaitOutcome,
};
use tasty_memory::writer::MemoryAccess;
use tasty_memory::{MemoryConfig, MemoryStore, MemoryValue, PutOpts, Scope};

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

fn open(path: &Path, config: MemoryConfig) -> (Arc<Mutex<MemoryStore>>, MemoryAccess) {
    let store = Arc::new(Mutex::new(
        MemoryStore::open_with_config(path, config).expect("open the memory db"),
    ));
    let access = MemoryAccess::inline(store.clone());
    (store, access)
}

/// IPC 경로처럼 쓰기 결과를 받아 저장소가 거절하면 실패한다.
fn write(access: &MemoryAccess, record: &ApprovalRecord) {
    let request = record_write(record).expect("a generated id has a record key");
    if let Err(why) = access.run_and_wait(request, Duration::from_secs(5)) {
        panic!("record {} was not stored: {why:?}", record.request.id);
    }
}

/// 대문자가 든 생성 ID의 응답·취소·만료 기록이 각 scope에 저장되고, DB를 다시 연 뒤에도
/// 원래 ID와 마지막 상태로 history에 나온다. 같은 scope의 세션 요약은 기록에 섞이지 않는다.
#[test]
fn generated_ids_are_stored_and_read_back_after_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("memory.db");
    let mut expected: Vec<(ApprovalId, Option<u32>, &str)> = Vec::new();
    {
        let (memory, access) = open(&path, MemoryConfig::default());
        let approvals = ApprovalStore::new();
        for workspace in [Some(7), None] {
            if let Some(wid) = workspace {
                memory
                    .lock()
                    .unwrap()
                    .put(
                        tasty_memory::HOST_OWNER,
                        &Scope::Workspace(wid),
                        SUMMARY_KEY,
                        &MemoryValue::Text("summary".into()),
                        &PutOpts {
                            expires_at: None,
                            cas: None,
                        },
                    )
                    .expect("summary put");
            }

            let created = approvals.request(request(workspace, None)).unwrap();
            let id = created.record.request.id.clone();
            assert!(
                id.as_str().bytes().any(|b| b.is_ascii_uppercase()),
                "the generated id carries uppercase letters: {id}"
            );
            write(&access, &created.record);
            let change = approvals
                .respond(&id, "approve".into(), Responder::User, None)
                .unwrap();
            write(&access, &change.record);
            expected.push((id, workspace, "responded"));

            let created = approvals.request(request(workspace, None)).unwrap();
            let id = created.record.request.id.clone();
            write(&access, &created.record);
            write(&access, &approvals.cancel(&id).unwrap().record);
            expected.push((id, workspace, "cancelled"));

            let created = approvals.request(request(workspace, Some(1))).unwrap();
            let id = created.record.request.id.clone();
            write(&access, &created.record);
            assert!(matches!(
                approvals.await_response(&id, Some(1)),
                Ok(WaitOutcome::TimedOut { .. })
            ));
            persist_and_wait(&access, &approvals.get(&id).unwrap());
            expected.push((id, workspace, "timed_out"));
        }
    }

    let (memory, _access) = open(&path, MemoryConfig::default());
    let mut guard = memory.lock().unwrap();
    let records: HashMap<ApprovalId, ApprovalRecord> =
        stored_records(&mut *guard, None, None, None)
            .expect("history reads every scope")
            .into_iter()
            .map(|r| (r.request.id.clone(), r))
            .collect();
    assert_eq!(records.len(), expected.len(), "{records:?}");
    for (id, workspace, state) in &expected {
        let record = records
            .get(id)
            .unwrap_or_else(|| panic!("{id} is in history"));
        assert_eq!(record.request.workspace_id, *workspace);
        let actual = match record.state {
            ApprovalState::Pending => "pending",
            ApprovalState::Responded { .. } => "responded",
            ApprovalState::Cancelled { .. } => "cancelled",
            ApprovalState::TimedOut { .. } => "timed_out",
        };
        assert_eq!(actual, *state, "{id}");
        let scope = match workspace {
            Some(wid) => Scope::Workspace(*wid),
            None => Scope::Global,
        };
        let key = record_key(id).expect("a generated id has a record key");
        assert!(
            guard.get(&scope, &key).unwrap().is_some(),
            "{key} in {scope:?}"
        );
    }
    let workspace_only = stored_records(&mut *guard, Some(7), None, None).unwrap();
    assert_eq!(workspace_only.len(), 3);
    let summary = guard.get(&Scope::Workspace(7), SUMMARY_KEY).unwrap();
    assert!(matches!(summary.map(|e| e.value), Some(MemoryValue::Text(t)) if t == "summary"));
}

/// 저장소가 기록을 거절해도 이미 끝난 메모리 상태 전이는 그대로 남는다.
#[test]
fn a_refused_write_keeps_the_memory_transition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (memory, access) = open(
        &dir.path().join("memory.db"),
        MemoryConfig {
            entry_max_bytes: 8,
            ..MemoryConfig::default()
        },
    );
    let approvals = ApprovalStore::new();
    let created = approvals.request(request(Some(7), None)).unwrap();
    let id = created.record.request.id.clone();
    let change = approvals
        .respond(&id, "approve".into(), Responder::User, None)
        .unwrap();
    persist_and_wait(&access, &change.record);
    persist(&access, &change.record);

    assert!(matches!(
        approvals.get(&id).unwrap().state,
        ApprovalState::Responded { .. }
    ));
    let mut guard = memory.lock().unwrap();
    assert!(
        stored_records(&mut *guard, None, None, None)
            .unwrap()
            .is_empty()
    );
}
