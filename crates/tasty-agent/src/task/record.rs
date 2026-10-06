//! [`Task`] 의 직렬화 표현.
//!
//! 메모리 안의 v2 결과는 내부 표현(int64 는 JSON 정수)이다. 저장·IPC·CLI 가 모두 이
//! serde 경계를 지나므로 여기서만 계약의 출력 스키마로 wire 표현(int64 는 10진 문자열)과
//! 오간다. v1 task 는 그대로 통과한다. 필드와 serde 속성은 [`Task`] 와 1:1 이며,
//! 변환이 구조체 리터럴로 모든 필드를 나열하므로 한쪽에만 필드를 추가하면 컴파일되지 않는다.

use serde::{Deserialize, Serialize};

use super::{
    OnFailure, Task, TaskCommand, TaskContract, TaskId, TaskResult, TaskState, TypedResult,
    WorkspaceId,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct TaskRepr {
    id: TaskId,
    workspace_id: WorkspaceId,
    name: String,
    command: TaskCommand,
    #[serde(default)]
    depends_on: Vec<TaskId>,
    state: TaskState,
    created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finished_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<TaskResult>,
    #[serde(default)]
    on_failure: OnFailure,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default)]
    reserved_for_fallback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contract: Option<TaskContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    typed_result: Option<TypedResult>,
}

/// 확정된 출력이 있는 v2 task 의 `typed_result.output` 과 v1 투영 `result.output` 을 바꾼다.
fn convert_outputs(
    contract: Option<&TaskContract>,
    command: &TaskCommand,
    result: &mut Option<TaskResult>,
    typed_result: &mut Option<TypedResult>,
    f: impl Fn(
        &super::types::TypeDefs,
        &super::types::TypeSchema,
        &serde_json::Value,
    ) -> Result<serde_json::Value, String>,
) -> Result<(), String> {
    let (Some(contract), Some(typed)) = (contract, typed_result.as_mut()) else {
        return Ok(());
    };
    if !typed.has_output {
        return Ok(());
    }
    let defs = contract.defs();
    let schema = contract.output_schema(command);
    typed.output = f(&defs, &schema, &typed.output)?;
    if let Some(projected) = result.as_mut().and_then(|r| r.output.as_mut()) {
        *projected = f(&defs, &schema, projected)?;
    }
    Ok(())
}

impl From<Task> for TaskRepr {
    fn from(task: Task) -> Self {
        let Task {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            mut result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            mut typed_result,
        } = task;
        // 인코딩은 실패하지 않는다(스키마와 맞지 않는 자리는 그대로 둔다).
        let encoded = convert_outputs(
            contract.as_ref(),
            &command,
            &mut result,
            &mut typed_result,
            |defs, schema, v| Ok(defs.encode_wire(schema, v)),
        );
        debug_assert!(encoded.is_ok());
        TaskRepr {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            typed_result,
        }
    }
}

impl TryFrom<TaskRepr> for Task {
    type Error = String;

    fn try_from(repr: TaskRepr) -> Result<Self, String> {
        let TaskRepr {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            mut result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            mut typed_result,
        } = repr;
        convert_outputs(
            contract.as_ref(),
            &command,
            &mut result,
            &mut typed_result,
            |defs, schema, v| {
                defs.decode_wire(schema, v)
                    .map_err(|e| format!("task {id}: stored output: {e}"))
            },
        )?;
        Ok(Task {
            id,
            workspace_id,
            name,
            command,
            depends_on,
            state,
            created_at,
            started_at,
            finished_at,
            result,
            on_failure,
            metadata,
            reserved_for_fallback,
            contract,
            typed_result,
        })
    }
}
