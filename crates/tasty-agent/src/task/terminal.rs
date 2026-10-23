//! Terminal notification obligations share the atomic write with the task record.
//! This additive namespace does not rewrite or backfill legacy task records.
use serde::{Deserialize, Serialize};
use tasty_memory::{BatchMode, ListOpts, MemoryStorage, MemoryValue, PutOpts, Scope, WriteOp};

use super::Task;
use crate::{AgentError, Result};

pub const PREFIX: &str = "tasty.agent.terminal.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalNotice {
    pub schema: u32,
    pub id: String,
    pub task: Task,
    pub revision: u64,
}

pub(super) fn put_terminal(
    mem: &mut dyn MemoryStorage,
    owner: &str,
    scope: &Scope,
    task_key: String,
    value: serde_json::Value,
    task: &Task,
    revision: u64,
) -> Result<()> {
    let notice = TerminalNotice {
        schema: 1,
        id: uuid::Uuid::new_v4().to_string(),
        task: task.clone(),
        revision,
    };
    let put = |key, value| WriteOp::Put {
        owner: owner.into(),
        scope: scope.clone(),
        key,
        value: MemoryValue::Json(value),
        opts: PutOpts::default(),
    };
    let ops = [
        put(task_key, value),
        put(
            format!("{PREFIX}{}", notice.id),
            serde_json::to_value(notice)?,
        ),
    ];
    // A rejected outbox write must also undo the terminal state, even inside a KeepApplied unit.
    let outcomes = mem
        .write_batch(BatchMode::Atomic, &ops)
        .map_err(|e| e.error)?;
    for outcome in outcomes {
        outcome?;
    }
    Ok(())
}

pub fn pending(mem: &dyn MemoryStorage, workspace: u32) -> Result<Vec<TerminalNotice>> {
    let rows = mem.list(
        &Scope::Workspace(workspace),
        &ListOpts {
            prefix: Some(PREFIX.into()),
            ..Default::default()
        },
    )?;
    rows.into_iter()
        .map(|row| {
            let MemoryValue::Json(value) = row.value else {
                return Err(AgentError::InvalidArgument(
                    "invalid terminal notice value".into(),
                ));
            };
            let notice: TerminalNotice = serde_json::from_value(value)?;
            if notice.schema != 1
                || !notice.task.state.is_terminal()
                || notice.task.workspace_id != workspace
                || notice.revision == 0
                || uuid::Uuid::parse_str(&notice.id).is_err()
                || row.key != format!("{PREFIX}{}", notice.id)
            {
                return Err(AgentError::InvalidArgument(
                    "invalid terminal notice schema or identity".into(),
                ));
            }
            Ok(notice)
        })
        .collect()
}

/// Acknowledge only this immutable notification, never a newer terminal round for the same task.
pub fn acknowledge(
    mem: &mut dyn MemoryStorage,
    owner: &str,
    workspace: u32,
    id: &str,
) -> tasty_memory::Result<()> {
    let key = format!("{PREFIX}{id}");
    if mem.get(&Scope::Workspace(workspace), &key)?.is_some() {
        mem.delete(owner, &Scope::Workspace(workspace), &key, None)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TaskState, TaskStore};
    use std::sync::atomic::AtomicU64;
    use tasty_memory::{HOST_OWNER, MemoryConfig, MemoryStore};

    fn task() -> Task {
        serde_json::from_value(serde_json::json!({
            "id":"p", "workspace_id":1, "name":"terminal identity", "created_at":0,
            "command":{"kind":"custom", "ipc_method":"system.ping", "params":{}},
            "state":{"kind":"running"}, "on_failure":{"kind":"continue_downstream"}
        }))
        .unwrap()
    }

    #[test]
    fn a_terminal_write_has_one_obligation_but_another_round_has_another_identity() {
        let mut mem = MemoryStore::open_in_memory().unwrap();
        let seq = AtomicU64::new(0);
        let mut task = task();
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        assert!(pending(&mem, 1).unwrap().is_empty());
        task.state = TaskState::Failed {
            error: "first".into(),
        };
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        let first = pending(&mem, 1).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].revision, 2);
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        assert_eq!(pending(&mem, 1).unwrap().len(), 1);
        task.state = TaskState::Running;
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        task.state = TaskState::Succeeded;
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        let all = pending(&mem, 1).unwrap();
        assert_eq!(all.len(), 2);
        assert_ne!(all[0].id, all[1].id);
        acknowledge(&mut mem, HOST_OWNER, 1, &first[0].id).unwrap();
        assert_eq!(pending(&mem, 1).unwrap().len(), 1);
    }

    #[test]
    fn a_rejected_notice_rolls_back_the_terminal_record_inside_a_keep_applied_unit() {
        let mut task = task();
        task.state = TaskState::Succeeded;
        let limit = serde_json::to_vec(&task).unwrap().len() as u64 + 8;
        let config = MemoryConfig {
            entry_max_bytes: limit,
            ..MemoryConfig::default()
        };
        let mut mem = MemoryStore::open_in_memory_with_config(config).unwrap();
        let seq = AtomicU64::new(0);
        task.state = TaskState::Running;
        TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .put(&task)
            .unwrap();
        task.state = TaskState::Succeeded;
        let mut rejected = false;
        MemoryStorage::run_unit(&mut mem, &mut |m| {
            rejected = TaskStore::new(m, HOST_OWNER, &seq).put(&task).is_err();
            Ok(())
        })
        .unwrap();
        assert!(rejected, "the notification must exceed the entry limit");
        let actual = TaskStore::new(&mut mem, HOST_OWNER, &seq)
            .get(1, &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(actual.state, TaskState::Running);
        assert!(pending(&mem, 1).unwrap().is_empty());
    }

    #[test]
    fn an_unknown_notification_schema_is_not_overwritten_or_acknowledged() {
        let mut mem = MemoryStore::open_in_memory().unwrap();
        let key = format!("{PREFIX}unrecognized");
        let value = MemoryValue::Json(serde_json::json!({"schema":99}));
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            &key,
            &value,
            &PutOpts::default(),
        )
        .unwrap();
        assert!(pending(&mem, 1).is_err());
        assert_eq!(
            mem.get(&Scope::Workspace(1), &key).unwrap().unwrap().value,
            value
        );
    }
}
